#!/usr/bin/env python3
"""Look at the ProPresenter Link block and the "Add to ProPresenter" window in a browser, without ProPresenter, without Playback and
without a second computer. Standard library only; read docs/preview-live-link.md ("The ProPresenter Link") for the whole recipe.

It starts, on this computer:

* a made-up ProPresenter folder in a temporary directory (`Libraries/Songs` with tiny synthetic songs, never a real song) and
  `tools/fake_propresenter_api.py` serving it as ProPresenter's HTTP API, with two playlists (one with headers),
* `tools/fake_playback_api.py` and `tools/fake_playback_library.py` (the Playback side the songs come from),
* and, in front of a StagePilot demo backend that you start yourself, a small **preview server**: it hands the browser everything
  the backend says, except that a page of its own (`/__preview/`) can make the ProPresenter Link look like it does on ANOTHER
  computer (set up, update available, needs the command again, not answering...). Only the answers of
  `/api/v1/propresenter-link/status` and `/api/v1/propresenter-link/helper-install` change; adding songs always goes through the real
  routes, the real engine and the made-up folder.

Because this computer is the ProPresenter computer, the backend's own answer is the **Local** one (and, not being a Mac, it cannot
import a missing song: that is the line you see). Nothing here touches ProPresenter, Playback or any other computer.

    cargo run -p stagepilot-server -- --demo --port 8765 --web-root frontend/dist        # in another terminal
    python3 tools/preview_propresenter_link.py --bind 0.0.0.0 --port 18900

The preview server asks for a one-time access word (printed at start, kept in a cookie) because it sits in front of a backend that
trusts its own computer: open the address it prints. Stop it with Ctrl-C.
"""

from __future__ import annotations

import argparse
import datetime
import http.client
import http.server
import json
import os
import secrets
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.dont_write_bytecode = True

import fake_playback_library as playback_library  # noqa: E402
import fake_propresenter_api as pp_api  # noqa: E402

BUNDLED = "0.1.0"

# ---- tiny synthetic songs (the same protobuf layout ProPresenter 19 uses; made up, no lyrics) --------------------------------


def varint(value):
    out = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        if not value:
            out.append(byte)
            return bytes(out)
        out.append(byte | 0x80)


def rec(number, payload):
    return varint(number << 3 | 2) + varint(len(payload)) + payload


def vrec(number, value):
    return varint(number << 3) + varint(value)


def fake_uuid(seed, n):
    return "%08X-0000-4000-8000-%012X" % (seed, n)


def synthetic_song(title, multitracks_id, seed, groups, arrangements, selected):
    """`groups`: [(name, [slide text])], `arrangements`: [(name, [group index])]. An empty group name is Count Off."""
    out = rec(1, bytes([0x08, 0x01, 0x12, 0x03]) + b"190") + rec(2, rec(1, fake_uuid(seed, 1).encode())) + rec(3, title.encode())
    out += rec(4, vrec(1, 1_600_000_000)) + rec(5, vrec(1, 1_600_000_001)) + rec(6, b"a category")
    out += rec(10, rec(1, fake_uuid(seed, 200 + selected).encode()))
    for i, (name, order) in enumerate(arrangements):
        body = rec(1, rec(1, fake_uuid(seed, 200 + i).encode())) + rec(2, name.encode())
        for g in order:
            body += rec(3, rec(1, fake_uuid(seed, 100 + g).encode()))
        out += rec(11, body)
    cues, number = b"", 0
    for g, (name, slides) in enumerate(groups):
        group = rec(1, rec(1, fake_uuid(seed, 100 + g).encode())) + rec(2, name.encode()) + rec(3, bytes([0x0D, 0, 0, 0x80, 0x3F]))
        group += rec(5, rec(1, fake_uuid(seed, 900 + g).encode()))
        body = rec(1, group)
        for text in slides:
            number += 1
            cue_uuid = fake_uuid(seed, 1000 + number)
            body += rec(2, rec(1, cue_uuid.encode()))
            graphic = rec(13, rec(5, text.encode()))
            action = rec(23, rec(2, rec(1, rec(1, rec(1, graphic)))))
            cue = rec(1, rec(1, cue_uuid.encode())) + rec(2, ("cue %d" % number).encode()) + rec(10, action)
            cues += rec(13, cue)
        out += rec(12, body)
    out += cues
    record = vrec(1, multitracks_id) + rec(2, b"customer") + rec(3, vrec(1, 1_900_000_000))
    return out + rec(21, record)


SECTIONS = ["", "Intro", "Verse 1", "Chorus 1", "Verse 2", "Chorus 2", "Bridge", "Chorus 3", "Outro"]


