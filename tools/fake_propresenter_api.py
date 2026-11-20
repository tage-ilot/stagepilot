#!/usr/bin/env python3
"""A stand-in for ProPresenter's own HTTP API (port 1025), for the parts the ProPresenter Link uses to put songs in a playlist:
libraries and playlists. Standard library only. It is NOT ProPresenter; it keeps the rules that were proven on a real
ProPresenter 19.0.1 (docs/propresenter.md, "ProPresenter Link"), so a client that works here does not trip over them there:

* `GET /v1/libraries`                 -> `[{"uuid","name","index"}]`                  (the libraries)
* `GET /v1/library/{id}`              -> `{"items":[{"id":{uuid,name,index}}]}`       (a song's `id.name` is `<Song> - [ Library ]`)
* `GET /v1/playlists`                 -> `[{"id":{uuid,name,index},"field_type":"playlist"}]`
* `GET /v1/playlist/{id}`             -> `{"id":{...},"items":[...]}`                 (each song item carries `presentation_info`)
* `POST /v1/playlists {name,type}`    -> a new, empty playlist (there is no way to delete one, as in ProPresenter)
* `PUT /v1/playlist/{id} {items}`     -> replaces the WHOLE list of items. A song item's `id.uuid` and
  `presentation_info.presentation_uuid` must both be the LIBRARY entry's uuid and `id.name` its display name WITHOUT the
  ` - [ Library ]` suffix; an unknown uuid or a wrong name makes the whole PUT answer 404 and changes nothing. Headers
  (`type: "header"`) and placeholders (`"placeholder"`) are items too and are not checked.
* `arrangement_name` that is sent is IGNORED: what comes back is the arrangement the song file has selected (a per-song table,
  `--arrangement "Song Name=Playback"`, otherwise `Default`), read after a PUT like ProPresenter does after about 10 s.
* An item's `duration` is returned as it was last sent (a client must send the values it read back).
* A wrong playlist id answers 404.

    python3 tools/fake_propresenter_api.py --port 11025 --song "Amazing Grace" --song "How Great" --playlist Sunday
    python3 tools/fake_propresenter_api.py --port 11025 --song "Amazing Grace" --arrangement "Amazing Grace=Playback"

Everything is kept in memory; nothing is written. It binds to 127.0.0.1 unless `--bind` says otherwise.
`State` can also be used in-process: `State(libraries={"Playback": ["Song"]}, playlists=["Sunday"], arrangements={"Song": "x"})`.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import threading
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

SUFFIX = " - [ Library ]"


def new_id(name, index=0):
    return {"uuid": str(uuid.uuid4()).upper(), "name": name, "index": index}


class State:
    def __init__(self, libraries=None, playlists=None, arrangements=None):
        self.lock = threading.Lock()
        self.libraries = []         # [{"id": {...}, "songs": [{"id": {...}}]}]
        self.playlists = []         # [{"id": {...}, "items": [...]}]
        self.arrangements = dict(arrangements or {})
        for name, songs in (libraries or {"Songs": []}).items():
            self.add_library(name, songs)
        for name in playlists or []:
            self.add_playlist(name)

    def add_library(self, name, songs=()):
        library = {"id": new_id(name, len(self.libraries)), "songs": []}
        for i, song in enumerate(songs):
            library["songs"].append({"id": new_id(song + SUFFIX, i)})
        self.libraries.append(library)
        return library

    def add_song(self, library_name, song):
        library = next(l for l in self.libraries if l["id"]["name"] == library_name)
        entry = {"id": new_id(song + SUFFIX, len(library["songs"]))}
        library["songs"].append(entry)
        return entry

    def add_playlist(self, name):
        playlist = {"id": new_id(name, len(self.playlists)), "items": []}
        self.playlists.append(playlist)
        return playlist

    def song_by_uuid(self, wanted):
        for library in self.libraries:
            for song in library["songs"]:
                if song["id"]["uuid"] == wanted:
                    return song
        return None

    def playlist(self, ident):
        for playlist in self.playlists:
            if ident in (playlist["id"]["uuid"], playlist["id"]["name"], str(playlist["id"]["index"])):
                return playlist
        return None

    def library(self, ident):
        for library in self.libraries:
            if ident in (library["id"]["uuid"], library["id"]["name"], str(library["id"]["index"])):
                return library
        return None

    def check_items(self, items):
        """None when every item is acceptable, else why the whole PUT is refused (404)."""
        if not isinstance(items, list):
            return "items must be a list"
        for item in items:
            if not isinstance(item, dict) or not isinstance(item.get("id"), dict):
                return "an item has no id"
            kind = item.get("type", "presentation")
            if kind in ("header", "placeholder"):
                continue
            info = item.get("presentation_info") or {}
            song = self.song_by_uuid(item["id"].get("uuid"))
            if song is None or info.get("presentation_uuid") != item["id"].get("uuid"):
                return "unknown presentation uuid"
            if item["id"].get("name") != song["id"]["name"][: -len(SUFFIX)]:
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
                song = self.song_by_uuid(item["id"]["uuid"])
                name = song["id"]["name"][: -len(SUFFIX)] if song else item["id"].get("name", "")
                info = item.setdefault("presentation_info", {})
                info["arrangement_name"] = self.arrangements.get(name, "Default")
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
            if path == "/v1/libraries":
                return self._send(200, [l["id"] for l in st.libraries])
            m = re.fullmatch(r"/v1/library/([^/]+)", path)
            if m:
                library = st.library(m.group(1))
                return self._send(200, {"items": [s for s in library["songs"]]}) if library else self._send(404)
            if path == "/v1/playlists":
                return self._send(200, [{"id": p["id"], "field_type": "playlist"} for p in st.playlists])
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
                return self._send(200, {"id": playlist["id"], "items": []})
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
            return self._send(200, {"id": playlist["id"], "items": st.read_items(playlist)})

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
    args = p.parse_args(argv)
    arrangements = dict(item.split("=", 1) for item in args.arrangement if "=" in item)
    state = State(libraries={"Songs": args.song}, playlists=args.playlist, arrangements=arrangements)
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
