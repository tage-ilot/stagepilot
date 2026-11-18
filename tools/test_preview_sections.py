"""Tests of tools/preview_sections.py, which serves the built section gallery: `/` is the gallery page, files come out of the gallery
folder only (no way up and out of it), nothing can be written, no folder is listed, and a missing build is said in one sentence.

    python3 -m unittest tools/test_preview_sections.py
"""

import http.client
import os
import sys
import tempfile
import threading
import unittest

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import preview_sections as sections  # noqa: E402


class PreviewSections(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.TemporaryDirectory()
        os.makedirs(os.path.join(self.root.name, "assets"))
        with open(os.path.join(self.root.name, "sections.html"), "w") as page:
            page.write("<!doctype html><title>gallery</title>")
        with open(os.path.join(self.root.name, "assets", "a.js"), "w") as script:
            script.write("console.log(1)")
        with open(os.path.join(os.path.dirname(self.root.name), "outside-secret.txt"), "w") as outside:
            outside.write("secret")
        self.server = sections.make_server(self.root.name, "127.0.0.1", 0)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.root.cleanup()
        try:
            os.remove(os.path.join(os.path.dirname(self.root.name), "outside-secret.txt"))
        except OSError:
            pass

    def ask(self, path, method="GET"):
        connection = http.client.HTTPConnection("127.0.0.1", self.server.server_address[1], timeout=5)
        connection.request(method, path)
        response = connection.getresponse()
        body = response.read()
        headers = {key.lower(): value for key, value in response.getheaders()}
        connection.close()
        return response.status, headers, body

    def test_the_root_is_the_gallery_page(self):
        for path in ("/", "/?frame=1", "/sections.html"):
            status, headers, body = self.ask(path)
            self.assertEqual(status, 200, path)
            self.assertIn(b"<title>gallery</title>", body)
            self.assertIn("text/html", headers["content-type"])

    def test_assets_are_served_and_never_cached(self):
        status, headers, body = self.ask("/assets/a.js")
        self.assertEqual((status, body), (200, b"console.log(1)"))
        self.assertEqual(headers["cache-control"], "no-store")

    def test_nothing_outside_the_folder_and_no_listing(self):
        self.assertEqual(self.ask("/../outside-secret.txt")[0], 404)
        self.assertEqual(self.ask("/%2e%2e/outside-secret.txt")[0], 404)
        self.assertEqual(self.ask("/assets/")[0], 404)
        self.assertEqual(self.ask("/nope.txt")[0], 404)

    def test_it_only_reads(self):
        for method in ("POST", "PUT", "DELETE", "PATCH"):
            self.assertEqual(self.ask("/", method)[0], 405, method)

    def test_a_gallery_that_was_not_built_is_said_in_one_sentence(self):
        with tempfile.TemporaryDirectory() as empty:
            with self.assertRaisesRegex(FileNotFoundError, "npm run build:gallery"):
                sections.make_server(empty, "127.0.0.1", 0)
            self.assertEqual(sections.main(["--root", empty, "--port", "0"]), 2)


if __name__ == "__main__":
    unittest.main()
