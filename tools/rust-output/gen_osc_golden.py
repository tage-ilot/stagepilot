"""Golden OSC packets: what the Python lights plugin puts on the wire.

Every case is sent through the real `UdpOscOutputBackend` (python-osc's `SimpleUDPClient`) to a
loopback UDP socket and the received datagram is recorded as hex. The Rust `stagepilot-osc` tests
assert byte-for-byte equality. Run from `backend/`:

    uv run --frozen python ../tools/rust-output/gen_osc_golden.py

Output: crates/stagepilot-osc/tests/fixtures/osc_golden.json (deterministic; CI regenerates it and
fails on a diff).
"""

from __future__ import annotations

import json
import socket
from contextlib import closing
from pathlib import Path

from stagepilot.core.config import OscArgument
from stagepilot.plugins.lights import UdpOscOutputBackend

OUT = Path(__file__).resolve().parents[2] / "crates/stagepilot-osc/tests/fixtures/osc_golden.json"


def arg(kind: str, value: object) -> OscArgument:
    return OscArgument.model_validate({"type": kind, "value": value})


INT_VALUES = [0, 1, -1, 7, 255, 256, 65535, 2**24, 2**31 - 1, -(2**31)]
FLOAT_VALUES = [
    0.0,
    -0.0,
    1.0,
    -1.0,
    0.5,
    0.1,
    0.3,
    1 / 3,
    100.0,
    127.0,
    16777217.0,
    1e-45,
    1.5e-45,
    1.17549435e-38,
    3.4028234663852886e38,
    -3.4028234663852886e38,
    123456.789,
    2.0**-24,
]
STRING_VALUES = [
    "",
    "a",
    "ab",
    "abc",
    "abcd",
    "abcde",
    "go",
    "Hello, world",
    "Z" * 511,
    "Z" * 512,
    "naïve café",
    "日本語",
    "🎹 lights",
    "with space",
    "line\nbreak",
]
ADDRESSES = [
    "/",
    "/a",
    "/ab",
    "/abc",
    "/abcd",
    "/blackout",
    "/cue/1/start",
    "/lightkey/cue/12/go",
    "/lk/*",
    "/a/b/c/d/e/f/g/h",
    "/{a,b}/[0-9]/?",
    "/café/ünï",
    "/" + "x" * 255,
]


def cases() -> list[tuple[str, list[OscArgument]]]:
    out: list[tuple[str, list[OscArgument]]] = []
    for address in ADDRESSES:
        out.append((address, []))
    for value in INT_VALUES:
        out.append(("/int", [arg("int", value)]))
    for value in FLOAT_VALUES:
        out.append(("/float", [arg("float", value)]))
    for value in [0, 1, -3, 127]:  # pydantic turns an integer float value into a float
        out.append(("/float/from/int", [arg("float", value)]))
    for value in STRING_VALUES:
        out.append(("/string", [arg("string", value)]))
    for value in (True, False):
        out.append(("/bool", [arg("bool", value)]))
    out.append(
        (
            "/cue/1/start",
            [arg("int", 7), arg("float", 0.5), arg("string", "go"), arg("bool", True)],
        )
    )
    out.append(("/mix", [arg("bool", False), arg("string", "x"), arg("int", -9), arg("float", 2.25)]))
    out.append(("/sixteen", [arg("int", n) for n in range(16)]))
    out.append(("/sixteen/bools", [arg("bool", n % 2 == 0) for n in range(16)]))
    out.append(("/sixteen/mixed", [arg(("int", "float", "string", "bool")[n % 4], (n, n + 0.25, "s" * n, n % 2 == 0)[n % 4]) for n in range(16)]))
    return out


def main() -> None:
    golden = []
    with closing(socket.socket(socket.AF_INET, socket.SOCK_DGRAM)) as receiver:
        receiver.bind(("127.0.0.1", 0))
        receiver.settimeout(2.0)
        output = UdpOscOutputBackend().open_output("127.0.0.1", receiver.getsockname()[1])
        for address, args in cases():
            output.send_message(address, args)
            data, _ = receiver.recvfrom(65535)
            golden.append(
                {
                    "address": address,
                    "args": [{"type": a.type, "value": a.value} for a in args],
                    "hex": data.hex(),
                }
            )
        output.close()
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(golden, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"{len(golden)} golden OSC packets -> {OUT}")


if __name__ == "__main__":
    main()
