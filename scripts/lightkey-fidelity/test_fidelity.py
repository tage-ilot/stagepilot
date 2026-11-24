"""Tests of the Lightkey fidelity tool on synthetic captures (no rig, no private data).

Run: python -m unittest discover -s scripts/lightkey-fidelity -v
"""
import io
import json
import math
import os
import pickle
import struct
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import compare  # noqa: E402
import sniff  # noqa: E402

STEP = compare.DMX_STEP
FRAME = 5                 # ms between frames in the synthetic captures


def cap(kind, channel, params=None, name=""):
    return {"kind": kind, "channel": channel, "settings": [{"lo": 0, "hi": -1, "params": params or {}}],
            "beams": None, "name": name}


def fixture_map():
    """16 RGB-and-dimmer fixtures C0..C15 (the chase) on universe 1, and one mover M1 (16-bit pan and tilt)."""
    fixtures = []
    for n in range(16):
        fixtures.append({
            "name": "C%d" % n, "universe": 1, "address": 10 * n, "profile": "test", "personality": "", "beams": 1,
            "footprint": 4, "panRange": 0.0, "tiltRange": 0.0,
            "capabilities": [cap("Intensity", 0)] + [cap("ColorComponent", 1 + i, {"componentName": c})
                                                     for i, c in enumerate(("Red", "Green", "Blue"))]})
    fixtures.append({
        "name": "M1", "universe": 1, "address": 200, "profile": "test", "personality": "", "beams": 1, "footprint": 5,
        "panRange": 540.0, "tiltRange": 270.0,
        "capabilities": [cap("Pan", 0), cap("PanFine", 1), cap("Tilt", 2), cap("TiltFine", 3), cap("Intensity", 4)]})
    return {"version": 1, "project": "synthetic", "fixtures": fixtures}


