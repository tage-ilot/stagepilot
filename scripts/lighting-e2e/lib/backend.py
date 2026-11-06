"""Start the headless backend (loopback, temp settings/data) and talk to it over its HTTP API only."""
from __future__ import annotations

import json
import os
import subprocess
import time
import urllib.error
import urllib.request
from pathlib import Path

CLK = os.sysconf("SC_CLK_TCK")


class Backend:
    def __init__(self, binary: Path, workdir: Path, port: int, settings: dict, log_name="backend.log"):
        self.binary, self.dir, self.port = Path(binary), Path(workdir), port
        self.dir.mkdir(parents=True, exist_ok=True)
        assert port not in (18766, 8101, 8100), "never the preview's ports"
        self.settings_path = self.dir / "settings.json"
        if not self.settings_path.exists():  # keep saved cue maps when a stage reuses the directory
            self.settings_path.write_text(json.dumps(settings, indent=1))
        self.log_path = self.dir / log_name
        self.proc: subprocess.Popen | None = None
        self.base = f"http://127.0.0.1:{port}"

    def start(self):
        env = {k: v for k, v in os.environ.items() if not k.startswith("STAGEPILOT_")}
        env.update({
            "STAGEPILOT_SETTINGS_PATH": str(self.settings_path),
            "STAGEPILOT_DATA_DIR": str(self.dir / "data"),
            "STAGEPILOT_HOST": "127.0.0.1", "STAGEPILOT_PORT": str(self.port),
            "HOME": str(self.dir / "home"), "XDG_CONFIG_HOME": str(self.dir / "home" / ".config"),
            "RUST_LOG": "info",
        })
        (self.dir / "home").mkdir(exist_ok=True)
        self.log = open(self.log_path, "ab")
        self.proc = subprocess.Popen([str(self.binary)], env=env, stdout=self.log, stderr=subprocess.STDOUT)
        for _ in range(100):
            try:
                self.get("/api/v1/health/live")
                return
            except Exception:
                time.sleep(0.1)
        raise RuntimeError("backend did not start; see " + str(self.log_path))

    def stop(self, kill=False):
        if not self.proc:
            return
        self.proc.kill() if kill else self.proc.terminate()
        try:
            self.proc.wait(15)
        except subprocess.TimeoutExpired:
            self.proc.kill()
        self.proc = None

    # ---- HTTP
    def call(self, method, path, body=None):
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(self.base + path, data=data, method=method,
                                     headers={"Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=20) as r:
                raw = r.read()
        except urllib.error.HTTPError as e:
            raise RuntimeError(f"{method} {path} -> {e.code} {e.read()[:300]!r}")
        return json.loads(raw) if raw else None

    def get(self, path):
        return self.call("GET", path)

    def post(self, path, body=None):
        return self.call("POST", path, {} if body is None else body)

    def put(self, path, body):
        return self.call("PUT", path, body)

    # ---- process stats (/proc)
    def stats(self) -> dict:
        pid = self.proc.pid
        status = dict(l.split(":\t", 1) for l in Path(f"/proc/{pid}/status").read_text().splitlines() if ":\t" in l)
        stat = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
        return {
            "rss_kb": int(status["VmRSS"].split()[0]), "threads": int(status["Threads"]),
            "fds": len(os.listdir(f"/proc/{pid}/fd")),
            "cpu_s": (int(stat[11]) + int(stat[12])) / CLK, "t": time.time(),
        }

    def log_lines(self):
        return self.log_path.read_text(errors="replace").splitlines()
