"""The ProPresenter Link helper (propresenter_link.py, the copy StagePilot carries): the key check (good, wrong, old, ten bad then 429,
a body that does not match its signature), who is answered (loopback, the named StagePilot computer), the path rules (escapes, symlinks),
create-only writes into the library Playback (hash mismatch, size limit, other libraries), the song index on synthetic `.pro` bytes, the
screen scripts (arguments only, steps, refusals) against a stand-in for osascript, and the signed self-update. No ProPresenter, no Mac.

Run:  python3 -m unittest discover -s scripts/propresenter-link-tests -v   (Python 3.9 or newer)"""
import base64
import hashlib
import http.client
import json
import logging
import os
import sys
import tempfile
import threading
import time
import unittest

sys.dont_write_bytecode = True      # never leave a __pycache__ inside the helper folder: it is a bundle of plain files
HERE = os.path.dirname(os.path.abspath(__file__))
HELPER = os.path.join(HERE, "..", "..", "crates", "stagepilot-propresenter", "assets", "propresenter-link")
GUI = os.path.join(HERE, "..", "..", "crates", "stagepilot-propresenter", "assets", "gui")
sys.path.insert(0, HERE)
sys.path.insert(0, HELPER)
import propresenter_link as ppl  # noqa: E402
import signing  # noqa: E402
from rfc8032_vectors import RFC8032_VECTORS  # noqa: E402

ppl.log.addHandler(logging.NullHandler())      # a refused request is not printed to the screen
KEY = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-_AbCdE"
REAL_PUBLIC_KEY = "Evky97ydM5zWJ02p6Wc7RguEHMglX3WRxiPrege4Op8="


def varint(value):
    out = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        if not value:
            out.append(byte)
            return bytes(out)
        out.append(byte | 0x80)


def song(song_id=None, filler=40):
    """A synthetic .pro: a text field, filler, and the licence record (field 21 holding field 1) when an id is given."""
    out = b"\x0a\x04info\x12" + varint(filler) + b"\x07" * filler
    if song_id is not None:
        record = b"\x08" + varint(song_id)
        out += b"\xaa\x01" + varint(len(record)) + record
    return out


def sha(data):
    return hashlib.sha256(data).hexdigest()


class Rig:
    """The helper on a temporary ProPresenter folder, served on a loopback port; loopback is allowed unless a test says otherwise."""

    def __init__(self, allow_loopback=True, allow_clients=()):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = os.path.join(self.tmp.name, "ProPresenter")
        for lib in ("Songs", "Playback"):
            os.makedirs(os.path.join(self.root, "Libraries", lib))
        ppl.CFG.clear()
        ppl.CFG.update(ppl.DEFAULTS)
        ppl.CFG.update(key=KEY, allow_loopback=allow_loopback, allow_clients=list(allow_clients), data_folder=self.root)
        ppl.FOLDER = ppl.Folder(self.root)
        ppl.THROTTLE = ppl.FailureThrottle()
        ppl.ACCESS.update(at=0.0, ok=False, blocker=None)
        self.server = ppl.Server(("127.0.0.1", 0), ppl.Handler)
        self.port = self.server.server_address[1]
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.tmp.cleanup()

    def lib(self, name):
        return os.path.join(self.root, "Libraries", name)

    def request(self, method, path, body=None, key=KEY, stamp=None, headers=None, sign_body=None, sig=None):
        """One request. The signature covers `sign_body` (default: the body); `sig` overrides it entirely."""
        stamp = str(int(time.time())) if stamp is None else str(stamp)
        sent = {"X-PPL-Time": stamp}
        if sig is not None:
            sent["X-PPL-Sig"] = sig
        elif key is not None:
            covered = (body if sign_body is None else sign_body) if method in ("PUT", "POST") else None
            sent["X-PPL-Sig"] = ppl.sign(key, path, stamp, method, covered)
        if body is not None:
            sent["Content-Length"] = str(len(body))
        elif method in ("PUT", "POST"):
            sent["Content-Length"] = "0"
        sent.update(headers or {})
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=10)
        try:
            conn.request(method, path, body=body, headers=sent)
            r = conn.getresponse()
            return r.status, r.read(), dict(r.getheaders())
        finally:
            conn.close()

    def json(self, method, path, value=None, **kw):
        status, data, _ = self.request(method, path, None if value is None else json.dumps(value).encode(), **kw)
        return status, json.loads(data or b"null")