def chase(ms):
    """Which chase fixture is lit at `ms`: one at a time, 0.2 s each (a loop of 3.2 s)."""
    return (max(ms, 0) // 200) % 16


def slots_at(ms, extra=None):
    """The 512 slots of universe 1 at `ms` milliseconds. `extra` = {slot: value} overrides (a flash)."""
    s = bytearray(512)
    on = chase(ms)
    for n in range(16):
        if n == on:
            s[10 * n] = 255
            s[10 * n + 1] = 255
    t = max(ms, 0) / 1000.0
    pan = int(round((0.5 + 0.3 * math.sin(2 * math.pi * t / 4.0)) * 65535))
    tilt = int(round((0.5 + 0.2 * math.cos(2 * math.pi * t / 5.0)) * 65535))
    s[200], s[201], s[202], s[203], s[204] = pan >> 8, pan & 255, tilt >> 8, tilt & 255, 255
    for slot, v in (extra or {}).items():
        s[slot] = v
    return bytes(s)


def capture(path, state, seconds, wall0, period=FRAME):
    """A capture file: a frame every `period` ms of `state(ms)`, from wall time `wall0` (seconds)."""
    frames = []
    for k in range(int(round(seconds * 1000)) // period + 1):
        frames.append((wall0 + k * period / 1000.0, 1, k % 256, state(k * period)))
    with open(path, "wb") as f:
        pickle.dump(frames, f, protocol=4)
    return path


class Base(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.fixtures = os.path.join(self.tmp.name, "fixtures.json")
        with open(self.fixtures, "w") as f:
            json.dump(fixture_map(), f)

    def path(self, name):
        return os.path.join(self.tmp.name, name)

    def run_compare(self, a, b, **kw):
        return compare.analyse(self.fixtures, a, b, **kw)

    @staticmethod
    def row(result, cls):
        return next(r for r in result["rows"] if r["class"] == cls)


class ShiftAndFlash(Base):
    def test_shift_40_ms_with_one_flash(self):
        a = capture(self.path("a.pkl"), lambda ms: slots_at(ms), 8.0, 1000.0)
        # B plays the same show 40 ms later, on another clock, and C3's dimmer flashes on for one 25 ms sample
        extra = lambda ms: {30: 255} if 1340 <= ms < 1365 else None    # A's grid second 1.3 as B plays it; C3 is off
        b = capture(self.path("b.pkl"), lambda ms: slots_at(ms - 40, extra(ms)), 8.0, 5000.0)
        r = self.run_compare(a, b)
        self.assertAlmostEqual(r["offset"], 0.040, delta=0.025)
        self.assertEqual(round(r["offset"] * 1000), 40)
        bright = self.row(r, "brightness")
        self.assertEqual(bright["flashes_b_total"], 1)
        self.assertEqual(bright["flashes_a_total"], 0)
        self.assertEqual(round(bright["over_step"] * bright["samples"]), 1)     # only the flash sample differs
        for cls in ("colour", "pan/tilt"):
            self.assertEqual(self.row(r, cls)["over_step"], 0.0, cls)
        self.assertEqual(self.row(r, "pan/tilt")["fixtures"], 1)
        self.assertEqual(self.row(r, "brightness")["fixtures"], 17)
        self.assertFalse(r["indistinguishable"])
        self.assertTrue(any("one-frame flashes" in w for w in r["reasons"]))
        worst = compare.examples(r["addresses"]["/fixture/C3/overrides/dimmer"])
        self.assertEqual(len(worst), 1)
        self.assertAlmostEqual(worst[0][0], 1.3, delta=1e-6)

    def test_identical_captures_report_zero(self):
        a = capture(self.path("a.pkl"), lambda ms: slots_at(ms), 6.0, 1000.0)
        r = self.run_compare(a, a)
        self.assertEqual(r["offset"], 0.0)
        for row in r["rows"]:
            if row["samples"]:
                self.assertEqual(row["over_step"], 0.0)
                self.assertEqual(row["over_002"], 0.0)
                self.assertEqual(row["max"], 0.0)
                self.assertEqual(row["flashes_a_total"], row["flashes_b_total"])
        self.assertTrue(r["indistinguishable"])
        self.assertEqual(r["spread"], 0.0)

    def test_same_show_on_another_clock_is_not_an_offset(self):
        a = capture(self.path("a.pkl"), lambda ms: slots_at(ms), 6.0, 1000.0)
        b = capture(self.path("b.pkl"), lambda ms: slots_at(ms), 6.0, 987654.0)
        self.assertEqual(self.run_compare(a, b)["offset"], 0.0)

    def test_one_late_fixture_shows_in_the_offset_spread(self):
        a = capture(self.path("a.pkl"), lambda ms: slots_at(ms), 8.0, 1000.0)

        def late(ms):
            s = bytearray(slots_at(ms - 40))
            later = slots_at(ms - 70)                # C5 plays 30 ms later than everything else
            s[50], s[51] = later[50], later[51]
            return bytes(s)
        b = capture(self.path("b.pkl"), late, 8.0, 5000.0)
        r = self.run_compare(a, b)
        self.assertEqual(round(r["offset"] * 1000), 40)
        self.assertAlmostEqual(r["spread"], 0.030, delta=0.010)
        self.assertIn("/C5/", r["spread_address"])

    def test_window_and_fixed_alignment(self):
        a = capture(self.path("a.pkl"), lambda ms: slots_at(ms), 8.0, 1000.0)
        b = capture(self.path("b.pkl"), lambda ms: slots_at(ms - 40), 8.0, 5000.0)
        r = self.run_compare(a, b, lo=2.0, hi=5.0, align_mode="0.04")
        self.assertEqual(r["window"], (2.0, 5.0))
        self.assertEqual(r["offset"], 0.04)
        self.assertEqual(self.row(r, "brightness")["over_step"], 0.0)
        r = self.run_compare(a, b, lo=2.0, hi=5.0, align_mode="none")
        self.assertGreater(self.row(r, "brightness")["over_step"], 0.01)


class Fades(Base):
    @staticmethod
    def fade(ms):
        s = bytearray(512)
        s[0] = int(round(255 * min(max(ms, 0) / 8000.0, 1.0)))
        return bytes(s)

    @staticmethod
    def stairs(ms):
        s = bytearray(512)
        s[0] = int(round(255 * min(max(ms, 0) // 1000 / 8.0, 1.0)))     # a new value once a second
        return bytes(s)

    def setUp(self):
        super().setUp()
        with open(self.fixtures, "w") as f:
            json.dump({"version": 1, "project": "fade", "fixtures": [{
                "name": "C0", "universe": 1, "address": 0, "profile": "test", "personality": "", "beams": 1,
                "footprint": 1, "panRange": 0.0, "tiltRange": 0.0, "capabilities": [cap("Intensity", 0)]}]}, f)

    def test_stair_stepped_fade_is_clearly_worse_than_a_smooth_one(self):
        ref = capture(self.path("ref.pkl"), self.fade, 8.0, 1000.0)
        smooth = capture(self.path("smooth.pkl"), self.fade, 8.0, 2000.0, period=25)
        stair = capture(self.path("stair.pkl"), self.stairs, 8.0, 3000.0, period=25)
        good = self.row(self.run_compare(ref, smooth), "brightness")
        bad = self.row(self.run_compare(ref, stair), "brightness")
        self.assertLessEqual(good["p99"], STEP + 1e-9)
        self.assertGreater(bad["p99"], 10 * STEP)
        self.assertGreater(bad["p99"], 10 * good["p99"])
        self.assertGreater(bad["over_step"], 0.5)

    def test_saved_song_against_its_capture(self):
        """--saved: the song's 25 ms grid is the reference; the capture starts 0.5 s before song second 2.0."""
        grid = {"song_title": "synthetic", "step_seconds": 0.025, "from_seconds": 2.0, "to_seconds": 6.0,
                "addresses": {"/fixture/C0/overrides/dimmer": [[round(min((2.0 + k * 0.025) / 8.0, 1.0), 4)] for k in range(161)]}}
        song = self.path("song.json")
        with open(song, "w") as f:
            json.dump(grid, f)
        a = capture(self.path("a.pkl"), lambda ms: self.fade(ms + 1500), 6.0, 1000.0, period=25)
        r = compare.analyse(self.fixtures, a, saved_path=song)
        self.assertTrue(r["saved"])
        self.assertAlmostEqual(r["offset"], -1.5, delta=0.025)
        row = self.row(r, "brightness")
        self.assertEqual(row["fixtures"], 1)
        self.assertLessEqual(row["p99"], STEP + 1e-9)
        out = io.StringIO()
        compare.report(r, out)
        self.assertIn("saved song", out.getvalue())


class Cli(Base):
    def test_main_prints_a_report_and_check_sets_the_exit_code(self):
        a = capture(self.path("a.pkl"), lambda ms: slots_at(ms), 4.0, 1000.0)
        b = capture(self.path("b.pkl"), lambda ms: slots_at(ms - 200), 4.0, 5000.0)
        argv = ["--fixtures", self.fixtures, "--a", a, "--b", a, "--check"]
        old, sys.stdout = sys.stdout, io.StringIO()
        try:
            code = compare.main(argv)
            text = sys.stdout.getvalue()
            self.assertEqual(code, 0)
            self.assertIn("INDISTINGUISHABLE", text)
            self.assertIn("brightness", text)
            self.assertEqual(compare.main(["--fixtures", self.fixtures, "--a", a, "--b", b, "--align", "none", "--check"]), 1)
            self.assertEqual(compare.main(["--fixtures", self.fixtures, "--a", a, "--b", b, "--align", "none"]), 0)
        finally:
            sys.stdout = old

    def test_bad_input_is_an_error_not_a_traceback(self):
        junk = self.path("junk.pkl")
        with open(junk, "wb") as f:
            f.write(b"not a pickle")
        old, sys.stderr = sys.stderr, io.StringIO()
        try:
            self.assertEqual(compare.main(["--fixtures", self.fixtures, "--a", junk, "--b", junk]), 2)
            self.assertIn("cannot read capture", sys.stderr.getvalue())
        finally:
            sys.stderr = old

    def test_a_capture_may_not_name_a_global(self):
        evil = self.path("evil.pkl")
        with open(evil, "wb") as f:
            pickle.dump(os.getcwd, f)                   # a pickle of a function: needs the global os.getcwd
        with self.assertRaises(compare.CompareError):
            compare.load_capture(evil)

    def test_helper_folder_stays_clean(self):
        folder = os.path.dirname(os.path.abspath(compare.load_helper().__file__))
        self.assertFalse(os.path.exists(os.path.join(folder, "__pycache__")))


class Sniff(unittest.TestCase):
    @staticmethod
    def packet(universe, seq, slots, start_code=0):
        d = bytearray(126 + len(slots))
        d[4:16] = b"ASC-E1.17\x00\x00\x00"
        d[111] = seq
        d[113:115] = struct.pack(">H", universe)
        d[123:125] = struct.pack(">H", len(slots) + 1)
        d[125] = start_code
        d[126:] = slots
        return bytes(d)

    def test_parse_frame(self):
        u, seq, slots = sniff.parse_frame(self.packet(2, 7, bytes([1, 2, 3])))
        self.assertEqual((u, seq, len(slots), slots[:4]), (2, 7, 512, bytes([1, 2, 3, 0])))
        self.assertIsNone(sniff.parse_frame(self.packet(2, 7, bytes([1]), start_code=0xDD)))
        self.assertIsNone(sniff.parse_frame(b"\x00" * 130))
        self.assertIsNone(sniff.parse_frame(b"short"))

    def test_group(self):
        self.assertEqual(sniff.group(1), "239.255.0.1")
        self.assertEqual(sniff.group(258), "239.255.1.2")

    def test_sniff_never_sends(self):
        with open(os.path.join(os.path.dirname(os.path.abspath(sniff.__file__)), "sniff.py")) as f:
            source = f.read()
        for word in ("sendto", ".send(", "sendall"):
            self.assertNotIn(word, source)


if __name__ == "__main__":
    unittest.main()
