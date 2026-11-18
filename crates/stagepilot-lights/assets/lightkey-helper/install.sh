#!/bin/sh
# Install lightkey-api for the current user on the Mac that runs Lightkey.
#   curl -fsSL https://github.com/huntrw6/lightkey-api/releases/latest/download/install.sh | sh
#   ... | sh -s -- --allow en0     let other computers on that network interface use the API
#   ... | sh -s -- --allow-for 10.30.0.5   same, using the interface that reaches that computer
#   ... | sh -s -- --cues          also install the optional active-cue-name reader
# Offline: put install.sh, lightkey_api.py, SHA256SUMS (and lightkey_cues.py for --cues) in one folder and run
# sh install.sh there. Every file is checked against SHA256SUMS and nothing is fetched from the network.
# From a clone, run ./install.sh. Safe to re-run: it upgrades the agent and keeps your config.
set -eu

REPO=huntrw6/lightkey-api
APP="$HOME/Library/Application Support/LightkeyAPI"
PLIST="$HOME/Library/LaunchAgents/io.github.lightkey-api.plist"
LABEL=io.github.lightkey-api
PY=${LIGHTKEY_PY:-/usr/bin/python3}      # LIGHTKEY_PY, LIGHTKEY_ROUTE and LIGHTKEY_UNAME exist so the tests can stub the Mac
ROUTE=${LIGHTKEY_ROUTE:-/sbin/route}
CC=${LIGHTKEY_CC:-/usr/bin/clang}                  # the same hooks for the launcher build and the folder-permission reset
CODESIGN=${LIGHTKEY_CODESIGN:-/usr/bin/codesign}
TCCUTIL=${LIGHTKEY_TCCUTIL:-/usr/bin/tccutil}
LAUNCHER_APP="$APP/Lightkey API.app"    # macOS names the app in its questions after this folder name
ALLOW=""
ALLOW_FOR=""
ALLOW_ADDRESS=""
CUES=no

while [ $# -gt 0 ]; do
  case "$1" in
    --allow) [ $# -ge 2 ] || { echo "--allow needs an interface name, for example en0"; exit 1; }; ALLOW="$2"; shift 2 ;;
    --allow-for) [ $# -ge 2 ] || { echo "--allow-for needs the IP address of the computer that will use the API"; exit 1; }; ALLOW_FOR="$2"; shift 2 ;;
    --allow-address) [ $# -ge 2 ] || { echo "--allow-address needs the IP address of this Mac that the other computer connects to"; exit 1; }; ALLOW_ADDRESS="$2"; shift 2 ;;
    --cues) CUES=yes; shift ;;
    -h|--help) sed -n '2,9p' "$0" 2>/dev/null || true; exit 0 ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

# --allow-address: the interface that has this Mac's own address, the one the other computer connects to. That is
# the interface that matters when this Mac reaches the other computer through a different network than the one
# the other computer reaches it on. --allow-for: the interface that routes to that computer. Both --allow and
# --allow-for may be given; when --allow-address finds its interface it is used instead of --allow-for.
IFCONFIG=${LIGHTKEY_IFCONFIG:-/sbin/ifconfig}
ALLOW_LIST="$ALLOW"
if [ -n "$ALLOW_ADDRESS" ]; then
  OWN_IFACE=$("$IFCONFIG" 2>/dev/null | awk -v ip="$ALLOW_ADDRESS" '/^[a-z0-9]+:/ {name = $1; sub(/:$/, "", name)} $1 == "inet" && $2 == ip {print name; exit}')
  if [ -n "$OWN_IFACE" ]; then
    echo "Allowing network interface $OWN_IFACE (it has $ALLOW_ADDRESS, the address StagePilot uses)."
    ALLOW_LIST="$ALLOW_LIST $OWN_IFACE"
    ALLOW_FOR=""
  elif [ -z "$ALLOW_FOR" ] && [ -z "$ALLOW" ]; then
    echo "This Mac has no network interface with the address $ALLOW_ADDRESS. Check the Lighting address in StagePilot, or use --allow <interface> (for example en0)."; exit 1
  else
    echo "This Mac has no interface with the address $ALLOW_ADDRESS; using the one that reaches StagePilot instead."
  fi
fi
if [ -n "$ALLOW_FOR" ]; then
  IFACE=$("$ROUTE" -n get "$ALLOW_FOR" 2>/dev/null | awk '$1 == "interface:" {print $2; exit}')
  [ -n "$IFACE" ] || { echo "Could not find a network interface that reaches $ALLOW_FOR. Check the address, or use --allow <interface> (for example en0)."; exit 1; }
  echo "Allowing network interface $IFACE (it reaches $ALLOW_FOR)."
  ALLOW_LIST="$ALLOW_LIST $IFACE"
