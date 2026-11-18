#!/bin/sh
# Install playback-library for the current user, on the Mac that runs MultiTracks Playback.
#   sh install.sh --key-file /path/to/key.txt   answer on any network path, but only to a caller that signs with this key.
#                                               The key is the first line of that file; the file is deleted once the key is stored.
#                                               It replaces the network limits below (they are cleared). Run again with a new file to change the key.
#   sh install.sh --allow-for 192.168.1.20      without a key: let the computer at that address use it (the interface that reaches it)
#   sh install.sh --allow en0                  without a key: let computers on that network interface use it
#   sh install.sh --allow-address 192.168.1.10  same, naming THIS Mac's own address that the other computer connects to
#   sh install.sh --client 192.168.1.20         without a key: answer only that computer (loopback always works)
#   curl -fsSL https://github.com/huntrw6/playback-library/releases/latest/download/install.sh | sh -s -- --key-file ...
# Offline: put install.sh, playback_library.py and SHA256SUMS in one folder and run sh install.sh there. Every file is
# checked against SHA256SUMS and nothing is fetched. From a clone, run ./install.sh. Safe to re-run: it upgrades the
# agent and keeps your config.
set -eu

REPO=huntrw6/playback-library
APP="$HOME/Library/Application Support/PlaybackLibrary"
PLIST="$HOME/Library/LaunchAgents/io.github.playback-library.plist"
LABEL=io.github.playback-library
PORT=8780
PY=${PLAYBACK_LIBRARY_PY:-/usr/bin/python3}     # PLAYBACK_LIBRARY_* exist so the tests can stub the Mac
ROUTE=${PLAYBACK_LIBRARY_ROUTE:-/sbin/route}
IFCONFIG=${PLAYBACK_LIBRARY_IFCONFIG:-/sbin/ifconfig}
CC=${PLAYBACK_LIBRARY_CC:-/usr/bin/clang}
CODESIGN=${PLAYBACK_LIBRARY_CODESIGN:-/usr/bin/codesign}
TCCUTIL=${PLAYBACK_LIBRARY_TCCUTIL:-/usr/bin/tccutil}
LAUNCHER_APP="$APP/Playback Library.app"        # macOS names the app in its questions after this folder name
KEY_FILE=""
ALLOW=""
ALLOW_FOR=""
ALLOW_ADDRESS=""
CLIENT=""

need() { [ $# -ge 2 ] || { echo "$1 needs a value."; exit 1; }; }
while [ $# -gt 0 ]; do
  case "$1" in
    --allow) need "$@"; ALLOW="$2"; shift 2 ;;
    --allow-for) need "$@"; ALLOW_FOR="$2"; shift 2 ;;
    --allow-address) need "$@"; ALLOW_ADDRESS="$2"; shift 2 ;;
    --client) need "$@"; CLIENT="$2"; shift 2 ;;
    --key-file) need "$@"; KEY_FILE="$2"; shift 2 ;;     # a file, never the key itself: a command line shows up in ps
    -h|--help) sed -n '2,10p' "$0" 2>/dev/null || true; exit 0 ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

# --allow-address picks the interface that has this Mac's own address (the one the other computer connects to).
# --allow-for picks the interface that routes to the other computer. When --allow-address finds its interface, it wins.
ALLOW_LIST="$ALLOW"
if [ -n "$ALLOW_ADDRESS" ]; then
  OWN_IFACE=$("$IFCONFIG" -a 2>/dev/null | awk -v ip="$ALLOW_ADDRESS" '/^[a-z0-9]+:/ {name = $1; sub(/:$/, "", name)} $1 == "inet" && $2 == ip {print name; exit}')
  if [ -n "$OWN_IFACE" ]; then
    echo "Allowing network interface $OWN_IFACE (it has $ALLOW_ADDRESS)."
    ALLOW_LIST="$ALLOW_LIST $OWN_IFACE"
    ALLOW_FOR=""
  elif [ -z "$ALLOW_FOR" ] && [ -z "$ALLOW" ]; then
    echo "This Mac has no network interface with the address $ALLOW_ADDRESS. Check the address, or use --allow <interface> (for example en0)."; exit 1
  else
    echo "This Mac has no interface with the address $ALLOW_ADDRESS; using the one that reaches the other computer instead."
  fi