class Base(unittest.TestCase):
    def setUp(self):
        self.rig = Rig()
        self.addCleanup(self.rig.close)


class KeyTests(Base):
    def test_a_signed_request_is_answered_and_health_says_what_this_computer_can_do(self):
        status, health = self.rig.json("GET", "/health")
        self.assertEqual(status, 200)
        self.assertEqual((health["name"], health["version"], health["auth"], health["updater"]), ("propresenter-link", ppl.__version__, "key", 1))
        self.assertEqual(health["libraries"], ["Playback", "Songs"])
        self.assertTrue(health["playback_library_exists"] and health["data_folder_exists"])
        self.assertFalse(health["can_import"] and sys.platform != "darwin")
        self.assertEqual(health["data_folder"], self.rig.root)

    def test_the_bare_health_says_only_that_a_key_is_needed(self):
        conn = http.client.HTTPConnection("127.0.0.1", self.rig.port, timeout=5)
        conn.request("GET", "/health")
        r = conn.getresponse()
        self.assertEqual((r.status, json.loads(r.read())), (200, {"ok": True, "name": "propresenter-link", "version": ppl.__version__, "auth": "required"}))
        conn.close()

    def test_a_wrong_key_an_old_time_a_future_time_and_a_missing_signature_are_401(self):
        self.assertEqual(self.rig.request("GET", "/index", key="x" * 43)[0], 401)
        self.assertEqual(self.rig.request("GET", "/index", stamp=int(time.time()) - 121)[0], 401)
        self.assertEqual(self.rig.request("GET", "/index", stamp=int(time.time()) + 121)[0], 401)
        self.assertEqual(self.rig.request("GET", "/index", key=None)[0], 401)
        self.assertEqual(self.rig.request("GET", "/index", stamp="12ab")[0], 401)
        self.assertEqual(self.rig.request("GET", "/index", stamp=int(time.time()) - 119)[0], 200, "inside the window")

    def test_a_signature_is_for_one_path_and_one_method(self):
        sig = ppl.sign(KEY, "/health", str(int(time.time())), "GET")
        self.assertEqual(self.rig.request("GET", "/index", sig=sig)[0], 401)
        sig = ppl.sign(KEY, "/index", str(int(time.time())), "GET")
        self.assertEqual(self.rig.request("POST", "/libraries/Sunday", b"{}", sig=sig)[0], 401)

    def test_ten_bad_signatures_lock_that_address_for_a_minute_and_the_right_key_waits_too(self):
        for _ in range(10):
            self.assertEqual(self.rig.request("GET", "/index", key="x" * 43)[0], 401)
        status, _body, headers = self.rig.request("GET", "/index", key="x" * 43)
        self.assertEqual(status, 429)
        self.assertTrue(1 <= int(headers["Retry-After"]) <= 61)
        self.assertEqual(self.rig.request("GET", "/index")[0], 429, "the right key does not get in while the block lasts")
        self.assertEqual(self.rig.request("PUT", "/libraries/Playback/files/A.pro", b"x", headers={"X-Sha256": sha(b"x")})[0], 429)

    def test_a_body_that_is_not_the_signed_body_is_refused_and_nothing_is_written(self):
        data = b"real bytes"
        status, _, _ = self.rig.request("PUT", "/libraries/Playback/files/A.pro", b"swapped bytes", sign_body=data, headers={"X-Sha256": sha(data)})
        self.assertEqual(status, 401)
        self.assertFalse(os.path.exists(os.path.join(self.rig.lib("Playback"), "A.pro")))
        status, _, _ = self.rig.request("POST", "/libraries/Evil", b'{"x": 1}', sign_body=b"{}")
        self.assertEqual(status, 401)
        self.assertFalse(os.path.exists(self.rig.lib("Evil")))
        # And a signature made without the body hash (a GET signature) does not do for a PUT.
        sig = ppl.sign(KEY, "/libraries/Playback/files/B.pro", str(int(time.time())), "PUT")
        self.assertEqual(self.rig.request("PUT", "/libraries/Playback/files/B.pro", data, sig=sig, headers={"X-Sha256": sha(data)})[0], 401)

    def test_no_usable_key_answers_the_bare_health_and_nothing_else(self):
        ppl.CFG["key"] = "short"
        self.assertEqual(self.rig.request("GET", "/index", key="short")[0], 401)
        conn = http.client.HTTPConnection("127.0.0.1", self.rig.port, timeout=5)
        conn.request("GET", "/health")
        self.assertEqual(json.loads(conn.getresponse().read())["auth"], "required")
        conn.close()

    def test_the_key_never_appears_in_an_answer_the_log_or_the_config_it_shows(self):
        for path in ("/health", "/", "/index"):
            self.assertNotIn(KEY.encode(), self.rig.request("GET", path)[1])
        self.assertNotIn(KEY.encode(), self.rig.request("GET", "/index", key="x" * 43)[1])
        self.assertNotIn("key", ppl.INDEX)


