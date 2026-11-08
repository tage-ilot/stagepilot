#!/bin/sh
# Install lightkey-api for the current user on the Mac that runs Lightkey.
#   curl -fsSL https://github.com/huntrw6/lightkey-api/releases/latest/download/install.sh | sh
#   ... | sh -s -- --allow en0     let other computers on that network interface use the API
#   ... | sh -s -- --cues          also install the optional active-cue-name reader
# From a clone, run ./install.sh. Safe to re-run: it upgrades the agent and keeps your config.
set -eu

REPO=huntrw6/lightkey-api
APP="$HOME/Library/Application Support/LightkeyAPI"
PLIST="$HOME/Library/LaunchAgents/io.github.lightkey-api.plist"
LABEL=io.github.lightkey-api
PY=/usr/bin/python3
ALLOW=""
CUES=no

while [ $# -gt 0 ]; do
  case "$1" in
    --allow) [ $# -ge 2 ] || { echo "--allow needs an interface name, for example en0"; exit 1; }; ALLOW="$2"; shift 2 ;;
    --cues) CUES=yes; shift ;;
    -h|--help) sed -n '2,5p' "$0" 2>/dev/null || true; exit 0 ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

[ "$(uname)" = Darwin ] || { echo "This installs on macOS, on the computer that runs Lightkey."; exit 1; }
$PY -c 'import sys; sys.exit(sys.version_info < (3, 9))' 2>/dev/null || {
  echo "Need Python 3.9 or newer at /usr/bin/python3. Run: xcode-select --install"; exit 1; }

mkdir -p "$APP" "$HOME/Library/LaunchAgents" "$HOME/Library/Logs"

# 1. Get the agent: from this folder if it is a clone, otherwise from the latest release (checked against its checksum).
#    The cue-name reader is optional. It stays if it is already installed, and --cues adds it.
FILES="lightkey_api.py"
if [ "$CUES" = yes ] || [ -f "$APP/lightkey_cues.py" ]; then FILES="$FILES lightkey_cues.py"; fi
HERE=""
case "$0" in */install.sh|install.sh) HERE=$(cd "$(dirname "$0")" && pwd) ;; esac
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
if [ -n "$HERE" ] && [ -f "$HERE/lightkey_api.py" ]; then
  for f in $FILES; do cp "$HERE/$f" "$TMP/$f"; done
else
  BASE="https://github.com/$REPO/releases/latest/download"
  curl -fsSL "$BASE/SHA256SUMS" -o "$TMP/SHA256SUMS" || { echo "Could not download the checksum file."; exit 1; }
  for f in $FILES; do
    curl -fsSL "$BASE/$f" -o "$TMP/$f" || { echo "Download failed. Check the internet connection."; exit 1; }
    WANT=$(awk -v n="$f" '$2 == n {print $1}' "$TMP/SHA256SUMS")
    GOT=$(shasum -a 256 "$TMP/$f" | awk '{print $1}')
    [ -n "$WANT" ] && [ "$WANT" = "$GOT" ] || { echo "Checksum for $f does not match. Nothing was installed."; exit 1; }
  done
fi
for f in $FILES; do cp "$TMP/$f" "$APP/$f.new" && mv "$APP/$f.new" "$APP/$f"; done

# 2. Config: keep an existing one; --allow adds the interface.
[ -f "$APP/config.json" ] || echo '{}' > "$APP/config.json"
if [ -n "$ALLOW" ]; then
  $PY - "$APP/config.json" "$ALLOW" <<'PYEOF'
import json, sys
path, name = sys.argv[1], sys.argv[2]
try:
    cfg = json.load(open(path))
except ValueError:
    cfg = {}
if name not in cfg.setdefault("allow_interfaces", []):
    cfg["allow_interfaces"].append(name)
json.dump(cfg, open(path, "w"), indent=2)
PYEOF
fi

# 3. Start it at login.
cat > "$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>$LABEL</string>
  <key>ProgramArguments</key><array><string>$PY</string><string>-u</string><string>$APP/lightkey_api.py</string><string>run</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>10</integer>
  <key>ProcessType</key><string>Interactive</string>
  <key>StandardErrorPath</key><string>$HOME/Library/Logs/lightkey-api.err</string>
  <key>StandardOutPath</key><string>$HOME/Library/Logs/lightkey-api.out</string>
</dict></plist>
EOF
plutil -lint "$PLIST" >/dev/null
UIDN=$(id -u)
launchctl bootout "gui/$UIDN/$LABEL" 2>/dev/null || true
launchctl bootstrap "gui/$UIDN" "$PLIST"

# 4. Read the fixtures from the project Lightkey has open (needs Lightkey running).
if $PY "$APP/lightkey_api.py" export-fixtures >/dev/null 2>&1; then
  FIX="fixtures: loaded from the open project"
elif [ -f "$APP/fixtures.json" ]; then
  FIX="fixtures: using the map from an earlier install. To refresh it, open your project in Lightkey and run:  $PY \"$APP/lightkey_api.py\" export-fixtures"
else
  FIX="fixtures: not loaded yet. Open your project in Lightkey, then run:  $PY \"$APP/lightkey_api.py\" export-fixtures"
fi

# 5. Check that it answers.
UP=no
for _ in 1 2 3 4 5 6 7 8; do
  if curl -fsS -m 2 http://127.0.0.1:8770/health >/dev/null 2>&1; then UP=yes; break; fi
  sleep 1
done
VER=$($PY "$APP/lightkey_api.py" --version)

echo
echo "lightkey-api $VER installed. Running: $UP"
echo "  $FIX"
echo "  Try: curl http://127.0.0.1:8770/fixtures/state"
if [ -z "$ALLOW" ] && ! grep -q allow_interfaces "$APP/config.json"; then
  echo "  Other computers get 'forbidden' until you allow a network interface. Run the installer again with"
  echo "  --allow <interface>. Your interfaces: $(ifconfig -l | tr ' ' '\n' | grep '^en' | while read -r i; do ifconfig "$i" | grep -q 'inet ' && printf '%s ' "$i"; done)"
fi
[ "$UP" = yes ] || { echo "  It did not answer. See ~/Library/Logs/lightkey-api.log"; exit 1; }
