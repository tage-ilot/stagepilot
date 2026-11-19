"""Tests of tools/fake_playback_library.py: that it checks keys exactly like the real helper (the bundled
playback_library.py is imported and its `sign` / `signature_ok` are compared), and what it serves.

    python3 -m unittest tools/test_fake_playback_library.py
"""

import datetime
import importlib.util
import json
import os
import sys
import tempfile
import threading
import time
import unittest
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import fake_playback_library as fake  # noqa: E402

HELPER = os.path.join(HERE, "..", "crates", "stagepilot-playback", "assets", "playback-helper", "playback_library.py")
spec = importlib.util.spec_from_file_location("real_playback_library", HELPER)
real = importlib.util.module_from_spec(spec)
spec.loader.exec_module(real)

KEY = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-_AbCdE"


def serve(*extra, key=KEY):
    handle = tempfile.NamedTemporaryFile("w", suffix=".key", delete=False)
    if key is not None:
        handle.write(key + "\n")
    handle.close()
    args = fake.argparse.Namespace(port=0, bind="127.0.0.1", key_file=handle.name, version="0.3.0", old=False, date="2026-11-22", no_setlist_on=[])
    for name, value in extra:
        setattr(args, name, value)
    server = fake.Server(("127.0.0.1", 0), fake.Handler)
    server.state = fake.State(args)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, "http://127.0.0.1:%d" % server.server_address[1], handle.name


def stop(server):
    server.shutdown()
    server.server_close()


def get(base, path, key=KEY, stamp=None, headers=None):
    request = urllib.request.Request(base + path, headers=dict(headers or {}))
    if key:
        stamp = str(int(time.time())) if stamp is None else str(stamp)
        request.add_header("X-PBL-Time", stamp)
        request.add_header("X-PBL-Sig", real.sign(key, path, stamp))
    try:
        with urllib.request.urlopen(request, timeout=5) as response:
            return response.status, response.read(), response.headers
    except urllib.error.HTTPError as error:
        return error.code, error.read(), error.headers