class WhoIsAnsweredTests(unittest.TestCase):
    def test_loopback_is_refused_unless_allowed_and_a_named_computer_only_when_it_is_named(self):
        rig = Rig(allow_loopback=False)
        self.addCleanup(rig.close)
        self.assertEqual(rig.request("GET", "/health")[0], 403)
        self.assertEqual(rig.request("GET", "/index")[0], 403)
        for address, want in (("192.168.1.20", True), ("::ffff:192.168.1.20", True), ("192.168.1.21", False), ("8.8.8.8", False), ("127.0.0.1", False), ("nonsense", False)):
            ppl.CFG["allow_clients"] = ["192.168.1.20"]
            self.assertEqual(ppl.client_allowed(address), want, address)
        ppl.CFG["allow_clients"] = ["8.8.8.8"]
        self.assertFalse(ppl.client_allowed("8.8.8.8"), "a public address is never answered")
        ppl.CFG["allow_loopback"] = True
        self.assertTrue(ppl.client_allowed("127.0.0.1"))
        self.assertTrue(ppl.client_allowed("::1"))
        ppl.CFG["allow_clients"] = []
        self.assertFalse(ppl.client_allowed("192.168.1.20"), "nobody is named: nobody is answered")


class FolderTests(Base):
    def test_listing_shows_only_pro_files(self):
        for name, data in (("B.pro", b"bb"), ("A.pro", b"a"), ("notes.txt", b"x"), (".hidden.pro", b"x")):
            with open(os.path.join(self.rig.lib("Songs"), name), "wb") as f:
                f.write(data)
        os.mkdir(os.path.join(self.rig.lib("Songs"), "dir.pro"))
        status, body = self.rig.json("GET", "/libraries/Songs/files")
        self.assertEqual([(f["name"], f["size"]) for f in body["files"]], [("A.pro", 1), ("B.pro", 2)])
        self.assertEqual(self.rig.request("GET", "/libraries/Nope/files")[0], 404)

    def test_a_file_is_read_with_its_checksum(self):
        with open(os.path.join(self.rig.lib("Songs"), "A (1).pro"), "wb") as f:
            f.write(b"hello")
        status, data, headers = self.rig.request("GET", "/libraries/Songs/files/A%20%281%29.pro")
        self.assertEqual((status, data, headers["X-Sha256"]), (200, b"hello", sha(b"hello")))
        self.assertEqual(self.rig.request("GET", "/libraries/Songs/files/Missing.pro")[0], 404)

    def test_paths_that_leave_the_folder_are_refused(self):
        with open(os.path.join(self.rig.root, "secret.pro"), "wb") as f:
            f.write(b"secret")
        for path in ("/libraries/../files", "/libraries/%2e%2e/files", "/libraries/Songs/files/..%2fsecret.pro", "/libraries/Songs/files/%2e%2e",
                     "/libraries/Songs/files/a%2fb.pro", "/libraries/Songs/files/..%5csecret.pro", "/libraries/Songs/files/x.txt",
                     "/libraries/%2fetc/files", "/libraries/.hidden/files", "/libraries/Songs/files/.pro", "/libraries//files"):
            status, body, _ = self.rig.request("GET", path)
            self.assertIn(status, (400, 404), path)
            self.assertNotIn(b"secret", body, path)
        for name in ("..", "a/b", "/abs", ".", ""):
            with self.assertRaises(ppl.Refused, msg=repr(name)):
                ppl.FOLDER.list_library(name)
            with self.assertRaises(ppl.Refused, msg=repr(name)):
                ppl.FOLDER.ensure_library(name)

    @unittest.skipIf(not hasattr(os, "symlink") or sys.platform == "win32", "symlinks")
    def test_a_symlink_that_leads_out_of_the_folder_is_never_followed(self):
        outside = os.path.join(self.rig.tmp.name, "outside")
        os.makedirs(outside)
        with open(os.path.join(outside, "Stolen.pro"), "wb") as f:
            f.write(b"stolen")
        os.symlink(outside, self.rig.lib("Linked"))
        os.symlink(os.path.join(outside, "Stolen.pro"), os.path.join(self.rig.lib("Songs"), "Link.pro"))
        with open(os.path.join(self.rig.lib("Songs"), "Real.pro"), "wb") as f:
            f.write(song(5))
        self.assertEqual(self.rig.request("GET", "/libraries/Linked/files")[0], 400)
        self.assertEqual(self.rig.request("GET", "/libraries/Linked/files/Stolen.pro")[0], 400)
        self.assertEqual(self.rig.request("GET", "/libraries/Songs/files/Link.pro")[0], 400)
        self.assertEqual([f["name"] for f in self.rig.json("GET", "/libraries/Songs/files")[1]["files"]], ["Real.pro"])
        index = self.rig.json("GET", "/index")[1]["songs"]
        self.assertEqual([(s["library"], s["file"]) for s in index], [("Songs", "Real.pro")])
        self.assertNotIn("Linked", self.rig.json("GET", "/health")[1]["libraries"])
        # A link inside the folder is fine; creating over a link is refused as a taken name and never writes through it.
        os.symlink(os.path.join(self.rig.lib("Songs"), "Real.pro"), os.path.join(self.rig.lib("Playback"), "Inside.pro"))
        self.assertEqual(self.rig.request("GET", "/libraries/Playback/files/Inside.pro")[0], 200)
        os.symlink(os.path.join(outside, "Stolen.pro"), os.path.join(self.rig.lib("Playback"), "Trap.pro"))
        data = b"new"
        self.assertEqual(self.rig.request("PUT", "/libraries/Playback/files/Trap.pro", data, headers={"X-Sha256": sha(data)})[0], 409)
        with open(os.path.join(outside, "Stolen.pro"), "rb") as f:
            self.assertEqual(f.read(), b"stolen")

    def test_a_library_is_made_with_a_plain_name_and_never_songs(self):
        self.assertEqual(self.rig.json("POST", "/libraries/Sunday%20Set-2", {}), (200, {"created": True}))
        self.assertTrue(os.path.isdir(self.rig.lib("Sunday Set-2")))
        self.assertEqual(self.rig.json("POST", "/libraries/Sunday%20Set-2", {}), (200, {"created": False}))
        for bad in ("Songs", "songs", "a_b", "a.b", "%20x", "x%20", "-x", "%C3%A9", "..", "a%2Fb"):
            self.assertEqual(self.rig.request("POST", "/libraries/" + bad, b"{}")[0], 400, bad)
        self.assertEqual(sorted(os.listdir(os.path.join(self.rig.root, "Libraries"))), ["Playback", "Songs", "Sunday Set-2"])


