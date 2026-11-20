"""A structural test of the screen scripts the ProPresenter Link sends to a Mac.

The scripts drive ProPresenter's own Search panel and Import dialog by typing, and they can only run on a Mac with a real
ProPresenter, never in CI. What CAN be checked here is that the safety checks around the typing are still in the text, so
nobody removes them by accident:

* before EVERY `keystroke` / `key code` the script asserts that ProPresenter is the front app and that the field meant to
  receive the keys has the keyboard (`assertFocus`), so keys never go to anything else;
* after the song number is typed, the Search field is read back and must equal the number exactly (case sensitive), else
  `ERR|type the song number|...` with at most 40 characters of what it held;
* after the name is typed, the Name field is read back and must equal the name exactly, the other text fields of the dialog
  must still hold what they held before, and the Library must still read Songs (`verifyDialog`), again right before Import;
* the helper's own copy of each script is the file that ships with StagePilot.

This proves the text is there, NOT that it works in ProPresenter: that needs the rig.

    python3 -m unittest discover -s scripts/propresenter-link-tests
"""

import os
import re
import sys
import unittest

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
GUI = os.path.join(ROOT, "crates", "stagepilot-propresenter", "assets", "gui")
HELPER = os.path.join(ROOT, "crates", "stagepilot-propresenter", "assets", "propresenter-link", "propresenter_link.py")

KEYS = re.compile(r"^\s*(keystroke|key code)\b")


def read(name):
    with open(os.path.join(GUI, name), encoding="utf-8", newline="") as handle:
        return handle.read()


def code_lines(text):
    """(number, text) of every line that is code: no blank lines and no `--` comment lines."""
    return [(n, line) for n, line in enumerate(text.split("\n"), 1) if line.strip() and not line.strip().startswith("--")]


def handler(text, name):
    match = re.search(r"^on %s\(.*?^end %s$" % (name, name), text, re.S | re.M)
    return match.group(0) if match else ""


class ImportScript(unittest.TestCase):
    def setUp(self):
        self.text = read("import_song.applescript")
        self.code = code_lines(self.text)

    def test_every_keystroke_is_right_after_a_focus_check_on_the_field_that_gets_it(self):
        keys = [(n, line) for n, line in self.code if KEYS.match(line)]
        self.assertEqual(len(keys), 5, "the four keystrokes and the one Return the script has: %r" % [k[1].strip() for k in keys])
        for index, (number, line) in enumerate(self.code):
            if not KEYS.match(line):
                continue
            before = self.code[index - 1][1].strip()
            self.assertRegex(before, r"^my assertFocus\(procName, .+, \"[A-Za-z ]+\"\)$", "line %d: %s" % (number, line.strip()))
        # The check names the right target for each key.
        targets = [self.code[i - 1][1].strip() for i, (_, line) in enumerate(self.code) if KEYS.match(line)]
        self.assertEqual([re.search(r"\"([^\"]+)\"\)$", t).group(1) for t in targets],
                         ["Search field", "Search field", "results list", "Name field", "Name field"])

    def test_the_focus_check_wants_the_front_app_and_the_focused_field_and_stops_otherwise(self):
        body = handler(self.text, "assertFocus")
        self.assertTrue(body, "assertFocus is missing")
        self.assertIn("frontmost of process procName", body)
        self.assertIn("focused of target", body)
        self.assertRegex(body, r"if isFront is not true then error")
        self.assertRegex(body, r"if hasFocus is not true then error")

    def test_the_song_number_is_read_back_and_must_be_the_number_exactly(self):
        typed = self.text.index("keystroke songId")
        after = self.text[typed:typed + 700]
        self.assertIn("set held to my textOf(field)", after)
        self.assertIn("if not my sameText(held, songId) then error", after)
        self.assertIn("instead of the song number", after)
        self.assertIn("my shortText(held)", after, "what the field held is printed, shortened")
        # ... inside the step that is named in the error line the engine shows.
        self.assertLess(self.text.index('set stepName to "type the song number"'), typed)
        self.assertLess(typed, self.text.index('set stepName to "read the search result"'))

    def test_the_comparison_is_case_sensitive_and_what_is_printed_is_at_most_forty_characters(self):
        self.assertRegex(handler(self.text, "sameText"), r"considering case\s+return \(\(a as text\) is equal to \(b as text\)\)")
        short = handler(self.text, "shortText")
        self.assertIn("if n > 40 then set n to 40", short)
        self.assertIn('"?"', short, "control characters are not printed")

    def test_the_name_is_read_back_the_other_fields_are_unchanged_and_the_library_is_still_songs(self):
        body = handler(self.text, "verifyDialog")
        self.assertTrue(body, "verifyDialog is missing")
        self.assertIn("my sameText(held, songName)", body, "the Name field must equal the name")
        self.assertIn("my sameText(nowHeld, item i of heldBefore)", body, "the other text fields must hold what they held")
        self.assertIn('my sameText(my textOf(libraryPopup), "Songs")', body, "the Library must still read Songs")
        # The values are read BEFORE the name is typed, compared right after, and again right before Import is pressed.
        before_typing = self.text.index("set end of heldBefore to my textOf(f)")
        typing = self.text.index("keystroke songName")
        checks = [m.start() for m in re.finditer(r"my verifyDialog\(textFields, heldBefore, nameIndex, songName, libraryPopup\)", self.text)]
        self.assertEqual(len(checks), 2)
        self.assertLess(before_typing, typing)
        self.assertLess(typing, checks[0])
        self.assertLess(checks[0], self.text.index("click playlistPopup"))
        self.assertLess(self.text.index("click playlistPopup"), checks[1])
        self.assertLess(checks[1], self.text.index('click button "Import" of dlg'))

    def test_an_error_cancels_the_dialog_and_closes_the_panel_and_prints_err_with_the_step(self):
        self.assertIn("my closeEverything(procName)", self.text.split("on error msg", 1)[1])
        self.assertIn('return "ERR|" & stepName & "|" & msg', self.text)
        self.assertIn('click button "Cancel" of dlg', handler(self.text, "closeEverything"))

    def test_the_number_must_be_digits_before_anything_is_typed(self):
        self.assertIn('"0123456789" does not contain (ch as text)', self.text)
        self.assertLess(self.text.index('"0123456789" does not contain'), self.text.index('set stepName to "find ProPresenter"'))


class OtherScripts(unittest.TestCase):
    def test_only_the_import_script_types_and_the_others_press_no_keys(self):
        for name in sorted(os.listdir(GUI)):
            if name == "import_song.applescript" or not name.endswith(".applescript"):
                continue
            for number, line in code_lines(read(name)):
                self.assertIsNone(KEYS.match(line), "%s line %d sends keys: %s" % (name, number, line.strip()))
                self.assertNotIn("click at", line, "%s line %d clicks a position" % (name, number))

    def test_the_helper_carries_exactly_the_scripts_that_ship_with_stagepilot(self):
        with open(HELPER, encoding="utf-8", newline="") as handle:
            helper = handle.read()
        for variable, name in (("CHECK_ACCESS_APPLESCRIPT", "check_access.applescript"), ("IMPORT_SONG_APPLESCRIPT", "import_song.applescript"),
                               ("SELECT_LIBRARY_APPLESCRIPT", "select_library.applescript")):
            self.assertIn("%s = r'''%s'''\n" % (variable, read(name)), helper, "%s differs from %s" % (variable, name))


if __name__ == "__main__":
    unittest.main()
