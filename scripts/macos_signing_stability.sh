#!/usr/bin/env bash
# Signing-identity stability check (macOS only). Builds two DIFFERENT fake StagePilot
# bundles (app + helper), signs both with the same identity via
# sign_macos_bundle.sh, and fails if any designated requirement or identifier differs, if
# the cdhash-only (ad-hoc) requirement slipped in, or if either bundle fails verification.
# With --identity/--keychain it uses that identity; otherwise it creates a throwaway one.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/sp-sign-stability.XXXXXX")"
KC="$WORK/test.keychain-db"
cleanup() { security delete-keychain "$KC" 2>/dev/null || true; rm -rf "$WORK"; }
trap cleanup EXIT

IDENTITY="${1:-}"; KEYCHAIN="${2:-}"
if [[ -z "$IDENTITY" ]]; then
  IDENTITY="StagePilot CI Stability Test"
  KEYCHAIN="$KC"
  PASS="$(uuidgen)"
  cat > "$WORK/openssl.cnf" <<CNF
[req]
distinguished_name=dn
x509_extensions=ext
prompt=no
[dn]
CN=$IDENTITY
[ext]
basicConstraints=critical,CA:false
keyUsage=critical,digitalSignature
extendedKeyUsage=critical,codeSigning
CNF
  openssl req -x509 -newkey rsa:2048 -nodes -days 30 -config "$WORK/openssl.cnf" \
    -keyout "$WORK/k.pem" -out "$WORK/c.pem" 2>/dev/null
  openssl pkcs12 -export -inkey "$WORK/k.pem" -in "$WORK/c.pem" -out "$WORK/id.p12" \
    -passout "pass:$PASS" 2>/dev/null
  security create-keychain -p "$PASS" "$KC"
  security set-keychain-settings -lut 600 "$KC"
  security unlock-keychain -p "$PASS" "$KC"
  security import "$WORK/id.p12" -k "$KC" -P "$PASS" -T /usr/bin/codesign >/dev/null
  security set-key-partition-list -S apple-tool:,apple: -s -k "$PASS" "$KC" >/dev/null
  security list-keychains -d user -s "$KC" $(security list-keychains -d user | tr -d '"')
fi

make_bundle() { # dir marker
  local app="$1/StagePilot.app"
  mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
  cat > "$app/Contents/Info.plist" <<PL
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>org.stagepilot.desktop</string>
<key>CFBundleExecutable</key><string>stagepilot-desktop</string>
<key>CFBundleName</key><string>StagePilot</string>
<key>CFBundleVersion</key><string>$2</string>
</dict></plist>
PL
  for bin in "$app/Contents/MacOS/stagepilot-desktop" "$app/Contents/Resources/cloudflared"; do
    printf 'int main(void){return %s;}\n' "$2" | cc -x c - -o "$bin"
  done
}

dr() { codesign -d -r- "$1" 2>&1 | sed -n 's/^designated => //p'; }
ident() { codesign -d --verbose=2 "$1" 2>&1 | sed -n 's/^Identifier=//p'; }

SIG_ARGS=(--identity "$IDENTITY" --keychain "$KEYCHAIN")
make_bundle "$WORK/a" 1; make_bundle "$WORK/b" 2
bash "$HERE/sign_macos_bundle.sh" --app "$WORK/a/StagePilot.app" "${SIG_ARGS[@]}" >/dev/null
bash "$HERE/sign_macos_bundle.sh" --app "$WORK/b/StagePilot.app" "${SIG_ARGS[@]}" >/dev/null

fail=0
for rel in "" "Contents/Resources/cloudflared"; do
  A="$WORK/a/StagePilot.app/$rel"; B="$WORK/b/StagePilot.app/$rel"
  da="$(dr "$A")"; db="$(dr "$B")"; ia="$(ident "$A")"; ib="$(ident "$B")"
  echo "[$rel] id=$ia | $da"
  [[ -n "$da" && "$da" == "$db" ]] || { echo "DRIFT in designated requirement: [$da] vs [$db]" >&2; fail=1; }
  [[ -n "$ia" && "$ia" == "$ib" && "$ia" != "-" ]] || { echo "DRIFT in identifier: $ia vs $ib" >&2; fail=1; }
  grep -q 'cdhash' <<<"$da" && { echo "cdhash-only requirement: not stable" >&2; fail=1; }
done
codesign --verify --deep --strict "$WORK/a/StagePilot.app"
codesign --verify --deep --strict "$WORK/b/StagePilot.app"
((fail==0)) && echo "Signing identity is stable across builds." || exit 1
