"""A deterministic mock of the Planning Center Services API and the StagePilot control-plane OAuth
relay, for the contract runs.

Both backends talk to this same server (Python through httpx with a host-rewriting transport, Rust
through reqwest with a configured base URL), so the requests each one makes are compared as well as
the answers it gives. Nothing here talks to the real Planning Center or the real control plane.

  GET  /__log     return (and clear) the request log: method, path, sorted query, auth kind
  POST /__mode    {"status": {"<path prefix>": 429}, "retry_after": "7", "delay": {"<prefix>": 1.5},
                   "garbage": ["<prefix>"], "expires_in": 60, "revoke_refresh": true}
                  -> fault injection; {} clears it
  POST /__reset   clear mode, written lengths and the log

Run standalone:  python mock_pco.py PORT
"""

from __future__ import annotations

import base64
import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any
from urllib.parse import parse_qsl, urlsplit

APP_ID, SECRET = "app-1", "secret-1"
TOKENS = {
    "access-1": "refresh-1",
    "access-2": "refresh-2",
}
FLOW = {"flow_id": "aabbccdd", "ticket": "ticket-1"}
FIRST_TOKENS = {
    "access_token": "access-1",
    "refresh_token": "refresh-1",
    "expires_in": 7200,
    "scope": "services",
    "token_type": "Bearer",
}
REFRESHED_TOKENS = {**FIRST_TOKENS, "access_token": "access-2", "refresh_token": "refresh-2"}

SERVICE_TYPES = [
    {"id": "111", "name": "Weekend Services", "sequence": 0},
    {"id": "222", "name": "Youth", "sequence": 1},
    {"id": "333", "name": "Old Archived", "sequence": 2, "archived_at": "2025-01-01T00:00:00Z"},
]
# Plans per service type. Today is 2026-10-18 (a Sunday) in America/Los_Angeles.
PLANS = {
    "111": [
        {"id": "1001", "title": "Sunday Worship", "dates": "October 18, 2026"},
        {"id": "1002", "title": "Next Month", "dates": "November 1, 2026"},
    ],
    "222": [{"id": "2001", "title": "Youth Night", "dates": "October 18, 2026"}],
    "333": [],
}
PLAN_TIMES = {
    "1001": [("5001", "2026-10-18T16:00:00Z", "service"), ("5000", "2026-10-18T14:00:00Z", "rehearsal")],
    "1002": [("5002", "2026-11-01T17:00:00Z", "service")],
    "2001": [("5003", "2026-10-19T02:00:00Z", "service")],
}
ITEMS = {
    "1001": [
        ("9001", "header", "Welcome", 1, None, None),
        ("9002", "song", "Amazing Grace", 2, 240, "7001"),
        ("9003", "song", "Holy Forever", 3, 336, "7002"),
        ("9004", "media", "Welcome Video", 4, 90, None),
        ("9005", "item", "Prayer", 5, 120, None),
        ("9006", "song", "  ", 6, 60, "7003"),
    ],
    "1002": [("9101", "song", "Future Song", 1, 200, "7101")],
    "2001": [("9201", "song", "Youth Song", 1, 300, "7201")],
}


def _item(item: tuple[Any, ...], length_override: dict[str, int]) -> dict[str, Any]:
    item_id, item_type, title, sequence, length, song = item
    data: dict[str, Any] = {
        "type": "Item",
        "id": item_id,
        "attributes": {
            "title": title,
            "item_type": item_type,
            "sequence": sequence,
            "length": length_override.get(item_id, length),
            "description": None,
        },
        "relationships": {"song": {"data": {"type": "Song", "id": song} if song else None}},
    }
    return data


class State:
    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.log: list[dict[str, Any]] = []
        self.mode: dict[str, Any] = {}
        self.lengths: dict[str, int] = {}


def _auth_kind(header: str | None) -> str:
    if not header:
        return "none"
    scheme, _, value = header.partition(" ")
    if scheme == "Basic":
        try:
            user, _, password = base64.b64decode(value).decode().partition(":")
        except ValueError:
            return "basic-invalid"
        return "basic-ok" if (user, password) == (APP_ID, SECRET) else "basic-wrong"
    if scheme == "Bearer":
        return f"bearer:{value}" if value in TOKENS else "bearer-unknown"
    return "other"


