#!/bin/sh
# Install propresenter-link for the current user, on the Mac that runs ProPresenter.
#   sh install.sh --key-file /path/to/key.txt --allow-for 192.168.1.20
#       --key-file    the first line of that file is the key StagePilot made; the file is deleted once the key is stored.
#       --allow-for   the address of the StagePilot computer: the only computer that is answered (and only when it signs with the key).
#   sh install.sh --allow-loopback   also answer programs on this Mac itself (off unless you ask for it)
# Offline: put install.sh, propresenter_link.py and SHA256SUMS in one folder and run sh install.sh there. Every file is
# checked against SHA256SUMS and nothing is fetched. Safe to re-run: it upgrades the agent and keeps your config.
set -eu

APP="$HOME/Library/Application Support/ProPresenterLink"
PLIST="$HOME/Library/LaunchAgents/io.stagepilot.propresenter-link.plist"
LABEL=io.stagepilot.propresenter-link
PORT=8790
PY=${PROPRESENTER_LINK_PY:-/usr/bin/python3}     # PROPRESENTER_LINK_* exist so the tests can stub the Mac
CC=${PROPRESENTER_LINK_CC:-/usr/bin/clang}
CODESIGN=${PROPRESENTER_LINK_CODESIGN:-/usr/bin/codesign}
LAUNCHER_APP="$APP/ProPresenter Link.app"        # macOS names the app in its questions after this folder name
KEY_FILE=""
ALLOW_FOR=""
ALLOW_LOOPBACK=""

need() { [ $# -ge 2 ] || { echo "$1 needs a value."; exit 1; }; }
while [ $# -gt 0 ]; do
  case "$1" in
    --allow-for) need "$@"; ALLOW_FOR="$2"; shift 2 ;;
    --allow-loopback) ALLOW_LOOPBACK=yes; shift ;;
    --key-file) need "$@"; KEY_FILE="$2"; shift 2 ;;     # a file, never the key itself: a command line shows up in ps
    -h|--help) sed -n '2,8p' "$0" 2>/dev/null || true; exit 0 ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

# --allow-for must be one address (IPv4 or IPv6), nothing else: it is written to the config as the only computer that is answered.
if [ -n "$ALLOW_FOR" ]; then
  case "$ALLOW_FOR" in
    *[!0-9A-Fa-f.:]*) echo "--allow-for takes the address of the StagePilot computer, for example 192.168.1.20."; exit 1 ;;
  esac
fi

sha256() { if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'; else sha256sum "$1" | awk '{print $1}'; fi; }

[ "${PROPRESENTER_LINK_UNAME:-$(uname)}" = Darwin ] || { echo "This installs on macOS, on the computer that runs ProPresenter."; exit 1; }
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
  [ -n "$ALLOW_FOR" ] || { echo "Say which computer may use it: add --allow-for <the StagePilot computer's address>. Nothing was installed."; exit 1; }
fi

mkdir -p "$APP" "$HOME/Library/LaunchAgents" "$HOME/Library/Logs"

# 1. Get the agent: the files sit next to this script (with a SHA256SUMS file: every file is checked) and nothing is downloaded.
HERE=""
case "$0" in */install.sh|install.sh) HERE=$(cd "$(dirname "$0")" && pwd) ;; esac
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
[ -n "$HERE" ] && [ -f "$HERE/propresenter_link.py" ] || { echo "propresenter_link.py is not next to install.sh. Run the setup command StagePilot shows."; exit 1; }
if [ -f "$HERE/SHA256SUMS" ]; then
  echo "Installing from the files next to install.sh. Nothing is downloaded."
  for f in propresenter_link.py install.sh; do
    [ -f "$HERE/$f" ] || continue
    WANT=$(awk -v n="$f" '$2 == n || $2 == "*" n {print $1}' "$HERE/SHA256SUMS")
    GOT=$(sha256 "$HERE/$f")
    if [ -z "$WANT" ] || [ "$WANT" != "$GOT" ]; then echo "Checksum for $f does not match SHA256SUMS. Nothing was installed."; exit 1; fi
  done