class CreateTests(Base):
    def put(self, name, data, digest=None, library="Playback"):
        return self.rig.request("PUT", "/libraries/%s/files/%s" % (library, name), data, headers={"X-Sha256": sha(data) if digest is None else digest})

    def test_a_new_file_is_created_once_and_never_replaced(self):
        data = song(77)
        status, body, _ = self.put("Song%20%281%29.pro", data)
        self.assertEqual((status, json.loads(body)), (200, {"created": "Song (1).pro", "size": len(data)}))
        with open(os.path.join(self.rig.lib("Playback"), "Song (1).pro"), "rb") as f:
            self.assertEqual(f.read(), data)
        self.assertEqual(self.put("Song%20%281%29.pro", b"other")[0], 409)
        with open(os.path.join(self.rig.lib("Playback"), "Song (1).pro"), "rb") as f:
            self.assertEqual(f.read(), data, "untouched")
        with open(os.path.join(self.rig.lib("Playback"), "Original.pro"), "wb") as f:
            f.write(b"orig")
        self.assertEqual(self.put("Original.pro", b"other")[0], 409)

    def test_a_checksum_that_does_not_match_adds_nothing_and_leaves_no_temporary_file(self):
        self.assertEqual(self.put("A.pro", b"abc", sha(b"abd"))[0], 422)
        for bad in ("", "abc", "g" * 64, "0" * 63):
            self.assertEqual(self.put("A.pro", b"abc", bad)[0], 400, bad)
        self.assertEqual(os.listdir(self.rig.lib("Playback")), [])

    def test_only_the_playback_library_takes_files_and_only_pro_files(self):
        for library in ("Songs", "Other", "playback", "Playback%20"):
            self.assertEqual(self.put("A.pro", b"x", library=library)[0], 403, library)
        for name in ("a.txt", ".pro", "x.PRO", "..", "a%2Fb.pro", ".a.pro"):
            self.assertEqual(self.put(name, b"x")[0], 400, name)
        self.assertEqual(os.listdir(self.rig.lib("Songs")), [])
        self.assertEqual(os.listdir(self.rig.lib("Playback")), [])
        os.rmdir(self.rig.lib("Playback"))
        self.assertEqual(self.put("A.pro", b"x")[0], 404, "a file write never makes the library")

    def test_the_size_limit_is_forty_megabytes(self):
        self.assertEqual(ppl.MAX_FILE_BYTES, 40 * 1024 * 1024)
        status, body, _ = self.rig.request("PUT", "/libraries/Playback/files/Big.pro", None, headers={"X-Sha256": "0" * 64, "Content-Length": str(ppl.MAX_FILE_BYTES + 1)})
        self.assertEqual(status, 413)
        with self.assertRaises(ppl.Refused) as caught:
            ppl.FOLDER.create_file("Playback", "Big.pro", b"\0" * (ppl.MAX_FILE_BYTES + 1), "0" * 64)
        self.assertEqual(caught.exception.status, 413)
        big = os.path.join(self.rig.lib("Songs"), "Huge.pro")
        with open(big, "wb") as f:
            f.truncate(ppl.MAX_FILE_BYTES + 1)
        self.assertEqual(self.rig.request("GET", "/libraries/Songs/files/Huge.pro")[0], 413)
        self.assertEqual([s["multitracks_id"] for s in self.rig.json("GET", "/index")[1]["songs"]], [None])

    def test_a_post_or_put_without_a_length_is_refused(self):
        conn = http.client.HTTPConnection("127.0.0.1", self.rig.port, timeout=5)
        stamp = str(int(time.time()))
        conn.putrequest("POST", "/import", skip_accept_encoding=True)
        conn.putheader("X-PPL-Time", stamp)
        conn.putheader("X-PPL-Sig", ppl.sign(KEY, "/import", stamp, "POST", b""))
        conn.endheaders()
        self.assertEqual(conn.getresponse().status, 411)
        conn.close()


