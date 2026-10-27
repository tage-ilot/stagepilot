#!/usr/bin/env python3
"""Contract replay: run the scripted scenarios against a StagePilot server and diff the result.

The golden transcripts in contract/transcripts.json were recorded once from the Python 2.41.1
backend; the Rust server must keep answering the same way. This tool only replays them (standard
library only, Python 3.9+):

    python3 scripts/contract/contract.py replay http://127.0.0.1:PORT [--profile NAME] [--mock-url URL]

It normalises volatile fields on both sides and exits 1 on any difference that is not listed in
APPROVED_DIVERGENCES. scripts/contract/run_rust.sh starts the Rust server and the mocks for every
profile and calls it. Volatile fields are listed in VOLATILE_KEYS below.
"""

from __future__ import annotations

import argparse
import copy
import http.cookiejar
import json
import re
import sys
import urllib.error
import urllib.request
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).parent))
from scenarios import LIGHTKEY_SCENARIOS, OUTPUT_SCENARIOS, SCENARIOS, Step  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "contract"
TRANSCRIPTS = OUT / "transcripts.json"

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
# Rust-only additions. The recorded Python app has no such key; the Rust server may add it, but only
# with this shape. Anything else extra is still a mismatch. Each entry names why the key exists.
RUST_EXTENSIONS = {
    # Lightkey fixture capture/replay (colour, intensity, movers) from the lightkey-api 0.4 relay was
    # built in Rust only (#262). `fixtures` is the relay link status; it is only reported once the relay
    # has been tried (in the contract mock it is unreachable, so link is "lost").
    "fixtures": {"link", "fixtures", "lost_packets", "recording"},
}
# Rust-only scalar additions to a response body (same rule as above: only with this type).
# `pin_is_default` tells the UI that the dashboard PIN was never changed (m13);
# `settings_degraded` / `settings_degraded_reason` say the saved settings file was damaged or newer
# than this build and defaults are in use (M4).
RUST_SCALAR_EXTENSIONS = {
    "pin_is_default": (bool,),
    "settings_degraded": (bool,),
    "settings_degraded_reason": (str, type(None)),
}
# Reviewed, intentional differences from the Python 2.41.1 recording (rc.4 audit fixes). Each entry
# is a regex over one diff line plus the reason. Anything not listed still fails the replay, so keep
# this list short and specific; each entry should be dropped when the transcripts are next
# re-recorded from the Rust server.
APPROVED_DIVERGENCES = [
    (r"\.(lights_status|connection_status): 'connected' != 'ready'$",
     "m5: Lighting OSC is 'ready' (socket open, delivery unconfirmed) until a cue is sent"),
    (r"\.outputs: length 0 != 1$",
     "m12: MIDI ports are listed read-only even when the MIDI output is not in use"),
    (r"lights\.detail: 'OSC ready for .* != 'OSC output to .* reopened\.|body\.message: 'OSC ready for .* != 'OSC output to .* reopened\.",
     "m5: after a cue was sent, a reopened OSC output reports connected"),
    (r"^(outputs: )?(mock saw the startup probe|mock log before the settings flows|ProPresenter calls for the settings save)\.mock_requests",
     "m2: the ProPresenter probe also reads GET /v1/timers/current for the timer state"),
    (r"^(outputs: )?propresenter settings, an unknown Look is refused after saving\.response\.body\.propresenter\.(look_found|look_id): ",
     "m3: an unknown ProPresenter look is refused and not saved"),
    (r"\.message: type str != NoneType \('Capture cleared\.' vs None\)$",
     "B3: Lightkey status messages expire after 10 s or on a state change"),
]
_APPROVED = [(re.compile(pattern), reason) for pattern, reason in APPROVED_DIVERGENCES]


def approved(problem: str) -> bool:
    return any(pattern.search(problem) for pattern, _ in _APPROVED)


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


# The mock Planning Center profile pins "today" here (pco_app.TODAY), whatever the wall clock says.
PINNED_TODAY = "2026-10-18"


def today_values(now: datetime | None = None) -> frozenset[str]:
    """Every date string that counts as "today" for the normaliser.

    The demo plan uses the date in the configured timezone, which can differ from the host's
    local date around midnight, so accept the UTC date and the dates one zone-offset either side
    (UTC-12 .. UTC+14), plus the date the Planning Center profile pins. The set depends only on
    ``now``, never on the host timezone.
    """
    now = now or datetime.now(timezone.utc)
    if now.tzinfo is None:
        now = now.replace(tzinfo=timezone.utc)
    now = now.astimezone(timezone.utc)
    days = {(now + timedelta(hours=h)).date().isoformat() for h in (-12, 0, 14)}
    return frozenset(days | {PINNED_TODAY})


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
    if isinstance(node, str) and node in today_values():
        # Plan dates are "today" (or the pinned PCO date); the transcripts must not expire.
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


class Live:
    """A real server at BASE_URL (HTTP via urllib, WebSocket via the small client below)."""

    def __init__(self, base: str) -> None:
        self.base = base.rstrip("/")
        # The recorded session kept its cookies between steps, so the live replay does too (dashboard PIN session).
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
        url = "ws" + self.base[4:] + step["ws"]
        messages: list[Any] = []
        sock = _WebSocket(url)
        try:
            for _ in range(step["receive"]):
                messages.append(json.loads(sock.recv(timeout=15)))
            for sub in step.get("after_receive", []):
                self.http(sub)
            for _ in range(step.get("then_receive", 0)):
                messages.append(json.loads(sock.recv(timeout=15)))
        finally:
            sock.close()
        return messages


