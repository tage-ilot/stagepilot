#!/usr/bin/env bash
set -euo pipefail

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
PACKAGE="$ROOT/tools/multitracks-cues"
TOOLS="$ROOT/.tools"
NODE_VERSION="22.23.1"

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) PLATFORM="darwin-arm64" ;;
  Darwin-x86_64) PLATFORM="darwin-x64" ;;
  *) echo "This setup command supports Intel and Apple Silicon macOS." >&2; exit 2 ;;
esac

supported_node() {
  "$1" -e 'const m=Number(process.versions.node.split(".")[0]);process.exit(m===22||m===24?0:1)' >/dev/null 2>&1
}

NODE=""
if command -v node >/dev/null 2>&1 && supported_node "$(command -v node)"; then
  NODE="$(command -v node)"
else
  RUNTIME="$TOOLS/node/node-v${NODE_VERSION}-${PLATFORM}"
  NODE="$RUNTIME/bin/node"
  if [ ! -x "$NODE" ]; then
    mkdir -p "$TOOLS/node"
    ARCHIVE="node-v${NODE_VERSION}-${PLATFORM}.tar.gz"
    BASE="https://nodejs.org/dist/v${NODE_VERSION}"
    TMP="$(mktemp -d "$TOOLS/node/setup.XXXXXX")"
    trap 'rm -rf "$TMP"' EXIT INT TERM
    curl --fail --location --proto '=https' --tlsv1.2 "$BASE/$ARCHIVE" -o "$TMP/$ARCHIVE"
    curl --fail --location --proto '=https' --tlsv1.2 "$BASE/SHASUMS256.txt" -o "$TMP/SHASUMS256.txt"
    EXPECTED="$(awk -v file="$ARCHIVE" '$2 == file { print $1 }' "$TMP/SHASUMS256.txt")"
    [ -n "$EXPECTED" ] || { echo "Node archive is absent from the official checksum manifest." >&2; exit 2; }
    ACTUAL="$(shasum -a 256 "$TMP/$ARCHIVE" | awk '{ print $1 }')"
    [ "$EXPECTED" = "$ACTUAL" ] || { echo "Node archive checksum verification failed." >&2; exit 2; }
    tar -xzf "$TMP/$ARCHIVE" -C "$TOOLS/node"
    rm -rf "$TMP"
    trap - EXIT INT TERM
  fi
fi

NPM_CLI="$(dirname "$NODE")/../lib/node_modules/npm/bin/npm-cli.js"
if [ ! -f "$NPM_CLI" ]; then
  NPM_CLI="$(command -v npm)"
  NPM=( "$NPM_CLI" )
else
  NPM=( "$NODE" "$NPM_CLI" )
fi

mkdir -p "$TOOLS"
LOCK_HASH="$(shasum -a 256 "$PACKAGE/package-lock.json" | awk '{ print $1 }')"
MARKER="$TOOLS/multitracks-cues-lock.sha256"
if [ ! -d "$PACKAGE/node_modules" ] || [ ! -f "$MARKER" ] || [ "$(cat "$MARKER")" != "$LOCK_HASH" ]; then
  "${NPM[@]}" --prefix "$PACKAGE" ci
  printf '%s\n' "$LOCK_HASH" > "$MARKER"
fi
"${NPM[@]}" --prefix "$PACKAGE" run typecheck
"${NPM[@]}" --prefix "$PACKAGE" test -- --run
"${NPM[@]}" --prefix "$PACKAGE" run lint
"${NPM[@]}" --prefix "$PACKAGE" run build
CODEX="$PACKAGE/node_modules/.bin/codex"
[ -x "$CODEX" ] || { echo "Pinned Codex executable was not installed." >&2; exit 2; }
CODEX_VERSION="$("$CODEX" --version)"
case "$CODEX_VERSION" in
  *"0.146.0") ;;
  *) echo "Codex version mismatch: expected 0.146.0, received $CODEX_VERSION" >&2; exit 2 ;;
esac
SCHEMA_TMP="$(mktemp -d "$TOOLS/codex-schema.XXXXXX")"
trap 'rm -rf "$SCHEMA_TMP"' EXIT INT TERM
"$CODEX" app-server generate-json-schema --out "$SCHEMA_TMP/json"
"$CODEX" app-server generate-ts --out "$SCHEMA_TMP/types"
grep -R -q '"account/login/start"' "$SCHEMA_TMP" || { echo "Codex schema lacks account/login/start." >&2; exit 2; }
grep -R -q 'item/tool/call' "$SCHEMA_TMP" || { echo "Codex schema lacks dynamic tool calls." >&2; exit 2; }
rm -rf "$SCHEMA_TMP"
trap - EXIT INT TERM
chmod +x "$ROOT/bin/stagepilot-cues"
echo "Ready: $ROOT/bin/stagepilot-cues (Codex $CODEX_VERSION)"

