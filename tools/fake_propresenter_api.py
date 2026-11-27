#!/usr/bin/env python3
"""A stand-in for ProPresenter's own HTTP API (port 1025), for the parts the ProPresenter Link uses to put songs in a playlist:
version, libraries and playlists. Standard library only. It is NOT ProPresenter; it keeps the rules that were proven on a real
ProPresenter 19.0.1 (docs/propresenter.md, "ProPresenter Link"), so a client that works here does not trip over them there:

* `GET /version`                     -> `{"host_description": "ProPresenter 19.0.1", "api_version": "v1", ...}`
* `GET /v1/libraries`                -> `[{"id": {"uuid","name","index"}}]`
* `GET /v1/library/{id}`             -> `{"updateType": "all", "items": [{"uuid","name","index"}]}`; a song's name is
  `<Song> - [ <Library> ]`; a file ProPresenter has not loaded yet is listed WITHOUT a uuid (see `--folder` below)
* `GET /v1/playlists`                -> `[{"id": {...}, "type": "playlist"}, {"id": {...}, "type": "group", "playlists": [...]}]`
* `GET /v1/playlist/{id}`            -> `{"id": {...}, "items": [...]}` (each song item carries `presentation_info`)
* `POST /v1/playlists {name,type}`   -> a new, empty playlist (there is no way to delete one, as in ProPresenter)
* `PUT /v1/playlist/{id} [items]`    -> replaces the WHOLE list of items, answers 204. A song item's `id.uuid` and
  `presentation_info.presentation_uuid` must both be the LIBRARY entry's uuid and `id.name` its display name WITHOUT the
  ` - [ Library ]` suffix; an unknown uuid or a wrong name makes the whole PUT answer 404 and changes nothing. Headers
  (`type: "header"`) and placeholders (`"placeholder"`) are items too and are not checked.
* `arrangement_name` that is sent is IGNORED: what comes back is the arrangement the song FILE has selected (read from the
  `.pro` itself in `--folder` mode; a per-song table, `--arrangement "Song Name=Playback"`, in memory mode).
* An item's `duration` is returned as it was last sent (a client must send the values it read back).
* `GET /v1/status/layers`            -> `{"video_input": true, "media": false, "slide": false, "announcements": false,
  "props": false, "messages": false, "audio": false}` (the answer on the real rig); what is on screen is set with
  `State.set_layers(slide=True, ...)`, and `State.set_layers_unreadable(True)` makes it answer something that is not that shape.
* A wrong playlist id answers 404.

Following Playback (the parts a follower needs; nothing here is written to a file):

* `GET /v1/playlist/{id}/{index}/trigger` -> 204; the item's presentation becomes the active one, on its first slide.
* `GET /v1/presentation/active/{n}/trigger` -> 204; slide `n` of the active presentation, counted in the arrangement the song
  file has SELECTED (a repeated group counts its slides again, so `n` is a FLAT position, not a raw cue index). 404 when nothing is
  active or `n` is not one of its slides (the layout is told with `State.set_layout`; a song with none accepts any `n`).
* `GET /v1/presentation/active` -> `{"presentation": {"id": {...}, "groups": []}}` (404 when nothing is active);
  `GET /v1/presentation/slide_index` -> `{"presentation_index": {"index": 13, "presentation_id": {...}}}` (`null` when nothing is
  active); `GET /v1/playlist/active` -> `{"presentation": {"playlist": {...}, "item": {...}}, "announcements": null}`.
* A test plays the operator with `State.click_slide(n)` and `State.open_presentation(name)`, and reads `State.triggers`
  (`("item", playlist_uuid, index)` and `("slide", n)`, in order). `State.fail_triggers = 500` makes every trigger answer that status;
  `State.ignore_items = True` accepts a playlist-item trigger and changes nothing on screen.

    python3 tools/fake_propresenter_api.py --port 11025 --song "Amazing Grace" --song "How Great" --playlist Sunday
    python3 tools/fake_propresenter_api.py --port 11025 --folder /tmp/ProPresenter --lazy select --playlist Sunday

`--folder DIR` makes the libraries the folders under `DIR/Libraries` and their songs the `.pro` files in them (the folder the
fake ProPresenter Link works on, so the two together are one fake ProPresenter computer). Files that were there when it started
have their uuid at once; a file placed later gets it as `--lazy` says: `now`, `never`, `select` (once the library was selected:
the fake link leaves a `.selected-<Library>` file in DIR) or a number of seconds after the file was written. That is the lazy
loading seen on a real ProPresenter. Everything else is kept in memory; nothing is written. It binds to 127.0.0.1 unless
`--bind` says otherwise. `State` can also be used in-process.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

SUFFIX = " - [ Library ]"


def new_id(name, index=0):
    return {"uuid": str(uuid.uuid4()).upper(), "name": name, "index": index}


def stable_uuid(*parts):
    return str(uuid.uuid5(uuid.NAMESPACE_URL, "/".join(parts))).upper()


# ---- the one thing read from a song file: its selected arrangement -----------------------------------------------------


def _varint(data, at):
    value = shift = 0
    while True:
        byte = data[at]
        at += 1
        value |= (byte & 0x7F) << shift
        if not byte & 0x80:
            return value, at
        shift += 7


def _fields(data):
    at = 0
    while at < len(data):
        tag, at = _varint(data, at)
        number, wire = tag >> 3, tag & 7
        if wire == 0:
            value, at = _varint(data, at)
        elif wire == 2:
            length, at = _varint(data, at)
            value, at = data[at:at + length], at + length
        elif wire == 1:
            value, at = data[at:at + 8], at + 8
        elif wire == 5:
            value, at = data[at:at + 4], at + 4
        else:
            raise ValueError("not protobuf")
        yield number, wire, value


def selected_arrangement(data):
    """The name of the arrangement a `.pro` has selected ('' when it has none or is not a song)."""
    try:
        selected, arrangements = None, {}
        for number, wire, value in _fields(data):
            if number == 10 and wire == 2:
                selected = next((v.decode() for n, w, v in _fields(value) if n == 1 and w == 2), None)
            elif number == 11 and wire == 2:
                uid = name = None
                for n, w, v in _fields(value):
                    if n == 1 and w == 2:
                        uid = next((x.decode() for m, y, x in _fields(v) if m == 1 and y == 2), None)
                    elif n == 2 and w == 2:
                        name = v.decode()
                if uid:
                    arrangements[uid] = name or ""
        return arrangements.get(selected, "") if selected else ""
    except (ValueError, IndexError, UnicodeDecodeError):
        return ""


# What the real rig answers when nothing is shown: a camera input is often on, which is no reason to stay away from the screen.
LAYERS_QUIET = {"video_input": True, "media": False, "slide": False, "announcements": False, "props": False, "messages": False, "audio": False}


class State:
    def __init__(self, libraries=None, playlists=None, arrangements=None, folder=None, lazy="now"):
        self.lock = threading.Lock()
        self.folder = folder
        self.lazy = lazy
        self.arrangements = dict(arrangements or {})
        self.memory = []          # memory mode: [{"id": {...}, "songs": [{"id": {...}}]}]
        self.playlists = []       # [{"id": {...}, "items": [...], "folder": "A" | None}]
        self.known = set()        # folder mode: (library, file) there at the start
        self.layers = dict(LAYERS_QUIET)
        self.layers_unreadable = False
        self.layer_reads = 0
        self.layouts = {}         # following: song display name -> {"groups": [cues per group, file order], "arrangement": [group positions]}
        self.active = None        # following: {"entry", "playlist", "item", "slide"} of the presentation layer
        self.triggers = []
        self.fail_triggers = None
        self.ignore_items = False
        if folder:
            for library, files in self._scan():
                self.known.update((library, f) for f in files)
        else:
            for name, songs in (libraries or {"Songs": []}).items():
                self.add_library(name, songs)
        for name in playlists or []:
            self.add_playlist(name)

    # -- what is on screen
    def set_layers(self, **flags):
        """Set layers (`slide=True`, `video_input=False`, ...); with no flag, back to nothing on screen (a camera input only)."""
        unknown = set(flags) - set(LAYERS_QUIET)
        if unknown:
            raise ValueError("unknown layer: %s" % ", ".join(sorted(unknown)))
        with self.lock:
            if not flags:
                self.layers = dict(LAYERS_QUIET)
                self.layers_unreadable = False
            self.layers.update({k: bool(v) for k, v in flags.items()})

    def set_layers_unreadable(self, on=True):
        with self.lock:
            self.layers_unreadable = bool(on)

    # -- following Playback
    def set_layout(self, display, groups, arrangement):
        """The slides of a song: `groups` is the cue count of each file group, `arrangement` the positions of the groups the
        selected arrangement plays, in order."""
        self.layouts[display] = {"groups": list(groups), "arrangement": list(arrangement)}

    def flat_raws(self, display):
        """The raw cue index (file order) of each flat slide of the selected arrangement; None when no layout was told."""
        layout = self.layouts.get(display)
        if layout is None:
            return None
        starts, total = [], 0
        for count in layout["groups"]:
            starts.append(total)
            total += count
        return [starts[g] + i for g in layout["arrangement"] for i in range(layout["groups"][g])]

    def raw_on_screen(self):
        """The raw cue index of the slide on screen (what a flat number really showed); None when not known."""
        if not self.active:
            return None
        raws = self.flat_raws(self.active["entry"]["display"])
        slide = self.active["slide"]
        return raws[slide] if raws is not None and slide < len(raws) else None

    def click_slide(self, n):
        """A person clicked slide `n` of the active presentation."""
        with self.lock:
            if self.active:
                self.active["slide"] = n

    def open_presentation(self, display):
        """A person opened another song in ProPresenter (its display name): it is the active one, on its first slide."""
        with self.lock:
            for library in self.libraries():
                for entry in library["entries"]:
                    if entry["uuid"] and entry["display"] == display:
                        self.active = {"entry": entry, "playlist": None, "item": None, "slide": 0}
                        return True
        return False

    def focus_playlist(self, ident, index):
        """The playlist and item ProPresenter last triggered, as if a person had clicked it (the screen is left alone)."""
        with self.lock:
            return self._show_item(ident, index, screen=False)

    def _show_item(self, ident, index, screen=True):
        playlist = self.playlist(ident)
        if playlist is None or not 0 <= index < len(playlist["items"]):
            return False
        item = playlist["items"][index]
        entry = self.song_by_uuid((item.get("presentation_info") or {}).get("presentation_uuid") or item["id"].get("uuid"))
        if entry is None:
            return False
        if self.ignore_items and screen:
            return True
        self.active = {"entry": entry, "playlist": playlist, "item": index, "slide": 0}
        return True

    def _show_slide(self, n):
        if not self.active:
            return False
        raws = self.flat_raws(self.active["entry"]["display"])
        if raws is not None and not 0 <= n < len(raws):
            return False
        self.active["slide"] = n
        return True

    # -- libraries
    def _scan(self):
        root = os.path.join(self.folder, "Libraries")
        try:
            names = sorted(n for n in os.listdir(root) if os.path.isdir(os.path.join(root, n)) and not n.startswith("."))
        except OSError:
            names = []
        for name in names:
            try:
                files = sorted(f for f in os.listdir(os.path.join(root, name)) if f.endswith(".pro") and not f.startswith("."))
            except OSError:
                files = []
            yield name, files

    def add_library(self, name, songs=()):
        library = {"id": new_id(name, len(self.memory)), "songs": []}
        for i, song in enumerate(songs):
            library["songs"].append({"id": new_id(song + SUFFIX, i)})
        self.memory.append(library)
        return library

    def add_song(self, library_name, song):
        library = next(l for l in self.memory if l["id"]["name"] == library_name)
        entry = {"id": new_id(song + SUFFIX, len(library["songs"]))}
        library["songs"].append(entry)
        return entry

    def _loaded(self, library, file):
        if (library, file) in self.known or self.lazy == "now":
            return True
        if self.lazy == "never":
            return False
        if self.lazy == "select":
            return os.path.exists(os.path.join(self.folder, ".selected-" + library))
        try:
            written = os.path.getmtime(os.path.join(self.folder, "Libraries", library, file))
        except OSError:
            return False
        return time.time() - written >= float(self.lazy)

    def libraries(self):
        """[{"id": {...}, "entries": [{"index", "name" (with suffix), "display", "uuid" | None, "library", "file"}]}]"""
        out = []
        if self.folder:
            for i, (name, files) in enumerate(self._scan()):
                entries = []
                for j, file in enumerate(files):
                    display = file[:-4]
                    loaded = self._loaded(name, file)
                    entries.append({"index": j, "name": "%s - [ %s ]" % (display, name), "display": display, "library": name, "file": file,
                                    "uuid": stable_uuid(name, file) if loaded else None})
                out.append({"id": {"uuid": stable_uuid("library", name), "name": name, "index": i}, "entries": entries})
            return out
        for library in self.memory:
            entries = [{"index": s["id"]["index"], "name": s["id"]["name"], "display": s["id"]["name"][: -len(SUFFIX)],
                        "uuid": s["id"]["uuid"], "library": library["id"]["name"], "file": None} for s in library["songs"]]
            out.append({"id": library["id"], "entries": entries})
        return out

    def song_by_uuid(self, wanted):
        for library in self.libraries():
            for entry in library["entries"]:
                if entry["uuid"] == wanted:
                    return entry
        return None

    def library(self, ident):
        for library in self.libraries():
            if ident in (library["id"]["uuid"], library["id"]["name"], str(library["id"]["index"])):
                return library
        return None

    def arrangement_of(self, entry):
        if self.folder and entry["file"]:
            try:
                with open(os.path.join(self.folder, "Libraries", entry["library"], entry["file"]), "rb") as f:
                    return selected_arrangement(f.read())
            except OSError:
                return ""
        return self.arrangements.get(entry["display"], "Default")

    # -- playlists
    def add_playlist(self, name, folder=None):
        playlist = {"id": new_id(name, len(self.playlists)), "items": [], "folder": folder}
        self.playlists.append(playlist)
        return playlist

    def playlist(self, ident):
        for playlist in self.playlists:
            if ident in (playlist["id"]["uuid"], playlist["id"]["name"], str(playlist["id"]["index"])):
                return playlist
        return None

    def playlist_tree(self):
        top, groups = [], {}
        for p in self.playlists:
            leaf = {"id": p["id"], "type": "playlist"}
            if not p["folder"]:
                top.append(leaf)
                continue
            if p["folder"] not in groups:
                groups[p["folder"]] = {"id": {"uuid": stable_uuid("group", p["folder"]), "name": p["folder"], "index": len(top)},
                                       "type": "group", "playlists": []}
                top.append(groups[p["folder"]])
            groups[p["folder"]]["playlists"].append(leaf)
        return top

    def check_items(self, items):
        """None when every item is acceptable, else why the whole PUT is refused (404)."""
        if not isinstance(items, list):
            return "items must be a list"
        for item in items:
            if not isinstance(item, dict) or not isinstance(item.get("id"), dict):
                return "an item has no id"
            if item.get("type", "presentation") in ("header", "placeholder"):
                continue
            info = item.get("presentation_info") or {}
            entry = self.song_by_uuid(item["id"].get("uuid"))
            if entry is None or info.get("presentation_uuid") != item["id"].get("uuid"):
                return "unknown presentation uuid"
            if item["id"].get("name") != entry["display"]:
                return "wrong presentation name"
        return None

    def put_items(self, playlist, items):
        stored = []
        for index, item in enumerate(items):
            item = json.loads(json.dumps(item))
            item.setdefault("id", {})["index"] = index
            stored.append(item)
        playlist["items"] = stored

    def read_items(self, playlist):
        """The items as ProPresenter shows them: the arrangement is the song's own selected one."""
        out = []
        for item in playlist["items"]:
            item = json.loads(json.dumps(item))
            if item.get("type", "presentation") not in ("header", "placeholder"):
                entry = self.song_by_uuid(item["id"]["uuid"])
                if entry:
                    item.setdefault("presentation_info", {})["arrangement_name"] = self.arrangement_of(entry)
            out.append(item)
        return out


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def _send(self, code, value=None):
        data = b"" if value is None else json.dumps(value).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def _body(self):
        length = int(self.headers.get("Content-Length") or 0)
        try:
            return json.loads(self.rfile.read(length) or b"null")
        except ValueError:
            return None

    def do_GET(self):
        st = self.server.state
        path = self.path.partition("?")[0].rstrip("/")
        with st.lock:
            if path == "/version":
                return self._send(200, {"name": "fake ProPresenter", "platform": "mac", "os_version": "14.0",
                                        "host_description": "ProPresenter 19.0.1", "api_version": "v1"})
            if path == "/v1/status/layers":
                st.layer_reads += 1
                return self._send(200, {"unexpected": "not the layers"} if st.layers_unreadable else dict(st.layers))
            if path == "/v1/libraries":
                return self._send(200, [{"id": l["id"]} for l in st.libraries()])
            m = re.fullmatch(r"/v1/library/([^/]+)", path)
            if m:
                library = st.library(m.group(1))
                if not library:
                    return self._send(404)
                items = []
                for e in library["entries"]:
                    item = {"index": e["index"], "name": e["name"]}
                    if e["uuid"]:
                        item["uuid"] = e["uuid"]
                    items.append(item)
                return self._send(200, {"updateType": "all", "items": items})
            if path == "/v1/playlists":
                return self._send(200, st.playlist_tree())
            if path == "/v1/playlist/active":
                active = st.active
                if not active or not active["playlist"]:
                    return self._send(200, {"presentation": None, "announcements": None})
                item = active["playlist"]["items"][active["item"]]
                return self._send(200, {"presentation": {"playlist": active["playlist"]["id"], "item": item["id"]}, "announcements": None})
            if path == "/v1/presentation/active":
                if not st.active:
                    return self._send(404)
                entry = st.active["entry"]
                return self._send(200, {"presentation": {"id": {"uuid": entry["uuid"], "name": entry["display"], "index": 0}, "groups": []}})
            if path == "/v1/presentation/slide_index":
                if not st.active:
                    return self._send(200, {"presentation_index": None})
                entry = st.active["entry"]
                return self._send(200, {"presentation_index": {"index": st.active["slide"],
                                                               "presentation_id": {"uuid": entry["uuid"], "name": entry["display"], "index": 0}}})
            m = re.fullmatch(r"/v1/playlist/([^/]+)/(\d+)/trigger", path)
            if m:
                st.triggers.append(("item", m.group(1), int(m.group(2))))
                if st.fail_triggers:
                    return self._send(st.fail_triggers)
                return self._send(204) if st._show_item(m.group(1), int(m.group(2))) else self._send(404)
            m = re.fullmatch(r"/v1/presentation/active/(\d+)/trigger", path)
            if m:
                st.triggers.append(("slide", int(m.group(1))))
                if st.fail_triggers:
                    return self._send(st.fail_triggers)
                return self._send(204) if st._show_slide(int(m.group(1))) else self._send(404)
            m = re.fullmatch(r"/v1/playlist/([^/]+)", path)
            if m:
                playlist = st.playlist(m.group(1))
                return self._send(200, {"id": playlist["id"], "items": st.read_items(playlist)}) if playlist else self._send(404)
        self._send(404)

    def do_POST(self):
        st = self.server.state
        path = self.path.partition("?")[0].rstrip("/")
        body = self._body()
        with st.lock:
            if path == "/v1/playlists" and isinstance(body, dict) and isinstance(body.get("name"), str) and body.get("type") == "playlist":
                playlist = st.add_playlist(body["name"])
                return self._send(200, {"id": playlist["id"], "type": "playlist"})
        self._send(404)

    def do_PUT(self):
        st = self.server.state
        path = self.path.partition("?")[0].rstrip("/")
        body = self._body()
        m = re.fullmatch(r"/v1/playlist/([^/]+)", path)
        with st.lock:
            playlist = st.playlist(m.group(1)) if m else None
            if playlist is None:
                return self._send(404)
            items = body.get("items") if isinstance(body, dict) else body
            if st.check_items(items) is not None:
                return self._send(404)
            st.put_items(playlist, items)
            return self._send(204)

    def do_DELETE(self):
        self._send(405)