fi
if [ -n "$ALLOW_FOR" ]; then
  IFACE=$("$ROUTE" -n get "$ALLOW_FOR" 2>/dev/null | awk '$1 == "interface:" {print $2; exit}')
  [ -n "$IFACE" ] || { echo "Could not find a network interface that reaches $ALLOW_FOR. Check the address, or use --allow <interface> (for example en0)."; exit 1; }
  echo "Allowing network interface $IFACE (it reaches $ALLOW_FOR)."
  ALLOW_LIST="$ALLOW_LIST $IFACE"
fi

sha256() { if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'; else sha256sum "$1" | awk '{print $1}'; fi; }

[ "${PLAYBACK_LIBRARY_UNAME:-$(uname)}" = Darwin ] || { echo "This installs on macOS, on the computer that runs Playback."; exit 1; }
$PY -c 'import sys; sys.exit(sys.version_info < (3, 9))' 2>/dev/null || {
  echo "Need Python 3.9 or newer at /usr/bin/python3. Run: xcode-select --install"; exit 1; }

# The key file is read here only to check it; nothing is printed from it.
if [ -n "$KEY_FILE" ]; then
  [ -f "$KEY_FILE" ] || { echo "Key file not found: $KEY_FILE"; exit 1; }
  $PY - "$KEY_FILE" <<'PYEOF' || exit 1
import sys
try:
    with open(sys.argv[1]) as f:
        key = f.readline().rstrip("\r\n")
except (OSError, UnicodeDecodeError):
    print("Could not read the key file. Nothing was installed.")
    sys.exit(1)
if len(key) < 32:
    print("The key in the key file is too short: it needs at least 32 characters. Nothing was installed.")
    sys.exit(1)
if not all(33 <= ord(c) <= 126 for c in key):
    print("The key must be one line of plain characters without spaces. Nothing was installed.")
    sys.exit(1)
PYEOF
fi

mkdir -p "$APP" "$HOME/Library/LaunchAgents" "$HOME/Library/Logs"

# 1. Get the agent. Three ways, never mixed:
#    - the files sit next to this script with a SHA256SUMS file: offline. Every file is checked, nothing is downloaded.
#    - the files sit next to this script without one (a git clone): copied as they are.
#    - otherwise (curl | sh): the latest release, each file checked against its checksum.
HERE=""
case "$0" in */install.sh|install.sh) HERE=$(cd "$(dirname "$0")" && pwd) ;; esac
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
if [ -n "$HERE" ] && [ -f "$HERE/playback_library.py" ] && [ -f "$HERE/SHA256SUMS" ]; then
  echo "Installing from the files next to install.sh. Nothing is downloaded."
  for f in playback_library.py install.sh; do
    [ -f "$HERE/$f" ] || continue
    WANT=$(awk -v n="$f" '$2 == n || $2 == "*" n {print $1}' "$HERE/SHA256SUMS")
    GOT=$(sha256 "$HERE/$f")
    if [ -z "$WANT" ] || [ "$WANT" != "$GOT" ]; then echo "Checksum for $f does not match SHA256SUMS. Nothing was installed."; exit 1; fi
  done
  cp "$HERE/playback_library.py" "$TMP/playback_library.py"
elif [ -n "$HERE" ] && [ -f "$HERE/playback_library.py" ]; then
  cp "$HERE/playback_library.py" "$TMP/playback_library.py"
