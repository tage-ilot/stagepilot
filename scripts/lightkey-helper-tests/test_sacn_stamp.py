"""The Lightkey helper stamps every sACN frame when it ARRIVES and decodes on its own thread (helper 0.6.1).

A synthetic Lightkey sends two universes at 40 Hz to the helper's own receive and decode threads over a real UDP socket: a smooth
2 s triangle ramp on 16 fixtures per universe (so every frame changes 16 fixtures), with an artificial 20 ms of decode time per
frame. Every logged event must carry the arrival time of its frame (not the time the decoder got to it), events of one address
must be 25 ms apart (not bunched), nothing may go back and forth on a one-way ramp, and the frames must stay in order.

Run:  python3 -m unittest discover -s scripts/lightkey-helper-tests -v   (Python 3.9 or newer)
"""
import os
import socket
import struct
import sys
import threading
import time
import unittest

sys.dont_write_bytecode = True      # never leave a __pycache__ inside the helper folder: it is a bundle of plain files

HERE = os.path.dirname(os.path.abspath(__file__))
HELPER = os.environ.get("LIGHTKEY_HELPER_DIR") or os.path.join(HERE, "..", "..", "crates", "stagepilot-lights", "assets", "lightkey-helper")
sys.path.insert(0, HELPER)
import lightkey_api as lk  # noqa: E402

PER_UNIVERSE = 16           # fixtures per universe, all of them change in every ramp frame
UNIVERSES = (1, 2)
LEAD, RAMP, TAIL = 5, 80, 5  # frames: still, 2 s of ramp at 40 Hz, still
DECODE_DELAY_S = 0.020       # artificial extra decode time per frame
FRAME_S = 0.025


def rgb_fixture(name, universe, address):
    caps = [{"kind": "ColorComponent", "channel": ch, "settings": [{"lo": 0, "hi": -1, "params": {"componentName": comp}}],
             "beams": None, "name": ""} for ch, comp in enumerate(("Red", "Green", "Blue"))]
    return {"profile": "Test RGB", "personality": "", "beams": 1, "footprint": 3, "panRange": 0.0, "tiltRange": 0.0,
            "capabilities": caps, "name": name, "universe": universe, "address": address}


def fixture_map():
    return {"version": 1, "project": "synthetic", "fixtures": [
        rgb_fixture("U%dF%d" % (u, i), u, 3 * i) for u in UNIVERSES for i in range(PER_UNIVERSE)]}


def ramp_value(k):
    """Level (0..240) of ramp frame k: up for the first half of the 80 frames, down for the second."""
    x = k / float(RAMP)
    return int(round(240 * (2 * x if x < 0.5 else 2 - 2 * x)))


def make_packet(universe, sequence, slots):
    d = bytearray(126 + 512)
    d[0:2] = b"\x00\x10"
    d[4:16] = b"ASC-E1.17\x00\x00\x00"
    d[111] = sequence & 255
    d[113:115] = struct.pack(">H", universe)
    d[125] = 0                              # start code
    d[126:126 + 512] = slots
    return bytes(d)


def free_port():
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


def start_pipeline_once():
    """Start the helper's real receive thread and (from 0.6.1) its decode thread, listening on a free local port. They are
    daemons and idle between tests."""
    if getattr(lk, "_test_pipeline", False):
        return
    lk._test_pipeline = True
    lk.SACN_PORT = free_port()
    lk.CFG["interface_check_s"] = 3600.0
    lk.sacn_ips = lambda: ["127.0.0.1"]
    threading.Thread(target=lk.sacn_loop, daemon=True).start()
    if hasattr(lk, "sacn_decode_loop"):
        threading.Thread(target=lk.sacn_decode_loop, daemon=True).start()