class IndexTests(Base):
    def write(self, library, name, data):
        with open(os.path.join(self.rig.lib(library), name), "wb") as f:
            f.write(data)

    def test_the_index_finds_a_valid_licence_record_and_nothing_in_the_rest(self):
        self.write("Songs", "Valid.pro", song(4000001, filler=300000))
        self.write("Songs", "NoRecord.pro", song(None))
        self.write("Songs", "Garbage.pro", b"not a protobuf at all " + b"\xff" * 12)
        whole = song(9)
        self.write("Playback", "Truncated.pro", whole[:-2])
        self.write("Playback", "Empty.pro", b"")
        self.write("Playback", "readme.txt", song(1))
        self.write("Playback", "Zero.pro", song(0))
        status, body = self.rig.json("GET", "/index")
        got = [(s["library"], s["file"], s["multitracks_id"]) for s in body["songs"]]
        self.assertEqual(got, [("Playback", "Empty.pro", None), ("Playback", "Truncated.pro", None), ("Playback", "Zero.pro", None),
                               ("Songs", "Garbage.pro", None), ("Songs", "NoRecord.pro", None), ("Songs", "Valid.pro", 4000001)])

    def test_an_unchanged_file_is_not_read_again_and_a_changed_one_is(self):
        self.write("Songs", "A.pro", song(11))
        self.assertEqual(self.rig.json("GET", "/index")[1]["songs"][0]["multitracks_id"], 11)
        calls = []
        real = ppl.multitracks_id
        ppl.multitracks_id = lambda path: calls.append(path) or real(path)
        try:
            self.rig.json("GET", "/index")
            self.assertEqual(calls, [])
            self.write("Songs", "A.pro", song(12, filler=41))
            self.assertEqual(self.rig.json("GET", "/index")[1]["songs"][0]["multitracks_id"], 12)
            self.assertEqual(len(calls), 1)
        finally:
            ppl.multitracks_id = real

    def test_a_big_song_is_never_loaded_whole(self):
        self.write("Songs", "Big.pro", song(31337, filler=5_000_000))
        opened = []
        real_open = open

        def spy(path, mode="r", *a, **kw):
            handle = real_open(path, mode, *a, **kw)
            if str(path).endswith("Big.pro"):
                opened.append(handle)
            return handle

        import builtins
        builtins.open = spy
        try:
            self.assertEqual(ppl.multitracks_id(os.path.join(self.rig.lib("Songs"), "Big.pro")), 31337)
        finally:
            builtins.open = real_open
        self.assertTrue(opened)


