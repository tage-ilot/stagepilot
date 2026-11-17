#!/usr/bin/env python3
"""A stand-in for the Playback helper (the "network script"), for looking at StagePilot's Playback Live Link in a
browser. Standard library only. It is NOT the real helper: it has no Playback database and no Playback.

It speaks the helper's HTTP protocol (version 0.3.0) well enough for StagePilot, and it checks the key the same
way the real helper does (`playback_library.py`, "key authentication"):

* with a key (`--key-file`, first line, at least 32 characters): every request except the bare `GET /health` must
  carry `X-PBL-Time: <unix seconds>` and `X-PBL-Sig: <hex HMAC-SHA256(key, "GET\\n" + path-with-query + "\\n" + time)>`;
  the time may be 120 s off; the signature is compared in constant time. The bare `/health` answers only
  `{"ok":true,"name":"playback-library","version":"0.3.0","auth":"required"}`. Anything else without a good signature is
  `401 {"error":"key required"}`; ten bad ones from one address in 60 s make that address get `429` for 60 s.
* without a key: it answers everyone and `/health` says `"auth":"none"` (`--old` leaves `auth` out, like helper 0.2.0).

What it serves (made up, from the demo plan's own song titles; the covers are small generated JPEGs):

* the setlist for `--date` (default: today, which is the date of the demo plan StagePilot loads first): SIX songs,
  in an order that differs from the demo plan, with the first four demo songs and two others, so the pairing shows
  matches, a different order and songs Playback has that the plan has not;
* the setlist for any other date: ONE song ("Battle Belongs"), so the other Sundays of the demo show songs that are
  "not in Playback's setlist";
* `/songs/<id>` (sections), `/songs/<id>/artwork` (JPEG with an ETag, `If-None-Match` answers 304),
  `/songs/<id>/waveform?points_per_second=N` (a made-up envelope), `/health`, `/setlists/latest`, `/setlists`.

    python3 tools/fake_playback_library.py --port 8780 --key-file /tmp/pbl-key.txt
    python3 tools/fake_playback_library.py --port 8780 --key-file /tmp/pbl-key.txt --version 0.2.9   # "update available"
    python3 tools/fake_playback_library.py --port 8780 --old                                        # helper 0.2.0, no key

The key file is the one StagePilot's install command would give the installer (`install.sh --key-file`). For a
preview, put any 43-character line in a file and tell StagePilot the same key (docs/preview-live-link.md).
It binds to 127.0.0.1 unless `--bind` says otherwise and never writes anything.
"""

from __future__ import annotations

import argparse
import base64
import datetime
import hashlib
import hmac
import json
import math
import re
import sys
import threading
import time
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

EPOCH_DATE = datetime.date(2000, 1, 1)
MIN_KEY_LENGTH = 32
TIME_WINDOW_S = 120
FAIL_LIMIT = 10
FAIL_WINDOW_S = 60
BLOCK_S = 60

# Six small generated JPEGs (96 x 96).
COVERS_B64 = [
    "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",
]

# (title, artist, key, original key, bpm, length_s, scan_length_s)
SIX = [
    ("Holy Forever", "Chris Tomlin", "G", "G", 68.0, 336.4, 331.9),
    ("Battle Belongs", "Phil Wickham", "Ab", "Bb", 65.0, 281.3, 276.9),
    ("Way Maker", "Sinach", "E", "E", 68.0, 392.0, 387.8),
    ("Gratitude", "Brandon Lake", "A", "B", 72.0, 352.2, 347.5),
    ("Firm Foundation", "Cody Carnes", "C", "C", 76.0, 377.0, 372.6),
    ("Goodness of God", "Bethel Music", "Ab", "Ab", 63.0, 301.1, 296.3),
]
ONE = [SIX[1]]
SECTIONS = ["Count Off", "Intro", "Verse 1", "Chorus 1", "Verse 2", "Chorus 2", "Bridge", "Chorus 3", "Outro"]


def log(message):
    print(time.strftime("%H:%M:%S"), message, flush=True)


def sign(key, target, stamp):
    return hmac.new(key.encode("utf-8"), ("GET\n%s\n%s" % (target, stamp)).encode("utf-8"), hashlib.sha256).hexdigest()


