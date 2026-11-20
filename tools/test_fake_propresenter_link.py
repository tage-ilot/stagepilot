"""Tests of tools/fake_propresenter_link.py: that it is the bundled helper with only its screen replaced (a request is signed and
answered exactly like the real thing), and what its stand-in screen does.

    python3 -m unittest tools/test_fake_propresenter_link.py
"""

import hashlib
import json
import os
import sys
import tempfile
import threading
import time
import unittest
import urllib.error
import urllib.request

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import fake_propresenter_link as fake  # noqa: E402

KEY = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-_AbCdE"


class Rig:
    def __init__(self, playback=True, version=None):
        self.helper = fake.load_helper()
        self.dir = tempfile.mkdtemp(prefix="fake-ppl-test-")
        self.folder = fake.make_folder(self.dir, playback)
        keyfile = os.path.join(self.dir, "key.txt")
        with open(keyfile, "w") as f:
            f.write(KEY + "\n")
        args = fake.argparse.Namespace(port=0, bind="127.0.0.1", allow_client=[], allow_loopback=True, version=version or self.helper.__version__)
        fake.configure(self.helper, args, KEY, self.folder)
        self.server, self.base = fake.serve(self.helper, args)

    def stop(self):
        self.server.shutdown()
        self.server.server_close()

    def call(self, method, path, body=None, key=KEY, stamp=None, headers=None):
        stamp = str(int(time.time())) if stamp is None else str(stamp)
        sent = {"X-PPL-Time": stamp}
        if key is not None:
            sent["X-PPL-Sig"] = self.helper.sign(key, path, stamp, method, body if method in ("PUT", "POST") else None)
        sent.update(headers or {})
        request = urllib.request.Request(self.base + path, data=body, method=method, headers=sent)
        try:
            with urllib.request.urlopen(request, timeout=5) as r:
                return r.status, r.read()
        except urllib.error.HTTPError as e:
            try:
                return e.code, e.read()
            finally:
                e.close()


class FakeProPresenterLinkTests(unittest.TestCase):
    def setUp(self):
        self.rig = Rig()
        self.addCleanup(self.rig.stop)

    def test_it_is_the_real_helper_with_the_same_key_check(self):
        status, body = self.rig.call("GET", "/health")
        self.assertEqual(status, 200)
        health = json.loads(body)
        self.assertEqual(health["auth"], "key")
        self.assertTrue(health["can_import"])
        self.assertEqual(health["libraries"], ["Playback", "Songs"])
        self.assertEqual(self.rig.call("GET", "/health", key="x" * 43)[0], 401)
        self.assertEqual(self.rig.call("GET", "/index", stamp=1)[0], 401)
        status, body = self.rig.call("GET", "/index")
        self.assertEqual([(s["file"], s["multitracks_id"]) for s in json.loads(body)["songs"]], [("Alpha.pro", 1001), ("Beta.pro", 1002)])

    def test_the_stand_in_screen_imports_into_songs_and_the_index_sees_it(self):
        body = json.dumps({"id": "5555", "name": "New Song", "playlist": "Sunday"}).encode()
        status, answer = self.rig.call("POST", "/import", body)
        self.assertEqual((status, json.loads(answer)), (200, {"file": "New Song.pro"}))
        self.assertTrue(os.path.isfile(os.path.join(self.rig.folder, "Libraries", "Songs", "New Song.pro")))
        status, answer = self.rig.call("GET", "/index")
        self.assertIn({"library": "Songs", "file": "New Song.pro", "multitracks_id": 5555}, json.loads(answer)["songs"])
        self.assertEqual(self.rig.call("POST", "/import", body)[0], 409)
        for bad in ({"id": "12a", "name": "X"}, {"id": "1", "name": "A.B"}, {"id": "1", "name": ""}):
            self.assertEqual(self.rig.call("POST", "/import", json.dumps(bad).encode())[0], 400, bad)

    def test_a_new_file_goes_only_to_playback_and_never_replaces(self):
        data = b"synthetic bytes"
        digest = hashlib.sha256(data).hexdigest()
        path = "/libraries/Playback/files/Alpha%20%281%29.pro"
        self.assertEqual(self.rig.call("PUT", path, data, headers={"X-Sha256": digest})[0], 200)
        self.assertEqual(self.rig.call("PUT", path, data, headers={"X-Sha256": digest})[0], 409)
        self.assertEqual(self.rig.call("PUT", "/libraries/Songs/files/X.pro", data, headers={"X-Sha256": digest})[0], 403)
        self.assertEqual(self.rig.call("PUT", "/libraries/Playback/files/Y.pro", data, headers={"X-Sha256": "0" * 64})[0], 422)
        self.assertFalse(os.path.exists(os.path.join(self.rig.folder, "Libraries", "Playback", "Y.pro")))
        status, body = self.rig.call("GET", "/libraries/Playback/files")
        self.assertEqual([f["name"] for f in json.loads(body)["files"]], ["Alpha (1).pro"])

    def test_select_library_says_it_tried_and_a_bad_name_is_refused(self):
        status, body = self.rig.call("POST", "/select-library", json.dumps({"name": "Playback"}).encode())
        self.assertEqual((status, json.loads(body)["tried"]), (200, True))
        self.assertEqual(self.rig.call("POST", "/select-library", json.dumps({"name": "../x"}).encode())[0], 400)

    def test_a_computer_that_is_not_the_named_one_is_refused(self):
        self.helper = self.rig.helper
        self.helper.CFG.update(allow_loopback=False, allow_clients=["192.168.1.20"])
        self.assertEqual(self.rig.call("GET", "/health")[0], 403)


class FakeVersionTests(unittest.TestCase):
    def test_an_older_version_can_be_shown(self):
        rig = Rig(version="0.0.9")
        self.addCleanup(rig.stop)
        status, body = rig.call("GET", "/health")
        self.assertEqual(json.loads(body)["version"], "0.0.9")


if __name__ == "__main__":
    unittest.main()
