"""The Lightkey helper's self-update (the copy of the helper that StagePilot carries): the Ed25519 check against the RFC 8032
vectors, every reason an update is refused (with nothing changed), a good update applied to a temporary install folder, a crash at
every step of the swap, and what /health says. The key pair that signs the test bundles is made here (signing.py) and is swapped in by
assigning lk.UPDATE_PUBLIC_KEY, which only code inside the process can do; the project's private key is never on a test machine.

Run:  python3 -m unittest discover -s scripts/lightkey-helper-tests -v   (Python 3.9 or newer)"""
import base64
import hashlib
import http.client
import json
import os
import shutil
import sys

sys.dont_write_bytecode = True      # never leave a __pycache__ inside the helper folder: it is a bundle of plain files
import tempfile
import threading
import time
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
HELPER = os.path.join(HERE, "..", "..", "crates", "stagepilot-lights", "assets", "lightkey-helper")
sys.path.insert(0, HERE)
sys.path.insert(0, HELPER)
import lightkey_api as lk  # noqa: E402
import signing  # noqa: E402
from rfc8032_vectors import RFC8032_VECTORS  # noqa: E402

REAL_PUBLIC_KEY = "Evky97ydM5zWJ02p6Wc7RguEHMglX3WRxiPrege4Op8="
FILES = ("lightkey_api.py", "lightkey_cues.py")


def helper_source(version, extra=""):
    """A file that passes for lightkey_api.py of that version: it compiles, and says its version the way the real one does."""
    return ('"""stand-in"""\n__version__ = "%s"\n%s\n' % (version, extra)).encode()


def snapshot(folder):
    """Every file under folder with its bytes (to prove that a refused update changed nothing)."""
    out = {}
    for root, _dirs, names in os.walk(folder):
        for name in names:
            path = os.path.join(root, name)
            with open(path, "rb") as f:
                out[os.path.relpath(path, folder)] = f.read()
    return out