class ScreenTests(Base):
    """The scripts are the bundled files, values are only passed as arguments, and a step that stops says which."""

    def setUp(self):
        super().setUp()
        self.calls = []
        self.answers = {}
        self.real_run = ppl.subprocess.run
        self.real_platform = sys.platform
        ppl.SCRIPT_DIR = os.path.join(self.rig.tmp.name, "scripts")
        ppl.is_mac = lambda: True
        ppl.IMPORT_WAIT_S = 1
        ppl.subprocess.run = self.fake_run
        self.addCleanup(self.restore)

    def restore(self):
        ppl.subprocess.run = self.real_run
        ppl.is_mac = lambda: sys.platform == "darwin"
        ppl.IMPORT_WAIT_S = 30

    def fake_run(self, argv, **kw):
        self.calls.append(list(argv))
        script = os.path.basename(argv[1]) if len(argv) > 1 else ""
        out = self.answers.get(script, "OK|1")
        if callable(out):
            out = out(argv)
        return type("Done", (), {"stdout": out + "\n", "stderr": "", "returncode": 0})()

    def test_the_scripts_inside_the_helper_are_the_bundled_files_byte_for_byte(self):
        for name in ("check_access.applescript", "import_song.applescript", "select_library.applescript", "click.swift"):
            with open(os.path.join(GUI, name), encoding="utf-8") as f:
                self.assertEqual(ppl.SCRIPTS[name], f.read(), name)

    def test_nothing_is_built_into_a_script_or_a_shell_line(self):
        for name, text in ppl.SCRIPTS.items():
            self.assertNotIn("do shell script", text, name)
        self.rig.json("GET", "/health")
        self.answers["import_song.applescript"] = lambda argv: self.make_song(argv, "OK|Evil.pro")
        status, body = self.rig.json("POST", "/import", {"id": "123", "name": "Evil; rm -rf", "playlist": "A'B\"C"})
        self.assertEqual(status, 200)
        run = [c for c in self.calls if os.path.basename(c[1]) == "import_song.applescript"][0]
        self.assertEqual(run[0], "/usr/bin/osascript")
        self.assertEqual(run[2:], ["123", "Evil; rm -rf", "A'B\"C"], "the values are arguments, one each")
        with open(run[1], encoding="utf-8") as f:
            self.assertEqual(f.read(), ppl.SCRIPTS["import_song.applescript"], "the script text holds none of them")

    def make_song(self, argv, answer):
        with open(os.path.join(self.rig.lib("Songs"), argv[3] + ".pro"), "wb") as f:
            f.write(song(int(argv[2])))
        return answer

    def test_an_import_waits_for_the_file_and_the_index_then_has_the_number(self):
        self.answers["import_song.applescript"] = lambda argv: self.make_song(argv, "OK|New Song.pro")
        status, body = self.rig.json("POST", "/import", {"id": "9876", "name": "New Song", "playlist": ""})
        self.assertEqual((status, body), (200, {"file": "New Song.pro"}))
        self.assertIn({"library": "Songs", "file": "New Song.pro", "multitracks_id": 9876}, self.rig.json("GET", "/index")[1]["songs"])
        self.assertEqual(self.rig.json("POST", "/import", {"id": "9876", "name": "New Song"})[0], 409, "the name is taken")

    def test_an_id_that_is_not_digits_or_a_name_with_a_dot_never_reaches_the_screen(self):
        for body in ({"id": "12a", "name": "X"}, {"id": "", "name": "X"}, {"id": "1 2", "name": "X"}, {"id": "-5", "name": "X"}, {"id": 5, "name": "X"},
                     {"id": "1", "name": "A.B"}, {"id": "1", "name": ""}, {"id": "1", "name": "a/b"}, {"id": "1", "name": "x", "playlist": "a\nb"}, {}):
            self.assertEqual(self.rig.json("POST", "/import", body)[0], 400, body)
        self.assertFalse([c for c in self.calls if "import_song" in c[1]])

    def test_a_step_that_stops_is_reported_by_name_and_nothing_is_added(self):
        self.answers["import_song.applescript"] = "ERR|read the search result|The search found 2 songs for this number, not exactly one."
        status, body = self.rig.json("POST", "/import", {"id": "12", "name": "X"})
        self.assertEqual(status, 502)
        self.assertIn("read the search result", body["error"])
        self.assertIn("not exactly one", body["error"])
        self.assertEqual(body["code"], "screen")
        self.assertEqual(os.listdir(self.rig.lib("Songs")), [])
        self.answers["import_song.applescript"] = "OK|X.pro"
        status, body = self.rig.json("POST", "/import", {"id": "12", "name": "X"})
        self.assertEqual((status, body["code"]), (504, "timeout"), "the screen said done and no file came")

    def test_without_the_permission_or_on_another_system_an_import_is_refused_before_the_screen(self):
        self.answers["check_access.applescript"] = "ERR|check|not allowed assistive access"
        ppl.ACCESS.update(at=0.0)
        status, body = self.rig.json("POST", "/import", {"id": "12", "name": "X"})
        self.assertEqual((status, body["code"]), (409, "cannot_import"))
        self.assertIn("Accessibility", body["error"])
        self.assertFalse([c for c in self.calls if "import_song" in c[1]])
        ppl.is_mac = lambda: False
        ppl.ACCESS.update(at=0.0)
        health = self.rig.json("GET", "/health")[1]
        self.assertFalse(health["can_import"])
        self.assertIn("Mac", health["import_blocker"])
        self.assertEqual(self.rig.json("POST", "/import", {"id": "12", "name": "X"})[1]["code"], "cannot_import")

    def test_only_one_import_runs_at_a_time(self):
        self.assertTrue(ppl.IMPORT_LOCK.acquire(False))
        try:
            self.assertEqual(self.rig.json("POST", "/import", {"id": "12", "name": "X"})[1]["code"], "busy")
        finally:
            ppl.IMPORT_LOCK.release()

    def test_select_library_is_a_best_effort_try(self):
        self.answers["select_library.applescript"] = "FOUND|120|340"
        self.assertEqual(self.rig.json("POST", "/select-library", {"name": "Playback"})[1]["tried"], True)
        click = [c for c in self.calls if c[:2] == ["/usr/bin/xcrun", "swift"]][0]
        self.assertEqual(click[-2:], ["120", "340"])
        self.answers["select_library.applescript"] = "ERR|find the library row|not in the list"
        self.assertEqual(self.rig.json("POST", "/select-library", {"name": "Playback"})[1]["tried"], False)
        self.assertEqual(self.rig.json("POST", "/select-library", {"name": "../x"})[0], 400)