class Stream:
    """One run of the synthetic Lightkey: sends the frames, keeps the time each one was sent, and collects the helper's log."""

    runs = 0

    def __init__(self, decode_delay=DECODE_DELAY_S, lock_hold_s=0.0):
        """decode_delay: extra decode time per frame. lock_hold_s: another thread (an HTTP request, the relay's key frame) holds
        the helper's state lock for that long every 300 ms."""
        Stream.runs += 1
        self.lock_hold_s = lock_hold_s
        self.lead_level = 100 + Stream.runs     # differs from the end of the run before, so the helper sees a new frame
        self.sent = []          # (universe, level, send time in ms), in send order
        self.decode_delay = decode_delay

    def frames(self):
        levels = [self.lead_level] * LEAD + [ramp_value(k) for k in range(RAMP)] + [0] * TAIL
        return levels

    def changed(self):
        """For each universe the sent frames that differ from the one before (those are the ones that make events)."""
        out = {u: [] for u in UNIVERSES}
        for u in UNIVERSES:
            prev = None
            for (uu, level, t) in self.sent:
                if uu != u:
                    continue
                if prev is not None and level != prev:
                    out[u].append((level, t))
                prev = level
        return out

    def run(self):
        start_pipeline_once()
        port = lk.SACN_PORT
        lk.TRACKER[0] = lk.LevelTracker(lk.Fixtures(lk.expand_map(fixture_map())), 100000)
        lk.LEVELS.clear()
        lk.S["dmx_dropped"] = 0
        if hasattr(lk, "SACN_QUEUE"):
            lk.SACN_QUEUE.clear()
        original = lk.LevelTracker.frame
        delay = self.decode_delay

        def slow_frame(tracker, universe, data, t):
            time.sleep(delay)
            return original(tracker, universe, data, t)

        lk.LevelTracker.frame = slow_frame
        running = [True]

        def hold_the_lock():
            while running[0]:
                time.sleep(0.3)
                with lk.cv:
                    time.sleep(self.lock_hold_s)

        if self.lock_hold_s:
            threading.Thread(target=hold_the_lock, daemon=True).start()
        try:
            time.sleep(0.3)                         # the receiver has opened its socket
            tx = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
            levels = self.frames()
            t0 = time.time() + 0.05
            for k, level in enumerate(levels):
                while time.time() < t0 + k * FRAME_S:
                    time.sleep(0.0005)
                for u in UNIVERSES:
                    packet = make_packet(u, k, bytes([level]) * (3 * PER_UNIVERSE) + bytes(512 - 3 * PER_UNIVERSE))
                    self.sent.append((u, level, time.time() * 1000.0))
                    tx.sendto(packet, ("127.0.0.1", port))
            tx.close()
            expected = sum(len(v) for v in self.changed().values())
            deadline = time.time() + 20
            while lk.TRACKER[0].seq < expected and time.time() < deadline:
                time.sleep(0.05)
            time.sleep(0.2)
        finally:
            running[0] = False
            lk.LevelTracker.frame = original
        self.events = list(lk.TRACKER[0].log)
        return self


def analyse(stream):
    """The numbers the PR quotes: lateness of event times, bunching, flashes, order."""
    changed = stream.changed()
    result = {"events": len(stream.events), "expected": sum(len(v) for v in changed.values()), "late_ms": [], "bunched": 0,
              "intervals": 0, "gaps": [], "gap_errors": [], "reversals": 0, "out_of_order": 0}
    for u in UNIVERSES:
        mine = [e for e in stream.events if e["universe"] == u]
        previous = None
        for ev, (level, sent_ms) in zip(mine, changed[u]):
            result["late_ms"].append(ev["t"] - sent_ms)
            if any(abs(ch["color"][0] * 255 - level) > 0.6 for ch in ev["changes"].values()):
                result["out_of_order"] += 1                    # this event does not carry the frame's own value
            if previous is not None:                           # every fixture of the universe changes in every frame
                gap, sent_gap = ev["t"] - previous[0], sent_ms - previous[1]
                result["intervals"] += 1
                result["gaps"].append(gap)
                result["gap_errors"].append(abs(gap - sent_gap))   # sent_gap is 25 ms, give or take the sending thread's own jitter
                if gap < 5:
                    result["bunched"] += 1
            previous = (ev["t"], sent_ms)
    last_dir, last_val = {}, {}
    for ev in stream.events:
        for name, ch in ev["changes"].items():
            v = ch["color"][0]
            if name in last_val and v != last_val[name]:
                d = 1 if v > last_val[name] else -1
                if last_dir.get(name) is not None and d != last_dir[name]:
                    result["reversals"] += 1                    # the triangle has exactly one turn per fixture
                last_dir[name] = d
            last_val[name] = v
    # a value that goes back and forth on a one-way ramp is a flash; the one turn of every fixture is the ramp itself
    result["flashes"] = max(0, result["reversals"] - PER_UNIVERSE * len(UNIVERSES))
    return result


