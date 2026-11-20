"""Tests of tools/fake_propresenter_api.py: the rules it keeps from the proven ProPresenter 19.0.1 API.

    python3 -m unittest tools/test_fake_propresenter_api.py
"""

import json
import os
import sys
import time
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


def varint(value):
    out = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        if not value:
            out.append(byte)
            return bytes(out)
        out.append(byte | 0x80)


def record(number, payload):
    return varint(number << 3 | 2) + varint(len(payload)) + payload


def song_file(selected="Playback", extra=b""):
    """A tiny made-up `.pro`: two arrangements (Full, Playback) and one of them selected."""
    def arrangement(uid, name):
        return record(11, record(1, record(1, uid.encode())) + record(2, name.encode()))
    chosen = {"Full": "AAAA", "Playback": "BBBB"}.get(selected)
    out = arrangement("AAAA", "Full") + arrangement("BBBB", "Playback")
    if chosen:
        out += record(10, record(1, chosen.encode()))
    return out + extra


class FakeProPresenterApiTests(unittest.TestCase):
    def setUp(self):
        self.api = Api(libraries={"Songs": ["Amazing Grace", "How Great"], "Playback": []}, playlists=["Sunday"], arrangements={"How Great": "Playback"})
        self.addCleanup(self.api.stop)
        self.sunday = self.api.state.playlists[0]
        self.grace, self.great = self.api.state.memory[0]["songs"]

    def test_libraries_and_playlists_are_listed(self):
        status, libraries = self.api.call("GET", "/v1/libraries")
        self.assertEqual([l["id"]["name"] for l in libraries], ["Songs", "Playback"])
        status, library = self.api.call("GET", "/v1/library/" + libraries[0]["id"]["uuid"])
        self.assertEqual(library["updateType"], "all")
        self.assertEqual([s["name"] for s in library["items"]], ["Amazing Grace - [ Library ]", "How Great - [ Library ]"])
        self.assertTrue(all(s["uuid"] for s in library["items"]))
        status, playlists = self.api.call("GET", "/v1/playlists")
        self.assertEqual([(p["id"]["name"], p["type"]) for p in playlists], [("Sunday", "playlist")])
        status, version = self.api.call("GET", "/version")
        self.assertEqual((status, version["host_description"], version["api_version"]), (200, "ProPresenter 19.0.1", "v1"))
        self.assertEqual(self.api.call("GET", "/v1/library/nope")[0], 404)
        self.assertEqual(self.api.call("GET", "/v1/playlist/nope")[0], 404)

    def test_a_playlist_is_made_and_there_is_no_delete(self):
        status, made = self.api.call("POST", "/v1/playlists", {"name": "Easter", "type": "playlist"})
        self.assertEqual((status, made["id"]["name"], made["type"]), (200, "Easter", "playlist"))
        self.assertEqual(self.api.call("POST", "/v1/playlists", {"name": "x"})[0], 404, "the type is needed")
        self.assertEqual(self.api.call("DELETE", "/v1/playlist/" + made["id"]["uuid"])[0], 405)

    def test_put_replaces_the_whole_list_and_the_arrangement_is_the_songs_own(self):
        path = "/v1/playlist/" + self.sunday["id"]["uuid"]
        status, body = self.api.call("PUT", path, [item(self.grace, duration=12.5), item(self.great)])
        self.assertEqual((status, body), (204, None), "the answer is empty, as ProPresenter's is")
        names = [(i["id"]["name"], i["presentation_info"]["arrangement_name"], i["duration"]) for i in self.api.call("GET", path)[1]["items"]]
        self.assertEqual(names, [("Amazing Grace", "Default", 12.5), ("How Great", "Playback", 0.0)])
        self.assertEqual(self.api.call("PUT", path, {"items": [item(self.great)]})[0], 204, "the items may also come in an object")
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
        status, body = self.api.call("PUT", path, [header, item(self.grace)])
        self.assertEqual(status, 204)
        self.assertEqual([i["type"] for i in self.api.call("GET", path)[1]["items"]], ["header", "presentation"])


