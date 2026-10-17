#!/usr/bin/env python3
"""Contract runner: record golden transcripts from the Python backend, replay them against any base URL.

  record   run the scenarios IN PROCESS through FastAPI's TestClient (the harness the backend tests
           use) against `create_app(Settings(demo_mode=True))` and write contract/transcripts.json
  replay   run the same scenarios against BASE_URL over real HTTP/WebSocket, normalise volatile
           fields on both sides and diff against the golden transcripts (exit 1 on any difference)
  openapi  write contract/openapi.json (the /api/v1/* subset of the FastAPI schema)

Run through the backend environment:  uv run --project backend --frozen python scripts/contract/contract.py replay http://127.0.0.1:PORT
Volatile fields are listed in VOLATILE_KEYS below and documented in docs/contract/README.md.
"""

from __future__ import annotations

import argparse
import copy
import json
import sys
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).parent))
from scenarios import SCENARIOS, Step  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "contract"
TRANSCRIPTS = OUT / "transcripts.json"
OPENAPI = OUT / "openapi.json"

# Values that legitimately differ between runs/builds. They must be present (same type) but their value
# is not compared. Matching is by JSON key name at any depth.
VOLATILE_KEYS = {
    "updated_at": "wall-clock timestamp of the last state change",
    "started_at": "wall-clock process/timer start",
    "last_activity_at": "wall-clock timestamp of the last plugin activity",
    "last_checked_at": "wall-clock timestamp",
    "last_cue_at": "wall-clock timestamp",
    "revision": "monotonic state counter; depends on how many internal updates ran at startup",
    "version": "application version string, changes every release",
    "date": "plan date, today's date in the configured timezone",
    "timestamp": "wall-clock timestamp",
    "position_seconds": "timer progress, depends on wall-clock time",
    "elapsed_seconds": "timer progress, depends on wall-clock time",
    "remaining_seconds": "timer progress, depends on wall-clock time",
}
# Any key ending in one of these suffixes is also volatile (wall-clock instants such as
# `last_attempt_at`, `last_successful_plan_reload_at`).
VOLATILE_SUFFIXES = ("_at",)
# Response headers that are compared (everything else, e.g. date/server/content-length, is ignored).
HEADERS_COMPARED = ("content-type",)


def _is_uuid(value: Any) -> bool:
    if not isinstance(value, str) or len(value) != 36:
        return False
    parts = value.split("-")
    return [len(p) for p in parts] == [8, 4, 4, 4, 12] and all(
        c in "0123456789abcdef" for p in parts for c in p
    )


def normalise(node: Any) -> Any:
    if isinstance(node, dict):
        out = {}
        for key, value in node.items():
            if key == "id" and _is_uuid(value):
                out[key] = "<uuid>"
            elif (key in VOLATILE_KEYS or key.endswith(VOLATILE_SUFFIXES)) and value is not None:
                out[key] = f"<{type(value).__name__}>"
            else:
                out[key] = normalise(value)
        return out
    if isinstance(node, list):
        return [normalise(v) for v in node]
    return node


def clean_headers(headers: Any) -> dict[str, str]:
    lowered = {k.lower(): v for k, v in dict(headers).items()}
    result = {}
    for name in HEADERS_COMPARED:
        if name in lowered:
            result[name] = lowered[name].split(";")[0].strip()
    return result


# ---- transports ---------------------------------------------------------------------------------


class InProcess:
    """The backend test harness: FastAPI TestClient over create_app(demo)."""

    def __init__(self) -> None:
        from fastapi.testclient import TestClient
        from stagepilot.core.config import Settings
        from stagepilot.main import create_app

        self._cm = TestClient(
            create_app(Settings(demo_mode=True), dashboard_auth_enforced=False)
        )
        self.client = self._cm.__enter__()

    def close(self) -> None:
        self._cm.__exit__(None, None, None)

    def http(self, step: Step) -> dict[str, Any]:
        r = self.client.request(step["method"], step["path"], json=step.get("json"))
        return _exchange(r.status_code, r.headers, r.content)

    def ws(self, step: Step) -> list[Any]:
        messages: list[Any] = []
        with self.client.websocket_connect(step["ws"]) as sock:
            for _ in range(step["receive"]):
                messages.append(sock.receive_json())
            for sub in step.get("after_receive", []):
                self.http(sub)
            for _ in range(step.get("then_receive", 0)):
                messages.append(sock.receive_json())
        return messages


