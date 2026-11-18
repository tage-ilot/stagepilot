"""The wrapper that launchd runs for the ProPresenter Link (written by install.sh, never replaced by an update), and the installer's own
refusals. The wrapper is the same design as the Lightkey helper's and the Library Link's (its tests are in scripts/lightkey-helper-tests);
here: that THIS installer writes it for THIS helper, and that the installer asks for what it must have before it changes anything.

Run:  python3 -m unittest discover -s scripts/propresenter-link-tests -v"""
import json
import os
import re
import subprocess
import sys

sys.dont_write_bytecode = True      # never leave a __pycache__ inside the helper folder: it is a bundle of plain files
import shutil
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
HELPER = os.path.join(HERE, "..", "..", "crates", "stagepilot-propresenter", "assets", "propresenter-link")
CRASHES = "import sys\nsys.exit(3)\n"
RUNS_AND_STOPS = "import sys\nsys.exit(0)\n"


def installer_text():
    with open(os.path.join(HELPER, "install.sh"), encoding="utf-8") as f:
        return f.read()


def wrapper_text():
    return re.search(r"<<'WRAPEOF'\n(.*?)\nWRAPEOF\n", installer_text(), re.S).group(1) + "\n"


class WrapperTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.app = self.tmp.name
        self.addCleanup(self.tmp.cleanup)
        self.put("propresenter_link_run.py", wrapper_text())

    def put(self, name, text):
        with open(os.path.join(self.app, name), "w") as f:
            f.write(text)

    def read(self, name):
        with open(os.path.join(self.app, name)) as f:
            return f.read()

    def has(self, name):
        return os.path.exists(os.path.join(self.app, name))

    def run_wrapper(self, grace="0.5"):
        env = dict(os.environ, PPL_WRAPPER_GRACE_S=grace)
        return subprocess.run([sys.executable, os.path.join(self.app, "propresenter_link_run.py")], capture_output=True, text=True, env=env, timeout=60).returncode

    def test_the_wrapper_starts_this_helper_and_passes_its_exit_code_on(self):
        import ast
        ast.parse(wrapper_text(), feature_version=(3, 9))
        self.assertIn('"propresenter_link.py"', wrapper_text())
        self.assertNotIn("playback", wrapper_text().lower())
        self.put("propresenter_link.py", "import sys\nsys.exit(7)\n")
        self.assertEqual(self.run_wrapper(), 7)

    def test_three_fast_failures_after_an_update_put_the_old_version_back(self):
        self.put("propresenter_link.py", CRASHES)
        self.put("propresenter_link.py.prev", RUNS_AND_STOPS)
        self.put("update-pending", json.dumps({"from": "0.1.0", "to": "0.2.0"}))
        self.assertEqual(self.run_wrapper(), 3)
        self.assertEqual(self.run_wrapper(), 3)
        self.assertEqual(self.run_wrapper(), 0)
        self.assertEqual(self.read("propresenter_link.py"), RUNS_AND_STOPS)
        failed = json.loads(self.read("update-failed"))
        self.assertEqual(failed["message"], "Version 0.2.0 would not start, so version 0.1.0 was put back.")
        sys.path.insert(0, HELPER)
        import propresenter_link as ppl
        self.assertEqual(ppl.last_update_error(self.app), failed["message"])


class InstallerTests(unittest.TestCase):
    """The installer refuses before it changes anything. It is run with a stand-in for the Mac (`PROPRESENTER_LINK_UNAME`)."""

    def run_installer(self, *args, key=None, uname="Darwin"):
        home = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, home, True)
        copy = os.path.join(home, "files")
        shutil.copytree(HELPER, copy, ignore=shutil.ignore_patterns("__pycache__"))
        argv = ["sh", os.path.join(copy, "install.sh")] + list(args)
        if key is not None:
            path = os.path.join(home, "key.txt")
            with open(path, "w") as f:
                f.write(key + "\n")
            argv += ["--key-file", path]
        env = dict(os.environ, HOME=home, PROPRESENTER_LINK_UNAME=uname, PROPRESENTER_LINK_PY=sys.executable)
        done = subprocess.run(argv, capture_output=True, text=True, env=env, timeout=60)
        return done, home

    def test_the_installer_is_valid_shell(self):
        self.assertEqual(subprocess.run(["sh", "-n", os.path.join(HELPER, "install.sh")]).returncode, 0)

    def test_it_installs_only_on_a_mac(self):
        done, home = self.run_installer("--allow-for", "192.168.1.20", key="k" * 43, uname="Linux")
        self.assertNotEqual(done.returncode, 0)
        self.assertIn("This installs on macOS", done.stdout)
        self.assertFalse(os.path.exists(os.path.join(home, "Library")))

    def test_a_key_needs_the_computer_that_may_use_it(self):
        done, home = self.run_installer(key="k" * 43)
        self.assertNotEqual(done.returncode, 0)
        self.assertIn("--allow-for", done.stdout)
        self.assertFalse(os.path.exists(os.path.join(home, "Library")))
        self.assertTrue(os.path.exists(os.path.join(home, "key.txt")), "the key file is only deleted once the key is stored")

    def test_a_short_key_and_a_bad_address_and_unknown_options_are_refused(self):
        done, home = self.run_installer("--allow-for", "192.168.1.20", key="short")
        self.assertIn("too short", done.stdout)
        self.assertFalse(os.path.exists(os.path.join(home, "Library")))
        done, _ = self.run_installer("--allow-for", "192.168.1.20; rm -rf /", key="k" * 43)
        self.assertIn("--allow-for takes the address", done.stdout)
        done, _ = self.run_installer("--bogus")
        self.assertIn("Unknown option", done.stdout)
        done, _ = self.run_installer("--allow-for")
        self.assertIn("needs a value", done.stdout)

    def test_the_installer_has_no_download_and_no_key_on_a_command_line(self):
        text = installer_text()
        self.assertNotIn("curl -fsSL", text)
        self.assertNotIn("github.com", text)
        self.assertNotIn("--key ", text.replace("--key-file", ""))
        self.assertIn('"$APP/config.json"', text)
        self.assertIn("chmod 600", text)

    def test_a_checksum_that_does_not_match_stops_the_install(self):
        home = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, home, True)
        copy = os.path.join(home, "files")
        shutil.copytree(HELPER, copy, ignore=shutil.ignore_patterns("__pycache__"))
        with open(os.path.join(copy, "propresenter_link.py"), "a") as f:
            f.write("# tampered\n")
        keyfile = os.path.join(home, "key.txt")
        with open(keyfile, "w") as f:
            f.write("k" * 43 + "\n")
        env = dict(os.environ, HOME=home, PROPRESENTER_LINK_UNAME="Darwin", PROPRESENTER_LINK_PY=sys.executable)
        done = subprocess.run(["sh", os.path.join(copy, "install.sh"), "--key-file", keyfile, "--allow-for", "192.168.1.20"], capture_output=True, text=True, env=env, timeout=60)
        self.assertNotEqual(done.returncode, 0)
        self.assertIn("does not match SHA256SUMS", done.stdout)
        self.assertFalse(os.path.exists(os.path.join(home, "Library", "Application Support", "ProPresenterLink", "propresenter_link.py")))


if __name__ == "__main__":
    unittest.main()
