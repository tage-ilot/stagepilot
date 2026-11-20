"""Tests of tools/preview_propresenter_link.py, the preview of the ProPresenter Link block and the Presenter Link section: the
made-up songs are real enough for StagePilot's engine (the fake API reads their arrangement), the access word keeps strangers out, a
page of states changes only the two routes it says, and everything else goes to the backend untouched.

    python3 -m unittest tools/test_fake_propresenter_preview.py
"""

import argparse
import datetime
import http.client
import http.server
import json
import os
import shutil
import sys
import tempfile
import threading
import unittest
import urllib.request

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import fake_playback_library as playback_library  # noqa: E402
import fake_propresenter_api as pp_api  # noqa: E402
import preview_propresenter_link as preview  # noqa: E402


class Backend(http.server.BaseHTTPRequestHandler):
    """A stand-in for the demo backend: it answers with what it was asked, so a test sees what the preview server forwarded."""

    seen = []

    def log_message(self, format, *args):
        pass

    def do_GET(self):
        self.reply()

    do_POST = do_PUT = do_DELETE = do_GET

    def reply(self):
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length) if length else b""
        Backend.seen.append({"method": self.command, "path": self.path, "host": self.headers.get("Host"), "origin": self.headers.get("Origin"), "cookie": self.headers.get("Cookie"), "body": body})
        data = json.dumps({"from": "backend", "path": self.path}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Set-Cookie", "session=1; Path=/")
        self.end_headers()
        self.wfile.write(data)


class Rig:
    def __init__(self):
        Backend.seen = []
        self.backend = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Backend)
        threading.Thread(target=self.backend.serve_forever, daemon=True).start()
        args = argparse.Namespace(token="the-word", port=0, lan_ip="192.168.1.20", stagepilot="127.0.0.1:%d" % self.backend.server_address[1],
                                  folder=tempfile.mkdtemp(prefix="pp-preview-test-"))
        preview.make_folder(args.folder)
        self.args = args
        self.state, self.api_server, self.api_url = preview.start_api(args.folder, 0, "now")
        self.preview = preview.Preview(args, self.state)
        preview.Handler.preview = self.preview
        self.server = preview.Server(("127.0.0.1", 0), preview.Handler)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.port = self.server.server_address[1]
        self.cookie = None

    def close(self):
        shutil.rmtree(self.args.folder, ignore_errors=True)
        self.server.shutdown()
        self.server.server_close()
        self.backend.shutdown()
        self.backend.server_close()
        self.api_server.shutdown()

    def call(self, method, path, body=None, cookie=True):
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=10)
        headers = {"Content-Type": "application/json"}
        if cookie:
            headers["Cookie"] = "pv=the-word"
        conn.request(method, path, body=json.dumps(body) if body is not None else None, headers=headers)
        response = conn.getresponse()
        data = response.read()
        result = (response.status, dict(response.getheaders()), data)
        conn.close()
        return result