class Live:
    """A real server at BASE_URL (HTTP via urllib, WebSocket via the `websockets` package)."""

    def __init__(self, base: str) -> None:
        self.base = base.rstrip("/")

    def close(self) -> None:
        pass

    def http(self, step: Step) -> dict[str, Any]:
        data = json.dumps(step["json"]).encode() if "json" in step else None
        req = urllib.request.Request(
            self.base + step["path"],
            data=data,
            method=step["method"],
            headers={"Content-Type": "application/json"} if data else {},
        )
        try:
            with urllib.request.urlopen(req, timeout=15) as resp:
                return _exchange(resp.status, resp.headers, resp.read())
        except urllib.error.HTTPError as err:
            return _exchange(err.code, err.headers, err.read())

    def ws(self, step: Step) -> list[Any]:
        from websockets.sync.client import connect

        url = "ws" + self.base[4:] + step["ws"]
        messages: list[Any] = []
        with connect(url, open_timeout=15) as sock:
            for _ in range(step["receive"]):
                messages.append(json.loads(sock.recv(timeout=15)))
            for sub in step.get("after_receive", []):
                self.http(sub)
            for _ in range(step.get("then_receive", 0)):
                messages.append(json.loads(sock.recv(timeout=15)))
        return messages


def _exchange(status: int, headers: Any, body: bytes) -> dict[str, Any]:
    text = body.decode("utf-8", errors="replace")
    try:
        parsed: Any = json.loads(text)
    except ValueError:
        parsed = text
    return {"status": status, "headers": clean_headers(headers), "body": parsed}


def run(transport: Any) -> list[dict[str, Any]]:
    results = []
    for step in SCENARIOS:
        entry: dict[str, Any] = {"name": step["name"]}
        if "ws" in step:
            entry["ws"] = step["ws"]
            entry["messages"] = transport.ws(step)
        else:
            entry["request"] = {k: step[k] for k in ("method", "path", "json") if k in step}
            entry["response"] = transport.http(step)
        results.append(entry)
    return results


# ---- diff ---------------------------------------------------------------------------------------


def diff(path: str, want: Any, got: Any, out: list[str]) -> None:
    if type(want) is not type(got):
        out.append(f"{path}: type {type(want).__name__} != {type(got).__name__} ({want!r} vs {got!r})")
    elif isinstance(want, dict):
        for k in sorted(set(want) | set(got)):
            if k not in got:
                out.append(f"{path}.{k}: missing from target")
            elif k not in want:
                out.append(f"{path}.{k}: extra in target ({got[k]!r})")
            else:
                diff(f"{path}.{k}", want[k], got[k], out)
    elif isinstance(want, list):
        if len(want) != len(got):
            out.append(f"{path}: length {len(want)} != {len(got)}")
        for i, (a, b) in enumerate(zip(want, got)):
            diff(f"{path}[{i}]", a, b, out)
    elif want != got:
        out.append(f"{path}: {want!r} != {got!r}")


def cmd_record(_: argparse.Namespace) -> int:
    transport = InProcess()
    try:
        transcripts = run(transport)
    finally:
        transport.close()
    OUT.mkdir(exist_ok=True)
    TRANSCRIPTS.write_text(
        json.dumps(
            {"volatile_keys": sorted(VOLATILE_KEYS), "volatile_suffixes": list(VOLATILE_SUFFIXES), "steps": normalise(transcripts)},
            indent=1,
            sort_keys=True,
            ensure_ascii=False,
        )
        + "\n",
        encoding="utf-8",
    )
    print(f"recorded {len(transcripts)} steps -> {TRANSCRIPTS.relative_to(ROOT)}")
    return 0


def cmd_replay(args: argparse.Namespace) -> int:
    golden = json.loads(TRANSCRIPTS.read_text(encoding="utf-8"))["steps"]
    live = Live(args.base_url)
    got = normalise(copy.deepcopy(run(live)))
    problems: list[str] = []
    for want, have in zip(golden, got):
        found: list[str] = []
        diff(want["name"], want, have, found)
        problems.extend(found)
        print(("PASS " if not found else "FAIL ") + want["name"])
    if len(golden) != len(got):
        problems.append(f"step count {len(golden)} != {len(got)}")
    for line in problems:
        print("  " + line)
    failed = sum(1 for want, have in zip(golden, got) if want != have)
    print(f"{len(golden) - failed}/{len(golden)} steps match")
    return 1 if problems else 0


def cmd_openapi(_: argparse.Namespace) -> int:
    from stagepilot.core.config import Settings
    from stagepilot.main import create_app

    schema = create_app(Settings(demo_mode=True), dashboard_auth_enforced=False).openapi()
    schema = copy.deepcopy(schema)
    schema["paths"] = {p: v for p, v in schema["paths"].items() if p.startswith("/api/v1/")}
    # The application version changes every release and is not part of the contract.
    schema["info"]["version"] = "<application version>"
    OUT.mkdir(exist_ok=True)
    OPENAPI.write_text(json.dumps(schema, indent=1, sort_keys=True, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"{len(schema['paths'])} paths -> {OPENAPI.relative_to(ROOT)}")
    return 0


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("record").set_defaults(fn=cmd_record)
    sub.add_parser("openapi").set_defaults(fn=cmd_openapi)
    rp = sub.add_parser("replay")
    rp.add_argument("base_url")
    rp.set_defaults(fn=cmd_replay)
    args = p.parse_args()
    return int(args.fn(args))


if __name__ == "__main__":
    sys.exit(main())
