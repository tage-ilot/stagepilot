"""Minimal OSC 1.0 encode/decode (messages and bundles) for the fakes. Stdlib only."""
from __future__ import annotations

import struct

NTP_UNIX_OFFSET = 2_208_988_800


def _pad(b: bytes) -> bytes:
    return b + b"\0" * (4 - len(b) % 4)


def _str(s: str) -> bytes:
    return _pad(s.encode())


def message(addr: str, args=()) -> bytes:
    """args: float -> 'f', int -> 'i', str -> 's', bool -> T/F, ('d', x) -> double."""
    tags, body = ",", b""
    for a in args:
        if isinstance(a, tuple) and a[0] == "d":
            tags += "d"
            body += struct.pack(">d", a[1])
        elif isinstance(a, bool):
            tags += "T" if a else "F"
        elif isinstance(a, int):
            tags += "i"
            body += struct.pack(">i", a)
        elif isinstance(a, float):
            tags += "f"
            body += struct.pack(">f", a)
        else:
            tags += "s"
            body += _str(str(a))
    return _str(addr) + _str(tags) + body


def timetag(unix: float) -> bytes:
    secs = int(unix) + NTP_UNIX_OFFSET
    frac = int((unix - int(unix)) * 4294967296.0) & 0xFFFFFFFF
    return struct.pack(">II", secs, frac)


def bundle(unix: float, messages: list[bytes]) -> bytes:
    out = b"#bundle\0" + timetag(unix)
    for m in messages:
        out += struct.pack(">i", len(m)) + m
    return out


def _read_str(data: bytes, i: int) -> tuple[str, int]:
    j = data.index(b"\0", i)
    s = data[i:j].decode(errors="replace")
    return s, (j + 4) & ~3


def decode_message(data: bytes):
    addr, i = _read_str(data, 0)
    if i >= len(data):
        return addr, []
    tags, i = _read_str(data, i)
    args = []
    for t in tags[1:]:
        if t == "f":
            args.append(struct.unpack_from(">f", data, i)[0]); i += 4
        elif t == "i":
            args.append(struct.unpack_from(">i", data, i)[0]); i += 4
        elif t == "d":
            args.append(struct.unpack_from(">d", data, i)[0]); i += 8
        elif t == "h":
            args.append(struct.unpack_from(">q", data, i)[0]); i += 8
        elif t == "s":
            s, i = _read_str(data, i); args.append(s)
        elif t == "T":
            args.append(True)
        elif t == "F":
            args.append(False)
        else:
            args.append(None)
    return addr, args
