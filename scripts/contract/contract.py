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
import http.cookiejar
import json
import sys
import urllib.error
import urllib.request
from datetime import date
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).parent))
from scenarios import LIGHTKEY_SCENARIOS, OUTPUT_SCENARIOS, SCENARIOS, Step  # noqa: E402

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
    "at": "wall-clock instant of a Playback monitor entry",
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


MOCK_PORT: list[int] = []  # the mock ProPresenter port of this run (outputs profile)
LK_PORT: list[int] = []  # the fake lightkey-api port of this run (lightkey profile)


def normalise(node: Any) -> Any:
    if isinstance(node, dict):
        out = {}
        for key, value in node.items():
            if key in ("id", "event_id", "cue_id") and _is_uuid(value):
                out[key] = "<uuid>"
            elif key == "port" and MOCK_PORT and value == MOCK_PORT[0]:
                out[key] = "<mock_port>"
            elif (key in VOLATILE_KEYS or key.endswith(VOLATILE_SUFFIXES)) and value is not None:
                out[key] = f"<{type(value).__name__}>"
            elif key == "details" and isinstance(value, str) and '"duration_s"' in value:
                # The scan summary carries how long the scan took, which is wall-clock.
                summary = json.loads(value)
                summary["duration_s"] = "<float>"
                out[key] = json.dumps(summary, sort_keys=True)
            elif key == "recent_events" and isinstance(value, list):
                # Plugins start concurrently, so the order of events published in the same
                # scheduling slot is not part of the contract: compare them as a sorted list.
                events = [normalise(v) for v in value]
                out[key] = sorted(events, key=lambda e: json.dumps(e, sort_keys=True))
            else:
                out[key] = normalise(value)
        return out
    if isinstance(node, list):
        return [normalise(v) for v in node]
    if isinstance(node, str) and node == date.today().isoformat():
        # Plan dates are "today"; the transcripts must not expire at midnight.
        return "<today>"
    if isinstance(node, str) and LK_PORT and f"127.0.0.1:{LK_PORT[0]}" in node:
        return node.replace(f"127.0.0.1:{LK_PORT[0]}", "127.0.0.1:<lightkey_port>")
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

    def __init__(self, app: Any = None) -> None:
        from fastapi.testclient import TestClient
        from stagepilot.core.config import Settings
        from stagepilot.main import create_app

        self._cm = TestClient(
            app if app is not None else create_app(Settings(demo_mode=True), dashboard_auth_enforced=False)
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
        # The TestClient keeps cookies between steps, so the live replay does too (dashboard PIN session).
        self.opener = urllib.request.build_opener(
            urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar())
        )

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
            with self.opener.open(req, timeout=15) as resp:
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


FAKE_PORT = 47901


class Fakes:
    """The scripted fake Playback (backend/tests/playback_fake.py) driven by `fake` steps."""

    def __init__(self) -> None:
        self.fake: Any = None

    def do(self, op: str, args: list[Any]) -> None:
        if op == "start":
            sys.path.insert(0, str(ROOT / "backend" / "tests"))
            from playback_fake import FakePlayback  # type: ignore[import-not-found]

            self.fake = FakePlayback()
            self.fake.start(FAKE_PORT)
        elif op == "wait_connected":
            if not self.fake.wait_connected(*args):
                raise RuntimeError("the server never connected to the fake Playback")
        else:
            getattr(self.fake, op)(*args)

    def close(self) -> None:
        if self.fake is not None:
            self.fake.close()
            self.fake = None


def wait_for(transport: Any, spec: dict[str, Any]) -> None:
    """Poll a GET until `body[key] == equals` (the Playback connection settles asynchronously)."""
    import time

    deadline = time.monotonic() + spec.get("timeout", 20)
    while True:
        body = transport.http({"method": "GET", "path": spec["path"]})["body"]
        value: Any = body
        for part in spec["key"].split("."):
            value = value.get(part) if isinstance(value, dict) else None
        if value == spec["equals"]:
            return
        if time.monotonic() > deadline:
            raise RuntimeError(f"timed out waiting for {spec['path']} {spec['key']} == {spec['equals']!r}")
        time.sleep(0.05)


def _mock(ctx: dict[str, Any], path: str) -> Any:
    request = urllib.request.Request(ctx["mock_url"] + path, method="GET")
    with urllib.request.urlopen(request, timeout=15) as response:
        body = response.read()
    return json.loads(body) if body else None


