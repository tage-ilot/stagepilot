"""The wrapper that launchd runs for the Lightkey helper (written by install.sh, never replaced by an update): it finishes an
interrupted swap, starts the agent, and after an update puts the old version back when the new one exits with an error within the
grace time three times in a row. The wrapper under test is the text inside install.sh; the agent is a small fake that crashes, or runs."""
import json
import os
import re
import subprocess
import sys

sys.dont_write_bytecode = True      # never leave a __pycache__ inside the helper folder: it is a bundle of plain files
import tempfile
import time
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
HELPER = os.path.join(HERE, "..", "..", "crates", "stagepilot-lights", "assets", "lightkey-helper")
CRASHES = "import sys\nsys.exit(3)\n"
RUNS_AND_STOPS = "import sys\nsys.exit(0)\n"


def wrapper_text():
    with open(os.path.join(HELPER, "install.sh"), encoding="utf-8") as f:
        text = f.read()
    return re.search(r"<<'WRAPEOF'\n(.*?)\nWRAPEOF\n", text, re.S).group(1) + "\n"


def fake(marker, body=""):
    """A fake agent: it writes its own marker, then does `body`."""
    return "import os\nopen(os.path.join(os.path.dirname(os.path.abspath(__file__)), 'started-%s'), 'a').write('x')\n%s" % (marker, body)


class WrapperTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.app = self.tmp.name
        self.wrapper = os.path.join(self.app, "lightkey_api_run.py")
        self.put("lightkey_api_run.py", wrapper_text())

    def tearDown(self):
        self.tmp.cleanup()

    def put(self, name, text):
        with open(os.path.join(self.app, name), "w") as f:
            f.write(text)

    def read(self, name):
        with open(os.path.join(self.app, name)) as f:
            return f.read()

    def has(self, name):
        return os.path.exists(os.path.join(self.app, name))

    def started(self, marker):
        path = os.path.join(self.app, "started-" + marker)
        if not os.path.exists(path):
            return 0
        with open(path) as f:
            return len(f.read())

    def run_wrapper(self, grace="0.5"):
        env = dict(os.environ, LIGHTKEY_WRAPPER_GRACE_S=grace)
        return subprocess.run([sys.executable, self.wrapper], capture_output=True, text=True, env=env, timeout=60).returncode

    def after_update(self, new_body, old_body=RUNS_AND_STOPS):
        self.put("lightkey_api.py", fake("new", new_body))
        self.put("lightkey_api.py.prev", fake("old", old_body))
        self.put("lightkey_cues.py", "# new cues\n")
        self.put("lightkey_cues.py.prev", "# old cues\n")
        self.put("update-pending", json.dumps({"from": "0.5.0", "to": "0.6.0"}))

    def test_the_wrapper_parses_as_python_39_and_starts_the_agent_passing_its_exit_code_on(self):
        import ast
        ast.parse(wrapper_text(), feature_version=(3, 9))
        self.put("lightkey_api.py", fake("a", "import sys\nsys.exit(0)\n"))
        self.assertEqual(self.run_wrapper(), 0)
        self.assertEqual(self.started("a"), 1)
        self.put("lightkey_api.py", fake("b", "import sys\nsys.exit(7)\n"))
        self.assertEqual(self.run_wrapper(), 7)

    def test_three_fast_failures_after_an_update_put_the_old_version_back_and_start_it(self):
        self.after_update(CRASHES)
        self.assertEqual(self.run_wrapper(), 3)
        self.assertEqual(self.run_wrapper(), 3)
        self.assertEqual((self.started("new"), self.started("old")), (2, 0))
        self.assertTrue(self.has("update-pending"), "two failures are not three")
        self.assertEqual(self.run_wrapper(), 0)                  # the third: the old version is back, and it runs and ends well
        self.assertEqual((self.started("new"), self.started("old")), (3, 1))
        self.assertEqual(self.read("lightkey_api.py"), fake("old", RUNS_AND_STOPS))
        self.assertEqual(self.read("lightkey_cues.py"), "# old cues\n", "every file of the set goes back together")
        self.assertFalse(self.has("lightkey_api.py.prev"))
        self.assertFalse(self.has("update-pending"))
        failed = json.loads(self.read("update-failed"))
        self.assertEqual(failed["version"], "0.6.0")
        self.assertEqual(failed["message"], "Version 0.6.0 would not start, so version 0.5.0 was put back.")
        sys.path.insert(0, HELPER)
        import lightkey_api as lk
        self.assertEqual(lk.last_update_error(self.app), failed["message"], "this is what /health reports as last_update_error")

    def test_a_success_in_between_starts_the_count_again(self):
        self.after_update(CRASHES)
        self.assertEqual(self.run_wrapper(), 3)
        self.assertEqual(self.run_wrapper(), 3)
        self.put("lightkey_api.py", fake("new", RUNS_AND_STOPS))
        self.assertEqual(self.run_wrapper(), 0)
        self.put("lightkey_api.py", fake("new", CRASHES))
        self.assertEqual(self.run_wrapper(), 3)
        self.assertTrue(self.has("lightkey_api.py.prev"), "one failure after a success is only one failure")
        self.assertFalse(self.has("update-failed"))

    def test_without_an_update_in_progress_a_crash_loop_never_changes_the_files(self):
        self.put("lightkey_api.py", fake("new", CRASHES))
        self.put("lightkey_api.py.prev", fake("old", RUNS_AND_STOPS))
        for _ in range(5):
            self.assertEqual(self.run_wrapper(), 3)
        self.assertEqual(self.started("old"), 0)
        self.assertFalse(self.has("update-failed"))

    def test_an_agent_that_stays_up_proves_the_new_version(self):
        self.after_update("import time\ntime.sleep(2)\n")
        self.assertEqual(self.run_wrapper(grace="0.5"), 0)
        self.assertFalse(self.has("update-pending"))
        self.put("lightkey_api.py", fake("new", CRASHES))
        for _ in range(4):
            self.assertEqual(self.run_wrapper(), 3)
        self.assertEqual(self.started("old"), 0, "a later crash loop must not send a proven version back")

    def test_an_interrupted_swap_is_finished_before_the_agent_starts_and_no_path_escapes(self):
        self.put("lightkey_api.py", fake("old", RUNS_AND_STOPS))
        os.makedirs(os.path.join(self.app, "update"))
        self.put(os.path.join("update", "lightkey_api.py"), fake("new", RUNS_AND_STOPS))
        self.put(os.path.join("update", "journal.json"), json.dumps({"from": "0.5.0", "to": "0.6.0", "files": ["lightkey_api.py", "../../evil.py"]}))
        self.assertEqual(self.run_wrapper(), 0)
        self.assertEqual((self.started("new"), self.started("old")), (1, 0))
        self.assertEqual(self.read("lightkey_api.py.prev"), fake("old", RUNS_AND_STOPS))
        self.assertFalse(self.has("update"))
        self.assertFalse(os.path.exists(os.path.join(os.path.dirname(self.app), "evil.py")))


if __name__ == "__main__":
    unittest.main()