class Ed25519Tests(unittest.TestCase):
    def test_the_pinned_public_key_is_32_bytes_and_is_not_the_test_key(self):
        self.assertEqual(lk.UPDATE_PUBLIC_KEY, REAL_PUBLIC_KEY)
        self.assertEqual(len(base64.b64decode(lk.UPDATE_PUBLIC_KEY)), 32)
        self.assertNotEqual(lk.UPDATE_PUBLIC_KEY, signing.public_b64())
        self.assertNotEqual(lk.UPDATE_PUBLIC_KEY, signing.public_b64(signing.OTHER_SEED))

    def test_rfc_8032_vectors_verify(self):
        for name, _secret, public, message, signature in RFC8032_VECTORS:
            with self.subTest(vector=name):
                self.assertTrue(lk.ed25519_verify(bytes.fromhex(public), bytes.fromhex(message), bytes.fromhex(signature)))

    def test_the_test_signer_reproduces_the_rfc_signatures(self):
        for name, secret, public, message, signature in RFC8032_VECTORS:
            with self.subTest(vector=name):
                self.assertEqual(signing.public_key(bytes.fromhex(secret)).hex(), public)
                self.assertEqual(signing.sign(bytes.fromhex(secret), bytes.fromhex(message)).hex(), signature)

    def test_a_changed_message_signature_or_key_is_refused(self):
        for name, _secret, public, message, signature in RFC8032_VECTORS:
            pub, msg, sig = bytes.fromhex(public), bytes.fromhex(message), bytes.fromhex(signature)
            with self.subTest(vector=name):
                self.assertFalse(lk.ed25519_verify(pub, msg + b"x", sig), "message extended")
                if msg:
                    self.assertFalse(lk.ed25519_verify(pub, msg[:-1] + bytes([msg[-1] ^ 1]), sig), "message bit flipped")
                    self.assertFalse(lk.ed25519_verify(pub, msg[:-1], sig), "message cut")
                for i in (0, 31, 32, 63):
                    flipped = bytearray(sig)
                    flipped[i] ^= 0x01
                    self.assertFalse(lk.ed25519_verify(pub, msg, bytes(flipped)), "signature byte %d flipped" % i)
                other = bytearray(pub)
                other[5] ^= 0x10
                self.assertFalse(lk.ed25519_verify(bytes(other), msg, sig), "another key")
                self.assertFalse(lk.ed25519_verify(signing.public_key(signing.TEST_SEED), msg, sig), "the test key")

    def test_short_long_and_empty_inputs_are_refused(self):
        _n, _s, public, message, signature = RFC8032_VECTORS[1]
        pub, msg, sig = bytes.fromhex(public), bytes.fromhex(message), bytes.fromhex(signature)
        for bad in (b"", sig[:63], sig[:32], sig + b"\x00", sig + sig, sig[1:]):
            self.assertFalse(lk.ed25519_verify(pub, msg, bad), len(bad))
        for bad in (b"", pub[:31], pub + b"\x00"):
            self.assertFalse(lk.ed25519_verify(bad, msg, sig), len(bad))
        self.assertFalse(lk.ed25519_verify(None, msg, sig))
        self.assertFalse(lk.ed25519_verify(pub, msg, None))
        self.assertFalse(lk.ed25519_verify(pub, msg, "text"))

    def test_a_second_encoding_of_a_signature_is_refused(self):
        """S + L is the same number modulo the group order, so a lax checker would accept two byte strings for one signature."""
        _n, _s, public, message, signature = RFC8032_VECTORS[0]
        pub, msg, sig = bytes.fromhex(public), bytes.fromhex(message), bytes.fromhex(signature)
        s = int.from_bytes(sig[32:], "little")
        self.assertLess(s + lk._ED_L, 2 ** 256)
        twin = sig[:32] + (s + lk._ED_L).to_bytes(32, "little")
        self.assertFalse(lk.ed25519_verify(pub, msg, twin))
        self.assertFalse(lk.ed25519_verify(pub, msg, sig[:32] + (2 ** 256 - 1).to_bytes(32, "little")))

    def test_points_that_are_not_on_the_curve_or_not_canonical_are_refused(self):
        _n, _s, public, message, signature = RFC8032_VECTORS[0]
        pub, msg, sig = bytes.fromhex(public), bytes.fromhex(message), bytes.fromhex(signature)
        not_canonical_y = (lk._ED_P + 1).to_bytes(32, "little")           # y = 1 written as p + 1
        self.assertFalse(lk.ed25519_verify(not_canonical_y, msg, sig))
        self.assertFalse(lk.ed25519_verify(pub, msg, not_canonical_y + sig[32:]))
        off_curve = None
        for y in range(2, 100):                                            # a y with no x on the curve
            if lk._ed_x_from_y(y, 0) is None:
                off_curve = y.to_bytes(32, "little")
                break
        self.assertIsNotNone(off_curve)
        self.assertFalse(lk.ed25519_verify(off_curve, msg, sig))
        self.assertFalse(lk.ed25519_verify(pub, msg, off_curve + sig[32:]))
        self.assertFalse(lk.ed25519_verify(bytes(32), msg, bytes(64)))     # all zeros
        identity = (1).to_bytes(32, "little")                              # the neutral point as a key and as R
        self.assertFalse(lk.ed25519_verify(identity, msg, identity + bytes(32)))

    def test_a_signature_by_another_key_pair_is_refused_and_a_good_one_is_accepted(self):
        message = b"SHA256SUMS bytes"
        good = signing.sign(signing.TEST_SEED, message)
        self.assertTrue(lk.ed25519_verify(signing.public_key(signing.TEST_SEED), message, good))
        self.assertFalse(lk.ed25519_verify(signing.public_key(signing.OTHER_SEED), message, good))




class SyntaxTests(unittest.TestCase):
    def test_the_agent_parses_as_python_39(self):
        """macOS command-line tools ship Python 3.9, which is what the helper runs on; CI also runs the real 3.9."""
        import ast
        for name in ("lightkey_api.py", "lightkey_cues.py"):
            path = os.path.join(HELPER, name)
            with open(path, encoding="utf-8") as f:
                ast.parse(f.read(), filename=path, feature_version=(3, 9))