fi

sha256() { if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'; else sha256sum "$1" | awk '{print $1}'; fi; }

[ "${LIGHTKEY_UNAME:-$(uname)}" = Darwin ] || { echo "This installs on macOS, on the computer that runs Lightkey."; exit 1; }
$PY -c 'import sys; sys.exit(sys.version_info < (3, 9))' 2>/dev/null || {
  echo "Need Python 3.9 or newer at /usr/bin/python3. Run: xcode-select --install"; exit 1; }

mkdir -p "$APP" "$HOME/Library/LaunchAgents" "$HOME/Library/Logs"

# 1. Get the agent. Three ways, never mixed:
#    - the files sit next to this script with a SHA256SUMS file: offline. Every file is checked, nothing is downloaded.
#    - the files sit next to this script without one (a git clone): copied as they are.
#    - otherwise (curl | sh): the latest release, each file checked against its checksum.
#    The cue-name reader is optional. It stays if it is already installed, and --cues adds it.
FILES="lightkey_api.py"
if [ "$CUES" = yes ] || [ -f "$APP/lightkey_cues.py" ]; then FILES="$FILES lightkey_cues.py"; fi
HERE=""
case "$0" in */install.sh|install.sh) HERE=$(cd "$(dirname "$0")" && pwd) ;; esac
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
if [ -n "$HERE" ] && [ -f "$HERE/lightkey_api.py" ] && [ -f "$HERE/SHA256SUMS" ]; then
  echo "Installing from the files next to install.sh. Nothing is downloaded."
  for f in $FILES; do
    [ -f "$HERE/$f" ] || { echo "$f is missing next to install.sh. Nothing was installed."; exit 1; }
  done
  for f in lightkey_api.py lightkey_cues.py install.sh uninstall.sh; do
    [ -f "$HERE/$f" ] || continue
    WANT=$(awk -v n="$f" '$2 == n || $2 == "*" n {print $1}' "$HERE/SHA256SUMS")
    GOT=$(sha256 "$HERE/$f")
    if [ -z "$WANT" ] || [ "$WANT" != "$GOT" ]; then
      # a file the install does not use may be absent from the list; one that it uses must match
      case " $FILES install.sh " in *" $f "*) echo "Checksum for $f does not match SHA256SUMS. Nothing was installed."; exit 1 ;; esac
      [ -z "$WANT" ] || { echo "Checksum for $f does not match SHA256SUMS. Nothing was installed."; exit 1; }
    fi
  done
  for f in $FILES; do cp "$HERE/$f" "$TMP/$f"; done
elif [ -n "$HERE" ] && [ -f "$HERE/lightkey_api.py" ]; then
  for f in $FILES; do cp "$HERE/$f" "$TMP/$f"; done
else
  BASE="https://github.com/$REPO/releases/latest/download"
  curl -fsSL "$BASE/SHA256SUMS" -o "$TMP/SHA256SUMS" || { echo "Could not download the checksum file."; exit 1; }
  for f in $FILES; do
    curl -fsSL "$BASE/$f" -o "$TMP/$f" || { echo "Download failed. Check the internet connection."; exit 1; }
    WANT=$(awk -v n="$f" '$2 == n {print $1}' "$TMP/SHA256SUMS")
    GOT=$(sha256 "$TMP/$f")
    [ -n "$WANT" ] && [ "$WANT" = "$GOT" ] || { echo "Checksum for $f does not match. Nothing was installed."; exit 1; }
  done