def song_groups():
    return [(name, ["made-up slide for %s" % (name or "count off")]) for name in SECTIONS]


def make_folder(root):
    """The made-up ProPresenter folder: Holy Forever, Gratitude and two files for Firm Foundation (so StagePilot has to ask); Battle
    Belongs is not there at all (it would have to be loaded through ProPresenter's own search)."""
    songs = os.path.join(root, "Libraries", "Songs")
    os.makedirs(songs)
    os.makedirs(os.path.join(root, "Libraries", "Playback"))
    today = datetime.date.today()
    days = (today - playback_library.EPOCH_DATE).days

    def content_id(index):
        return 5_000_000 + 1_000_000 + days * 10 + index + 1

    def write(file, title, index, seed):
        groups = song_groups()
        data = synthetic_song(title, content_id(index), seed, groups, [("Full", list(range(len(groups))))], 0)
        with open(os.path.join(songs, file + ".pro"), "wb") as handle:
            handle.write(data)

    write("Holy Forever", "Holy Forever", 0, 0xA0000001)
    write("Gratitude", "Gratitude", 3, 0xA0000002)
    write("Firm Foundation", "Firm Foundation", 4, 0xA0000003)
    write("Firm Foundation (Live)", "Firm Foundation", 4, 0xA0000004)


# ---- ProPresenter's API, with two playlists ---------------------------------------------------------------------------------


def header(name, index):
    return {"id": {"uuid": str(uuid.uuid4()).upper(), "name": name, "index": index}, "type": "header", "is_hidden": False, "is_pco": False,
            "header_color": {"red": 0.2, "green": 0.4, "blue": 0.8, "alpha": 1.0}, "duration": 0.0}


TIMER = {"id": {"uuid": "11111111-0000-4000-8000-000000000001", "name": "Song Countdown", "index": 0}, "allows_overrun": False, "countdown": {"duration": 300.0}}
LOOK = {"id": {"uuid": "22222222-0000-4000-8000-000000000001", "name": "Default", "index": 0}}


def with_timer_and_looks(handler):
    """The timer and Look answers the Presentation panel asks for, so the ProPresenter line reads Connected (the fake only knows songs)."""
    original = handler.do_GET

    def do_GET(self):
        path = self.path.partition("?")[0].rstrip("/")
        answers = {"/v1/timers": [TIMER], "/v1/timers/current": [{**TIMER, "state": "stopped"}], "/v1/looks": [LOOK], "/v1/look/current": LOOK}
        if path in answers:
            return self._send(200, answers[path])
        return original(self)

    handler.do_GET = do_GET


def start_api(folder, port, lazy):
    with_timer_and_looks(pp_api.Handler)
    state = pp_api.State(folder=folder, lazy=lazy)
    sunday = state.add_playlist("Sunday Morning Service")
    state.put_items(sunday, [header("Welcome", 0), header("Worship", 1), header("Message", 2), header("Closing", 3)])
    state.add_playlist("Wednesday Night")
    state.add_playlist("Youth Night", folder="Special services")
    server, url = pp_api.serve(state, "127.0.0.1", port)
    return state, server, url


# ---- what the Link looks like on ANOTHER computer ---------------------------------------------------------------------------

SCENARIOS = [
    ("live", "As this computer really is (Local)", None),
    ("not_installed", "Another computer: Not installed", {"state": "not_installed", "paired": False}),
    ("ready", "Another computer: Ready, version 0.1.0", {"state": "ready", "version": BUNDLED}),
    ("no_import", "Another computer: Ready, cannot import (macOS Accessibility not allowed)",
     {"state": "ready", "version": BUNDLED, "can_import": False,
      "import_blocker": "Allow the ProPresenter Link in System Settings > Privacy & Security > Accessibility, then try again."}),
    ("outdated", "Another computer: Update available (StagePilot cannot update it)",
     {"state": "outdated", "version": "0.0.9", "update": {"mode": "manual", "version": BUNDLED, "message": "The ProPresenter Link could not update itself. Run the setup command on the ProPresenter computer to update it."}}),
    ("outdated_auto", "Another computer: Update available (StagePilot updates it)",
     {"state": "outdated", "version": "0.0.9", "update": {"mode": "automatic", "version": BUNDLED, "message": "StagePilot updates the ProPresenter Link by itself when nothing is playing."}}),
    ("needs_command", "Another computer: Needs the command again", {"state": "needs_command"}),
    ("not_answering", "Another computer: Not answering", {"state": "not_answering"}),
]


