"""Golden OSC packets for the Lightkey replay: what Python puts on the wire for every command.

For each take and start position the real `replay_plan` is run, and every command (`sync_now`
then `later`) is sent through the real `UdpOscOutputBackend` (python-osc's `SimpleUDPClient`) to a
loopback UDP socket; the received datagram is recorded as hex. The Rust test
`crates/stagepilot-lights/tests/lightkey_osc_packets.rs` runs its own `replay_plan` over the same
takes and asserts byte-for-byte equality of every packet, and that the link sends them in that
order. Lightkey replay sends the address only (no arguments), so there is nothing else to encode.
Run from `backend/`:

    uv run --frozen python ../tools/rust-output/gen_lightkey_osc_golden.py

Output: crates/stagepilot-lights/tests/fixtures/lightkey_osc_golden.json (deterministic; CI
regenerates it and fails on a diff). Nothing is sent off this machine.
"""

from __future__ import annotations

import json
import socket
from contextlib import closing
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from stagepilot.core.lightkey import LightkeyEdge, LightkeyTake, osc_address, replay_plan
from stagepilot.plugins.lights import UdpOscOutputBackend

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "crates/stagepilot-lights/tests/fixtures/lightkey_osc_golden.json"
GOLDEN_TAKE = ROOT / "backend/tests/golden/lightkey_golden.json"

AWKWARD = [
    "Warm White, 70%",
    "Café ☕",
    "日本語 キュー",
    "two  spaces",
    "Cue#1",
    "Strobe [fast]",
    "{a,b}",
    "?*",
    "quote\"s'",
    "x" * 200,
    "a",
    "ab",
    "abc",
    "abcd",
    "abcde",
]


def take_json(take: LightkeyTake) -> dict[str, Any]:
    return json.loads(take.model_dump_json())


def awkward_take() -> LightkeyTake:
    edges = [
        LightkeyEdge(at_seconds=float(index + 1), added=[name]) for index, name in enumerate(AWKWARD[1:])
    ]
    edges.append(LightkeyEdge(at_seconds=30, removed=[AWKWARD[0], AWKWARD[1]]))
    return LightkeyTake(
        song_key="awkward",
        song_title="Awkward names",
        recorded_at=datetime(2026, 10, 20, 20, 0, tzinfo=UTC),
        duration_seconds=60,
        initial=[AWKWARD[0]],
        edges=edges,
    )


def cases() -> list[tuple[str, LightkeyTake, list[float]]]:
    golden = json.loads(GOLDEN_TAKE.read_text())
    return [
        ("golden take", LightkeyTake.model_validate(golden["take"]), [0, 10, 40, 100, 200]),
        ("awkward cue names", awkward_take(), [0, 5, 29, 31]),
    ]


def main() -> None:
    out: list[dict[str, Any]] = []
    with closing(socket.socket(socket.AF_INET, socket.SOCK_DGRAM)) as receiver:
        receiver.bind(("127.0.0.1", 0))
        receiver.settimeout(2.0)
        output = UdpOscOutputBackend().open_output("127.0.0.1", receiver.getsockname()[1])
        for name, take, positions in cases():
            for position in positions:
                sync_now, later = replay_plan(take, position)
                packets = []
                for command in [*sync_now, *later]:
                    address = osc_address(command.cue, command.verb)
                    output.send_message(address, [])
                    data, _ = receiver.recvfrom(65535)
                    packets.append(
                        {
                            "cue": command.cue,
                            "verb": command.verb,
                            "at_seconds": command.at_seconds,
                            "address": address,
                            "hex": data.hex(),
                        }
                    )
                out.append(
                    {
                        "name": name,
                        "take": take_json(take),
                        "position": position,
                        "sync_now": len(sync_now),
                        "packets": packets,
                    }
                )
        output.close()
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(out, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    total = sum(len(case["packets"]) for case in out)
    print(f"{len(out)} replay cases, {total} golden Lightkey OSC packets -> {OUT}")


if __name__ == "__main__":
    main()
