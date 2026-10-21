"""Generate the transport-fidelity fixture: crafted server->client byte streams plus the
verdict (accept / reject) of the Python hand-rolled client (`plugins/playback_api/ws.py`).

The Rust test (`crates/stagepilot-playback/tests/transport.rs`) replays the same bytes and
must reach the same verdict unless the case is listed in `INTENTIONAL_DIFFERENCES` there.

Run from repo root:  cd backend && uv run python ../tools/rust-playback/gen_transport_cases.py
All payloads are synthetic.
"""

from __future__ import annotations

import json
import socket
import struct
import sys
import threading
from pathlib import Path

from stagepilot.plugins.playback_api.ws import WebSocket, WSClosed

OUT = Path(__file__).resolve().parents[2] / "crates/stagepilot-playback/tests/fixtures/transport_cases.json"


def hb(position: float) -> bytes:
    return json.dumps(
        {"heartbeat": {"stateData": {
            "setlistSongID": 101, "sequenceTime": position,
            "sequencerPlayState": {"stopped": {}}, "padPlayerPlayState": {"stopped": {}},
            "setlistCloudVersion": 1}}},
        separators=(",", ":"),
    ).encode()


def frame(opcode: int, payload: bytes, *, fin: bool = True, rsv: int = 0, mask: bool = False,
          length_form: str = "auto") -> bytes:
    b0 = (0x80 if fin else 0) | (rsv << 4) | opcode
    n = len(payload)
    form = length_form
    if form == "auto":
        form = "7" if n < 126 else "16" if n < 65536 else "64"
    m = 0x80 if mask else 0
    if form == "7":
        head = bytes([b0, m | n])
    elif form == "16":
        head = bytes([b0, m | 126]) + struct.pack(">H", n)
    else:
        head = bytes([b0, m | 127]) + struct.pack(">Q", n)
    if mask:
        key = b"\x01\x02\x03\x04"
        return head + key + bytes(c ^ key[i % 4] for i, c in enumerate(payload))
    return head + payload


FINAL = frame(1, hb(42.0))  # every accepted case must end by delivering position 42
CASES: list[tuple[str, list[bytes]]] = [
    ("plain_text", [frame(1, hb(7.0))]),
    ("rsv1_set", [frame(1, hb(7.0), rsv=4)]),
    ("rsv2_set", [frame(1, hb(7.0), rsv=2)]),
    ("rsv3_set", [frame(1, hb(7.0), rsv=1)]),
    ("masked_server_frame", [frame(1, hb(7.0), mask=True)]),
    ("fragmented_text", [frame(1, hb(7.0)[:20], fin=False), frame(0, hb(7.0)[20:60], fin=False),
                         frame(0, hb(7.0)[60:])]),
    ("fragmented_with_ping_between", [frame(1, hb(7.0)[:20], fin=False), frame(9, b"hi"),
                                      frame(0, hb(7.0)[20:])]),
    ("continuation_without_start", [frame(0, hb(7.0))]),
    ("new_text_inside_fragmented_message", [frame(1, hb(7.0)[:20], fin=False), frame(1, hb(7.0))]),
    ("binary_frame", [frame(2, hb(7.0))]),
    ("reserved_opcode_3", [frame(3, b"x")]),
    ("reserved_opcode_11", [frame(11, b"x")]),
    ("oversize_single_frame", [frame(1, b" " * (1024 * 1024 + 1))]),
    ("exactly_1mib_single_frame", [frame(1, hb(7.0) + b" " * (1024 * 1024 - len(hb(7.0))))]),
    ("oversize_by_fragments", [frame(1, b" " * (600 * 1024), fin=False),
                               frame(0, b" " * (600 * 1024))]),
    ("oversize_declared_64bit_length", [bytes([0x81, 127]) + struct.pack(">Q", 2**40)]),
    ("ping_between_messages", [frame(9, b"abc"), frame(1, hb(7.0))]),
    ("ping_oversize_control", [frame(9, b"x" * 126)]),
    ("ping_fragmented", [frame(9, b"x", fin=False)]),
    ("unsolicited_pong", [frame(10, b"x"), frame(1, hb(7.0))]),
    ("server_close_frame", [frame(8, struct.pack(">H", 1000))]),
    ("invalid_utf8_text", [frame(1, b"\xff\xfe\xfd")]),
    ("non_minimal_16bit_length", [frame(1, hb(7.0), length_form="16")]),
    ("non_minimal_64bit_length", [frame(1, hb(7.0), length_form="64")]),
    ("empty_text_message", [frame(1, b""), frame(1, hb(7.0))]),
    ("text_not_json", [frame(1, b"not json"), frame(1, hb(7.0))]),
]


def python_verdict(stream: bytes) -> dict:
    a, b = socket.socketpair()
    ws = object.__new__(WebSocket)
    ws._sock = b
    ws._buffer = bytearray()
    ws._parts = bytearray()
    ws._fragmented = False
    ws._write_lock = threading.Lock()
    # Feed from a thread: oversize streams exceed the socket buffer.
    def feed() -> None:
        try:
            a.sendall(stream)
        except OSError:
            pass
        finally:
            try:
                a.shutdown(socket.SHUT_WR)
            except OSError:
                pass
    t = threading.Thread(target=feed, daemon=True)
    t.start()
    messages: list[str] = []
    verdict = "accept"
    try:
        while True:
            m = ws.recv(timeout=2.0)
            if m is None:
                verdict = "accept"  # drained without error (stream ended or idle)
                break
            messages.append(m)
            if '"sequenceTime":42.0' in m:
                break
    except WSClosed as exc:
        verdict = f"reject:{exc}"
    a.close()
    b.close()
    reached = any('"sequenceTime":42.0' in m for m in messages)
    if verdict == "accept" and not reached:
        verdict = "reject:stream ended"
    return {"verdict": "accept" if verdict == "accept" else "reject", "python_reason": verdict}


def main() -> None:
    out = []
    for name, frames in CASES:
        stream = b"".join(frames) + FINAL
        v = python_verdict(stream)
        out.append({"name": name, "frames": [f.hex() if len(f) < 4096 else None for f in frames],
                    "generate": name if any(len(f) >= 4096 for f in frames) else None, **v})
        print(f"{name:40s} {v['verdict']:7s} {v['python_reason']}", file=sys.stderr)
    OUT.write_text(json.dumps(out, indent=1))


main()
