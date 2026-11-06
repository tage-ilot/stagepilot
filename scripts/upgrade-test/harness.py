"""Small stdlib helpers for the upgrade test: start/stop a headless build and call its HTTP API.

Loopback only, free ports, one scratch directory per scenario. Nothing here touches the
preview's files or ports (18766 / 8101) or any rig.
"""
import http.cookiejar
import json
import os
import signal
import socket
import subprocess
import time
import urllib.error
import urllib.request

FORBIDDEN_PORTS = {18766, 8100, 8101, 8765, 8770, 21600, 21700}
KEYRING_SERVICE = "StagePilotUpgradeTest"


def free_port(kind=socket.SOCK_STREAM):
    while True:
        with socket.socket(socket.AF_INET, kind) as s:
            s.bind(("127.0.0.1", 0))
            port = s.getsockname()[1]
        if port not in FORBIDDEN_PORTS:
            return port


class Server:
    """One headless process on 127.0.0.1 with its own settings file and data dir."""

    def __init__(self, label, binary, workdir, port=None, extra_env=None):
        self.label = label
        self.binary = binary
        self.dir = workdir
        os.makedirs(workdir, exist_ok=True)
        self.settings_path = os.path.join(workdir, "settings.json")
        self.port = port or free_port()
        self.base = f"http://127.0.0.1:{self.port}"
        self.log_path = os.path.join(workdir, f"{label}.log")
        self.extra_env = extra_env or {}
        self.proc = None
        self.jar = http.cookiejar.CookieJar()
        self.opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(self.jar))

    def env(self):
        env = {k: v for k, v in os.environ.items() if not k.startswith("STAGEPILOT_")}
        env.update(
            STAGEPILOT_HOST="127.0.0.1",
            STAGEPILOT_PORT=str(self.port),
            STAGEPILOT_SERVICE_SOURCE="demo",
            STAGEPILOT_MIDI_SOURCE="simulated",
            STAGEPILOT_TIMER_OUTPUT="simulated",
            STAGEPILOT_SETTINGS_PATH=self.settings_path,
            STAGEPILOT_DATA_DIR=self.dir,
            STAGEPILOT_KEYRING_SERVICE=KEYRING_SERVICE,
            STAGEPILOT_LOG_LEVEL="info",
            HOME=self.dir,  # never read the real ~/.config/StagePilot
        )
        env.update(self.extra_env)
        return env

    def start(self, wait=20.0):
        assert self.port not in FORBIDDEN_PORTS
        self.log = open(self.log_path, "ab")
        self.proc = subprocess.Popen(
            [self.binary], env=self.env(), stdout=self.log, stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        deadline = time.time() + wait
        while time.time() < deadline:
            if self.proc.poll() is not None:
                return False
            try:
                self.opener.open(self.base + "/api/v1/health", timeout=1).read()
                return True
            except Exception:
                time.sleep(0.2)
        return False

    def stop(self):
        if self.proc and self.proc.poll() is None:
            self.proc.send_signal(signal.SIGTERM)
            try:
                self.proc.wait(10)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()
        code = self.proc.returncode if self.proc else None
        if getattr(self, "log", None):
            self.log.close()
        self.proc = None
        return code

    def call(self, method, path, body=None, headers=None):
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(self.base + path, data=data, method=method)
        if data is not None:
            req.add_header("Content-Type", "application/json")
        # Mutating routes want a same-origin browser; say so the way the web UI does.
        req.add_header("Origin", self.base)
        for k, v in (headers or {}).items():
            req.add_header(k, v)
        try:
            with self.opener.open(req, timeout=15) as r:
                raw = r.read().decode()
                status = r.status
        except urllib.error.HTTPError as e:
            raw = e.read().decode()
            status = e.code
        try:
            return status, json.loads(raw)
        except ValueError:
            return status, raw

    def log_text(self):
        try:
            return open(self.log_path, errors="replace").read()
        except OSError:
            return ""

    def log_issues(self, since=0):
        """WARN/ERROR log lines after `since` bytes, as short strings."""
        out = []
        for line in self.log_text()[since:].splitlines():
            try:
                rec = json.loads(line)
            except ValueError:
                if line.strip():
                    out.append(line.strip()[:300])
                continue
            if rec.get("level") in ("WARN", "ERROR"):
                f = rec.get("fields", {})
                extra = " ".join(f"{k}={v}" for k, v in f.items() if k != "message")
                extra = extra.replace(self.dir, "<scratch>")
                out.append(f"{rec['level']} {f.get('message', '')} {extra}"[:600])
        return out
