"""A deterministic fake of the `lightkey-api` agent for the contract runs.

Both backends poll this same server (Python through httpx, Rust through reqwest). It reports one
active cue set and a sequence number, and answers `GET /time` and `GET /log?since=N` in the shape the
real agent uses (docs/archive/rust-handoff/lightkey-record-replay.md). Control endpoints for the runner:

  GET /__reset                       back to the starting set ("Warm"), seq 1, no events
  GET /__change?add=A,B&remove=C     change the active set (one event with a DMX timestamp)

Nothing here talks to a real Lightkey. Run standalone:  python mock_lightkey.py PORT
"""

from __future__ import annotations

import json
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any
from urllib.parse import parse_qs, urlparse

START = ["Warm"]


class State:
    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.active = list(START)
        self.seq = 1
        self.events: list[dict[str, Any]] = []


STATE = State()


def _now_ms() -> float:
    return time.time() * 1000.0


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

    def do_GET(self) -> None:  # noqa: N802
        url = urlparse(self.path)
        query = parse_qs(url.query)
        with STATE.lock:
            if url.path == "/__reset":
                STATE.__init__()
                self._send(204)
            elif url.path == "/__change":
                add = [n for n in query.get("add", [""])[0].split(",") if n]
                remove = [n for n in query.get("remove", [""])[0].split(",") if n]
                STATE.active = sorted((set(STATE.active) - set(remove)) | set(add))
                STATE.seq += 1
                STATE.events.append(
                    {
                        "seq": STATE.seq,
                        "online": True,
                        "added": sorted(add),
                        "removed": sorted(remove),
                        "effective_ms": _now_ms(),
                        "effective_src": "dmx",
                    }
                )
                self._send(204)
            elif url.path == "/time":
                self._send(200, {"t": _now_ms()})
            elif url.path == "/log":
                since = int(query.get("since", ["0"])[0])
                self._send(
                    200,
                    {
                        "events": [e for e in STATE.events if e["seq"] > since],
                        "state": {
                            "online": True,
                            "active": STATE.active,
                            "seq": STATE.seq,
                            "server_time": _now_ms(),
                        },
                    },
                )
            else:
                self._send(404, {"error": "unknown endpoint"})


def start(port: int = 0) -> tuple[ThreadingHTTPServer, int]:
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, server.server_address[1]


if __name__ == "__main__":
    server, port = start(int(sys.argv[1]))
    print(f"mock lightkey-api on 127.0.0.1:{port}", flush=True)
    threading.Event().wait()