class Server(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True


def serve(state, bind="127.0.0.1", port=0):
    """Start a server on a thread; returns (server, base_url)."""
    server = Server((bind, port), Handler)
    server.state = state
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, "http://%s:%d" % (bind, server.server_address[1])


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("--port", type=int, default=1025)
    p.add_argument("--bind", default="127.0.0.1")
    p.add_argument("--song", action="append", default=[], help="a song in the library Songs (repeat)")
    p.add_argument("--playlist", action="append", default=[], help="a playlist that exists from the start (repeat)")
    p.add_argument("--arrangement", action="append", default=[], help="Song=Arrangement: the arrangement the song file has selected")
    p.add_argument("--folder", default=None, help="the ProPresenter folder whose Libraries/* are the libraries")
    p.add_argument("--lazy", default="now", help="folder mode: when a file placed later gets its uuid: now, never, select or a number of seconds")
    args = p.parse_args(argv)
    arrangements = dict(item.split("=", 1) for item in args.arrangement if "=" in item)
    state = State(libraries={"Songs": args.song}, playlists=args.playlist, arrangements=arrangements, folder=args.folder, lazy=args.lazy)
    server, url = serve(state, args.bind, args.port)
    print("fake ProPresenter API on %s" % url, flush=True)
    try:
        threading.Event().wait()
    except KeyboardInterrupt:
        pass
    server.shutdown()
    return 0


if __name__ == "__main__":
    sys.exit(main())