def signature_ok(key, target, stamp, sig, now):
    if not isinstance(stamp, str) or not isinstance(sig, str) or not re.fullmatch(r"[0-9]{1,12}", stamp):
        return False
    if abs(now - int(stamp)) > TIME_WINDOW_S:
        return False
    return hmac.compare_digest(sign(key, target, stamp).encode(), sig.strip().lower().encode("utf-8", "replace"))


class Throttle:
    """Ten bad signatures from one address in a minute: that address gets 429 for a minute."""

    def __init__(self):
        self.lock = threading.Lock()
        self.fails = {}
        self.blocked = {}

    def blocked_for(self, addr, now):
        with self.lock:
            until = self.blocked.get(addr, 0)
            return max(0, until - now)

    def failed(self, addr, now):
        with self.lock:
            recent = [t for t in self.fails.get(addr, []) if now - t < FAIL_WINDOW_S] + [now]
            self.fails[addr] = recent
            if len(recent) >= FAIL_LIMIT:
                self.blocked[addr] = now + BLOCK_S
                self.fails[addr] = []


class State:
    def __init__(self, args):
        self.args = args
        self.key = None
        self.key_file = args.key_file
        self.throttle = Throttle()
        self.covers = [base64.b64decode(c) for c in COVERS_B64]
        self.reload_key()

    def reload_key(self):
        """The key is read from the file every time it changes, so a preview can pair after the fake started."""
        key = None
        if self.key_file:
            try:
                with open(self.key_file) as handle:
                    key = handle.readline().rstrip("\r\n")
            except OSError:
                key = None
        self.key = key if key else None

    def mode(self):
        self.reload_key()
        if self.key is None:
            return "none"
        return "key" if len(self.key) >= MIN_KEY_LENGTH else "locked"

    def songs_for(self, date):
        six_date = self.args.date or datetime.date.today().isoformat()
        return (SIX if date == six_date else ONE), six_date

    def song_id(self, date, index):
        # A whole number like the real ones, and one that says which date and which song it is.
        days = (datetime.date.fromisoformat(date) - EPOCH_DATE).days
        return 1_000_000 + days * 10 + index + 1

    def setlist(self, date):
        rows, six_date = self.songs_for(date)
        items = []
        for i, (title, artist, key, original, bpm, length, scan) in enumerate(rows):
            sid = self.song_id(date, i)
            cover = self.covers[(i + (0 if rows is SIX else 1)) % len(self.covers)]
            items.append({
                "setlist_song_id": sid, "order": i + 1, "title": title, "artist": artist,
                "key": {"id": 3, "name": key}, "key_original": {"id": 1, "name": original},
                "bpm": bpm, "bpm_original": bpm, "length_s": length, "scan_length_s": scan,
                "arrangement": "Default", "planning_center_song_id": None, "rental_expires_utc": None,
                "artwork": {"path": "/songs/%d/artwork" % sid, "type": "image/jpeg", "bytes": len(cover),
                            "etag": '"%s"' % hashlib.sha1(cover).hexdigest()[:16]},
            })
        return {"setlist_id": int(date.replace("-", "")), "title": "Sunday %s" % date,
                "performance_date": date, "items": items}

    def find_song(self, sid):
        days, index = divmod(sid - 1_000_001, 10)
        try:
            date = (EPOCH_DATE + datetime.timedelta(days=days)).isoformat()
        except (OverflowError, ValueError):
            return None
        rows = self.songs_for(date)[0]
        return (date, index, rows[index]) if sid >= 1_000_001 and index < len(rows) else None

    def dates(self):
        """The dates `/setlists` lists: the six-song date and the Sundays around today."""
        today = datetime.date.today()
        out = {self.args.date or today.isoformat()}
        for day in range(-14, 70):
            d = today + datetime.timedelta(days=day)
            if d.weekday() == 6:
                out.add(d.isoformat())
        return sorted(out)

    def health(self, authenticated):
        body = {"ok": True, "version": self.args.version, "database": "ok", "detail": "",
                "database_updated_utc": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
                "library_revision": "fake" + hashlib.sha1(json.dumps(self.args.date or "today").encode()).hexdigest()[:8],
                "setlists": len(self.dates()), "songs": 40, "playback_running": True, "songs_folder": True}
        if not self.args.old:
            body["auth"] = {"none": "none", "key": "key", "locked": "key_too_short"}[self.mode()]
        return body