class StampAtArrivalTests(unittest.TestCase):
    def check(self, r):
        self.assertEqual(r["events"], r["expected"], "one event per changed frame")
        self.assertEqual(r["out_of_order"], 0, "events carry their own frame's value, in order")
        worst = max(abs(x) for x in r["late_ms"])
        self.assertLessEqual(worst, 3, "every event time is within 3 ms of its frame's arrival (worst %.1f ms)" % worst)
        gaps = r["gaps"]
        self.assertEqual(r["bunched"], 0, "no event comes less than 5 ms after the one before it")
        self.assertLessEqual(max(r["gap_errors"]), 3, "events of one address are as far apart as their frames, 25 ms (min %s max %s)" % (min(gaps), max(gaps)))
        self.assertEqual(r["flashes"], 0, "no value goes back and forth on a one-way ramp")

    def eventually(self, make_stream):
        """Three tries: a loaded test machine can delay the sending thread; the old code fails every one of them."""
        failure = None
        for _ in range(3):
            r = analyse(make_stream().run())
            try:
                self.check(r)
                return
            except AssertionError as e:
                failure = e
        raise failure

    def test_events_carry_the_arrival_time_when_decoding_is_slow(self):
        self.eventually(lambda: Stream(decode_delay=DECODE_DELAY_S))

    def test_events_carry_the_arrival_time_when_another_thread_holds_the_lock(self):
        self.eventually(lambda: Stream(decode_delay=0.0, lock_hold_s=0.040))

    def test_kernel_time_stamp_is_used_when_it_is_credible(self):
        now = 1700000000.5
        good = struct.pack("<qi", 1700000000, 400000) + b"\0" * 4
        self.assertAlmostEqual(lk.sacn_kernel_time([(socket.SOL_SOCKET, lk._SO_TIMESTAMP, good)], now), 1700000000.4)
        far = struct.pack("<qi", 1700000100, 0) + b"\0" * 4
        self.assertEqual(lk.sacn_kernel_time([(socket.SOL_SOCKET, lk._SO_TIMESTAMP, far)], now), now)
        self.assertEqual(lk.sacn_kernel_time([(socket.SOL_SOCKET, lk._SO_TIMESTAMP, b"\0")], now), now)
        self.assertEqual(lk.sacn_kernel_time([], now), now)

    def test_a_bad_datagram_is_ignored_and_the_receiver_keeps_reading(self):
        a, b = socket.socket(socket.AF_INET, socket.SOCK_DGRAM), socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        a.bind(("127.0.0.1", 0))
        a.settimeout(2.0)
        for kernel in (False, lk.sacn_enable_kernel_stamp(a)):
            for junk in (b"", b"x" * 10, b"\x00" * 200, b"\x00\x10\x00\x00ASC-E1.17\x00\x00\x00" + b"\x00" * 50):
                b.sendto(junk, a.getsockname())
                self.assertIsNone(lk.sacn_read(a, kernel))
            before = int(time.time() * 1000)
            b.sendto(make_packet(1, 0, bytes(512)), a.getsockname())
            t, d = lk.sacn_read(a, kernel)
            self.assertLessEqual(before - 5, t)
            self.assertLessEqual(t, int(time.time() * 1000) + 5)
            self.assertEqual(len(d), 126 + 512)
        a.close()
        b.close()

    def test_a_full_queue_drops_the_oldest_frame_and_counts_it(self):
        old_max, lk.SACN_QUEUE_MAX = lk.SACN_QUEUE_MAX, 3
        try:
            with lk.SACN_QUEUE_CV:                  # the decoder cannot take frames while the test holds the lock
                lk.SACN_QUEUE.clear()
                lk.S["dmx_dropped"] = 0
                for t in range(1, 6):
                    lk.sacn_enqueue(t, make_packet(1, t, bytes(512)))
                self.assertEqual([item[0] for item in lk.SACN_QUEUE], [3, 4, 5])
                self.assertEqual(lk.S["dmx_dropped"], 2)
                lk.SACN_QUEUE.clear()
        finally:
            lk.SACN_QUEUE_MAX = old_max
            lk.S["dmx_dropped"] = 0

    def test_a_hook_that_raises_does_not_end_the_decoder(self):
        calls = []
        lk.TRACKER[0] = None
        lk.HOOKS["dmx"].append(lambda t, uni: (calls.append((t, uni)), 1 / 0))
        try:
            last = {}
            lk.sacn_frame(last, 1000, 1, make_packet(1, 0, bytes(512)))
            with self.assertRaises(ZeroDivisionError):
                lk.sacn_frame(last, 1025, 1, make_packet(1, 1, bytes([1]) + bytes(511)))
            self.assertEqual(calls, [(1025, 1)])
            start_pipeline_once()
            for t, level in ((1050, 2), (1075, 3), (1100, 4)):       # universe 7: the first frame only sets the baseline
                lk.sacn_enqueue(t, make_packet(7, level, bytes([level]) + bytes(511)))
            deadline = time.time() + 5
            while len(calls) < 3 and time.time() < deadline:
                time.sleep(0.02)
        finally:
            lk.HOOKS["dmx"].pop()
        self.assertGreaterEqual(len(calls), 3, "the decoder thread kept going after the hook raised")


if __name__ == "__main__":
    unittest.main()