class PeerTests(unittest.TestCase):
    def test_only_the_local_network_is_private(self):
        for address in ("127.0.0.1", "127.8.9.10", "10.0.0.5", "10.255.255.254", "172.16.0.1", "172.31.255.1", "192.168.1.20",
                        "169.254.10.10", "::1", "fd12:3456::1", "fc00::1", "fe80::1", "fe80::1%en0", "::ffff:192.168.1.20", "::ffff:127.0.0.1"):
            self.assertTrue(lk.peer_is_private(address), address)
        for address in ("8.8.8.8", "172.32.0.1", "172.15.255.255", "192.169.0.1", "11.0.0.1", "100.64.0.1", "0.0.0.0", "255.255.255.255",
                        "2001:db8::1", "2606:4700::1111", "::ffff:8.8.8.8", "", "not an address", "192.168.1"):
            self.assertFalse(lk.peer_is_private(address), address)




class SwapTests(unittest.TestCase):
    """The swap of several files, cut short at every step by a crash, then finished by the next start."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.app = self.tmp.name
        self.old = {"a.py": b'__version__ = "1.0.0"\n# old a\n', "b.py": b"# old b\n"}
        self.new = {"a.py": b'__version__ = "1.1.0"\n# new a\n', "b.py": b"# new b\n"}
        for name, blob in self.old.items():
            with open(os.path.join(self.app, name), "wb") as f:
                f.write(blob)

    def tearDown(self):
        self.tmp.cleanup()

    def files(self):
        return {n: open(os.path.join(self.app, n), "rb").read() for n in ("a.py", "b.py")}

    def test_a_clean_swap_gives_the_new_set_and_keeps_the_old_one(self):
        lk.apply_update(self.app, self.new, "1.0.0", "1.1.0")
        self.assertEqual(self.files(), self.new)
        self.assertEqual({n: open(os.path.join(self.app, n + ".prev"), "rb").read() for n in self.old}, self.old)
        self.assertEqual(lk.read_marker(self.app, "update-pending"), {"from": "1.0.0", "to": "1.1.0"})
        self.assertFalse(lk.finish_interrupted_update(self.app), "nothing left to finish")

    def test_a_crash_at_any_replace_leaves_the_old_set_or_something_the_next_start_completes(self):
        real_replace = os.replace
        # apply_update renames: the journal, then a.prev, a, b.prev, b, then the pending mark. Cut it before each one.
        for cut in range(1, 8):
            with self.subTest(cut=cut):
                shutil.rmtree(self.app)
                os.makedirs(self.app)
                for name, blob in self.old.items():
                    with open(os.path.join(self.app, name), "wb") as f:
                        f.write(blob)
                calls = []

                def failing(src, dst, _cut=cut):
                    calls.append(dst)
                    if len(calls) == _cut:
                        raise OSError("the power went out")
                    return real_replace(src, dst)

                os.replace = failing
                try:
                    lk.apply_update(self.app, self.new, "1.0.0", "1.1.0")
                    crashed = False
                except OSError:
                    crashed = True
                finally:
                    os.replace = real_replace
                self.assertTrue(crashed or cut > len(calls), cut)
                if not os.path.exists(os.path.join(self.app, "update", "journal.json")):
                    # no journal: the swap never began, or it is over. Either way the set is whole.
                    self.assertIn(self.files(), (self.old, self.new))
                    continue
                self.assertTrue(lk.finish_interrupted_update(self.app))
                self.assertEqual(self.files(), self.new, "after the next start's recovery the new set is complete")
                self.assertEqual({n: open(os.path.join(self.app, n + ".prev"), "rb").read() for n in self.old}, self.old, "the old set is kept whole")
                self.assertFalse(os.path.exists(os.path.join(self.app, "update")))
                self.assertEqual(lk.read_marker(self.app, "update-pending"), {"from": "1.0.0", "to": "1.1.0"})

    def test_a_crash_before_the_journal_leaves_the_old_set_untouched(self):
        real_replace = os.replace

        def failing(src, dst):
            raise OSError("the power went out")

        os.replace = failing
        try:
            with self.assertRaises(OSError):
                lk.apply_update(self.app, self.new, "1.0.0", "1.1.0")
        finally:
            os.replace = real_replace
        self.assertEqual(self.files(), self.old)
        self.assertFalse(lk.finish_interrupted_update(self.app))

    def test_a_file_that_does_not_compile_stops_everything_before_the_swap(self):
        broken = dict(self.new, **{"b.py": b"def broken(:\n"})
        with self.assertRaises(lk.UpdateRejected):
            lk.apply_update(self.app, broken, "1.0.0", "1.1.0")
        self.assertEqual(self.files(), self.old)
        self.assertEqual(sorted(os.listdir(self.app)), ["a.py", "b.py"])

    def test_a_journal_naming_other_files_is_ignored(self):
        os.makedirs(os.path.join(self.app, "update"))
        lk.write_marker(os.path.join(self.app, "update"), "journal.json", {"from": "1", "to": "2", "files": ["../evil.py"]})
        self.assertFalse(lk.finish_interrupted_update(self.app))
        lk.write_marker(os.path.join(self.app, "update"), "journal.json", {"from": "1", "to": "2", "files": ["x.sh"]})
        self.assertFalse(lk.finish_interrupted_update(self.app))


if __name__ == "__main__":
    unittest.main()


class UpdateBase(unittest.TestCase):
    """A helper server on loopback with its install folder in a temp dir, accepting bundles signed by the test key."""

    @classmethod
    def setUpClass(cls):
        cls.server = None

    @classmethod
    def tearDownClass(cls):
        if cls.server:
            cls.server.shutdown()
            cls.server.server_close()

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.app = self.tmp.name
        for name in FILES:
            with open(os.path.join(self.app, name), "wb") as f:
                f.write(helper_source("0.6.0", "# the running one"))
        self.saved = {n: getattr(lk, n) for n in ("APP_DIR", "UPDATE_PUBLIC_KEY", "schedule_restart", "peer_is_private", "__version__")}
        lk.APP_DIR = self.app
        lk.UPDATE_PUBLIC_KEY = signing.public_b64()
        lk.__version__ = "0.6.0"
        self.restarts = []
        lk.schedule_restart = lambda server: self.restarts.append(server)
        lk.CFG.clear()
        lk.CFG.update(lk.DEFAULTS)
        if not type(self).server:
            type(self).server = lk.Server(("127.0.0.1", 0), lk.Handler)
            threading.Thread(target=type(self).server.serve_forever, daemon=True).start()
        self.port = type(self).server.server_address[1]

    def tearDown(self):
        for name, value in self.saved.items():
            setattr(lk, name, value)
        self.tmp.cleanup()

    def request(self, method, target, payload=None, headers=None):
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=20)
        try:
            h = {"Content-Type": "application/json"} if payload is not None else {}
            h.update(headers or {})
            conn.request(method, target, body=payload, headers=h)
            r = conn.getresponse()
            data = r.read()
            if r.status == 200 and target == "/update":      # the helper schedules its stop just after the answer went out
                for _ in range(200):
                    if self.restarts:
                        break
                    time.sleep(0.01)
            return r.status, json.loads(data) if data else None
        finally:
            conn.close()

    def post(self, request=None, raw=None, target="/update"):
        return self.request("POST", target, raw if raw is not None else signing.body(request))

    def good(self, version="0.6.1", names=FILES, extra=None):
        return signing.bundle({n: helper_source(version, "# new") for n in names},
                              extra={"install.sh": b"#!/bin/sh\n", "uninstall.sh": b"#!/bin/sh\n", "VERSION": (version + "\n").encode()} if extra is None else extra)

    def assert_refused(self, status, request=None, message=None, **kw):
        before = snapshot(self.app)
        code, answer = self.post(request, **kw)
        self.assertEqual(code, status, answer)
        self.assertIn("error", answer)
        if message:
            self.assertIn(message, answer["error"])
        self.assertEqual(snapshot(self.app), before, "a refused update must change nothing")
        self.assertEqual(self.restarts, [], "a refused update must not restart the helper")
        return answer


class GoodUpdateTests(UpdateBase):
    def test_a_signed_newer_update_is_installed_and_the_helper_stops(self):
        code, answer = self.post(self.good("0.6.1"))
        self.assertEqual((code, answer), (200, {"updating": True, "from": "0.6.0", "to": "0.6.1"}))
        for name in FILES:
            self.assertEqual(open(os.path.join(self.app, name), "rb").read(), helper_source("0.6.1", "# new"))
            self.assertEqual(open(os.path.join(self.app, name + ".prev"), "rb").read(), helper_source("0.6.0", "# the running one"))
        self.assertEqual(lk.read_marker(self.app, "update-pending"), {"from": "0.6.0", "to": "0.6.1"})
        self.assertFalse(os.path.exists(os.path.join(self.app, "update")), "the staging folder is gone")
        self.assertEqual(len(self.restarts), 1, "the helper is asked to stop so that launchd starts the new version")

    def test_the_cue_reader_is_replaced_only_where_it_is_installed(self):
        os.remove(os.path.join(self.app, "lightkey_cues.py"))
        self.assertEqual(self.post(self.good("0.6.1"))[0], 200)
        self.assertFalse(os.path.exists(os.path.join(self.app, "lightkey_cues.py")))
        self.assertTrue(os.path.exists(os.path.join(self.app, "lightkey_api.py.prev")))

    def test_the_real_helper_file_is_a_valid_update_for_an_older_version(self):
        """What StagePilot will send: these very files, signed. They must pass their own checks and compile."""
        real = {n: open(os.path.join(HELPER, n), "rb").read() for n in FILES}
        lk.__version__ = "0.5.0"
        code, answer = self.post(signing.bundle(real, extra={"install.sh": b"x"}))
        self.assertEqual(code, 200, answer)
        self.assertEqual(answer, {"updating": True, "from": "0.5.0", "to": self.saved["__version__"]})

    def test_a_numeric_version_compare(self):
        lk.__version__ = "0.9.0"
        self.assertEqual(self.post(self.good("0.10.0"))[0], 200, "0.10.0 is newer than 0.9.0")


class RefusalTests(UpdateBase):
    def test_an_address_that_is_not_on_the_local_network_is_refused_first(self):
        lk.peer_is_private = lambda address: False
        answer = self.assert_refused(403, self.good())
        self.assertIn("local network", answer["error"])

    def test_the_real_key_alone_is_what_production_trusts(self):
        lk.UPDATE_PUBLIC_KEY = REAL_PUBLIC_KEY
        self.assert_refused(422, self.good(), message="not signed")

    def test_a_bundle_signed_by_another_key_is_refused(self):
        self.assert_refused(422, signing.bundle({n: helper_source("0.6.1") for n in FILES}, seed=signing.OTHER_SEED), message="not signed")

    def test_a_signature_that_is_not_64_bytes_or_not_base64_is_refused(self):
        for sig in ("", "AAAA", base64.b64encode(bytes(63)).decode(), base64.b64encode(bytes(65)).decode(), "!!not base64!!", None, 5):
            request = self.good()
            request["sig"] = sig
            before = snapshot(self.app)
            code, _answer = self.post(request)
            self.assertIn(code, (400, 422), sig)
            self.assertEqual(snapshot(self.app), before)
        self.assertEqual(self.restarts, [])

    def test_changed_checksum_text_breaks_the_signature(self):
        request = self.good()
        request["sums"] = request["sums"] + "\n"
        self.assert_refused(422, request, message="not signed")

    def test_a_file_that_is_not_the_one_in_the_signed_list_is_refused(self):
        request = self.good("0.6.1")
        request["files"]["lightkey_api.py"] = base64.b64encode(helper_source("0.9.9", "# evil")).decode()
        self.assert_refused(422, request, message="checksums")

    def test_file_names_must_be_exactly_the_expected_set(self):
        for files in ({"lightkey_api.py": helper_source("0.6.1")},                                  # the cue reader missing
                      {"lightkey_api.py": helper_source("0.6.1"), "lightkey_cues.py": helper_source("0.6.1"), "install.sh": b"#!/bin/sh\n"},
                      {"install.sh": b"#!/bin/sh\n"}, {}):
            self.assert_refused(422, signing.bundle(files, extra={"x.sh": b"1"}), message="exactly the files")
        for name in ("../lightkey_api.py", "/etc/passwd", "sub/lightkey_api.py", ".."):
            files = {n: helper_source("0.6.1") for n in FILES}
            request = signing.bundle(files)
            request["files"] = {name if n == "lightkey_api.py" else n: v for n, v in request["files"].items()}
            before = snapshot(self.app)
            code, _answer = self.post(request)
            self.assertEqual(code, 422, name)
            self.assertEqual(snapshot(self.app), before, name)

    def test_the_same_version_and_older_versions_are_refused(self):
        for version in ("0.6.0", "0.5.9", "0.5.0", "0.0.1"):
            self.assertIn("not newer", self.assert_refused(409, self.good(version))["error"])

    def test_a_file_without_a_version_or_with_a_bad_one_is_refused(self):
        for source in (b"x = 1\n", b'__version__ = "latest"\n', b'__version__ = "1.2"\n', b'  __version__ = "9.9.9"\n', b'# __version__ = "9.9.9"\n'):
            self.assert_refused(422, signing.bundle({n: source for n in FILES}), message="which version")

    def test_a_file_that_does_not_compile_is_refused_and_nothing_is_left_behind(self):
        files = {"lightkey_api.py": helper_source("0.6.1"), "lightkey_cues.py": helper_source("0.6.1", "def broken(:")}
        self.assert_refused(422, signing.bundle(files), message="compile")
        self.assertEqual(sorted(os.listdir(self.app)), sorted(FILES))

    def test_a_version_that_failed_to_start_here_is_not_sent_again(self):
        lk.write_marker(self.app, "update-failed", {"version": "0.6.1", "message": "Version 0.6.1 would not start."})
        self.assert_refused(409, self.good("0.6.1"), message="did not start")
        self.assertEqual(self.post(self.good("0.6.2"))[0], 200)

    def test_nothing_is_installed_while_a_take_is_being_recorded(self):
        real = lk.REC.active
        lk.REC.active = {"name": "x", "t0": 0, "events": [], "fixtures": []}
        try:
            self.assert_refused(409, self.good(), message="recorded")
        finally:
            lk.REC.active = real

    def test_bad_requests(self):
        before = snapshot(self.app)
        for raw in (b"not json", b"[]", b'"text"', b"{}", b'{"files": [], "sums": "", "sig": ""}', b'{"files": {"lightkey_api.py": 5}, "sums": "", "sig": ""}'):
            code, _answer = self.post(raw=raw)
            self.assertIn(code, (400, 422), raw)
        request = self.good()
        request["files"]["lightkey_api.py"] = "this is not base64!"
        self.assertEqual(self.post(request)[0], 400)
        self.assertEqual(snapshot(self.app), before)
        self.assertEqual(self.restarts, [])

    def test_the_largest_body_is_two_megabytes_and_a_length_is_needed(self):
        self.assertEqual(lk.UPDATE_MAX_BYTES, 2 * 1024 * 1024)
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=20)
        try:
            conn.putrequest("POST", "/update")
            conn.putheader("Content-Length", str(lk.UPDATE_MAX_BYTES + 1))
            conn.endheaders()
            self.assertEqual(conn.getresponse().status, 413)
        finally:
            conn.close()
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=20)
        try:
            conn.putrequest("POST", "/update")
            conn.endheaders()
            self.assertEqual(conn.getresponse().status, 411)
        finally:
            conn.close()
        self.assertEqual(sorted(os.listdir(self.app)), sorted(FILES))

    def test_a_second_update_while_one_is_being_installed_is_refused(self):
        self.assertTrue(lk.UPDATE_LOCK.acquire(False))
        try:
            self.assert_refused(409, self.good(), message="another update")
        finally:
            lk.UPDATE_LOCK.release()


class HealthTests(UpdateBase):
    def test_health_reports_the_updater_and_nothing_else_new_when_all_is_well(self):
        code, health = self.request("GET", "/health")
        self.assertEqual(code, 200)
        self.assertEqual(health["updater"], 1)
        self.assertNotIn("last_update_error", health)
        self.assertEqual(health["version"], "0.6.0")

    def test_health_reports_last_update_error_while_the_note_is_there(self):
        lk.write_marker(self.app, "update-failed", {"version": "0.6.1", "message": "Version 0.6.1 would not start, so version 0.6.0 was put back."})
        _code, health = self.request("GET", "/health")
        self.assertEqual(health["last_update_error"], "Version 0.6.1 would not start, so version 0.6.0 was put back.")

    def test_other_posts_still_do_what_they_did(self):
        code, answer = self.request("POST", "/nothing-here", b"{}")
        self.assertEqual(code, 404, answer)