def helper_status(over):
    status = {
        "mode": "helper", "state": "not_installed", "message": "", "bundled_version": BUNDLED, "can_import": True,
        "libraries": ["Songs", "Playback"], "playback_library_exists": True, "computer_address": "192.0.2.10", "data_folder": "/Users/church/Documents/ProPresenter",
        "paired": True, "updater": True, "stale": False,
    }
    status.update(over)
    if status["state"] in ("not_installed", "not_answering", "needs_command"):
        status.update({"can_import": False, "libraries": [], "playback_library_exists": False, "data_folder": None})
    status["message"] = status.get("message") or "Preview of the ProPresenter Link on another computer."
    return status


class Preview:
    def __init__(self, args, api_state):
        self.args = args
        self.api_state = api_state
        self.scenario = "live"
        self.session_until = None
        self.token = args.token or secrets.token_urlsafe(9)
        self.backend = args.stagepilot

    def over(self):
        return dict((key, over) for key, _, over in SCENARIOS)[self.scenario]

    def install_reply(self):
        self.session_until = time.time() + 30 * 60
        stamp = datetime.datetime.fromtimestamp(self.session_until, datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
        lan = self.args.lan_ip
        url = "http://%s:%d/propresenter-helper/%s/install.sh" % (lan, self.args.port, secrets.token_hex(16))
        return {"command": "curl -fsS %s | sh" % url, "url": url, "expires_at": stamp, "lan_ip": lan, "port": self.args.port, "bundled_version": BUNDLED}


PAGE = """<!doctype html><meta charset=utf-8><meta name=viewport content="width=device-width,initial-scale=1"><title>ProPresenter Link preview</title>
<style>body{font:16px/1.5 system-ui;max-width:42rem;margin:1rem auto;padding:0 1rem}a.b,button{display:block;margin:.4rem 0;padding:.6rem .8rem;border:2px solid #444;border-radius:6px;text-decoration:none;color:inherit;background:#fff;font:inherit;text-align:left;width:100%%}
.on{background:#ffe9a8!important}code{background:#eee;padding:0 .2rem}</style>
<h1>ProPresenter Link preview</h1>
<p>Pick how the ProPresenter Link looks, then <a href="/">open StagePilot</a> (Configuration &gt; Presentation, and Playback &gt; songs). Nothing here touches a real ProPresenter.</p>
%(items)s
<h2>Other things to try</h2>
<form method=post action="/__preview/lazy"><button>Simulate: ProPresenter loads the new songs only after "Open the Playback library once" (press, then add songs)</button></form>
<form method=post action="/__preview/select"><button>I opened the Playback library in ProPresenter (let it load the new songs)</button></form>
<form method=post action="/__preview/reset"><button>Start over: forget the songs added to the playlists</button></form>
<p>Made-up folder: <code>%(folder)s</code></p>
"""


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    preview: Preview

    def log_message(self, format, *args):
        pass

    # -- access: a one-time word in the address, then a cookie
    def allowed(self):
        cookie = self.headers.get("Cookie", "")
        return ("pv=%s" % self.preview.token) in [c.strip() for c in cookie.split(";")]

    def send_text(self, code, body, ctype="text/html; charset=utf-8", extra=()):
        data = body.encode() if isinstance(body, str) else body
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(data)))
        for k, v in extra:
            self.send_header(k, v)
        self.end_headers()
        self.wfile.write(data)

    def control(self, method):
        path = urllib.parse.urlparse(self.path).path
        if method == "GET" and path == "/__preview/":
            items = "".join('<a class="b%s" href="/__preview/set?scenario=%s">%s</a>' % (" on" if key == self.preview.scenario else "", key, label) for key, label, _ in SCENARIOS)
            self.send_text(200, PAGE % {"items": items, "folder": self.preview.args.folder})
        elif method == "GET" and path == "/__preview/set":
            key = urllib.parse.parse_qs(urllib.parse.urlparse(self.path).query).get("scenario", ["live"])[0]
            if key in dict((k, 1) for k, _, _ in SCENARIOS):
                self.preview.scenario = key
            self.send_text(303, "", extra=[("Location", "/__preview/")])
        elif method == "POST" and path == "/__preview/select":
            open(os.path.join(self.preview.args.folder, ".selected-Playback"), "w").close()
            self.send_text(303, "", extra=[("Location", "/__preview/")])
        elif method == "POST" and path == "/__preview/lazy":
            self.preview.api_state.lazy = "select"
            try:
                os.remove(os.path.join(self.preview.args.folder, ".selected-Playback"))
            except OSError:
                pass
            self.send_text(303, "", extra=[("Location", "/__preview/")])
        elif method == "POST" and path == "/__preview/reset":
            for playlist in self.preview.api_state.playlists:
                playlist["items"] = [i for i in playlist["items"] if i.get("type") in ("header", "placeholder")]
            self.send_text(303, "", extra=[("Location", "/__preview/")])
        else:
            self.send_text(404, "not here")

    def handle_one(self, method):
        parsed = urllib.parse.urlparse(self.path)
        query = urllib.parse.parse_qs(parsed.query)
        if query.get("pv", [None])[0] == self.preview.token:
            clean = urllib.parse.urlunparse(parsed._replace(query=urllib.parse.urlencode([(k, v) for k, vs in query.items() if k != "pv" for v in vs])))
            self.send_text(303, "", extra=[("Location", clean or "/"), ("Set-Cookie", "pv=%s; Path=/; HttpOnly; SameSite=Lax" % self.preview.token)])
            return
        if not self.allowed():
            self.send_text(403, "This preview needs the address it printed when it started (it has ?pv=... in it).", "text/plain")
            return
        if parsed.path.startswith("/__preview/"):
            self.control(method)
            return
        scenario = self.preview.over()
        if scenario is not None and parsed.path == "/api/v1/propresenter-link/status" and method == "GET":
            self.send_text(200, json.dumps(helper_status(scenario)), "application/json")
            return
        if scenario is not None and parsed.path == "/api/v1/propresenter-link/helper-install":
            if method == "POST":
                self.read_body()
                self.send_text(200, json.dumps(self.preview.install_reply()), "application/json")
            else:
                self.send_text(200, json.dumps({"ended": True}), "application/json")
            return
        self.forward(method)

    def read_body(self):
        length = int(self.headers.get("Content-Length") or 0)
        return self.rfile.read(length) if length else b""

    def forward(self, method):
        body = self.read_body()
        host, _, port = self.preview.backend.partition(":")
        conn = http.client.HTTPConnection(host, int(port or 80), timeout=60)
        headers = {k: v for k, v in self.headers.items() if k.lower() not in ("host", "origin", "referer", "connection", "content-length", "accept-encoding", "cookie")}
        headers["Host"] = self.preview.backend
        headers["Origin"] = "http://" + self.preview.backend
        cookie = "; ".join(c.strip() for c in self.headers.get("Cookie", "").split(";") if not c.strip().startswith("pv="))
        if cookie:
            headers["Cookie"] = cookie
        if body:
            headers["Content-Length"] = str(len(body))
        try:
            conn.request(method, self.path, body=body or None, headers=headers)
            response = conn.getresponse()
            data = response.read()
        except OSError as error:
            self.send_text(502, "StagePilot did not answer: %s" % error, "text/plain")
            return
        self.send_response(response.status)
        for key, value in response.getheaders():
            if key.lower() in ("transfer-encoding", "connection", "content-length", "set-cookie"):
                continue
            self.send_header(key, value)
        for value in response.msg.get_all("Set-Cookie") or []:
            self.send_header("Set-Cookie", value)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)
        conn.close()

    def tunnel(self):
        """The window's live connection (a WebSocket): the bytes are carried both ways untouched."""
        host, _, port = self.preview.backend.partition(":")
        try:
            upstream = socket.create_connection((host, int(port or 80)), timeout=10)
        except OSError:
            self.send_text(502, "StagePilot did not answer", "text/plain")
            return
        lines = ["%s %s HTTP/1.1" % (self.command, self.path)]
        for key, value in self.headers.items():
            if key.lower() == "host":
                value = self.preview.backend
            elif key.lower() == "origin":
                value = "http://" + self.preview.backend
            elif key.lower() == "cookie":
                value = "; ".join(c.strip() for c in value.split(";") if not c.strip().startswith("pv="))
            lines.append("%s: %s" % (key, value))
        upstream.sendall(("\r\n".join(lines) + "\r\n\r\n").encode())
        upstream.settimeout(None)
        self.connection.settimeout(None)

        def pipe(a, b):
            try:
                while True:
                    chunk = a.recv(65536)
                    if not chunk:
                        break
                    b.sendall(chunk)
            except OSError:
                pass
            finally:
                for s in (a, b):
                    try:
                        s.shutdown(socket.SHUT_RDWR)
                    except OSError:
                        pass

        threading.Thread(target=pipe, args=(upstream, self.connection), daemon=True).start()
        pipe(self.connection, upstream)
        self.close_connection = True

    def do_GET(self):
        if self.headers.get("Upgrade", "").lower() == "websocket" and self.allowed():
            self.tunnel()
        else:
            self.handle_one("GET")

    def do_POST(self):
        self.handle_one("POST")

    def do_PUT(self):
        self.handle_one("PUT")

    def do_DELETE(self):
        self.handle_one("DELETE")


