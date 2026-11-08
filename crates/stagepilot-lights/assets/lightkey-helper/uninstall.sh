#!/bin/sh
# Remove lightkey-api: the LaunchAgent and the installed agent. Keeps your settings and takes unless you pass --purge.
#   curl -fsSL https://github.com/huntrw6/lightkey-api/releases/latest/download/uninstall.sh | sh -s -- --purge
set -eu
APP="$HOME/Library/Application Support/LightkeyAPI"
PLIST="$HOME/Library/LaunchAgents/io.github.lightkey-api.plist"
launchctl bootout "gui/$(id -u)/io.github.lightkey-api" 2>/dev/null || true
rm -f "$PLIST" "$APP/lightkey_api.py" "$APP/lightkey_cues.py"
if [ "${1:-}" = "--purge" ]; then rm -rf "$APP"; fi
echo "Removed. Delete the 'Lightkey API Active Cues' Shortcut in the Shortcuts app if you no longer want it."