def _substitute(node: Any, ctx: dict[str, Any]) -> Any:
    if node == "@MOCK_PORT@":
        return ctx["mock_port"]
    if isinstance(node, dict):
        return {k: _substitute(v, ctx) for k, v in node.items()}
    if isinstance(node, list):
        return [_substitute(v, ctx) for v in node]
    return node


def _mask(node: Any, keys: list[str]) -> Any:
    if isinstance(node, dict):
        return {k: ("<masked>" if k in keys else _mask(v, keys)) for k, v in node.items()}
    if isinstance(node, list):
        return [_mask(v, keys) for v in node]
    return node


def run(
    transport: Any,
    scenarios: list[Step] | None = None,
    ctx: dict[str, Any] | None = None,
) -> list[dict[str, Any]]:
    results = []
    fakes = Fakes()
    try:
        _run_steps(transport, fakes, results, SCENARIOS if scenarios is None else scenarios, ctx)
    finally:
        fakes.close()
    return results


def _run_steps(
    transport: Any,
    fakes: Fakes,
    results: list[dict[str, Any]],
    scenarios: list[Step],
    ctx: dict[str, Any] | None,
) -> None:
    import time

    for step in scenarios:
        if "sleep" in step:
            time.sleep(step["sleep"])
            continue
        if "mock" in step:
            assert ctx is not None
            _mock(ctx, step["mock"])
            continue
        if "mock_log" in step:
            assert ctx is not None
            results.append({"name": step["name"], "mock_requests": _mock(ctx, "/__log")})
            continue
        entry: dict[str, Any] = {"name": step["name"]}
        if "fake" in step:
            fakes.do(step["fake"], step.get("args", []))
            entry["fake"] = step["fake"]
            results.append(entry)
            continue
        if "wait" in step:
            wait_for(transport, step["wait"])
            entry["wait"] = step["wait"]
            results.append(entry)
            continue
        if "ws" in step:
            entry["ws"] = step["ws"]
            messages = transport.ws(step)
            # Messages after the first are pushed mid-burst: one action changes the state several
            # times and which intermediate revision a client sees first depends on scheduling (the
            # recorded Python run itself saw a half-applied state). Their content is covered by the
            # HTTP step that follows, so only their envelope is compared.
            entry["messages"] = messages[:1] + [
                {"type": m.get("type"), "data_keys": sorted(m.get("data", {}))} for m in messages[1:]
            ]
        else:
            # The recorded request is the template (placeholders intact); the wire request has the
            # run's mock port substituted.
            entry["request"] = {k: step[k] for k in ("method", "path", "json") if k in step}
            wire = _substitute(step, ctx) if ctx is not None else step
            response = transport.http(wire)
            if "mask" in step:
                response = {**response, "body": _mask(response["body"], step["mask"])}
            entry["response"] = response
        results.append(entry)


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


def record_pco() -> list[dict[str, Any]]:
    """Run the pco profile against the Python app served over real HTTP on loopback.

    The Planning Center plugin awaits its refresh task from inside the request, which the
    in-process TestClient portal cancels mid-flight, so this profile (unlike the demo profile) is
    recorded exactly the way it is replayed: through a real server. Planning Center and the
    control plane are the mock.
    """

    import socket
    import threading
    import time

    import mock_pco
    import pco_app
    import pco_profile
    import uvicorn

    mock_server, mock_port = mock_pco.start()
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]
    server = uvicorn.Server(
        uvicorn.Config(pco_app.create(mock_port), host="127.0.0.1", port=port, log_level="warning")
    )
    thread = threading.Thread(target=server.run, daemon=True)
    thread.start()
    for _ in range(100):
        if server.started:
            break
        time.sleep(0.1)
    try:
        return normalise(pco_profile.run_pco(Live(f"http://127.0.0.1:{port}"), f"http://127.0.0.1:{mock_port}"))
    finally:
        server.should_exit = True
        thread.join(timeout=10)
        mock_server.shutdown()


def _dump(steps: list[dict[str, Any]]) -> dict[str, Any]:
    return {
        "volatile_keys": sorted(VOLATILE_KEYS),
        "volatile_suffixes": list(VOLATILE_SUFFIXES),
        "steps": normalise(steps),
    }