fi
cp "$HERE/propresenter_link.py" "$TMP/propresenter_link.py"
cp "$TMP/propresenter_link.py" "$APP/propresenter_link.py.new" && mv "$APP/propresenter_link.py.new" "$APP/propresenter_link.py"
# A manual install is the new truth: forget what an automatic update left (the old version kept as .prev, a half-finished swap,
# the "unproven" mark and the failure sentence), so the wrapper cannot put an older version back over this one.
rm -rf "$APP/update"
rm -f "$APP"/*.prev "$APP"/*.prev.tmp "$APP/update-pending" "$APP/update-failed" "$APP/run-state.json"

# 1b. The wrapper: launchd runs this small file, and it starts the agent. It finishes an update that was cut short and puts the old
#     version back when a new one will not start (docs/api.md, "Updates"). Only this installer writes it; an update never replaces it.
cat > "$APP/propresenter_link_run.py.new" <<'WRAPEOF'
#!/usr/bin/env python3
"""Starts the ProPresenter Link agent, and puts the old version back when a freshly installed one will not start.

launchd runs this file (through the ProPresenter Link launcher). The installer writes it and an update never replaces it, so
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
HELPER = os.path.join(APP, "propresenter_link.py")
FILES = ("propresenter_link.py",)                                # what an update replaces
GRACE_S = float(os.environ.get("PPL_WRAPPER_GRACE_S", "20"))   # "within 20 s" (tests shorten it)
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
mv "$APP/propresenter_link_run.py.new" "$APP/propresenter_link_run.py"

# 2. Config: keep an existing one. --key-file sets the key and the computer that is answered (--allow-for) and clears older
#    limits; --allow-loopback lets programs on this Mac ask too. The file is mode 600 because the key is in it.
(umask 077; [ -f "$APP/config.json" ] || echo '{}' > "$APP/config.json")
if [ -n "$ALLOW_FOR" ] || [ -n "$KEY_FILE" ] || [ -n "$ALLOW_LOOPBACK" ]; then
  $PY - "$APP/config.json" "$ALLOW_FOR" "$KEY_FILE" "$ALLOW_LOOPBACK" <<'PYEOF'
import json, os, sys, tempfile
path, client, keyfile, loopback = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
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
    cfg["allow_clients"] = []                      # a new key means a new setup: the answered computer is the one named now
    cfg.pop("allow_loopback", None)
if client and client not in cfg.setdefault("allow_clients", []):
    cfg["allow_clients"].append(client)
if loopback:
    cfg["allow_loopback"] = True
fd, tmp = tempfile.mkstemp(dir=os.path.dirname(path), prefix=".config.")      # created 600
with os.fdopen(fd, "w") as f:
    json.dump(cfg, f, indent=2)
os.replace(tmp, path)
PYEOF
fi
chmod 600 "$APP/config.json"
if [ -n "$KEY_FILE" ]; then
  rm -f "$KEY_FILE" || echo "Could not delete the key file $KEY_FILE. Delete it yourself."
  echo "Key authentication is on: only $ALLOW_FOR is answered, and only when it signs with the key."
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
        f.write("X-PPL-Time: %s\nX-PPL-Sig: %s\n" % (stamp, sig))
PYEOF
  if [ -s "$TMP/health.hdr" ]; then
    curl -fsS -m 2 -H @"$TMP/health.hdr" "http://127.0.0.1:$PORT/health"
  else
    curl -fsS -m 2 "http://127.0.0.1:$PORT/health"
  fi
}

# 2b. The agent starts through a tiny signed app named "ProPresenter Link". macOS asks about another app's data only
#     for something it can name; a bare python3 background job is refused without any question. If this step fails,
#     the agent still runs the old way, just without that question.
build_launcher() {
  LB=$(mktemp -d "${TMPDIR:-/tmp}/propresenter-link-launcher.XXXXXX") || return 1
  cat > "$LB/launcher.c" <<'CEOF'
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/wait.h>
#include <unistd.h>
static pid_t child = 0;
static void forward(int sig) { if (child > 0) kill(child, sig); }
int main(int argc, char **argv) {
    if (argc < 2) { fprintf(stderr, "usage: propresenter-link script [args...]\n"); return 2; }
    signal(SIGTERM, forward); signal(SIGINT, forward); signal(SIGHUP, forward);
    setenv("PROPRESENTER_LINK_LAUNCHER", "1", 1);
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
  <key>CFBundleIdentifier</key><string>io.stagepilot.propresenter-link</string>
  <key>CFBundleName</key><string>ProPresenter Link</string>
  <key>CFBundleExecutable</key><string>propresenter-link</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSUIElement</key><true/>
  <key>NSLocalNetworkUsageDescription</key><string>ProPresenter Link lets StagePilot on your network read ProPresenter's song files, add new ones to the Playback library and ask ProPresenter to import a song.</string>
</dict></plist>
PEOF
  cat "$LB/launcher.c" "$LB/Info.plist" > "$LB/both"
  NEWSTAMP=$(sha256 "$LB/both")
  if [ -x "$LAUNCHER_APP/Contents/MacOS/propresenter-link" ] && [ "$(cat "$APP/launcher.stamp" 2>/dev/null || true)" = "$NEWSTAMP" ]; then
    rm -rf "$LB"; return 0          # same launcher as last time: keep it, so macOS keeps the answers it was given
  fi
  mkdir -p "$LB/ProPresenter Link.app/Contents/MacOS"
  "$CC" -O2 -o "$LB/ProPresenter Link.app/Contents/MacOS/propresenter-link" "$LB/launcher.c" || { rm -rf "$LB"; return 1; }
  cp "$LB/Info.plist" "$LB/ProPresenter Link.app/Contents/Info.plist"
  "$CODESIGN" --force --sign - "$LB/ProPresenter Link.app" || { rm -rf "$LB"; return 1; }
  rm -rf "$LAUNCHER_APP"
  mv "$LB/ProPresenter Link.app" "$LAUNCHER_APP"
  printf '%s' "$NEWSTAMP" > "$APP/launcher.stamp"
  rm -rf "$LB"
}
if build_launcher >"$APP/launcher-build.log" 2>&1; then
  START="$LAUNCHER_APP/Contents/MacOS/propresenter-link"
  START_ARGS="<string>$APP/propresenter_link_run.py</string>"
else
  echo "Could not set up the ProPresenter Link launcher (see $APP/launcher-build.log). The agent will run without it."
  START="$PY"
  START_ARGS="<string>-u</string><string>$APP/propresenter_link_run.py</string>"
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
  <key>StandardErrorPath</key><string>$HOME/Library/Logs/propresenter-link.err</string>
  <key>StandardOutPath</key><string>$HOME/Library/Logs/propresenter-link.out</string>
</dict></plist>
EOF
plutil -lint "$PLIST" >/dev/null
UIDN=$(id -u)
echo
echo "macOS may ask to let ProPresenter Link use your local network and your Documents folder (that is ProPresenter's own song folder). Click Allow."
echo "To let StagePilot import a song through ProPresenter's Search, also open System Settings > Privacy & Security > Accessibility and switch on ProPresenter Link."
launchctl bootout "gui/$UIDN/$LABEL" 2>/dev/null || true
STARTED=no
for _ in 1 2 3 4 5; do          # bootstrap right after bootout can fail with an I/O error while launchd is still removing the old job
  sleep 1
  if launchctl bootstrap "gui/$UIDN" "$PLIST" 2>/dev/null; then STARTED=yes; break; fi
done
[ "$STARTED" = yes ] || launchctl bootstrap "gui/$UIDN" "$PLIST"

# 4. Check that it is listening. From this Mac any answer counts, also a refusal: loopback is refused unless --allow-loopback was given.
UP=no
for _ in 1 2 3 4 5 6 7 8; do
  CODE=$(curl -s -m 2 -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/health" 2>/dev/null || true)
  case "$CODE" in 000|"") ;; *) UP=yes; break ;; esac
  sleep 1
done
VER=$($PY "$APP/propresenter_link.py" --version)
echo
echo "propresenter-link $VER installed. Running: $UP"
KEY_SET=$($PY -c '
import json, sys
try:
    with open(sys.argv[1]) as f:
        cfg = json.load(f)
    key = cfg.get("key")
    who = cfg.get("allow_clients") or []
except Exception:
    key, who = None, []
print("yes" if isinstance(key, str) and len(key) >= 32 and who else "no")' "$APP/config.json" 2>/dev/null || echo no)
if [ "$KEY_SET" = yes ]; then
  echo "  Key installed. Only the StagePilot computer you named is answered, and only when it signs with the key."
else
  echo "  No key or no StagePilot computer is set yet, so nothing is answered. Run the setup command from StagePilot."
fi
echo "  ProPresenter's folder: ${PROPRESENTER_LINK_FOLDER:-$HOME/Documents/ProPresenter}"
[ "$UP" = yes ] || { echo "  It did not answer. See ~/Library/Logs/propresenter-link.log"; exit 1; }