class FolderModeTests(unittest.TestCase):
    """The libraries are the folders of a ProPresenter folder and a file placed later is loaded as `--lazy` says."""

    def make(self, lazy):
        import tempfile
        self.root = tempfile.mkdtemp(prefix="fake-pp-api-")
        for library in ("Songs", "Playback"):
            os.makedirs(os.path.join(self.root, "Libraries", library))
        with open(os.path.join(self.root, "Libraries", "Songs", "Old.pro"), "wb") as f:
            f.write(song_file("Full"))
        self.api = Api(folder=self.root, lazy=lazy)
        self.addCleanup(self.api.stop)
        self.addCleanup(__import__("shutil").rmtree, self.root, True)

    def place(self, library, name, selected="Playback"):
        with open(os.path.join(self.root, "Libraries", library, name), "wb") as f:
            f.write(song_file(selected))

    def listing(self, library):
        status, libraries = self.api.call("GET", "/v1/libraries")
        uuid = next(l["id"]["uuid"] for l in libraries if l["id"]["name"] == library)
        return self.api.call("GET", "/v1/library/" + uuid)[1]["items"]

    def test_files_there_at_the_start_are_loaded_and_later_ones_follow_the_lazy_rule(self):
        self.make("select")
        self.assertEqual([(i["name"], bool(i.get("uuid"))) for i in self.listing("Songs")], [("Old - [ Songs ]", True)])
        self.place("Playback", "New (1).pro")
        self.assertEqual([(i["name"], "uuid" in i) for i in self.listing("Playback")], [("New (1) - [ Playback ]", False)], "no id yet")
        open(os.path.join(self.root, ".selected-Playback"), "w").close()
        self.assertTrue(self.listing("Playback")[0]["uuid"], "the id comes once the library was selected")

    def test_never_and_a_number_of_seconds(self):
        self.make("never")
        self.place("Playback", "A.pro")
        self.assertNotIn("uuid", self.listing("Playback")[0])
        self.api.state.lazy = 0.2
        self.assertNotIn("uuid", self.listing("Playback")[0])
        time.sleep(0.3)
        self.assertIn("uuid", self.listing("Playback")[0])

    def test_a_playlist_item_must_name_a_loaded_file_and_reads_back_the_files_own_arrangement(self):
        self.make("now")
        self.place("Playback", "New (1).pro", "Playback")
        self.place("Playback", "Other (1).pro", "Full")
        playlist = self.api.state.add_playlist("Sunday")
        path = "/v1/playlist/" + playlist["id"]["uuid"]
        items = {i["name"].split(" - ")[0]: i["uuid"] for i in self.listing("Playback")}
        def entry(name):
            return {"id": {"uuid": items[name], "name": name, "index": 0}, "type": "presentation", "presentation_info": {"presentation_uuid": items[name], "arrangement_name": "Whatever"}}
        self.assertEqual(self.api.call("PUT", path, [entry("New (1)"), entry("Other (1)")])[0], 204)
        read = self.api.call("GET", path)[1]["items"]
        self.assertEqual([(i["id"]["name"], i["presentation_info"]["arrangement_name"]) for i in read], [("New (1)", "Playback"), ("Other (1)", "Full")])
        wrong = entry("New (1)")
        wrong["id"]["name"] = "New (1) - [ Playback ]"
        self.assertEqual(self.api.call("PUT", path, [wrong])[0], 404)

    def test_playlists_inside_a_folder_are_listed_as_a_group(self):
        self.make("now")
        self.api.state.add_playlist("Top")
        self.api.state.add_playlist("Evening", folder="Services")
        status, tree = self.api.call("GET", "/v1/playlists")
        self.assertEqual([(p["id"]["name"], p["type"]) for p in tree], [("Top", "playlist"), ("Services", "group")])
        self.assertEqual([p["id"]["name"] for p in tree[1]["playlists"]], ["Evening"])

    def test_the_selected_arrangement_is_read_from_the_file_and_junk_is_nothing(self):
        self.assertEqual(fake.selected_arrangement(song_file("Playback")), "Playback")
        self.assertEqual(fake.selected_arrangement(song_file("Full")), "Full")
        self.assertEqual(fake.selected_arrangement(song_file(None)), "")
        self.assertEqual(fake.selected_arrangement(b"\xff\xff\xff"), "")
        self.assertEqual(fake.selected_arrangement(b""), "")


class Layers(unittest.TestCase):
    """GET /v1/status/layers: what is on screen, as the real rig answers it."""

    def setUp(self):
        self.api = Api(libraries={"Songs": []})
        self.addCleanup(self.api.stop)

    def test_it_answers_like_the_real_rig_with_a_camera_input_and_nothing_on_screen(self):
        status, layers = self.api.call("GET", "/v1/status/layers")
        self.assertEqual(status, 200)
        self.assertEqual(layers, {"video_input": True, "media": False, "slide": False, "announcements": False,
                                  "props": False, "messages": False, "audio": False})

    def test_a_layer_is_set_from_a_test_and_cleared_again(self):
        self.api.state.set_layers(slide=True, props=True)
        layers = self.api.call("GET", "/v1/status/layers")[1]
        self.assertTrue(layers["slide"] and layers["props"])
        self.assertTrue(layers["video_input"], "the other layers stay as they were")
        self.api.state.set_layers()
        layers = self.api.call("GET", "/v1/status/layers")[1]
        self.assertFalse(any(v for k, v in layers.items() if k != "video_input"))
        self.assertEqual(self.api.state.layer_reads, 2)
        with self.assertRaises(ValueError):
            self.api.state.set_layers(sparkles=True)

    def test_it_can_answer_something_that_is_not_the_layers(self):
        self.api.state.set_layers_unreadable()
        status, layers = self.api.call("GET", "/v1/status/layers")
        self.assertEqual(status, 200)
        self.assertNotIn("slide", layers)
        self.api.state.set_layers()
        self.assertIn("slide", self.api.call("GET", "/v1/status/layers")[1])


if __name__ == "__main__":
    unittest.main()
