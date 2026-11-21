"""The Library Link's song folder lookup in the copy that StagePilot carries (the real tests of the helper are in its own repository).
Playback keeps some songs whose database row says content type 13 in a folder of type 32 (`32-29109-30`); only that pairing is
looked up, and the exact folder always wins.

Run:  python3 -m unittest discover -s scripts/playback-helper-tests -v   (Python 3.9 or newer)"""
import os
import shutil
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True      # never leave a __pycache__ inside the helper folder: it is a bundle of plain files
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "..", "crates", "stagepilot-playback", "assets", "playback-helper"))
import playback_library as pl  # noqa: E402


class Songs(pl.Library):
    """A library that only knows where its Songs folder is."""

    def __init__(self, folder):
        self.folder = folder

    def songs_dir(self):
        return self.folder


class SongFolderTests(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.mkdtemp(prefix="songs-")
        self.addCleanup(shutil.rmtree, self.root, ignore_errors=True)
        self.lib = Songs(self.root)

    def make(self, *names):
        for name in names:
            os.makedirs(os.path.join(self.root, name))

    def found(self, kind, cid):
        folder = self.lib.song_folder({"contenttypeid": kind, "contentid": cid})
        return os.path.basename(folder) if folder else None

    def test_a_type_13_song_without_its_own_folder_takes_the_type_32_one(self):
        self.make("32-1-5")
        self.assertEqual(self.found(13, 1), "32-1-5")

    def test_the_exact_folder_always_wins(self):
        self.make("32-1-5", "13-1-9")
        self.assertEqual(self.found(13, 1), "13-1-9")

    def test_no_other_pairing_and_no_guessing_by_id(self):
        self.make("32-3-1", "32-4-1", "13-5-1", "26-1-1", "32-10-1", "32-60-1")
        self.assertIsNone(self.found(12, 3))        # a click-track row
        self.assertIsNone(self.found(99, 4))        # an unknown type
        self.assertIsNone(self.found(32, 5))        # not the other way round
        self.assertIsNone(self.found(13, 6))        # 6 is not 60
        self.assertIsNone(self.found(13, 1))        # another type with the same id
        self.assertIsNone(self.found(None, None))

    def test_without_a_songs_folder_there_is_none(self):
        shutil.rmtree(self.root)
        self.assertIsNone(self.found(13, 1))


if __name__ == "__main__":
    unittest.main()