def waveform(points_per_second, length_s, seed):
    step = 1.0 / points_per_second
    n = min(20_000, max(1, int(length_s * points_per_second)))
    peaks = []
    for i in range(n):
        t = i * step
        envelope = 0.35 + 0.35 * math.sin(t / 23.0 + seed) ** 2 + 0.2 * math.sin(t / 3.1) ** 2
        peaks.append(round(min(1.0, max(0.02, envelope)), 3))
    return {"step_s": step, "peak": peaks}


def sections_for(length_s):
    n = len(SECTIONS)
    weights = [0.04, 0.07, 0.14, 0.13, 0.14, 0.13, 0.12, 0.15, 0.08]
    start = 0.0
    out = []
    for i, (name, share) in enumerate(zip(SECTIONS, weights)):
        end = length_s if i == n - 1 else start + length_s * share
        out.append({"order": i, "name": name, "performed_start": round(start, 3), "performed_end": round(end, 3)})
        start = end
    return out


class Server(ThreadingHTTPServer):
    state: State


class Handler(BaseHTTPRequestHandler):
    server: Server

    server_version = "fake-playback-library"
    protocol_version = "HTTP/1.1"

    def log_message(self, format, *args):  # noqa: A002  # one line per request, never the headers
        log("%s %s -> %s" % (self.command, self.path.split("?")[0], args[1] if len(args) > 1 else ""))

    def _send(self, status, body, content_type="application/json", extra=None):
        data = body if isinstance(body, bytes) else json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-store")
        for name, value in (extra or {}).items():
            self.send_header(name, value)
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(data)

    def _target(self):
        words = getattr(self, "requestline", "").split()
        return words[1] if len(words) == 3 else self.path

    def _authorized(self, path):
        """True: carry on. False: the answer (200 minimal, 401 or 429) has been sent."""
        state = self.server.state
        now = time.time()
        addr = self.client_address[0]
        left = state.throttle.blocked_for(addr, now)
        if left:
            self._send(429, {"error": "too many bad requests"}, extra={"Retry-After": str(int(left) + 1)})
            return False
        mode = state.mode()
        if mode == "none":
            return True
        stamp, sig = self.headers.get("X-PBL-Time"), self.headers.get("X-PBL-Sig")
        if mode == "key" and stamp is not None and sig is not None and signature_ok(state.key, self._target(), stamp, sig, now):
            return True
        if path.rstrip("/") == "/health" and (mode == "locked" or (stamp is None and sig is None)):
            self._send(200, {"ok": True, "name": "playback-library", "version": state.args.version, "auth": "required"})
            return False
        state.throttle.failed(addr, now)
        self._send(401, {"error": "key required"})
        return False

    def do_GET(self):
        state = self.server.state
        path, _, query = self.path.partition("?")
        if not self._authorized(path):
            return
        q = {k: urllib.parse.unquote_plus(v) for k, v in (p.split("=", 1) for p in query.split("&") if "=" in p)}
        path = path.rstrip("/") or "/"
        if path == "/health":
            return self._send(200, state.health(True))
        if path == "/setlists":
            return self._send(200, {"setlists": [{"setlist_id": int(d.replace("-", "")), "title": "Sunday %s" % d,
                                                  "performance_date": d, "songs": len(state.songs_for(d)[0])}
                                                 for d in reversed(state.dates())]})
        m = re.fullmatch(r"/setlists/([^/]+)", path)
        if m:
            which = urllib.parse.unquote(m.group(1))
            if which == "latest":
                today = datetime.date.today().isoformat()
                past = [d for d in state.dates() if d <= today] or state.dates()
                return self._send(200, state.setlist(past[-1]))
            if which.isdigit() and len(which) == 8:
                # Playback's own number for a setlist (what `/setlists` lists as `setlist_id`): here the date without dashes.
                which = "%s-%s-%s" % (which[:4], which[4:6], which[6:])
            elif which in state.args.no_setlist_on:
                return self._send(404, {"error": "no setlist for that date"})
            try:
                datetime.date.fromisoformat(which)
            except ValueError:
                return self._send(404, {"error": "not found"})
            return self._send(200, state.setlist(which))
        m = re.fullmatch(r"/songs/(\d+)(?:/(waveform|artwork|beats))?", path)
        if m:
            found = state.find_song(int(m.group(1)))
            if not found:
                return self._send(404, {"error": "not found"})
            date, index, row = found
            what = m.group(2)
            if what == "artwork":
                rows = state.songs_for(date)[0]
                cover = state.covers[(index + (0 if rows is SIX else 1)) % len(state.covers)]
                etag = '"%s"' % hashlib.sha1(cover).hexdigest()[:16]
                if self.headers.get("If-None-Match") == etag:
                    return self._send(304, b"", "image/jpeg", {"ETag": etag})
                return self._send(200, cover, "image/jpeg", {"ETag": etag, "Cache-Control": "private, max-age=86400"})
            if what == "waveform":
                try:
                    pps = max(1.0, min(64.0, float(q.get("points_per_second", 4))))
                except ValueError:
                    return self._send(400, {"error": "points_per_second must be a number"})
                return self._send(200, waveform(pps, row[5], index))
            if what == "beats":
                return self._send(404, {"error": "not found"})
            return self._send(200, {"setlist_song_id": int(m.group(1)), "title": row[0],
                                    "arrangement_detail": {"title": "Default", "sections": sections_for(row[5])}})
        if path in ("/", "/index.json"):
            return self._send(200, {"name": "fake playback-library", "version": state.args.version})
        return self._send(404, {"error": "not found"})

    def do_POST(self):
        self._send(405, {"error": "read-only"})

    do_PUT = do_DELETE = do_POST