class Fake(unittest.TestCase):
    def setUp(self):
        self.server, self.base, self.path = serve()
        self.addCleanup(stop, self.server)
        self.addCleanup(os.unlink, self.path)

    def test_the_signature_is_the_real_helpers(self):
        for target in ("/health", "/songs/5/waveform?points_per_second=8&clock=performed"):
            self.assertEqual(fake.sign(KEY, target, "1794945365"), real.sign(KEY, target, "1794945365"))
        self.assertTrue(fake.signature_ok(KEY, "/health", "1000", real.sign(KEY, "/health", "1000"), 1100))
        self.assertFalse(fake.signature_ok(KEY, "/health", "1000", real.sign(KEY, "/health", "1000"), 1121))
        self.assertEqual(
            fake.signature_ok(KEY, "/x", "1000", "00", 1000), real.signature_ok(KEY, "/x", "1000", "00", 1000)
        )

    def test_a_bare_health_is_minimal_and_a_signed_one_is_full(self):
        status, body, _ = get(self.base, "/health", key=None)
        self.assertEqual(status, 200)
        self.assertEqual(json.loads(body), {"ok": True, "name": "playback-library", "version": "0.3.0", "auth": "required"})
        status, body, _ = get(self.base, "/health")
        health = json.loads(body)
        self.assertEqual((status, health["auth"], health["database"]), (200, "key", "ok"))
        self.assertIn("library_revision", health)

    def test_everything_else_needs_a_good_signature_in_time(self):
        self.assertEqual(get(self.base, "/setlists/latest", key=None)[0], 401)
        self.assertEqual(get(self.base, "/setlists/latest", key="X" * 43)[0], 401)
        self.assertEqual(get(self.base, "/setlists/latest", stamp=int(time.time()) - 500)[0], 401)
        self.assertEqual(get(self.base, "/setlists/latest")[0], 200)

    def test_ten_bad_signatures_lock_the_address_out(self):
        for _ in range(10):
            get(self.base, "/setlists/latest", key="X" * 43)
        status, _, headers = get(self.base, "/health")
        self.assertEqual(status, 429)
        self.assertTrue(headers["Retry-After"])

    def test_six_songs_for_the_date_and_one_for_any_other(self):
        status, body, _ = get(self.base, "/setlists/2026-11-22")
        self.assertEqual(status, 200)
        items = json.loads(body)["items"]
        self.assertEqual(len(items), 6)
        self.assertEqual([i["order"] for i in items], [1, 2, 3, 4, 5, 6])
        self.assertTrue(all(i["artwork"]["type"] == "image/jpeg" for i in items))
        other = json.loads(get(self.base, "/setlists/2026-11-29")[1])
        self.assertEqual([i["title"] for i in other["items"]], ["Battle Belongs"])
        self.assertEqual(get(self.base, "/setlists/not-a-date")[0], 404)

    def test_a_setlist_is_found_by_its_number_and_a_date_can_have_none(self):
        listed = json.loads(get(self.base, "/setlists")[1])["setlists"]
        self.assertTrue(all(isinstance(s["setlist_id"], int) and s["songs"] >= 1 for s in listed))
        by_number = json.loads(get(self.base, "/setlists/20261122")[1])
        self.assertEqual((by_number["setlist_id"], by_number["performance_date"], len(by_number["items"])), (20261122, "2026-11-22", 6))
        self.assertEqual(get(self.base, "/setlists/9999")[0], 404)
        self.server.state.args.no_setlist_on = ["2026-12-06"]
        self.assertEqual(get(self.base, "/setlists/2026-12-06")[0], 404)
        self.assertEqual(get(self.base, "/setlists/2026-11-29")[0], 200)

    def test_a_song_has_sections_a_cover_and_a_waveform(self):
        item = json.loads(get(self.base, "/setlists/2026-11-22")[1])["items"][0]
        sid = item["setlist_song_id"]
        detail = json.loads(get(self.base, "/songs/%d" % sid)[1])
        # The MultiTracks number: in the setlist and in the song's own answer, the same, and not the setlist-song number.
        self.assertIsInstance(item["content_id"], int)
        self.assertEqual(detail["content_id"], item["content_id"])
        self.assertNotEqual(item["content_id"], sid)
        # A Song Master ID looks like the real ones (4 to 6 digits); the setlist song number is a much longer one.
        self.assertTrue(1_000 <= item["content_id"] <= 999_999, item["content_id"])
        self.assertEqual(item["content_id"], fake.song_master_id(sid))
        sections = detail["arrangement_detail"]["sections"]
        self.assertGreater(len(sections), 5)
        self.assertAlmostEqual(sections[-1]["performed_end"], item["length_s"], places=2)
        status, jpeg, headers = get(self.base, item["artwork"]["path"])
        self.assertEqual((status, jpeg[:3], headers["ETag"]), (200, b"\xff\xd8\xff", item["artwork"]["etag"]))
        self.assertEqual(len(jpeg), item["artwork"]["bytes"])
        self.assertEqual(get(self.base, item["artwork"]["path"], headers={"If-None-Match": headers["ETag"]})[0], 304)
        wave = json.loads(get(self.base, "/songs/%d/waveform?points_per_second=8&clock=performed" % sid)[1])
        self.assertEqual(wave["step_s"], 0.125)
        self.assertTrue(all(0 <= p <= 1 for p in wave["peak"]))
        self.assertEqual(get(self.base, "/songs/999/waveform")[0], 404)

    def test_no_key_means_everyone_is_answered_and_old_has_no_auth_field(self):
        server, base, path = serve(key=None)
        self.addCleanup(stop, server)
        self.addCleanup(os.unlink, path)
        health = json.loads(get(base, "/health", key=None)[1])
        self.assertEqual(health["auth"], "none")
        self.assertEqual(get(base, "/setlists/latest", key=None)[0], 200)
        server, base, path = serve(("old", True), key=None)
        self.addCleanup(stop, server)
        self.addCleanup(os.unlink, path)
        self.assertNotIn("auth", json.loads(get(base, "/health", key=None)[1]))

    def test_a_key_written_after_start_is_picked_up(self):
        server, base, path = serve(key=None)
        self.addCleanup(stop, server)
        self.addCleanup(os.unlink, path)
        self.assertEqual(get(base, "/setlists/latest", key=None)[0], 200)
        with open(path, "w") as handle:
            handle.write(KEY + "\n")
        self.assertEqual(get(base, "/setlists/latest", key=None)[0], 401)
        self.assertEqual(get(base, "/setlists/latest")[0], 200)

    def test_the_default_date_is_today(self):
        server, base, path = serve(("date", None))
        self.addCleanup(stop, server)
        self.addCleanup(os.unlink, path)
        today = datetime.date.today().isoformat()
        self.assertEqual(len(json.loads(get(base, "/setlists/" + today)[1])["items"]), 6)


class Pairing(unittest.TestCase):
    KEY = "k3y_for-the-script-TEST-0123456789ABCDEFGH"

    def test_the_key_is_found_in_the_script_stagepilot_serves(self):
        script = "#!/bin/sh\nTMP=$(mktemp -d)\n  printf '%s\\n' '" + self.KEY + "' > \"$TMP/key.txt\"\n  chmod 600 \"$TMP/key.txt\"\n"
        self.assertEqual(fake.key_from_script(script), self.KEY)
        self.assertIsNone(fake.key_from_script("#!/bin/sh\necho nothing here\n"))
        self.assertIsNone(fake.key_from_script("printf '%s\\n' 'short' > \"$TMP/key.txt\""))


if __name__ == "__main__":
    unittest.main()