"""A deterministic mock of the control plane's diagnostics upload route (contract runs).

Both backends upload their diagnostic bundle here (Python through httpx, Rust through reqwest) and
the requests each one makes are compared as well as its answers. `GET /__log` returns (and clears)
the request log. A bundle containing `MODE:429` is answered 429 and one containing `MODE:500` is
answered 500; anything else is 201.

Nothing here talks to a real control plane. Run standalone:  python mock_control.py PORT
"""

from __future__ import annotations

import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any

CREDENTIAL = "spi_abcd1234." + "s" * 43


class State:
    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.log: list[dict[str, Any]] = []


STATE = State()


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_args: Any) -> None:
        pass

    def _send(self, status: int, body: Any) -> None:
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self) -> None:  # noqa: N802
        if self.path == "/__log":
            with STATE.lock:
                log, STATE.log = STATE.log, []
            self._send(200, log)
        else:
            self._send(404, {"error": "unknown endpoint"})

    def do_POST(self) -> None:  # noqa: N802
        length = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(length)
        text = raw.decode("utf-8", errors="replace")
        try:
            body: Any = json.loads(text)
        except ValueError:
            body = text
        auth = self.headers.get("Authorization")
        entry = {
            "method": "POST",
            "path": self.path,
            "authorization": "bearer-ok" if auth == f"Bearer {CREDENTIAL}" else "bearer-other",
            "content_type": self.headers.get("Content-Type"),
            "body": body,
        }
        with STATE.lock:
            STATE.log.append(entry)
        if self.path != "/v1/installations/abcd1234/diagnostics":
            self._send(404, {"error": "not found"})
        elif "MODE:429" in text:
            self._send(429, {"error": "slow down"})
        elif "MODE:500" in text:
            self._send(500, {"error": "server error"})
        else:
            self._send(201, {"ok": True})


def start(port: int = 0) -> tuple[ThreadingHTTPServer, int]:
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, server.server_address[1]


if __name__ == "__main__":
    server, port = start(int(sys.argv[1]))
    print(f"mock control plane on 127.0.0.1:{port}", flush=True)
    threading.Event().wait()