def record_outputs() -> list[dict[str, Any]]:
    """Run the outputs profile in process: fake MIDI, mock ProPresenter, OSC to a dead loopback port."""

    import mock_propresenter
    import outputs_app

    server, port = mock_propresenter.start()
    MOCK_PORT[:] = [port]
    transport = InProcess(outputs_app.create(port))
    try:
        return normalise(
            run(
                transport,
                OUTPUT_SCENARIOS,
                {"mock_url": f"http://127.0.0.1:{port}", "mock_port": port},
            )
        )
    finally:
        transport.close()
        server.shutdown()
        MOCK_PORT[:] = []


def record_lightkey() -> list[dict[str, Any]]:
    """Run the lightkey profile against the Python app served over real HTTP on loopback.

    Like the pco profile it is recorded exactly the way it is replayed: through a real server,
    because the Lightkey link polls in the background and needs the app's event loop running
    between requests. The Lightkey agent is the fake in `mock_lightkey.py`.
    """

    import socket
    import threading
    import time

    import lightkey_app
    import mock_lightkey
    import uvicorn

    agent, agent_port = mock_lightkey.start()
    LK_PORT[:] = [agent_port]
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]
    server = uvicorn.Server(
        uvicorn.Config(lightkey_app.create(agent_port), host="127.0.0.1", port=port, log_level="warning")
    )
    thread = threading.Thread(target=server.run, daemon=True)
    thread.start()
    for _ in range(100):
        if server.started:
            break
        time.sleep(0.1)
    try:
        return normalise(
            run(
                Live(f"http://127.0.0.1:{port}"),
                LIGHTKEY_SCENARIOS,
                {"mock_url": f"http://127.0.0.1:{agent_port}", "mock_port": agent_port},
            )
        )
    finally:
        server.should_exit = True
        thread.join(timeout=10)
        agent.shutdown()
        LK_PORT[:] = []


def cmd_record(_: argparse.Namespace) -> int:
    transport = InProcess()
    try:
        transcripts = run(transport)
    finally:
        transport.close()
    document = _dump(transcripts)
    pco_steps = record_pco()
    outputs = record_outputs()
    lightkey = record_lightkey()
    document["pco_steps"] = pco_steps
    document["output_steps"] = outputs
    document["lightkey_steps"] = lightkey
    OUT.mkdir(exist_ok=True)
    TRANSCRIPTS.write_text(
        json.dumps(document, indent=1, sort_keys=True, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    print(
        f"recorded {len(transcripts)} steps + {len(pco_steps)} pco steps + {len(outputs)} output steps + {len(lightkey)} lightkey steps"
        f" -> {TRANSCRIPTS.relative_to(ROOT)}"
    )
    return 0


def cmd_replay(args: argparse.Namespace) -> int:
    document = json.loads(TRANSCRIPTS.read_text(encoding="utf-8"))
    golden = document["steps"]
    live = Live(args.base_url)
    ctx = None
    scenarios = None
    if args.profile == "pco":
        import pco_profile

        golden = document["pco_steps"]
        got = normalise(copy.deepcopy(pco_profile.run_pco(live, args.mock_url)))
    else:
        if args.profile == "lightkey":
            golden = document["lightkey_steps"]
            scenarios = LIGHTKEY_SCENARIOS
            ctx = {"mock_url": args.mock_url, "mock_port": int(args.mock_url.rsplit(":", 1)[1])}
            LK_PORT[:] = [ctx["mock_port"]]
        elif args.profile == "outputs":
            golden = document["output_steps"]
            scenarios = OUTPUT_SCENARIOS
            ctx = {"mock_url": args.mock_url, "mock_port": int(args.mock_url.rsplit(":", 1)[1])}
            MOCK_PORT[:] = [ctx["mock_port"]]
        got = normalise(copy.deepcopy(run(live, scenarios, ctx)))
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
    print(f"{len(golden) - failed}/{len(golden)} steps match" + (f" ({args.profile} profile)" if args.profile else ""))
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
    rp.add_argument("--profile", choices=["pco", "outputs", "lightkey"], default=None)
    rp.add_argument("--mock-url", default=None, help="mock base URL (pco: Planning Center, outputs: ProPresenter)")
    rp.set_defaults(fn=cmd_replay)
    args = p.parse_args()
    return int(args.fn(args))


if __name__ == "__main__":
    sys.exit(main())