class UpdateTests(Base):
    """POST /update: the same checks as the other helpers (their code is the same block), through this helper's own door."""

    def setUp(self):
        super().setUp()
        self.app = os.path.join(self.rig.tmp.name, "app")
        os.makedirs(self.app)
        ppl.APP_DIR = self.app
        self.real_key = ppl.UPDATE_PUBLIC_KEY
        ppl.UPDATE_PUBLIC_KEY = signing.public_b64()
        self.addCleanup(self.restore)
        with open(os.path.join(self.app, "propresenter_link.py"), "w") as f:
            f.write('__version__ = "0.1.0"\n')

    def restore(self):
        ppl.UPDATE_PUBLIC_KEY = self.real_key
        ppl.APP_DIR = os.path.dirname(os.path.abspath(ppl.__file__))

    def source(self, version):
        return ('"""stand-in"""\n__version__ = "%s"\n' % version).encode()

    def post(self, request, **kw):
        body = signing.body(request)
        return self.rig.request("POST", "/update", body, headers={"Content-Type": "application/json"}, **kw)

    def test_the_pinned_key_is_the_projects_and_the_test_key_is_not(self):
        self.assertEqual(self.real_key, REAL_PUBLIC_KEY)
        for name, _secret, public, message, signature in RFC8032_VECTORS:
            self.assertTrue(ppl.ed25519_verify(bytes.fromhex(public), bytes.fromhex(message), bytes.fromhex(signature)), name)

    def test_a_good_signed_update_is_installed_and_the_helper_stops_so_that_the_new_one_starts(self):
        request = signing.bundle({"propresenter_link.py": self.source("0.2.0")}, extra={"install.sh": b"#!/bin/sh\n"})
        status, body, _ = self.post(request)
        self.assertEqual((status, json.loads(body)), (200, {"updating": True, "from": ppl.__version__, "to": "0.2.0"}))
        with open(os.path.join(self.app, "propresenter_link.py"), "rb") as f:
            self.assertEqual(f.read(), self.source("0.2.0"))
        with open(os.path.join(self.app, "propresenter_link.py.prev"), "rb") as f:
            self.assertEqual(f.read(), b'__version__ = "0.1.0"\n')
        with open(os.path.join(self.app, "update-pending")) as f:
            self.assertEqual(json.load(f), {"from": ppl.__version__, "to": "0.2.0"})

    def test_every_refusal_leaves_the_files_as_they_were(self):
        with open(os.path.join(self.app, "propresenter_link.py"), "rb") as f:
            before = f.read()
        good = signing.bundle({"propresenter_link.py": self.source("0.2.0")})
        cases = {
            "signed by another key": signing.bundle({"propresenter_link.py": self.source("0.2.0")}, seed=signing.OTHER_SEED),
            "an older version": signing.bundle({"propresenter_link.py": self.source("0.0.9")}),
            "the same version": signing.bundle({"propresenter_link.py": self.source(ppl.__version__)}),
            "a file the helper does not take": signing.bundle({"propresenter_link.py": self.source("0.2.0"), "evil.py": b"x = 1\n"}),
            "no file at all": signing.bundle({}),
            "a file that is not Python": signing.bundle({"propresenter_link.py": b'__version__ = "0.2.0"\ndef (\n'}),
            "a file changed after signing": dict(good, files={"propresenter_link.py": base64.b64encode(self.source("0.3.0")).decode()}),
        }
        for label, request in cases.items():
            status, body, _ = self.post(request)
            self.assertIn(status, (400, 409, 422), label)
            with open(os.path.join(self.app, "propresenter_link.py"), "rb") as f:
                self.assertEqual(f.read(), before, label)
        self.assertEqual(sorted(os.listdir(self.app)), ["propresenter_link.py"])

    def test_the_update_needs_the_key_over_the_body_and_a_local_network_caller(self):
        request = signing.bundle({"propresenter_link.py": self.source("0.2.0")})
        body = signing.body(request)
        self.assertEqual(self.rig.request("POST", "/update", body, key="x" * 43)[0], 401)
        self.assertEqual(self.rig.request("POST", "/update", body, sign_body=b"{}")[0], 401)
        self.assertEqual(self.rig.request("GET", "/update")[0], 404)
        self.assertFalse(os.path.exists(os.path.join(self.app, "update-pending")))

    def test_health_says_the_helper_can_update_and_why_the_last_update_did_not_stay(self):
        self.assertEqual(self.rig.json("GET", "/health")[1]["updater"], 1)
        with open(os.path.join(self.app, "update-failed"), "w") as f:
            json.dump({"version": "0.2.0", "message": "Version 0.2.0 would not start, so version 0.1.0 was put back."}, f)
        self.assertEqual(self.rig.json("GET", "/health")[1]["last_update_error"], "Version 0.2.0 would not start, so version 0.1.0 was put back.")


if __name__ == "__main__":
    unittest.main()