def make_handler(state: State) -> type[BaseHTTPRequestHandler]:
    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, format: str, *args: Any) -> None:  # noqa: A002 - silence
            return

        def _send(self, status: int, body: Any, headers: dict[str, str] | None = None) -> None:
            payload = json.dumps(body).encode() if not isinstance(body, bytes) else body
            try:
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(payload)))
                for key, value in (headers or {}).items():
                    self.send_header(key, value)
                self.end_headers()
                self.wfile.write(payload)
            except (BrokenPipeError, ConnectionResetError):
                return  # the client gave up (the timeout scenario)

        def _body(self) -> dict[str, Any]:
            length = int(self.headers.get("Content-Length") or 0)
            raw = self.rfile.read(length) if length else b""
            try:
                parsed = json.loads(raw) if raw else {}
            except ValueError:
                return {}
            return parsed if isinstance(parsed, dict) else {}

        def _entry(self, path: str, query: list[tuple[str, str]], body: dict[str, Any] | None) -> None:
            entry: dict[str, Any] = {
                "method": self.command,
                "path": path,
                "query": dict(sorted(query)),
                "auth": _auth_kind(self.headers.get("Authorization")),
            }
            if path.startswith("/services/"):
                entry["api_version"] = self.headers.get("X-PCO-API-Version")
                entry["accept"] = self.headers.get("Accept")
            if body is not None:
                allowed = {"flow_id", "ticket", "redirect_uri", "refresh_token_present", "code"}
                safe = {k: v for k, v in body.items() if k in allowed}
                if "refresh_token" in body:
                    safe["refresh_token_present"] = bool(body["refresh_token"])
                if "code_verifier" in body:
                    safe["code_verifier_length"] = len(str(body["code_verifier"]))
                entry["body"] = dict(sorted(safe.items()))
                entry["body_keys"] = sorted(body)
            with state.lock:
                state.log.append(entry)

        def _fault(self, path: str) -> bool:
            import time

            mode = state.mode
            for prefix, seconds in mode.get("delay", {}).items():
                if path.startswith(prefix):
                    time.sleep(float(seconds))
            for prefix in mode.get("garbage", []):
                if path.startswith(prefix):
                    self._send(200, b"not json at all")
                    return True
            for prefix, status in mode.get("status", {}).items():
                if path.startswith(prefix):
                    headers = {"Retry-After": str(mode["retry_after"])} if mode.get("retry_after") else {}
                    self._send(int(status), {"errors": [{"title": "injected"}]}, headers)
                    return True
            return False

        def do_GET(self) -> None:  # noqa: N802
            split = urlsplit(self.path)
            path, query = split.path, parse_qsl(split.query, keep_blank_values=True)
            if path == "/__log":
                with state.lock:
                    log, state.log = state.log, []
                self._send(200, log)
                return
            self._entry(path, query, None)
            if self._fault(path):
                return
            self._api(path, dict(query))

        def do_PATCH(self) -> None:  # noqa: N802
            split = urlsplit(self.path)
            body = self._body()
            self._entry(split.path, parse_qsl(split.query), None)
            if self._fault(split.path):
                return
            parts = split.path.strip("/").split("/")
            if len(parts) == 8 and parts[:2] == ["services", "v2"] and parts[6] == "items":
                item_id = parts[7]
                length = body.get("data", {}).get("attributes", {}).get("length")
                if isinstance(length, int) and self._auth_ok():
                    with state.lock:
                        state.lengths[item_id] = length
                    self._api(split.path, {})
                    return
            self._send(404, {"errors": [{"title": "not found"}]})

        def do_POST(self) -> None:  # noqa: N802
            split = urlsplit(self.path)
            path = split.path
            body = self._body()
            if path == "/__mode":
                with state.lock:
                    state.mode = body
                self._send(200, {})
                return
            if path == "/__reset":
                with state.lock:
                    state.mode, state.lengths, state.log = {}, {}, []
                self._send(200, {})
                return
            self._entry(path, parse_qsl(split.query), body)
            if self._fault(path):
                return
            if path == "/v1/planning-center/oauth/flow":
                self._send(201, FLOW)
            elif path == "/v1/planning-center/oauth/token":
                code = body.get("code")
                if code == "good-code":
                    expires = state.mode.get("expires_in")
                    self._send(200, {**FIRST_TOKENS, **({"expires_in": expires} if expires else {})})
                elif code == "bad-grant":
                    self._send(400, {"error": "invalid_grant"})
                elif code == "busy":
                    self._send(429, {"error": "slow_down"})
                else:
                    self._send(500, {"error": "server_error"})
            elif path == "/v1/planning-center/oauth/refresh":
                if state.mode.get("revoke_refresh"):
                    self._send(400, {"error": "invalid_grant"})
                elif body.get("refresh_token") == "refresh-1":
                    self._send(200, REFRESHED_TOKENS)
                else:
                    self._send(400, {"error": "invalid_grant"})
            else:
                self._send(404, {"error": "not found"})

        def _auth_ok(self) -> bool:
            kind = _auth_kind(self.headers.get("Authorization"))
            return kind == "basic-ok" or kind.startswith("bearer:")

        def _api(self, path: str, query: dict[str, str]) -> None:
            if not self._auth_ok():
                self._send(401, {"errors": [{"title": "Unauthorized"}]})
                return
            parts = path.strip("/").split("/")
            if parts[:3] == ["services", "v2", "service_types"]:
                if len(parts) == 3:
                    self._service_types(query)
                    return
                service_type = parts[3]
                if len(parts) == 5 and parts[4] == "plans":
                    plans = PLANS.get(service_type, [])
                    data = [
                        {
                            "type": "Plan",
                            "id": plan["id"],
                            "attributes": {"title": plan["title"], "dates": plan["dates"]},
                        }
                        for plan in plans
                    ]
                    self._send(200, {"data": data, "meta": {"total_count": len(data), "count": len(data)}})
                    return
                if len(parts) == 7 and parts[4] == "plans" and parts[6] == "plan_times":
                    times = PLAN_TIMES.get(parts[5], [])
                    if query.get("where[time_type]"):
                        times = [t for t in times if t[2] == query["where[time_type]"]]
                    data = [
                        {
                            "type": "PlanTime",
                            "id": time_id,
                            "attributes": {"starts_at": starts, "time_type": kind, "name": ""},
                        }
                        for time_id, starts, kind in times
                    ]
                    self._send(200, {"data": data, "meta": {"total_count": len(data)}})
                    return
                if len(parts) == 7 and parts[4] == "plans" and parts[6] == "items":
                    items = ITEMS.get(parts[5], [])
                    data = [_item(i, state.lengths) for i in items]
                    self._send(200, {"data": data, "meta": {"total_count": len(data)}})
                    return
                if len(parts) == 8 and parts[4] == "plans" and parts[6] == "items":
                    for item in ITEMS.get(parts[5], []):
                        if item[0] == parts[7]:
                            self._send(200, {"data": _item(item, state.lengths)})
                            return
            self._send(404, {"errors": [{"title": "not found"}]})

        def _service_types(self, query: dict[str, str]) -> None:
            def resource(raw: dict[str, Any]) -> dict[str, Any]:
                attributes = {"name": raw["name"], "sequence": raw["sequence"]}
                if "archived_at" in raw:
                    attributes["archived_at"] = raw["archived_at"]
                return {"type": "ServiceType", "id": raw["id"], "attributes": attributes}

            offset = int(query.get("offset", "0"))
            if offset == 0:
                page = SERVICE_TYPES[:2]
                body = {
                    "data": [resource(raw) for raw in page],
                    "links": {"next": "/services/v2/service_types?offset=2&order=sequence&per_page=100"},
                    "meta": {"total_count": 3, "count": 2, "next": {"offset": 2}},
                }
            else:
                body = {
                    "data": [resource(raw) for raw in SERVICE_TYPES[2:]],
                    "meta": {"total_count": 3, "count": 1},
                }
            self._send(200, body)

    return Handler


def start(port: int = 0) -> tuple[ThreadingHTTPServer, int]:
    server = ThreadingHTTPServer(("127.0.0.1", port), make_handler(State()))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, server.server_address[1]


if __name__ == "__main__":
    srv, bound = start(int(sys.argv[1]))
    print(f"mock planning center on {bound}", flush=True)
    threading.Event().wait()
