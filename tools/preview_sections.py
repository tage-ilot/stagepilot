#!/usr/bin/env python3
"""Serve the section gallery (every section template and every configuration page in every state, plus the Connection section for Lighting, Playback and Presentation) on the local network, so a
design can be looked at in a browser on any computer or phone. Standard library only; no backend is needed: the data on the page is made up.

    cd frontend && npm run build:gallery        # writes frontend/dist-gallery (never part of the app or the desktop bundle)
    python3 tools/preview_sections.py --bind 0.0.0.0 --port 8102

Open http://<this computer>:8102/ . The page has a width switch (360 px, 1280 px, Fit). It only reads files: it never writes, never asks a
backend and never touches ProPresenter, Playback or Lightkey. Stop it with Ctrl-C.
"""

import argparse
import functools
import http.server
import os
import socket
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_ROOT = os.path.join(os.path.dirname(HERE), "frontend", "dist-gallery")
START_PAGE = "sections.html"


class Handler(http.server.SimpleHTTPRequestHandler):
    """Files from the gallery folder, `/` is the gallery page, read-only, never listed, never cached (a rebuild shows at once)."""

    server_version = "StagePilotSections"

    def translate_path(self, path):
        clean = path.partition("?")[0].partition("#")[0]
        if clean in ("", "/"):
            path = "/" + START_PAGE
        return super().translate_path(path)

    def list_directory(self, path):  # never show a folder's files
        self.send_error(404, "Not found")
        return None

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def do_POST(self):
        self.send_error(405, "Read-only")

    do_PUT = do_DELETE = do_PATCH = do_POST

    def log_message(self, format, *args):  # noqa: A002 - the base class's name
        sys.stderr.write("%s - %s\n" % (self.address_string(), format % args))


def make_server(root, bind, port):
    """The server for `root` (created, not started). Raises FileNotFoundError when the gallery was not built."""
    if not os.path.isfile(os.path.join(root, START_PAGE)):
        raise FileNotFoundError("%s is not built: run `npm run build:gallery` in frontend/ first" % os.path.join(root, START_PAGE))
    return http.server.ThreadingHTTPServer((bind, port), functools.partial(Handler, directory=root))


def lan_address():
    """This computer's LAN address, for the line that says where to look (nothing is sent: a UDP socket only picks the interface)."""
    probe = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        probe.connect(("192.0.2.1", 9))
        return probe.getsockname()[0]
    except OSError:
        return "127.0.0.1"
    finally:
        probe.close()


def main(argv=None):
    parser = argparse.ArgumentParser(description="Serve frontend/dist-gallery, the section gallery.")
    parser.add_argument("--bind", default="127.0.0.1", help="0.0.0.0 (or this computer's address) to open it to the network")
    parser.add_argument("--port", type=int, default=8102)
    parser.add_argument("--root", default=DEFAULT_ROOT, help="the built gallery (default: frontend/dist-gallery)")
    args = parser.parse_args(argv)
    try:
        server = make_server(args.root, args.bind, args.port)
    except FileNotFoundError as error:
        print(error, file=sys.stderr)
        return 2
    shown = lan_address() if args.bind in ("0.0.0.0", "") else args.bind
    print("Section gallery\n  open:  http://%s:%d/\n  files: %s" % (shown, server.server_address[1], args.root), flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