else
  BASE="https://github.com/$REPO/releases/latest/download"
  curl -fsSL "$BASE/SHA256SUMS" -o "$TMP/SHA256SUMS" || { echo "Could not download the checksum file."; exit 1; }
  curl -fsSL "$BASE/playback_library.py" -o "$TMP/playback_library.py" || { echo "Download failed. Check the internet connection."; exit 1; }
  WANT=$(awk '$2 == "playback_library.py" {print $1}' "$TMP/SHA256SUMS")
  [ -n "$WANT" ] && [ "$WANT" = "$(sha256 "$TMP/playback_library.py")" ] || { echo "Checksum for playback_library.py does not match. Nothing was installed."; exit 1; }
fi
cp "$TMP/playback_library.py" "$APP/playback_library.py.new" && mv "$APP/playback_library.py.new" "$APP/playback_library.py"
# A manual install is the new truth: forget what an automatic update left (the old version kept as .prev, a half-finished swap,
# the "unproven" mark and the failure sentence), so the wrapper cannot put an older version back over this one.
rm -rf "$APP/update"
rm -f "$APP"/*.prev "$APP"/*.prev.tmp "$APP/update-pending" "$APP/update-failed" "$APP/run-state.json"

# 1b. The wrapper: launchd runs this small file, and it starts the agent. It finishes an update that was cut short and puts the old
#     version back when a new one will not start (docs/api.md, "Updates"). Only this installer writes it; an update never replaces it.
cat > "$APP/playback_library_run.py.new" <<'WRAPEOF'
#!/usr/bin/env python3
"""Starts the Playback Library agent, and puts the old version back when a freshly installed one will not start.

launchd runs this file (through the Playback Library launcher). The installer writes it and an update never replaces it, so
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
HELPER = os.path.join(APP, "playback_library.py")
FILES = ("playback_library.py",)                                # what an update replaces
GRACE_S = float(os.environ.get("PBL_WRAPPER_GRACE_S", "20"))   # "within 20 s" (tests shorten it)
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
mv "$APP/playback_library_run.py.new" "$APP/playback_library_run.py"

# 2. Config: keep an existing one; --allow, --allow-for, --allow-address and --client add to it. --key-file sets the key
#    and first clears the network limits (the key replaces them). The file is mode 600 because the key is in it.
(umask 077; [ -f "$APP/config.json" ] || echo '{}' > "$APP/config.json")
if [ -n "$ALLOW_LIST" ] || [ -n "$CLIENT" ] || [ -n "$KEY_FILE" ]; then
  $PY - "$APP/config.json" "$CLIENT" "$KEY_FILE" $ALLOW_LIST <<'PYEOF'
import json, os, sys, tempfile
path, client, keyfile = sys.argv[1], sys.argv[2], sys.argv[3]
try:
    with open(path) as f:
        cfg = json.load(f)
except ValueError:
    cfg = {}
if not isinstance(cfg, dict):
    cfg = {}
if keyfile:
    with open(keyfile) as f:
        cfg["key"] = f.readline().rstrip("\r\n")
    for limit in ("allow_interfaces", "allow_clients", "allowed_local_ips"):
        cfg.pop(limit, None)
for name in sys.argv[4:]:
    if name not in cfg.setdefault("allow_interfaces", []):
        cfg["allow_interfaces"].append(name)
if client and client not in cfg.setdefault("allow_clients", []):
    cfg["allow_clients"].append(client)
fd, tmp = tempfile.mkstemp(dir=os.path.dirname(path), prefix=".config.")      # created 600
with os.fdopen(fd, "w") as f:
    json.dump(cfg, f, indent=2)
os.replace(tmp, path)
PYEOF
fi
chmod 600 "$APP/config.json"
if [ -n "$KEY_FILE" ]; then
  rm -f "$KEY_FILE" || echo "Could not delete the key file $KEY_FILE. Delete it yourself."
  echo "Key authentication is on: any network path works, only a caller that signs with the key is answered, and the old network limits were cleared."
fi

# Ask the agent's /health. With a key in config.json the request is signed; the signature reaches curl through a
# header file, never the command line.
signed_health() {
  $PY - "$APP/config.json" "$TMP/health.hdr" <<'PYEOF'
import hashlib, hmac, json, os, sys, time
path, out = sys.argv[1], sys.argv[2]
try:
    with open(path) as f:
        key = json.load(f).get("key")
except Exception:
    key = None
with os.fdopen(os.open(out, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600), "w") as f:
    if isinstance(key, str) and len(key) >= 32:
        stamp = str(int(time.time()))
        sig = hmac.new(key.encode(), ("GET\n/health\n" + stamp).encode(), hashlib.sha256).hexdigest()
        f.write("X-PBL-Time: %s\nX-PBL-Sig: %s\n" % (stamp, sig))
PYEOF
  if [ -s "$TMP/health.hdr" ]; then
    curl -fsS -m 2 -H @"$TMP/health.hdr" "http://127.0.0.1:$PORT/health"
  else
    curl -fsS -m 2 "http://127.0.0.1:$PORT/health"
  fi
}

# 2b. The agent starts through a tiny signed app named "Playback Library". macOS asks about another app's data only
#     for something it can name; a bare python3 background job is refused without any question. If this step fails,
#     the agent still runs the old way, just without that question.
build_launcher() {
  LB=$(mktemp -d "${TMPDIR:-/tmp}/playback-library-launcher.XXXXXX") || return 1
  cat > "$LB/launcher.c" <<'CEOF'
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/wait.h>
#include <unistd.h>
static pid_t child = 0;
static void forward(int sig) { if (child > 0) kill(child, sig); }
int main(int argc, char **argv) {
    if (argc < 2) { fprintf(stderr, "usage: playback-library script [args...]\n"); return 2; }
    signal(SIGTERM, forward); signal(SIGINT, forward); signal(SIGHUP, forward);
    setenv("PLAYBACK_LIBRARY_LAUNCHER", "1", 1);
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
  <key>CFBundleIdentifier</key><string>io.github.playback-library</string>
  <key>CFBundleName</key><string>Playback Library</string>
  <key>CFBundleExecutable</key><string>playback-library</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSUIElement</key><true/>
  <key>NSLocalNetworkUsageDescription</key><string>Playback Library lets StagePilot on your network read the songs, keys and arrangements in Playback.</string>
</dict></plist>
PEOF
  cat "$LB/launcher.c" "$LB/Info.plist" > "$LB/both"
  NEWSTAMP=$(sha256 "$LB/both")
  if [ -x "$LAUNCHER_APP/Contents/MacOS/playback-library" ] && [ "$(cat "$APP/launcher.stamp" 2>/dev/null || true)" = "$NEWSTAMP" ]; then
    rm -rf "$LB"; return 0          # same launcher as last time: keep it, so macOS keeps the answers it was given
  fi
  mkdir -p "$LB/Playback Library.app/Contents/MacOS"
  "$CC" -O2 -o "$LB/Playback Library.app/Contents/MacOS/playback-library" "$LB/launcher.c" || { rm -rf "$LB"; return 1; }
  cp "$LB/Info.plist" "$LB/Playback Library.app/Contents/Info.plist"
  "$CODESIGN" --force --sign - "$LB/Playback Library.app" || { rm -rf "$LB"; return 1; }
  rm -rf "$LAUNCHER_APP"
  mv "$LB/Playback Library.app" "$LAUNCHER_APP"
  printf '%s' "$NEWSTAMP" > "$APP/launcher.stamp"
  rm -rf "$LB"
}
if build_launcher >"$APP/launcher-build.log" 2>&1; then
  START="$LAUNCHER_APP/Contents/MacOS/playback-library"
  START_ARGS="<string>$APP/playback_library_run.py</string>"
else
  echo "Could not set up the Playback Library launcher (see $APP/launcher-build.log). The agent will run without it."
  START="$PY"
  START_ARGS="<string>-u</string><string>$APP/playback_library_run.py</string>"
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
  <key>StandardErrorPath</key><string>$HOME/Library/Logs/playback-library.err</string>
  <key>StandardOutPath</key><string>$HOME/Library/Logs/playback-library.out</string>
</dict></plist>
EOF
plutil -lint "$PLIST" >/dev/null
UIDN=$(id -u)
echo
echo "macOS may ask to let Playback Library use your local network and read data from other apps (that is Playback's own song library). Click Allow. It asks again every time the agent starts (login, restart). Nothing is changed in Playback and no audio is read."
# Playback's data was refused before: forget that answer so macOS asks again instead of keeping the old No.
signed_health 2>/dev/null | $PY -c '
import json, sys
try:
    state = json.load(sys.stdin)["database"]
except Exception:
    state = ""
if state == "denied":
    print("SystemPolicyAppData")' 2>/dev/null | while read -r SERVICE; do
  "$TCCUTIL" reset "$SERVICE" "$LABEL" >/dev/null 2>&1 || true
done
launchctl bootout "gui/$UIDN/$LABEL" 2>/dev/null || true
STARTED=no
for _ in 1 2 3 4 5; do          # bootstrap right after bootout can fail with an I/O error while launchd is still removing the old job
  sleep 1
  if launchctl bootstrap "gui/$UIDN" "$PLIST" 2>/dev/null; then STARTED=yes; break; fi
done
[ "$STARTED" = yes ] || launchctl bootstrap "gui/$UIDN" "$PLIST"

# 4. Check that it answers, and wait up to 45 seconds for the answer to the data question.
UP=no
for _ in 1 2 3 4 5 6 7 8; do
  if curl -fsS -m 2 "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; then UP=yes; break; fi
  sleep 1
done
VER=$($PY "$APP/playback_library.py" --version)
DB=unknown
if [ "$UP" = yes ]; then
  for _ in $(seq 1 45); do
    DB=$(signed_health 2>/dev/null | $PY -c '
import json, sys
try:
    h = json.load(sys.stdin)
    print(h["database"] + " " + str(h.get("songs", "")))
except Exception:
    print("unknown")' 2>/dev/null || echo unknown)
    case "$DB" in "not read yet"*|waiting*|unknown*) sleep 1 ;; *) break ;; esac
  done
fi

echo
echo "playback-library $VER installed. Running: $UP"
case "$DB" in
  ok*) echo "  Playback's library: readable ($(echo "$DB" | awk '{print $2}') songs)" ;;
  denied*) echo "  Playback's library: macOS said no. Run this installer again to be asked once more, or open System Settings > Privacy & Security and allow Playback Library (Full Disk Access also works)." ;;
  missing*) echo "  Playback's library: not found. Open Playback once on this Mac and load a setlist, then check again." ;;
  waiting*) echo "  Playback's library: macOS has not answered yet. Click Allow on the question on screen; the agent keeps waiting and picks it up." ;;
  *) echo "  Playback's library: $DB (click Allow if macOS is asking)." ;;
esac
echo "  Try: curl http://127.0.0.1:$PORT/setlists/latest"
KEY_SET=$($PY -c '
import json, sys
try:
    with open(sys.argv[1]) as f:
        key = json.load(f).get("key")
except Exception:
    key = None
print("yes" if isinstance(key, str) and len(key) >= 32 else "no")' "$APP/config.json" 2>/dev/null || echo no)
if [ "$KEY_SET" = yes ]; then
  echo "  Key installed. Any network path works, but only a caller that signs with the key is answered."
elif ! grep -q allow_interfaces "$APP/config.json"; then
  echo "  Other computers get 'forbidden' until you allow a network interface or install a key. Run the installer again with"
  echo "  --key-file <file> (recommended), or --allow <interface> or --allow-for <ip of that computer>."
fi
[ "$UP" = yes ] || { echo "  It did not answer. See ~/Library/Logs/playback-library.log"; exit 1; }
