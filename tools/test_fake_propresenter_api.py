"""Tests of tools/fake_propresenter_api.py: the rules it keeps from the proven ProPresenter 19.0.1 API.

    python3 -m unittest tools/test_fake_propresenter_api.py
"""

import json
import os
import sys
import unittest
import urllib.error
import urllib.request

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import fake_propresenter_api as fake  # noqa: E402


class Api:
    def __init__(self, **kwargs):
        self.state = fake.State(**kwargs)
        self.server, self.base = fake.serve(self.state)

    def stop(self):
        self.server.shutdown()
        self.server.server_close()

    def call(self, method, path, value=None):
        data = None if value is None else json.dumps(value).encode()
        request = urllib.request.Request(self.base + path, data=data, method=method, headers={"Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(request, timeout=5) as r:
                body = r.read()
                return r.status, json.loads(body) if body else None
        except urllib.error.HTTPError as e:
            try:
                return e.code, None
            finally:
                e.close()


def item(song, kind="presentation", name=None, uuid=None, duration=0.0):
    entry = song["id"]
    shown = entry["name"][: -len(fake.SUFFIX)]
    return {"id": {"uuid": uuid or entry["uuid"], "name": name or shown, "index": 0}, "type": kind, "is_hidden": False, "is_pco": False,
            "presentation_info": {"presentation_uuid": uuid or entry["uuid"], "arrangement_name": "Whatever"}, "header_color": None, "duration": duration}


class FakeProPresenterApiTests(unittest.TestCase):
    def setUp(self):
        self.api = Api(libraries={"Songs": ["Amazing Grace", "How Great"], "Playback": []}, playlists=["Sunday"], arrangements={"How Great": "Playback"})
        self.addCleanup(self.api.stop)
        self.sunday = self.api.state.playlists[0]
        self.grace, self.great = self.api.state.libraries[0]["songs"]

    def test_libraries_and_playlists_are_listed(self):
        status, libraries = self.api.call("GET", "/v1/libraries")
        self.assertEqual([l["name"] for l in libraries], ["Songs", "Playback"])
        status, library = self.api.call("GET", "/v1/library/" + libraries[0]["uuid"])
        self.assertEqual([s["id"]["name"] for s in library["items"]], ["Amazing Grace - [ Library ]", "How Great - [ Library ]"])
        status, playlists = self.api.call("GET", "/v1/playlists")
        self.assertEqual([p["id"]["name"] for p in playlists], ["Sunday"])
        self.assertEqual(self.api.call("GET", "/v1/library/nope")[0], 404)
        self.assertEqual(self.api.call("GET", "/v1/playlist/nope")[0], 404)

    def test_a_playlist_is_made_and_there_is_no_delete(self):
        status, made = self.api.call("POST", "/v1/playlists", {"name": "Easter", "type": "playlist"})
        self.assertEqual((status, made["id"]["name"]), (200, "Easter"))
        self.assertEqual(self.api.call("POST", "/v1/playlists", {"name": "x"})[0], 404, "the type is needed")
        self.assertEqual(self.api.call("DELETE", "/v1/playlist/" + made["id"]["uuid"])[0], 405)

    def test_put_replaces_the_whole_list_and_the_arrangement_is_the_songs_own(self):
        path = "/v1/playlist/" + self.sunday["id"]["uuid"]
        status, body = self.api.call("PUT", path, {"items": [item(self.grace, duration=12.5), item(self.great)]})
        self.assertEqual(status, 200)
        names = [(i["id"]["name"], i["presentation_info"]["arrangement_name"], i["duration"]) for i in body["items"]]
        self.assertEqual(names, [("Amazing Grace", "Default", 12.5), ("How Great", "Playback", 0.0)])
        status, body = self.api.call("PUT", path, {"items": [item(self.great)]})
        self.assertEqual([i["id"]["name"] for i in self.api.call("GET", path)[1]["items"]], ["How Great"], "the old items are gone")

    def test_a_wrong_uuid_or_name_refuses_the_whole_put_and_changes_nothing(self):
        path = "/v1/playlist/" + self.sunday["id"]["uuid"]
        self.api.call("PUT", path, {"items": [item(self.grace)]})
        for bad in (item(self.great, uuid="NOT-A-UUID"), item(self.great, name="How Great - [ Library ]"), item(self.great, name="Other")):
            self.assertEqual(self.api.call("PUT", path, {"items": [item(self.great), bad]})[0], 404)
            self.assertEqual([i["id"]["name"] for i in self.api.call("GET", path)[1]["items"]], ["Amazing Grace"])
        mismatch = item(self.great)
        mismatch["presentation_info"]["presentation_uuid"] = self.grace["id"]["uuid"]
        self.assertEqual(self.api.call("PUT", path, {"items": [mismatch]})[0], 404)
        self.assertEqual(self.api.call("PUT", "/v1/playlist/unknown", {"items": []})[0], 404)

    def test_headers_and_placeholders_are_items_and_are_not_checked(self):
        path = "/v1/playlist/" + self.sunday["id"]["uuid"]
        header = {"id": {"uuid": "H1", "name": "Worship", "index": 0}, "type": "header", "header_color": {"red": 1, "green": 0, "blue": 0, "alpha": 1}}
        status, body = self.api.call("PUT", path, {"items": [header, item(self.grace)]})
        self.assertEqual(status, 200)
        self.assertEqual([i["type"] for i in body["items"]], ["header", "presentation"])


if __name__ == "__main__":
    unittest.main()
