#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 (--app APP | --dmg DMG | --archive APP_TAR_GZ) --arch (x86_64|arm64) [--no-launch]" >&2
  exit 2
}

MODE=""
INPUT=""
EXPECTED_ARCH=""
NO_LAUNCH=0
while (($#)); do
  case "$1" in
    --app|--dmg|--archive) MODE="${1#--}"; INPUT="${2:-}"; shift 2 ;;
    --arch) EXPECTED_ARCH="${2:-}"; shift 2 ;;
    --no-launch) NO_LAUNCH=1; shift ;;
    *) usage ;;
  esac
done
[[ -n "$MODE" && -n "$INPUT" && -n "$EXPECTED_ARCH" ]] || usage
[[ "$EXPECTED_ARCH" == "x86_64" || "$EXPECTED_ARCH" == "arm64" ]] || usage
[[ -e "$INPUT" ]] || { echo "Missing release artifact: $INPUT" >&2; exit 1; }

TEMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/stagepilot-release-verify.XXXXXX")"
MOUNT_POINT=""
BACKEND_PID=""
LOG_FILE="$TEMP_ROOT/backend.log"
cleanup() {
  if [[ -n "$BACKEND_PID" ]] && kill -0 "$BACKEND_PID" 2>/dev/null; then
    kill "$BACKEND_PID" 2>/dev/null || true
    wait "$BACKEND_PID" 2>/dev/null || true
  fi
  if [[ -n "$MOUNT_POINT" ]]; then
    hdiutil detach "$MOUNT_POINT" -quiet 2>/dev/null || true
  fi
  rm -rf "$TEMP_ROOT"
}
trap cleanup EXIT

case "$MODE" in
  app)
    APP="$INPUT"
    ;;
  dmg)
    MOUNT_POINT="$TEMP_ROOT/mount"
    mkdir -p "$MOUNT_POINT"
    hdiutil attach "$INPUT" -readonly -nobrowse -mountpoint "$MOUNT_POINT" -quiet
    APP_COUNT="$(find "$MOUNT_POINT" -maxdepth 2 -type d -name 'StagePilot.app' | wc -l | tr -d ' ')"
    [[ "$APP_COUNT" -eq 1 ]] || { echo "Expected exactly one StagePilot.app in $INPUT." >&2; exit 1; }
    SOURCE_APP="$(find "$MOUNT_POINT" -maxdepth 2 -type d -name 'StagePilot.app' -print -quit)"
    cp -R "$SOURCE_APP" "$TEMP_ROOT/StagePilot.app"
    APP="$TEMP_ROOT/StagePilot.app"
    UNINSTALLER="$(find "$MOUNT_POINT" -maxdepth 1 -type f -name 'Uninstall StagePilot.command' -print -quit)"
    [[ -n "$UNINSTALLER" ]] || { echo "DMG is missing the macOS uninstaller (Uninstall StagePilot.command)." >&2; exit 1; }
    [[ -x "$UNINSTALLER" ]] || { echo "Uninstall StagePilot.command in the DMG is not executable." >&2; exit 1; }
    bash -n "$UNINSTALLER" || { echo "Uninstall StagePilot.command has a syntax error." >&2; exit 1; }
    ;;
  archive)
    mkdir -p "$TEMP_ROOT/archive"
    tar -xzf "$INPUT" -C "$TEMP_ROOT/archive"
    APP_COUNT="$(find "$TEMP_ROOT/archive" -type d -name 'StagePilot.app' | wc -l | tr -d ' ')"
    [[ "$APP_COUNT" -eq 1 ]] || { echo "Expected exactly one StagePilot.app in $INPUT." >&2; exit 1; }
    APP="$(find "$TEMP_ROOT/archive" -type d -name 'StagePilot.app' -print -quit)"
    ;;
  *) usage ;;
esac