class Server(http.server.ThreadingHTTPServer):
    daemon_threads = True


def lan_address():
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
            probe.connect(("192.0.2.1", 9))
            return probe.getsockname()[0]
    except OSError:
        return "127.0.0.1"


def call(base, method, path, body=None):
    request = urllib.request.Request(base + path, method=method, data=json.dumps(body).encode() if body is not None else None, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=15) as response:
        return json.loads(response.read() or b"null")


def configure(args, api_url):
    """Tell the demo backend where the made-up ProPresenter, Playback API and Library Link are."""
    base = "http://" + args.stagepilot
    port = int(api_url.rsplit(":", 1)[1])
    settings = call(base, "GET", "/api/v1/settings")["settings"]
    settings["integration_modes"]["timer_output"] = "propresenter"
    settings["propresenter"].update({"enabled": True, "host": "127.0.0.1", "port": port})
    settings["propresenter"].setdefault("link", {})["data_folder"] = args.folder
    call(base, "PUT", "/api/v1/settings", settings)
    call(base, "POST", "/api/v1/propresenter/settings", {"host": "127.0.0.1", "port": port, "timer_name": "Song Countdown", "look_id": None, "request_timeout_seconds": 3})
    call(base, "PUT", "/api/v1/playback-api/settings", {"enabled": True, "host": "127.0.0.1", "port": args.playback_port, "auto_scan": False, "fast_transport": True})
    # The Library Link is on the Playback computer (here: this one) at its own port; a different port would be an address override the window clears.
    call(base, "PUT", "/api/v1/playback-library/settings", {"address": "" if args.library_port == 8780 else "127.0.0.1:%d" % args.library_port})


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--port", type=int, default=18900, help="the preview server")
    parser.add_argument("--bind", default="127.0.0.1", help="0.0.0.0 (or this computer's address) to open it to the network")
    parser.add_argument("--stagepilot", default="127.0.0.1:8765", help="the demo backend")
    parser.add_argument("--api-port", type=int, default=11025, help="the made-up ProPresenter API")
    parser.add_argument("--playback-port", type=int, default=18080)
    parser.add_argument("--library-port", type=int, default=8780, help="the Library Link stand-in: the real one is always on the Playback computer at 8780, and the window clears any other address")
    parser.add_argument("--folder", help="the made-up ProPresenter folder (default: a new temporary one)")
    parser.add_argument("--lazy", default="now", help="when a new file gets its id in the made-up ProPresenter: now, select, never or seconds")
    parser.add_argument("--token", help="the access word (default: a random one)")
    parser.add_argument("--lan-ip", default=None, help="the address shown in the made-up setup command")
    parser.add_argument("--no-configure", action="store_true", help="do not tell the backend where the made-up computers are")
    args = parser.parse_args(argv)
    args.lan_ip = args.lan_ip or lan_address()
    made = args.folder is None
    args.folder = args.folder or tempfile.mkdtemp(prefix="pp-preview-")
    if made:
        make_folder(args.folder)
    api_state, api_server, api_url = start_api(args.folder, args.api_port, args.lazy)
    children = []
    key_file = os.path.join(args.folder, ".library-key")
    for command in (
        [sys.executable, os.path.join(HERE, "fake_playback_api.py"), "--port", str(args.playback_port)],
        [sys.executable, os.path.join(HERE, "fake_playback_library.py"), "--port", str(args.library_port), "--key-file", key_file],
    ):
        children.append(subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
    preview = Preview(args, api_state)
    Handler.preview = preview
    server = Server((args.bind, args.port), Handler)
    if not args.no_configure:
        deadline = time.time() + 20
        while True:
            try:
                configure(args, api_url)
                break
            except urllib.error.HTTPError as error:
                print("The demo backend refused a setting: %s %s" % (error.code, error.read()[:300]), file=sys.stderr)
                return 1
            except OSError as error:
                if time.time() > deadline:
                    print("The demo backend at %s did not answer (%s). Start it first." % (args.stagepilot, error), file=sys.stderr)
                    return 1
                time.sleep(1)
    shown = args.lan_ip if args.bind in ("0.0.0.0", "") else args.bind
    print("ProPresenter Link preview", flush=True)
    print("  open:     http://%s:%d/?pv=%s" % (shown, args.port, preview.token))
    print("  states:   http://%s:%d/__preview/" % (shown, args.port))
    print("  folder:   %s" % args.folder)
    sys.stdout.flush()
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
        api_server.shutdown()
        for child in children:
            child.terminate()
        if made:
            shutil.rmtree(args.folder, ignore_errors=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