class PreviewTests(unittest.TestCase):
    def setUp(self):
        self.rig = Rig()
        self.addCleanup(self.rig.close)

    def test_the_made_up_songs_are_songs_the_engine_can_read(self):
        songs = os.path.join(self.rig.args.folder, "Libraries", "Songs")
        files = sorted(os.listdir(songs))
        self.assertEqual(files, ["Firm Foundation (Live).pro", "Firm Foundation (Studio).pro", "Gratitude.pro", "Holy Forever (Live).pro", "Holy Forever.pro"])
        for name in files:
            with open(os.path.join(songs, name), "rb") as handle:
                self.assertEqual(pp_api.selected_arrangement(handle.read()), "Full", name)
        self.assertTrue(os.path.isdir(os.path.join(self.rig.args.folder, "Libraries", "Playback")))

    def test_the_numbers_inside_the_songs_are_the_numbers_playback_gives_the_same_songs(self):
        days = (datetime.date.today() - playback_library.EPOCH_DATE).days
        songs = os.path.join(self.rig.args.folder, "Libraries", "Songs")
        want = {"Holy Forever.pro": ("Holy Forever", 0), "Holy Forever (Live).pro": ("Holy Forever", 0), "Gratitude.pro": ("Gratitude", 3),
                "Firm Foundation (Live).pro": ("Firm Foundation", 4), "Firm Foundation (Studio).pro": ("Firm Foundation", 4)}
        for file, (title, index) in want.items():
            with open(os.path.join(songs, file), "rb") as handle:
                data = handle.read()
            number = playback_library.song_master_id(1_000_000 + days * 10 + index + 1)
            self.assertIn(preview.vrec(1, number), data, file)
            self.assertIn(title.encode(), data)

    def test_no_real_song_text_is_made_up_in_the_files(self):
        with open(os.path.join(self.rig.args.folder, "Libraries", "Songs", "Holy Forever.pro"), "rb") as handle:
            data = handle.read()
        self.assertIn(b"made-up slide for", data)

    def test_the_made_up_propresenter_has_playlists_with_headers_and_answers_the_timer_questions(self):
        for path, check in (
            ("/v1/playlists", lambda body: [p["id"]["name"] for p in body] == ["Sunday Morning Service", "Wednesday Night", "Special services"]),
            ("/v1/timers", lambda body: body[0]["id"]["name"] == "Song Countdown"),
            ("/v1/looks", lambda body: body[0]["id"]["name"] == "Default"),
            ("/version", lambda body: body["host_description"].startswith("ProPresenter")),
        ):
            with urllib.request.urlopen(self.rig.api_url + path, timeout=10) as response:
                self.assertTrue(check(json.loads(response.read())), path)
        sunday = self.rig.state.playlists[0]
        self.assertEqual([i["id"]["name"] for i in sunday["items"]], ["Welcome", "Worship", "Message", "Closing"])

    def test_nobody_gets_in_without_the_word(self):
        status, _, body = self.rig.call("GET", "/api/v1/state", cookie=False)
        self.assertEqual(status, 403)
        self.assertEqual(Backend.seen, [])
        status, _, _ = self.rig.call("GET", "/?pv=wrong", cookie=False)
        self.assertEqual(status, 403)
        self.assertEqual(Backend.seen, [])

    def test_the_word_in_the_address_becomes_a_cookie_and_is_not_kept_in_the_address(self):
        status, headers, _ = self.rig.call("GET", "/dash?pv=the-word&x=1", cookie=False)
        self.assertEqual(status, 303)
        self.assertEqual(headers["Location"], "/dash?x=1")
        self.assertIn("pv=the-word", headers["Set-Cookie"])
        self.assertIn("HttpOnly", headers["Set-Cookie"])
        self.assertEqual(Backend.seen, [])

    def test_everything_else_goes_to_the_backend_as_if_asked_there_and_the_word_is_never_passed_on(self):
        status, headers, body = self.rig.call("POST", "/api/v1/propresenter-link/add-songs/apply", {"plan_id": "x", "skip": []})
        self.assertEqual(status, 200)
        self.assertEqual(json.loads(body), {"from": "backend", "path": "/api/v1/propresenter-link/add-songs/apply"})
        self.assertEqual(headers["Set-Cookie"], "session=1; Path=/")
        seen = Backend.seen[-1]
        self.assertEqual((seen["method"], seen["host"], seen["origin"]), ("POST", self.rig.args.stagepilot, "http://" + self.rig.args.stagepilot))
        self.assertIsNone(seen["cookie"])
        self.assertEqual(json.loads(seen["body"]), {"plan_id": "x", "skip": []})

    def test_the_status_and_setup_routes_are_the_backends_until_a_state_is_picked(self):
        status, _, body = self.rig.call("GET", "/api/v1/propresenter-link/status")
        self.assertEqual(json.loads(body)["from"], "backend")
        status, _, body = self.rig.call("POST", "/api/v1/propresenter-link/helper-install", {})
        self.assertEqual(json.loads(body)["from"], "backend")

    def test_a_picked_state_changes_only_those_two_routes(self):
        for key, label, over in preview.SCENARIOS:
            if over is None:
                continue
            self.assertEqual(self.rig.call("GET", "/__preview/set?scenario=" + key)[0], 303)
            status, _, body = self.rig.call("GET", "/api/v1/propresenter-link/status")
            said = json.loads(body)
            self.assertEqual((said["mode"], said["state"]), ("helper", over["state"]), key)
            self.assertEqual(said["bundled_version"], "0.1.1")
            self.assertTrue(said["message"])
            if said["state"] in ("not_installed", "not_answering", "needs_command"):
                self.assertFalse(said["can_import"])
        self.rig.call("GET", "/__preview/set?scenario=ready")
        status, _, body = self.rig.call("GET", "/api/v1/playback-library/status")
        self.assertEqual(json.loads(body)["from"], "backend")
        status, _, body = self.rig.call("GET", "/api/v1/propresenter-link/playlists")
        self.assertEqual(json.loads(body)["from"], "backend")
        self.rig.call("GET", "/__preview/set?scenario=live")
        self.assertEqual(json.loads(self.rig.call("GET", "/api/v1/propresenter-link/status")[2])["from"], "backend")

    def test_the_cannot_import_state_carries_the_one_sentence_fix_and_the_outdated_one_a_way_to_update(self):
        self.rig.call("GET", "/__preview/set?scenario=no_import")
        said = json.loads(self.rig.call("GET", "/api/v1/propresenter-link/status")[2])
        self.assertTrue(said["state"] == "ready" and not said["can_import"])
        self.assertIn("Accessibility", said["import_blocker"])
        self.rig.call("GET", "/__preview/set?scenario=outdated")
        said = json.loads(self.rig.call("GET", "/api/v1/propresenter-link/status")[2])
        self.assertEqual(said["update"]["mode"], "manual")
        self.rig.call("GET", "/__preview/set?scenario=outdated_auto")
        self.assertEqual(json.loads(self.rig.call("GET", "/api/v1/propresenter-link/status")[2])["update"]["mode"], "automatic")

    def test_a_picked_state_gives_a_made_up_setup_command_that_ends(self):
        self.rig.call("GET", "/__preview/set?scenario=not_installed")
        status, _, body = self.rig.call("POST", "/api/v1/propresenter-link/helper-install", {})
        made = json.loads(body)
        self.assertEqual(status, 200)
        self.assertTrue(made["command"].startswith("curl -fsS http://192.168.1.20:"))
        self.assertIn("/propresenter-helper/", made["command"])
        self.assertTrue(made["expires_at"].endswith("Z"))
        self.assertEqual(made["bundled_version"], "0.1.1")
        status, _, body = self.rig.call("DELETE", "/api/v1/propresenter-link/helper-install")
        self.assertEqual(json.loads(body), {"ended": True})
        # Nothing of it reached the backend.
        self.assertEqual([seen for seen in Backend.seen if "helper-install" in seen["path"]], [])

    def test_the_state_page_lists_every_state_and_marks_the_chosen_one(self):
        status, _, body = self.rig.call("GET", "/__preview/")
        page = body.decode()
        self.assertEqual(status, 200)
        for _, label, _ in preview.SCENARIOS:
            self.assertIn(label.replace(">", "&gt;"), page.replace(">", "&gt;"))
        self.rig.call("GET", "/__preview/set?scenario=needs_command")
        page = self.rig.call("GET", "/__preview/")[2].decode()
        self.assertEqual(page.count('class="b on"'), 1)

    def test_start_over_forgets_added_songs_but_keeps_the_headers(self):
        sunday = self.rig.state.playlists[0]
        sunday["items"].append({"id": {"uuid": "X", "name": "A song", "index": 4}, "type": "presentation"})
        self.assertEqual(self.rig.call("POST", "/__preview/reset")[0], 303)
        self.assertEqual([i["id"]["name"] for i in sunday["items"]], ["Welcome", "Worship", "Message", "Closing"])

    def test_loading_a_new_file_only_after_the_library_was_opened(self):
        self.rig.call("POST", "/__preview/lazy")
        self.assertEqual(self.rig.state.lazy, "select")
        marker = os.path.join(self.rig.args.folder, ".selected-Playback")
        self.assertFalse(os.path.exists(marker))
        self.rig.call("POST", "/__preview/select")
        self.assertTrue(os.path.exists(marker))

    def test_playback_can_stop_giving_songs_and_give_them_again(self):
        # A stand-in process for the Library Link: the page stops it (the Presenter Link then lists Planning Center names) and starts it again.
        self.rig.preview.library_command = [sys.executable, "-c", "import time; time.sleep(120)"]
        self.rig.preview.start_library()
        self.addCleanup(self.rig.preview.stop_library)
        self.assertTrue(self.rig.preview.library_running())
        _, _, page = self.rig.call("GET", "/__preview/")
        self.assertIn(b"given (the Library Link answers)", page)
        status, _, _ = self.rig.call("POST", "/__preview/playback-off")
        self.assertEqual(status, 303)
        self.assertFalse(self.rig.preview.library_running())
        _, _, page = self.rig.call("GET", "/__preview/")
        self.assertIn(b"none (the plan", page)
        status, _, _ = self.rig.call("POST", "/__preview/playback-on")
        self.assertEqual(status, 303)
        self.assertTrue(self.rig.preview.library_running())

    def test_pretend_slides_are_on_screen_and_clear_them_again(self):
        page = self.rig.call("GET", "/__preview/")[2].decode()
        self.assertIn("Pretend ProPresenter is showing slides", page)
        self.assertIn("nothing (a camera input and audio do not count)", page)
        self.assertEqual(self.rig.call("POST", "/__preview/slides")[0], 303)
        self.assertTrue(self.rig.state.layers["slide"])
        self.assertIn("<b>slide</b>", self.rig.call("GET", "/__preview/")[2].decode())
        self.assertEqual(self.rig.call("POST", "/__preview/clear")[0], 303)
        self.assertFalse(self.rig.state.layers["slide"])
        self.assertTrue(self.rig.state.layers["video_input"])

    def test_a_made_up_song_has_two_different_count_off_groups_so_the_preview_shows_a_guess(self):
        songs = os.path.join(self.rig.args.folder, "Libraries", "Songs")
        with open(os.path.join(songs, "Gratitude.pro"), "rb") as handle:
            gratitude = handle.read()
        with open(os.path.join(songs, "Holy Forever.pro"), "rb") as handle:
            holy = handle.read()
        self.assertIn(b"count off, take 2", gratitude)
        self.assertNotIn(b"count off, take 2", holy)


if __name__ == "__main__":
    unittest.main()
