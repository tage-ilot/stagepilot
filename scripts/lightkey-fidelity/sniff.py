#!/usr/bin/env python3
"""Record the raw sACN (E1.31) output of Lightkey: every frame as (wall time, universe, sequence, slots).

Run it on the Lighting computer, next to Lightkey. It only listens: it joins the multicast groups of the
universes and never sends a packet of its own (no OSC, no sACN, nothing to Lightkey). Python 3.9+, standard
library only.

    python3 sniff.py capture.pkl 90                 # 90 seconds, universes 1 and 2
    python3 sniff.py capture.pkl 90 --universes 1,2,3 --interface 192.168.4.20

The file is a pickle (protocol 4) of a list of (wall_seconds, universe, sequence, slots_512_bytes) tuples, the
shape compare.py reads. compare.py refuses anything else in it, but still only open captures you made yourself.
"""
import argparse
import pickle
import socket
import struct
import sys
import time

PORT = 5568
ACN_ID = b"ASC-E1.17\x00\x00\x00"


def group(universe):
    """The multicast address of a universe: 239.255.<hi>.<lo>."""
    return "239.255.%d.%d" % (universe >> 8, universe & 255)


def parse_frame(data):
    """(universe, sequence, slots) of an sACN data packet, or None for anything else (other protocol, a
    non-zero start code, a packet too short to hold its own slots). Slots are padded to 512 with zeros."""
    if len(data) < 126 or data[4:16] != ACN_ID or data[125] != 0:
        return None
    count = struct.unpack(">H", data[123:125])[0] - 1        # the property count includes the start code
    if count < 0:
        return None
    slots = bytes(data[126:126 + min(count, 512)])
    return struct.unpack(">H", data[113:115])[0], data[111], slots + b"\x00" * (512 - len(slots))


def local_address():
    """The address of the interface the multicast traffic arrives on, found without sending anything
    (connecting a UDP socket only asks the routing table)."""
    probe = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        probe.connect(("239.255.0.1", PORT))
        return probe.getsockname()[0]
    except OSError:
        return "0.0.0.0"
    finally:
        probe.close()


def record(universes, seconds, interface=None):
    interface = interface or local_address()
    rx = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    rx.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    if hasattr(socket, "SO_REUSEPORT"):
        try:
            rx.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEPORT, 1)    # Lightkey's own listeners keep working
        except OSError:
            pass
    rx.bind(("", PORT))
    rx.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 4 << 20)
    for u in universes:
        rx.setsockopt(socket.IPPROTO_IP, socket.IP_ADD_MEMBERSHIP,
                      socket.inet_aton(group(u)) + socket.inet_aton(interface))
    rx.settimeout(0.2)
    wanted = set(universes)
    out = []
    end = time.time() + seconds
    try:
        while time.time() < end:
            try:
                data, _addr = rx.recvfrom(1500)
            except socket.timeout:
                continue
            now = time.time()
            frame = parse_frame(data)
            if frame and frame[0] in wanted:
                out.append((now,) + frame)
    except KeyboardInterrupt:
        print("stopped early; saving what was heard", file=sys.stderr)
    finally:
        rx.close()
    return out


def main(argv=None):
    ap = argparse.ArgumentParser(description="Record Lightkey's raw sACN output (listen only).")
    ap.add_argument("out", help="file to write (pickle)")
    ap.add_argument("seconds", type=float, help="how long to record")
    ap.add_argument("--universes", default="1,2", help="comma separated universes (default 1,2)")
    ap.add_argument("--interface", help="address of the network interface to listen on (default: the one the "
                                        "routing table uses for multicast)")
    args = ap.parse_args(argv)
    try:
        universes = sorted({int(x) for x in args.universes.split(",") if x.strip()})
    except ValueError:
        ap.error("--universes is a comma separated list of numbers, like 1,2")
    if not universes or not all(1 <= u <= 63999 for u in universes):
        ap.error("universes are 1 to 63999")
    frames = record(universes, args.seconds, args.interface)
    with open(args.out, "wb") as f:
        pickle.dump(frames, f, protocol=4)
    per = {u: sum(1 for x in frames if x[1] == u) for u in universes}
    print("frames %d over %.1f s, per universe %s -> %s" % (
        len(frames), (frames[-1][0] - frames[0][0]) if frames else 0.0, per, args.out))
    if not frames:
        print("nothing heard: is Lightkey running, output on, and the universes right?", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
