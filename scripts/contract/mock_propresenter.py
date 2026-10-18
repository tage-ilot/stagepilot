"""A deterministic mock of ProPresenter's HTTP API for the contract runs.

Both backends talk to this same server (Python through httpx, Rust through reqwest), so the
requests each one makes are compared as well as the answers it gives. It keeps real state: a
countdown timer that must be re-defined with PUT /v1/timer/<id>/reset, an "elapsed" decoy timer,
two saved Looks and a current Look. `GET /__log` returns (and clears) the request log.

Nothing here talks to a real ProPresenter. Run standalone:  python mock_propresenter.py PORT
"""

from __future__ import annotations

import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any

TIMER = {"uuid": "timer-uuid-1", "name": "Song Countdown", "index": 0}
DECOY = {"uuid": "timer-uuid-2", "name": "Message Timer", "index": 1}
LOOKS = [
    {"uuid": "look-worship", "name": "Worship", "index": 0},
    {"uuid": "look-default", "name": "Default", "index": 1},
]


class State:
    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.log: list[str] = []
        self.timer: dict[str, Any] = self._timer(60)
        self.run_state = "stopped"
        self.current_look = LOOKS[1]
        self.missing = False

    @staticmethod
    def _timer(duration: int) -> dict[str, Any]:
        return {
            "id": TIMER,
            "allows_overrun": False,
            "countdown": {"duration": duration},
        }

    def timer_view(self) -> dict[str, Any]:
        duration = self.timer.get("countdown", {}).get("duration", 0)
        return {
            **self.timer,
            "state": self.run_state,
            "time": f"{duration // 3600:02d}:{duration % 3600 // 60:02d}:{duration % 60:02d}",
        }

    def decoy_view(self) -> dict[str, Any]:
        return {
            "id": DECOY,
            "allows_overrun": False,
            "elapsed": {"start_time": 0},
            "state": "stopped",
            "time": "00:00:00",
        }


STATE = State()


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_args: object) -> None:  # silence
        return

    def _send(self, status: int, body: Any = None) -> None:
        data = b"" if body is None else json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def _body(self) -> Any:
        length = int(self.headers.get("Content-Length") or 0)
        return json.loads(self.rfile.read(length)) if length else None

    def _handle(self, method: str) -> None:
        path = self.path
        body = self._body()
        if path == "/__log":
            with STATE.lock:
                lines, STATE.log = STATE.log, []
            self._send(200, lines)
            return
        if path == "/__reset":
            with STATE.lock:
                STATE.__init__()
            self._send(204)
            return
        if path.startswith("/__missing/"):
            with STATE.lock:
                STATE.missing = path.endswith("/1")
            self._send(204)
            return
        with STATE.lock:
            entry = f"{method} {path}"
            if body is not None:
                entry += " " + json.dumps(body, sort_keys=True, separators=(",", ":"))
            STATE.log.append(entry)
            if method == "GET" and path == "/v1/timers":
                self._send(200, [] if STATE.missing else [STATE.timer_view(), STATE.decoy_view()])
            elif method == "GET" and path == "/v1/looks":
                self._send(200, [{"id": look} for look in LOOKS])
            elif method == "GET" and path == "/v1/look/current":
                self._send(200, {"id": STATE.current_look})
            elif method == "GET" and path.startswith("/v1/look/") and path.endswith("/trigger"):
                uuid = path.split("/")[3]
                look = next((item for item in LOOKS if item["uuid"] == uuid), None)
                if look is None:
                    self._send(404, {"error": "no such look"})
                else:
                    STATE.current_look = look
                    self._send(204)
            elif method == "GET" and path == f"/v1/timer/{TIMER['uuid']}":
                self._send(200, STATE.timer_view())
            elif method == "PUT" and path == f"/v1/timer/{TIMER['uuid']}/reset":
                STATE.timer = dict(body)
                STATE.run_state = "stopped"
                self._send(204)
            elif method == "GET" and path == f"/v1/timer/{TIMER['uuid']}/start":
                STATE.run_state = "running"
                self._send(204)
            elif method == "GET" and path == f"/v1/timer/{TIMER['uuid']}/stop":
                STATE.run_state = "stopped"
                self._send(204)
            elif method == "GET" and path == f"/v1/timer/{TIMER['uuid']}/reset":
                STATE.run_state = "stopped"
                self._send(204)
            else:
                self._send(404, {"error": "unknown endpoint"})

    def do_GET(self) -> None:  # noqa: N802
        self._handle("GET")

    def do_PUT(self) -> None:  # noqa: N802
        self._handle("PUT")

    def do_POST(self) -> None:  # noqa: N802
        self._handle("POST")


def start(port: int = 0) -> tuple[ThreadingHTTPServer, int]:
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, server.server_address[1]


if __name__ == "__main__":
    server, port = start(int(sys.argv[1]))
    print(f"mock ProPresenter on 127.0.0.1:{port}", flush=True)
    threading.Event().wait()