# The in-process Rust server is the backend: no separate backend executable or Python runtime
# may be shipped, and the built frontend must be bundled.
[[ -z "$(find "$APP" \( -iname 'stagepilot-backend*' -o -iname 'libpython*' -o -iname 'base_library.zip' \) -print -quit)" ]] || {
  echo "The bundle contains a separate backend executable or Python runtime files." >&2
  exit 1
}
[[ -s "$APP/Contents/Resources/stagepilot_web/index.html" ]] || { echo "The built frontend (stagepilot_web/index.html) is missing." >&2; exit 1; }
[[ -x "$APP/Contents/Resources/cloudflared" ]] || { echo "Bundled cloudflared is missing or not executable." >&2; exit 1; }
MAIN_NAME="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$APP/Contents/Info.plist")"
MAIN="$APP/Contents/MacOS/$MAIN_NAME"
[[ -x "$MAIN" ]] || { echo "Missing StagePilot application executable." >&2; exit 1; }
HELP_FOLDER="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleHelpBookFolder' "$APP/Contents/Info.plist")"
HELP_NAME="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleHelpBookName' "$APP/Contents/Info.plist")"
[[ "$HELP_FOLDER" == "StagePilot.help" ]] || { echo "StagePilot Help folder is not registered." >&2; exit 1; }
[[ "$HELP_NAME" == "org.stagepilot.desktop.help" ]] || { echo "StagePilot Help identifier is not registered." >&2; exit 1; }
HELP_ROOT="$APP/Contents/Resources/$HELP_FOLDER/Contents/Resources/English.lproj"
[[ -s "$HELP_ROOT/index.html" ]] || { echo "StagePilot Help landing page is missing." >&2; exit 1; }
[[ -s "$HELP_ROOT/StagePilot.helpindex" ]] || { echo "StagePilot Help search index is missing." >&2; exit 1; }
find "$HELP_ROOT/pages" -type f -name '*.html' -print -quit | grep -q . || {
  echo "StagePilot Help contains no searchable documentation pages." >&2
  exit 1
}

echo "Application: $APP"
file "$MAIN"
file "$MAIN" | grep -q "$EXPECTED_ARCH" || { echo "Application is not $EXPECTED_ARCH." >&2; exit 1; }
codesign --verify --deep --strict --verbose=4 "$APP"
# Stable identity: fixed identifiers and a designated requirement pinned to the StagePilot
# signing certificate (public cert: docs/stagepilot-signing-cert.pem), never a per-build cdhash.
if [[ "${STAGEPILOT_EXPECT_STABLE_SIGNING:-}" == "1" ]]; then
  PINNED_LEAF="78e4f3fad152196a9dc358629f7f36c5c4b57b74"
  check_dr() {
    local item="$1" id="$2" dr
    dr="$(codesign -d -r- "$item" 2>&1 | sed -n 's/^designated => //p')"
    echo "designated ($id): $dr"
    # A self-signed certificate is its own root, so macOS writes "certificate root = H...".
    [[ "$dr" == "identifier \"$id\" and certificate root = H\"$PINNED_LEAF\"" ]] || {
      echo "Unexpected designated requirement for $item: $dr" >&2
      exit 1
    }
  }
  check_dr "$APP" "org.stagepilot.desktop"
fi
SIGNATURE="$(codesign -d --verbose=4 "$MAIN" 2>&1)"
printf '%s\n' "$SIGNATURE"
grep -q 'flags=.*runtime' <<<"$SIGNATURE" && {
  echo "Application unexpectedly has the Hardened Runtime flag." >&2
  exit 1
}

verify_minos() {
  local binary="$1"
  local label="$2"
  local minos
  minos="$(xcrun vtool -show-build "$binary" 2>/dev/null | awk '/minos / {print $2}' | sort -Vu | tail -1)"
  if [[ -z "$minos" ]]; then
    # Older toolchains write LC_VERSION_MIN_MACOSX rather than LC_BUILD_VERSION.
    minos="$(
      otool -l "$binary" 2>/dev/null |
        awk '
          $1 == "cmd" { version_command = ($2 == "LC_VERSION_MIN_MACOSX") }
          version_command && $1 == "version" {
            print $2
            version_command = 0
          }
        ' |
        sort -Vu |
        tail -1
    )"
  fi
  [[ -n "$minos" ]] || { echo "Could not determine deployment target for $label." >&2; exit 1; }
  python3 - "$minos" "$label" <<'PY'
import sys
value, label = sys.argv[1:]
parts = tuple(int(part) for part in value.split("."))
if parts > (12, 0):
    raise SystemExit(f"{label} requires macOS {value}, newer than supported 12.0")
print(f"{label} minimum macOS: {value}")
PY
}
verify_minos "$MAIN" "StagePilot"

if [[ "$NO_LAUNCH" == "1" ]]; then
  # Static checks only. StagePilot is a GUI app: a second launch of the same bundle in one
  # CI login session never reaches setup (the first launch is registered with LaunchServices), so
  # the single real first-launch is scripts/smoke_rust_app.mjs, run after these static checks.
  echo "Static checks passed (launch skipped)."
  exit 0