KEY_LINE = re.compile(r"^\s*printf '%s\\n' '([A-Za-z0-9_-]{32,})' > \"\$TMP/key\.txt\"\s*$", re.M)


def key_from_script(script):
    """The key inside the one-time install script StagePilot serves (what the real installer is given as `--key-file`)."""
    found = KEY_LINE.search(script)
    return found.group(1) if found else None


def pair_from(url, key_file):
    """Preview only: fetch StagePilot's one-time install script, keep its key in `key_file` (mode 600), run nothing."""
    import os
    import urllib.request
    with urllib.request.urlopen(url, timeout=10) as response:  # noqa: S310 (the URL is typed by the person previewing)
        key = key_from_script(response.read().decode("utf-8", "replace"))
    if not key:
        raise SystemExit("no key found in the script at that address")
    descriptor = os.open(key_file, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(descriptor, "w") as handle:
        handle.write(key + "\n")
    log("paired: the key from the install script is in %s" % key_file)


def main(argv=None):
    parser = argparse.ArgumentParser(description="A stand-in for the Playback helper (see the top of this file).")
    parser.add_argument("--port", type=int, default=8780)
    parser.add_argument("--bind", default="127.0.0.1")
    parser.add_argument("--key-file", help="file whose first line is the key; read again on every request, so it may be made later")
    parser.add_argument("--version", default="0.3.0", help="the version it reports (try 0.2.9 for the update notice)")
    parser.add_argument("--old", action="store_true", help="behave like helper 0.2.0: no auth field in /health")
    parser.add_argument("--date", help="the date that gets the six-song setlist (default: today)")
    parser.add_argument("--no-setlist-on", action="append", default=[], metavar="DATE", help="a date Playback has no setlist for (answers 404 for it, the way a Sunday with no setlist does); repeatable")
    parser.add_argument("--pair-from", metavar="URL", help="before starting, fetch StagePilot's one-time install script at URL and keep its key in --key-file (preview only)")
    args = parser.parse_args(argv)
    if args.pair_from:
        if not args.key_file:
            parser.error("--pair-from needs --key-file")
        pair_from(args.pair_from, args.key_file)
    server = Server((args.bind, args.port), Handler)
    server.state = State(args)
    log("fake playback-library %s on http://%s:%d (key: %s; six songs for %s)" % (
        args.version, args.bind, args.port, server.state.mode(), args.date or "today"))
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