fi
for f in $FILES; do cp "$TMP/$f" "$APP/$f.new" && mv "$APP/$f.new" "$APP/$f"; done
# A manual install is the new truth: forget what an automatic update left (the old version kept as .prev, a half-finished swap,
# the "unproven" mark and the failure sentence), so the wrapper cannot put an older version back over this one.
rm -rf "$APP/update"
rm -f "$APP"/*.prev "$APP"/*.prev.tmp "$APP/update-pending" "$APP/update-failed" "$APP/run-state.json"

# 1b. The wrapper: launchd runs this small file, and it starts the agent. It finishes an update that was cut short and puts the old
#     version back when a new one will not start. Only this installer writes it; an update never replaces it.
cat > "$APP/lightkey_api_run.py.new" <<'WRAPEOF'
#!/usr/bin/env python3
"""Starts the Lightkey API agent, and puts the old version back when a freshly installed one will not start.

launchd runs this file (through the Lightkey API launcher). The installer writes it and an update never replaces it, so
it stays the same whatever version the agent is. It does three things:

1. Finishes an update that a crash cut short (update/journal.json is there), so the agent never starts from a half-swapped set.
2. Starts the agent and waits for it. The agent exits 0 on purpose after it installed an update; launchd then starts this file
   again, which starts the new version.
3. When an update has just been installed (update-pending is there) and the agent exits with an error within 20 seconds of
   starting, three times in a row, it puts the .prev files back, writes update-failed (the agent shows it as
   last_update_error) and starts the old version. An agent that stays up for 20 seconds proves the new version: update-pending
   goes away and no later crash can send it back.
"""
import json
import os
import shutil
import signal
import subprocess
import sys
import time

APP = os.path.dirname(os.path.abspath(__file__))
HELPER = os.path.join(APP, "lightkey_api.py")
FILES = ("lightkey_api.py", "lightkey_cues.py")                  # what an update replaces (the cue reader only where it is installed)
GRACE_S = float(os.environ.get("LIGHTKEY_WRAPPER_GRACE_S", "20"))   # "within 20 s" (tests shorten it)
FAILS = 3


def path(name):
    return os.path.join(APP, name)


def read(name):
    try:
        with open(path(name), encoding="utf-8") as f:
            value = json.load(f)
        return value if isinstance(value, dict) else None
    except (OSError, ValueError):
        return None


def write(name, value):
    with open(path(name) + ".tmp", "w", encoding="utf-8") as f:
        json.dump(value, f)
        f.flush()
        os.fsync(f.fileno())
    os.replace(path(name) + ".tmp", path(name))


def remove(name):
    try:
        os.remove(path(name))
    except OSError:
        pass


def finish_swap():
    """The same swap the agent does after an update (keep the running file as NAME.prev, rename the staged file over it),
    run again after a crash: a file whose staged copy is gone was done before."""
    journal = read(os.path.join("update", "journal.json"))
    if not journal:
        return
    for name in journal.get("files", []):
        if name not in FILES or not os.path.exists(path(os.path.join("update", name))):
            continue
        if os.path.exists(path(name)):
            shutil.copyfile(path(name), path(name) + ".prev.tmp")
            os.replace(path(name) + ".prev.tmp", path(name) + ".prev")
        os.replace(path(os.path.join("update", name)), path(name))
    write("update-pending", {"from": journal.get("from"), "to": journal.get("to")})
    remove("update-failed")
    remove(os.path.join("update", "journal.json"))
    shutil.rmtree(path("update"), ignore_errors=True)


def put_old_version_back():
    """Restores the .prev files (each one is used up by its own rename, so a crash here is finished by the next start),
    says why in update-failed and ends the unproven state."""
    pending = read("update-pending") or {}
    restored = 0
    for name in FILES:
        if os.path.exists(path(name + ".prev")):
            os.replace(path(name + ".prev"), path(name))
            restored += 1
    new = pending.get("to") or "the new version"
    old = pending.get("from")
    message = "Version %s would not start, so %s was put back." % (new, "version " + old if old else "the old version")
    write("update-failed", {"version": pending.get("to"), "message": message if restored else "Version %s would not start." % new})
    remove("update-pending")
    return restored > 0


def main():
    finish_swap()
    stopping = []
    child = []

    def forward(signum, _frame):
        stopping.append(signum)
        if child and child[0].poll() is None:
            child[0].send_signal(signum)

    for name in ("SIGTERM", "SIGINT", "SIGHUP"):
        signal.signal(getattr(signal, name), forward)
    while True:
        state = read("run-state.json") or {}
        process = subprocess.Popen([sys.executable, "-u", HELPER, "run"])
        child[:] = [process]
        started = time.monotonic()
        try:
            code = process.wait(timeout=GRACE_S)
        except subprocess.TimeoutExpired:
            if os.path.exists(path("update-pending")):      # it has been up long enough: this version is proven
                remove("update-pending")
            write("run-state.json", {"fast_failures": 0})
            code = process.wait()
        if stopping or code == 0 or time.monotonic() - started >= GRACE_S:
            write("run-state.json", {"fast_failures": 0})
            return 0 if stopping else code
        state["fast_failures"] = int(state.get("fast_failures", 0)) + 1
        write("run-state.json", state)
        if state["fast_failures"] >= FAILS and os.path.exists(path("update-pending")):
            put_old_version_back()
            write("run-state.json", {"fast_failures": 0})
            continue                                         # start the version that was put back, now
        return code


if __name__ == "__main__":
    sys.exit(main())
WRAPEOF
mv "$APP/lightkey_api_run.py.new" "$APP/lightkey_api_run.py"

# 2. Config: keep an existing one; --allow and --allow-for add the interface.
[ -f "$APP/config.json" ] || echo '{}' > "$APP/config.json"
if [ -n "$ALLOW_LIST" ]; then
  $PY - "$APP/config.json" $ALLOW_LIST <<'PYEOF'
import json, sys
path = sys.argv[1]
try:
    cfg = json.load(open(path))
except ValueError:
    cfg = {}
for name in sys.argv[2:]:
    if name not in cfg.setdefault("allow_interfaces", []):
        cfg["allow_interfaces"].append(name)
json.dump(cfg, open(path, "w"), indent=2)
PYEOF
fi

# 2b. The agent starts through a tiny signed app named "Lightkey API". macOS asks about Documents and Downloads only for
#     something it can name; a bare python3 background job is refused without any question. If this step fails, the
#     agent still runs the old way, just without access to those two folders.
build_launcher() {
  LB=$(mktemp -d "${TMPDIR:-/tmp}/lightkey-launcher.XXXXXX") || return 1
  cat > "$LB/launcher.c" <<'CEOF'
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/wait.h>
#include <unistd.h>
static pid_t child = 0;
static void forward(int sig) { if (child > 0) kill(child, sig); }
int main(int argc, char **argv) {
    if (argc < 2) { fprintf(stderr, "usage: lightkey-api script [args...]\n"); return 2; }
    signal(SIGTERM, forward); signal(SIGINT, forward); signal(SIGHUP, forward);
    setenv("LIGHTKEY_LAUNCHER", "1", 1);
    child = fork();
    if (child == 0) {
        char **args = calloc((size_t)argc + 2, sizeof(char *));
        args[0] = "/usr/bin/python3"; args[1] = "-u";
        for (int i = 1; i < argc; i++) args[i + 1] = argv[i];
        execv("/usr/bin/python3", args); perror("exec python3"); _exit(127);
    }
    int status = 0;
    while (waitpid(child, &status, 0) < 0) { }
    return WIFEXITED(status) ? WEXITSTATUS(status) : 1;
}
CEOF
  cat > "$LB/Info.plist" <<'PEOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>io.github.lightkey-api</string>
  <key>CFBundleName</key><string>Lightkey API</string>
  <key>CFBundleExecutable</key><string>lightkey-api</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSUIElement</key><true/>
  <key>NSLocalNetworkUsageDescription</key><string>Lightkey API lets StagePilot on your network read Lightkey's lights.</string>
  <key>NSDocumentsFolderUsageDescription</key><string>Lightkey API reads your Lightkey project to learn the fixtures.</string>
  <key>NSDownloadsFolderUsageDescription</key><string>Lightkey API reads your Lightkey project to learn the fixtures.</string>
</dict></plist>
PEOF
  cat "$LB/launcher.c" "$LB/Info.plist" > "$LB/both"
  NEWSTAMP=$(sha256 "$LB/both")
  if [ -x "$LAUNCHER_APP/Contents/MacOS/lightkey-api" ] && [ "$(cat "$APP/launcher.stamp" 2>/dev/null || true)" = "$NEWSTAMP" ]; then
    rm -rf "$LB"; return 0          # same launcher as last time: keep it, so macOS keeps the answers it was given
  fi
  mkdir -p "$LB/Lightkey API.app/Contents/MacOS"
  "$CC" -O2 -o "$LB/Lightkey API.app/Contents/MacOS/lightkey-api" "$LB/launcher.c" || { rm -rf "$LB"; return 1; }
  cp "$LB/Info.plist" "$LB/Lightkey API.app/Contents/Info.plist"
  "$CODESIGN" --force --sign - "$LB/Lightkey API.app" || { rm -rf "$LB"; return 1; }
  rm -rf "$LAUNCHER_APP" "$APP/LightkeyAPI.app"        # the second name is what the first 0.5.0 builds used
  mv "$LB/Lightkey API.app" "$LAUNCHER_APP"
  printf '%s' "$NEWSTAMP" > "$APP/launcher.stamp"
  rm -rf "$LB"
}
if build_launcher >"$APP/launcher-build.log" 2>&1; then
  START="$LAUNCHER_APP/Contents/MacOS/lightkey-api"
  START_ARGS="<string>$APP/lightkey_api_run.py</string>"
else
  echo "Could not set up the Lightkey API launcher (see $APP/launcher-build.log). The agent will run without access to Documents and Downloads."
  START="$PY"
  START_ARGS="<string>-u</string><string>$APP/lightkey_api_run.py</string>"
fi

# 3. Start it at login.
cat > "$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>$LABEL</string>
  <key>ProgramArguments</key><array><string>$START</string>$START_ARGS</array>
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
echo
echo "macOS may ask to let Lightkey API use your local network and read your Documents and Downloads folders. Click Allow each time. The folders are only used to find your Lightkey project."
rm -f "$APP/tcc_primed"        # the agent asks again after an install or upgrade, while someone is at the Mac
# A folder the running agent was refused is forgotten here, so macOS asks again instead of keeping the old No.
curl -fsS -m 2 http://127.0.0.1:8770/health 2>/dev/null | $PY -c '
import json, sys
try:
    f = json.load(sys.stdin)["watch"]["folders"]
except Exception:
    f = {}
for name, state in sorted(f.items()):
    if state == "denied" and name in ("Documents", "Downloads"):
        print("SystemPolicy%sFolder" % name)' 2>/dev/null | while read -r SERVICE; do
  "$TCCUTIL" reset "$SERVICE" "$LABEL" >/dev/null 2>&1 || true
done
launchctl bootout "gui/$UIDN/$LABEL" 2>/dev/null || true
STARTED=no
for _ in 1 2 3 4 5; do          # bootstrap right after bootout can fail with an I/O error while launchd is still removing the old job
  sleep 1
  if launchctl bootstrap "gui/$UIDN" "$PLIST" 2>/dev/null; then STARTED=yes; break; fi
done
[ "$STARTED" = yes ] || launchctl bootstrap "gui/$UIDN" "$PLIST"

# 4. Read the fixtures from the project Lightkey has open (needs Lightkey running).
if $PY "$APP/lightkey_api.py" export-fixtures >/dev/null 2>&1; then
  FIX="fixtures: loaded from the open project"
elif [ -f "$APP/fixtures.json" ]; then
  FIX="fixtures: using the map from an earlier install. The agent re-reads the open Lightkey project by itself (or press Read fixtures in StagePilot)."
else
  FIX="fixtures: not loaded yet. Open your project in Lightkey; the agent reads it by itself within a few minutes (or press Read fixtures in StagePilot)."
fi

# 5. Check that it answers.
UP=no
for _ in 1 2 3 4 5 6 7 8; do
  if curl -fsS -m 2 http://127.0.0.1:8770/health >/dev/null 2>&1; then UP=yes; break; fi
  sleep 1
done
VER=$($PY "$APP/lightkey_api.py" --version)

# 6. Wait up to 45 seconds for the answers to the folder question (the agent reports it in /health).
FOLDERS=unknown
if [ "$UP" = yes ]; then
  for _ in $(seq 1 45); do
    FOLDERS=$(curl -fsS -m 2 http://127.0.0.1:8770/health 2>/dev/null | $PY -c '
import json, sys
try:
    f = json.load(sys.stdin)["watch"]["folders"]
    print(" ".join("%s %s" % (k, f[k]) for k in ("Documents", "Downloads")))
except Exception:
    print("unknown")' 2>/dev/null || echo unknown)
    case "$FOLDERS" in *unknown*) sleep 1 ;; *) break ;; esac
  done
fi

echo
echo "lightkey-api $VER installed. Running: $UP"
echo "  $FIX"
case "$FOLDERS" in
  *denied*) echo "  folders: $FOLDERS. Run this installer again to be asked once more, or open System Settings > Privacy & Security > Files and Folders and allow Lightkey API." ;;
  *unknown*) echo "  folders: no answer yet. Click Allow if macOS is asking." ;;
  *) echo "  folders: $FOLDERS" ;;
esac
echo "  Try: curl http://127.0.0.1:8770/fixtures/state"
if [ -z "$ALLOW_LIST" ] && ! grep -q allow_interfaces "$APP/config.json"; then
  echo "  Other computers get 'forbidden' until you allow a network interface. Run the installer again with"
  echo "  --allow <interface> (or --allow-for <ip of that computer>). Your interfaces: $(ifconfig -l | tr ' ' '\n' | grep '^en' | while read -r i; do ifconfig "$i" | grep -q 'inet ' && printf '%s ' "$i"; done)"
fi
[ "$UP" = yes ] || { echo "  It did not answer. See ~/Library/Logs/lightkey-api.log"; exit 1; }