fi

PORT="$(python3 - <<'PY'
import socket
with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
)"
SETTINGS="$TEMP_ROOT/settings.json"
printf '{"service_source":"demo","midi_source":"simulated","timer_output":"simulated","server_port":%s}\n' "$PORT" > "$SETTINGS"
# The desktop shell is single-instance: a previous verification run (the .app pass before the
# DMG pass) must be fully gone, with its single-instance socket, or this launch becomes a silent
# secondary instance with no server.
pkill -9 -x "$MAIN_NAME" 2>/dev/null || true
sleep 1
find "${TMPDIR:-/tmp}" -maxdepth 1 -name '*org.stagepilot.desktop*' -exec ls -l {} + >&2 2>/dev/null || true
find "${TMPDIR:-/tmp}" -maxdepth 1 -name '*org.stagepilot.desktop*' -exec rm -f {} + 2>/dev/null || true
# A pristine profile per launch: the app's logs, Remote bootstrap and window state of an earlier
# launch (the .app pass) must not influence this one.
LAUNCH_HOME="$TEMP_ROOT/home"
mkdir -p "$LAUNCH_HOME"
HOME="$LAUNCH_HOME" \
STAGEPILOT_HOST=127.0.0.1 \
STAGEPILOT_PORT="$PORT" \
STAGEPILOT_SETTINGS_PATH="$SETTINGS" \
"$MAIN" >"$LOG_FILE" 2>&1 &
BACKEND_PID="$!"

for _ in {1..120}; do
  if ! kill -0 "$BACKEND_PID" 2>/dev/null; then
    echo "StagePilot exited before readiness." >&2
    cat "$LOG_FILE" >&2
    exit 1
  fi
  if curl --fail --silent --show-error "http://127.0.0.1:$PORT/api/v1/health" >"$TEMP_ROOT/health.json"; then
    grep -q '"application_status"' "$TEMP_ROOT/health.json" || {
      echo "Health response did not identify StagePilot." >&2
      cat "$TEMP_ROOT/health.json" >&2
      exit 1
    }
    COOKIE_JAR="$TEMP_ROOT/dashboard-cookie.txt"
    curl --fail --silent --show-error \
      --cookie-jar "$COOKIE_JAR" \
      --header "Content-Type: application/json" \
      --data '{"pin":"1234"}' \
      "http://127.0.0.1:$PORT/api/v1/dashboard-auth/login" >/dev/null
    curl --fail --silent --show-error \
      --cookie "$COOKIE_JAR" \
      "http://127.0.0.1:$PORT/api/v1/state" >/dev/null
    # The in-process server must be the only backend: no Python process may exist under the
    # launched application.
    if ps -A -o pid=,ppid=,command= | awk -v root="$BACKEND_PID" '$2 == root' | grep -Eiq 'python|stagepilot-backend'; then
      echo "A Python backend process was spawned by StagePilot." >&2
      exit 1
    fi
    curl --fail --silent --show-error "http://127.0.0.1:$PORT/" | grep -qi '<script' || {
      echo "The bundled frontend is not served by the in-process server." >&2
      exit 1
    }
    echo "Health check passed."
    exit 0
  fi
  sleep 0.25
done

echo "Timed out waiting for StagePilot health." >&2
cat "$LOG_FILE" >&2
echo "--- diagnostics ---" >&2
kill -0 "$BACKEND_PID" 2>/dev/null && echo "app still running (pid $BACKEND_PID)" >&2 || echo "app exited" >&2
ps -A -o pid=,ppid=,etime=,command= | grep -i stagepilot | grep -v grep >&2 || true
lsof -nP -a -p "$BACKEND_PID" -i 2>&1 | head -20 >&2 || true
ls -l /tmp/*org_stagepilot* >&2 2>/dev/null || true
sample "$BACKEND_PID" 2 -mayDie 2>&1 | head -90 >&2 || true
for f in "$LAUNCH_HOME"/Library/Logs/org.stagepilot.desktop/* "$HOME"/Library/Logs/org.stagepilot.desktop/* "$HOME"/Library/Logs/DiagnosticReports/stagepilot* ; do
  [[ -f "$f" ]] && { echo "== $f" >&2; tail -n 60 "$f" >&2; }
done
find "$LAUNCH_HOME/Library" -maxdepth 3 2>/dev/null | head -20 >&2 || true
exit 1