class _WebSocket:
    """A minimal blocking WebSocket client (text frames, RFC 6455), standard library only."""

    def __init__(self, url: str, timeout: float = 15) -> None:
        import base64
        import os
        import socket
        from urllib.parse import urlsplit

        parts = urlsplit(url)
        self.sock = socket.create_connection((parts.hostname, parts.port or 80), timeout=timeout)
        key = base64.b64encode(os.urandom(16)).decode()
        path = (parts.path or "/") + (f"?{parts.query}" if parts.query else "")
        self.sock.sendall(
            (
                f"GET {path} HTTP/1.1\r\nHost: {parts.hostname}:{parts.port or 80}\r\n"
                f"Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\n"
                "Sec-WebSocket-Version: 13\r\n\r\n"
            ).encode()
        )
        response = b""
        while b"\r\n\r\n" not in response:
            chunk = self.sock.recv(4096)
            if not chunk:
                raise RuntimeError("WebSocket closed during the handshake")
            response += chunk
        head, _, self._buffer = response.partition(b"\r\n\r\n")
        status_line = head.split(b"\r\n")[0]
        if b" 101 " not in status_line:
            raise RuntimeError("WebSocket upgrade refused: " + repr(status_line))

    def _read(self, size: int) -> bytes:
        while len(self._buffer) < size:
            chunk = self.sock.recv(65536)
            if not chunk:
                raise RuntimeError("WebSocket closed")
            self._buffer += chunk
        data, self._buffer = self._buffer[:size], self._buffer[size:]
        return data

    def _send(self, opcode: int, payload: bytes) -> None:
        import os

        mask = os.urandom(4)
        header = bytes((0x80 | opcode,))
        if len(payload) < 126:
            header += bytes((0x80 | len(payload),))
        else:
            header += bytes((0x80 | 126,)) + len(payload).to_bytes(2, "big")
        self.sock.sendall(header + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(payload)))

    def recv(self, timeout: float = 15) -> str:
        self.sock.settimeout(timeout)
        message = b""
        while True:
            first, second = self._read(2)
            size = second & 127
            if size == 126:
                size = int.from_bytes(self._read(2), "big")
            elif size == 127:
                size = int.from_bytes(self._read(8), "big")
            payload = self._read(size)
            opcode = first & 15
            if opcode == 9:  # ping
                self._send(10, payload)
            elif opcode == 8:  # close
                raise RuntimeError("WebSocket closed by the server")
            elif opcode in (1, 2, 0):
                message += payload
                if first & 0x80:
                    return message.decode("utf-8")

    def close(self) -> None:
        try:
            self._send(8, b"")
        except OSError:
            pass
        self.sock.close()


def _exchange(status: int, headers: Any, body: bytes) -> dict[str, Any]:
    text = body.decode("utf-8", errors="replace")
    try:
        parsed: Any = json.loads(text)
    except ValueError:
        parsed = text
    return {"status": status, "headers": clean_headers(headers), "body": parsed}


FAKE_PORT = 47901


class Fakes:
    """The scripted fake Playback (scripts/contract/playback_fake.py) driven by `fake` steps."""

    def __init__(self) -> None:
        self.fake: Any = None

    def do(self, op: str, args: list[Any]) -> None:
        if op == "start":
            sys.path.insert(0, str(Path(__file__).parent))
            from playback_fake import FakePlayback

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
                shape = RUST_EXTENSIONS.get(k)
                if shape is not None and isinstance(got[k], dict) and shape <= set(got[k]):
                    continue
                scalar = RUST_SCALAR_EXTENSIONS.get(k)
                if scalar is not None and type(got[k]) in scalar:
                    continue
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
    elif args.profile in ("remote", "diagnostics", "static"):
        import remote_profile

        urls = {"local": args.base_url, "ingress": args.ingress_url or args.base_url}
        golden = document[f"{args.profile}_steps"]
        scenarios_for = {
            "remote": remote_profile.REMOTE_SCENARIOS,
            "diagnostics": remote_profile.DIAGNOSTICS_SCENARIOS,
            "static": remote_profile.STATIC_SCENARIOS,
        }
        got = normalise(
            copy.deepcopy(remote_profile.run(scenarios_for[args.profile], urls, args.mock_url))
        )
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
        found = [line for line in found if not approved(line)]
        problems.extend(found)
        print(("PASS " if not found else "FAIL ") + want["name"])
    if len(golden) != len(got):
        problems.append(f"step count {len(golden)} != {len(got)}")
    for line in problems:
        print("  " + line)
    failed = sum(1 for want, have in zip(golden, got) if want != have)
    print(f"{len(golden) - failed}/{len(golden)} steps match" + (f" ({args.profile} profile)" if args.profile else ""))
    return 1 if problems else 0


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    rp = sub.add_parser("replay")
    rp.add_argument("base_url")
    rp.add_argument("--profile", choices=["pco", "outputs", "remote", "diagnostics", "static", "lightkey"], default=None)
    rp.add_argument("--mock-url", default=None, help="mock base URL (pco: Planning Center, outputs: ProPresenter)")
    rp.add_argument("--ingress-url", default=None, help="remote profile: the trusted ingress base URL")
    rp.set_defaults(fn=cmd_replay)
    args = p.parse_args()
    return int(args.fn(args))


if __name__ == "__main__":
    sys.exit(main())
