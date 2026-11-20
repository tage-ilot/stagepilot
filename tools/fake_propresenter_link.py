#!/usr/bin/env python3
"""A stand-in for the ProPresenter Link (the helper on the ProPresenter computer), for looking at StagePilot's ProPresenter Link
without a ProPresenter and without a screen. Standard library only.

It is the REAL helper (`crates/stagepilot-propresenter/assets/propresenter-link/propresenter_link.py`) loaded as a module and run
on a temporary ProPresenter folder, so every answer, every refusal, the key check (X-PPL-Time / X-PPL-Sig, 120 s window, 10 bad
requests in 60 s then 429, the body hash of PUT and POST), the path rules, the create-only writes into the library Playback, the
size limit and the song index are the helper's own. Only the screen is replaced: the Accessibility check says yes, "importing a
song" writes a small synthetic `.pro` (never a real song) into the Songs library, and "selecting a library" says it tried.

    python3 tools/fake_propresenter_link.py --port 18790 --key-file /tmp/ppl-key.txt --allow-client 127.0.0.1 --allow-loopback
    python3 tools/fake_propresenter_link.py --port 18790 --key-file /tmp/ppl-key.txt --version 0.0.9      # "update available"

The key file is the one StagePilot's setup command gives the installer (`install.sh --key-file`): put any 43-character line in a
file and tell StagePilot the same key. It binds to 127.0.0.1 unless `--bind` says otherwise. The folder is made fresh in a
temporary directory (`--folder` uses another one) with the libraries Songs (two synthetic songs) and, with `--playback`, Playback.
"""

from __future__ import annotations

import argparse
import importlib.util
import logging
import os
import sys
import tempfile
import threading

sys.dont_write_bytecode = True      # never leave a __pycache__ next to the helper: its folder is a bundle of plain files

HERE = os.path.dirname(os.path.abspath(__file__))
HELPER = os.path.join(HERE, "..", "crates", "stagepilot-propresenter", "assets", "propresenter-link", "propresenter_link.py")


def load_helper():
    spec = importlib.util.spec_from_file_location("fake_propresenter_link_helper", HELPER)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def varint(value):
    out = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        if not value:
            out.append(byte)
            return bytes(out)
        out.append(byte | 0x80)


def synthetic_song(song_id=None, filler=64):
    """A tiny made-up `.pro`: a text field, some filler and (when `song_id` is given) the licence record the helper's index reads."""
    out = b"\x0a\x04info\x12" + varint(filler) + b"\x09" * filler
    if song_id:
        record = b"\x08" + varint(song_id)
        out += b"\xaa\x01" + varint(len(record)) + record
    return out


def configure(helper, args, key, folder):
    """Point the helper at `folder` and swap its screen for a stand-in."""
    helper.CFG.update(key=key, bind=args.bind, port=args.port, allow_clients=list(args.allow_client), allow_loopback=bool(args.allow_loopback),
                      data_folder=folder, log_file=os.path.join(folder, "..", "fake-propresenter-link.log"))
    helper.FOLDER = helper.Folder(folder)
    helper.log.addHandler(logging.NullHandler())        # a refused request is not printed to the screen
    helper.__version__ = args.version
    helper.INDEX["version"] = args.version
    helper.is_mac = lambda: True
    helper.accessibility = lambda: (True, None)
    helper.app_version = lambda: "19.0.1"

    def import_song(song_id, name, playlist):
        # What the helper checks before it touches the screen, then what the screen would have produced.
        if not isinstance(song_id, str) or not song_id.isdigit() or len(song_id) > 18:
            raise helper.Refused(400, "bad_id", "The song number must be digits only.")
        name = helper.import_name(name)
        target = os.path.join(helper.FOLDER.library_path(helper.SONGS_LIBRARY), name + ".pro")
        if os.path.lexists(target):
            raise helper.Refused(409, "exists", "A song with that name is already in the Songs library.")
        # `--import-dir DIR`: DIR/<song number>.pro is what the import writes (a test's own made-up song); else a tiny stand-in.
        bytes_ = None
        import_dir = getattr(args, "import_dir", None)
        if import_dir:
            try:
                with open(os.path.join(import_dir, song_id + ".pro"), "rb") as f:
                    bytes_ = f.read()
            except OSError:
                bytes_ = None
        with open(target, "wb") as f:
            f.write(bytes_ if bytes_ is not None else synthetic_song(int(song_id)))
        return {"file": name + ".pro"}

    helper.search_import = import_song
    def select_library(name):
        if not helper.library_name(name):
            return None
        # What a person's click on the library does in ProPresenter: the fake ProPresenter API (`--lazy select`) loads the files then.
        try:
            with open(os.path.join(folder, ".selected-" + name), "w"):
                pass
        except OSError:
            pass
        return {"tried": True, "detail": ""}

    helper.select_library = select_library


def make_folder(base, playback=True):
    root = os.path.join(base, "ProPresenter")
    for lib in ("Songs", "Playback") if playback else ("Songs",):
        os.makedirs(os.path.join(root, "Libraries", lib), exist_ok=True)
    for name, song_id in (("Alpha.pro", 1001), ("Beta.pro", 1002)):
        with open(os.path.join(root, "Libraries", "Songs", name), "wb") as f:
            f.write(synthetic_song(song_id))
    return root


def serve(helper, args):
    """Start the helper's server (a thread); returns (server, base_url)."""
    server = helper.Server((args.bind, args.port), helper.Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, "http://%s:%d" % (args.bind, server.server_address[1])


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("--port", type=int, default=8790)
    p.add_argument("--bind", default="127.0.0.1")
    p.add_argument("--key-file", required=True, help="a file whose first line is the key (32 or more characters)")
    p.add_argument("--allow-client", action="append", default=[], help="a client address that is answered (the StagePilot computer)")
    p.add_argument("--allow-loopback", action="store_true", help="also answer programs on this computer")
    p.add_argument("--version", default=None, help="the version it reports (default: the bundled helper's)")
    p.add_argument("--folder", default=None, help="use this ProPresenter folder instead of a fresh temporary one")
    p.add_argument("--playback", action="store_true", help="start with a Playback library folder")
    p.add_argument("--import-dir", default=None, help="an import writes DIR/<song number>.pro when it is there (made-up songs of a test)")
    args = p.parse_args(argv)
    helper = load_helper()
    args.version = args.version or helper.__version__
    with open(args.key_file) as f:
        key = f.readline().rstrip("\r\n")
    base = tempfile.mkdtemp(prefix="fake-propresenter-link-")
    folder = args.folder or make_folder(base, args.playback)
    configure(helper, args, key, folder)
    server, url = serve(helper, args)
    print("fake ProPresenter Link %s on %s, folder %s" % (args.version, url, folder), flush=True)
    try:
        threading.Event().wait()
    except KeyboardInterrupt:
        pass
    server.shutdown()
    return 0


if __name__ == "__main__":
    sys.exit(main())
