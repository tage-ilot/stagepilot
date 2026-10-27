#!/usr/bin/env bash
# Re-sign a Tauri-built StagePilot.app with a STABLE identity, inside-out, with fixed
# identifiers, so the designated requirement is identical on every build:
#   designated => identifier "org.stagepilot.desktop" and certificate leaf = H"<cert hash>"
# macOS Local Network (TCC) and Keychain ACL approvals follow the designated requirement,
# so they then survive updates. No Hardened Runtime, no --deep. Usage:
#   sign_macos_bundle.sh --app APP --identity NAME --keychain KEYCHAIN
#   sign_macos_bundle.sh --app APP --adhoc          (fork PR builds only)
set -euo pipefail

APP="" IDENTITY="" KEYCHAIN="" ADHOC=0
while (($#)); do
  case "$1" in
    --app) APP="${2:-}"; shift 2 ;;
    --identity) IDENTITY="${2:-}"; shift 2 ;;
    --keychain) KEYCHAIN="${2:-}"; shift 2 ;;
    --adhoc) ADHOC=1; shift ;;
    *) echo "Unknown argument: $1" >&2; exit 2 ;;
  esac
done
[[ -d "$APP" ]] || { echo "Missing app bundle: $APP" >&2; exit 1; }
if ((ADHOC)); then
  SIGN=(codesign --force --sign - --timestamp=none)
else
  [[ -n "$IDENTITY" && -n "$KEYCHAIN" ]] || { echo "--identity and --keychain are required (no silent ad-hoc fallback)." >&2; exit 1; }
  SIGN=(codesign --force --sign "$IDENTITY" --keychain "$KEYCHAIN" --timestamp=none)
fi

APP_ID="org.stagepilot.desktop"
CLOUDFLARED_ID="org.stagepilot.cloudflared"

sign_one() { # file identifier
  [[ -f "$1" ]] || return 0
  "${SIGN[@]}" --identifier "$2" "$1"
}

# Inside-out: the helper binary first, then the bundle itself.
sign_one "$APP/Contents/Resources/cloudflared" "$CLOUDFLARED_ID"
"${SIGN[@]}" --identifier "$APP_ID" "$APP"

codesign --verify --deep --strict --verbose=2 "$APP"
echo "== designated requirements =="
codesign -d -r- "$APP" 2>&1 | grep -E '^designated' || true
