#!/usr/bin/env python3
"""playback-library: what MultiTracks Playback knows about its songs, over HTTP (macOS, read-only).

Playback keeps its setlists, the chosen key, the current arrangement, the beat grid and a picture of every
song's waveform on the Mac that runs it. This agent reads that library (a read-only copy of Playback's own
database and the waveform pictures) and serves it as JSON to other computers on your network: setlists,
songs, key, BPM, length, sections in the order they are played, beats, tempo changes, stems, cues, and the
waveform as plain numbers re-timed to the arranged song. It never sends audio, never reads the song audio
files and never changes anything in Playback. The song and setlist list is only what Playback itself holds.

With a key in config.json (0.3.0), every request except the bare /health must be signed with it (docs/api.md, "Key"); any network
path then works. Without a key it behaves as 0.2.0.

Python 3.9+ standard library only. Not affiliated with MultiTracks.

Commands:
  run                 start the agent (what the LaunchAgent runs)
  check               verify that the Playback library can be read, and print a summary
  dump WHAT           print JSON without starting the server. WHAT: health | setlists | current | latest |
                      song ID | waveform ID | beats ID | library | outputs | history
  --version           print the version
"""
import base64
import collections
import hashlib
import hmac
import itertools
import json
import logging
import logging.handlers
import os
import plistlib
import re
import shutil
import socket
import sqlite3
import struct
import subprocess
import sys
import threading
import time
import urllib.parse
import zlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

__version__ = "0.4.1"

HOME = os.path.expanduser("~")
BUNDLE_ID = "com.multitracks.custommixplayer"
APP_SUPPORT = os.path.join(HOME, "Library", "Application Support", "PlaybackLibrary")
DEFAULT_CONFIG_PATH = os.path.join(APP_SUPPORT, "config.json")
CORE_DATA_EPOCH = 978307200  # Core Data counts seconds from 2001-01-01 UTC
FOLDER_PAIRED_FROM, FOLDER_PAIRED_TO = 13, 32    # a type-13 row whose song folder is not there is looked up as type 32 (same content id), nothing else

DEFAULTS = {
    "port": 8780,
    "bind": "",                      # "" = all interfaces, filtered by the allow-list below
    "key": "",                       # the shared key (32+ characters). When set, every request but the bare /health must be signed
    "allow_interfaces": [],          # interfaces (e.g. ["en0"]) whose address may be queried; loopback always may. With a key: an extra limit, ignored when empty
    "allowed_local_ips": [],         # extra server-side addresses that may be queried
    "allow_clients": [],             # if not empty, only these client addresses may connect (loopback always may). With a key: an extra limit
    "container": os.path.join(HOME, "Library", "Containers", BUNDLE_ID, "Data"),
    "data_dir": APP_SUPPORT,         # the read-only database copy, the waveform cache and history.jsonl
    "refresh_min_interval_s": 5.0,   # look for a changed database at most this often
    "watch_interval_s": 30.0,        # background check (keeps the history even when nobody asks)
    "history": True,                 # note every change to the key or arrangement of this week's songs
    "history_days": 10,              # ... for setlists performed within this many days of now
    "prefetch_waveforms": True,      # decode the newest setlists' waveform pictures in the background
    "prefetch_setlists": 2,
    "process_name": "Playback",
    "log_file": os.path.join(HOME, "Library", "Logs", "playback-library.log"),
}
CFG = dict(DEFAULTS)
CONFIG_PATH = DEFAULT_CONFIG_PATH
log = logging.getLogger("playback-library")
APP_DIR = os.path.dirname(os.path.abspath(__file__))     # where the installer put this file; an update replaces the files here
UPDATE_FILES = ("playback_library.py",)                   # the only file an update may carry

# ===== BEGIN helper-update core =====
# This block is the same, byte for byte, in the Playback helper (playback_library.py) and the Lightkey helper
# (lightkey_api.py); StagePilot's tests keep the two copies equal. It holds everything a signed self-update needs:
# the pinned public key, a pure-Python Ed25519 verifier (macOS command-line Python 3.9 has no Ed25519), the checks
# an update must pass, and the staged, journaled swap of the files. It makes no network connection.
UPDATE_PUBLIC_KEY = "Evky97ydM5zWJ02p6Wc7RguEHMglX3WRxiPrege4Op8="    # StagePilot's helper-signing key: raw 32 bytes, base64
UPDATE_MAX_BYTES = 2 * 1024 * 1024         # the largest request body /update reads
UPDATER_LEVEL = 1                          # what /health reports as "updater": this helper can update itself, protocol 1
_VERSION_LINE = re.compile(rb'^__version__ = "([0-9]+\.[0-9]+\.[0-9]+)"[ \t]*\r?$', re.M)
_SUMS_LINE = re.compile(r"^([0-9a-f]{64}) [ *]([A-Za-z0-9_][A-Za-z0-9_.-]*)$")


class UpdateRejected(Exception):
    """An update that must not be installed. `message` is one plain sentence; `status` the HTTP code to answer."""

    def __init__(self, message, status=422):
        Exception.__init__(self, message)
        self.message, self.status = message, status


# ---- Ed25519 signature check (RFC 8032, section 5.1.7), the plain reference algorithm. One check per update, so speed
# does not matter; clarity does. Points are kept as extended coordinates (X, Y, Z, T) with x = X/Z, y = Y/Z, T = X*Y/Z.
_ED_P = 2 ** 255 - 19                                           # the field: numbers modulo this prime
_ED_L = 2 ** 252 + 27742317777372353535851937790883648493       # the order of the base point
_ED_D = -121665 * pow(121666, _ED_P - 2, _ED_P) % _ED_P         # the curve constant d = -121665/121666
_ED_SQRT_M1 = pow(2, (_ED_P - 1) // 4, _ED_P)                   # a square root of -1


def _ed_x_from_y(y, sign):
    """The x of the curve point with this y and this low bit of x, or None when there is no such point."""
    if y >= _ED_P:
        return None
    x2 = (y * y - 1) * pow(_ED_D * y * y + 1, _ED_P - 2, _ED_P) % _ED_P
    if x2 == 0:
        return None if sign else 0
    x = pow(x2, (_ED_P + 3) // 8, _ED_P)
    if (x * x - x2) % _ED_P:
        x = x * _ED_SQRT_M1 % _ED_P
    if (x * x - x2) % _ED_P:
        return None
    return x if x & 1 == sign else _ED_P - x


def _ed_decode(data):
    """A 32-byte point encoding (y, with the low bit of x in the top bit) as a point, or None when it is not a valid point."""
    if len(data) != 32:
        return None
    y = int.from_bytes(data, "little")
    sign, y = y >> 255, y & ((1 << 255) - 1)
    x = _ed_x_from_y(y, sign)
    return None if x is None else (x, y, 1, x * y % _ED_P)


def _ed_add(a, b):
    """Point addition (the complete formula of RFC 8032, section 5.1.4)."""
    p = _ED_P
    e = (a[1] - a[0]) * (b[1] - b[0]) % p
    f = (a[1] + a[0]) * (b[1] + b[0]) % p
    g = 2 * a[3] * b[3] * _ED_D % p
    h = 2 * a[2] * b[2] % p
    e, f, g, h = f - e, h - g, h + g, f + e
    return (e * f % p, g * h % p, f * g % p, e * h % p)


def _ed_mul(n, point):
    """n * point by double-and-add."""
    result = (0, 1, 1, 0)                                      # the neutral point
    while n:
        if n & 1:
            result = _ed_add(result, point)
        point = _ed_add(point, point)
        n >>= 1
    return result


def _ed_same(a, b):
    """The two points are equal (compared without dividing: x1/z1 = x2/z2 and y1/z1 = y2/z2)."""
    return (a[0] * b[2] - b[0] * a[2]) % _ED_P == 0 and (a[1] * b[2] - b[1] * a[2]) % _ED_P == 0


def _ed_is_small_order(point):
    """The point is one of the 8 points of order dividing 8 (eight times it is the neutral point)."""
    x, y, z, _t = _ed_mul(8, point)
    return x % _ED_P == 0 and (y - z) % _ED_P == 0


_ED_BASE_Y = 4 * pow(5, _ED_P - 2, _ED_P) % _ED_P
_ED_BASE = (_ed_x_from_y(_ED_BASE_Y, 0), _ED_BASE_Y, 1, _ed_x_from_y(_ED_BASE_Y, 0) * _ED_BASE_Y % _ED_P)


def ed25519_verify(public_key, message, signature):
    """True only when `signature` (64 bytes) is a valid Ed25519 signature of `message` under `public_key` (32 bytes).
    Anything malformed (a wrong length, a point that is not on the curve, a non-canonical S) is simply False."""
    try:
        if len(public_key) != 32 or len(signature) != 64:
            return False
        a = _ed_decode(bytes(public_key))
        r = _ed_decode(bytes(signature[:32]))
        s = int.from_bytes(signature[32:], "little")
        if a is None or r is None or s >= _ED_L:               # S must be below the group order (no second encoding of a signature)
            return False
        if _ed_is_small_order(a) or _ed_is_small_order(r):    # no honest key or signature has one; with one the check proves nothing
            return False
        k = int.from_bytes(hashlib.sha512(bytes(signature[:32]) + bytes(public_key) + bytes(message)).digest(), "little") % _ED_L
        return _ed_same(_ed_mul(s, _ED_BASE), _ed_add(r, _ed_mul(k, a)))     # S*B == R + k*A
    except Exception:
        return False


# ---- what an update must pass
def parse_version(text):
    """'1.2.3' as (1, 2, 3); None for anything else."""
    m = re.fullmatch(r"([0-9]+)\.([0-9]+)\.([0-9]+)", text.strip()) if isinstance(text, str) else None
    return tuple(int(g) for g in m.groups()) if m else None


def parse_sums(text):
    """SHA256SUMS text as {file name: hex digest}. Every non-empty line must be '<64 hex>  <plain file name>', no name twice."""
    table = {}
    for line in text.split("\n"):
        if not line.strip():
            continue
        m = _SUMS_LINE.match(line.rstrip("\r"))
        if not m or m.group(2) in table:
            raise UpdateRejected("The update's checksum list is not valid.")
        table[m.group(2)] = m.group(1)
    return table


def check_update(request, expected, version_file, current):
    """Run the checks on a decoded /update body, in this order, and return (the new version text, {name: bytes}):
    the signature of SHA256SUMS against the pinned key; every file's SHA-256 against SHA256SUMS; the file names are
    exactly `expected`; the new version (read from `version_file`) is above `current`. Raises UpdateRejected."""
    files, sums, sig = request.get("files"), request.get("sums"), request.get("sig")
    if not isinstance(files, dict) or not isinstance(sums, str) or not isinstance(sig, str) or not all(isinstance(v, str) for v in files.values()):
        raise UpdateRejected("The update request is not in the expected shape.", 400)
    try:
        sig_bytes = base64.b64decode(sig, validate=True)
        signed = sums.encode("utf-8")
    except ValueError:                                         # not base64, or text that cannot be bytes (UnicodeEncodeError is a ValueError)
        sig_bytes, signed = b"", b""
    if not ed25519_verify(base64.b64decode(UPDATE_PUBLIC_KEY), signed, sig_bytes):
        raise UpdateRejected("The update is not signed by the StagePilot project.")
    table = parse_sums(sums)
    data = {}
    for name, encoded in files.items():
        try:
            data[name] = base64.b64decode(encoded, validate=True)
        except ValueError:
            raise UpdateRejected("A file in the update is not valid base64.", 400)
        if table.get(name) != hashlib.sha256(data[name]).hexdigest():
            raise UpdateRejected("A file in the update does not match the signed checksums.")
    if set(data) != set(expected):
        raise UpdateRejected("The update does not hold exactly the files this helper takes.")
    found = _VERSION_LINE.search(data[version_file])
    new, old = parse_version(found.group(1).decode()) if found else None, parse_version(current)
    if new is None or old is None:
        raise UpdateRejected("The update does not say which version it is.")
    if new <= old:
        raise UpdateRejected("The update is not newer than the version that runs (%s)." % current, 409)
    return found.group(1).decode(), data


def read_marker(app_dir, name):
    """The JSON object in app_dir/name, or None."""
    try:
        with open(os.path.join(app_dir, name), encoding="utf-8") as f:
            value = json.load(f)
        return value if isinstance(value, dict) else None
    except (OSError, ValueError):
        return None


def write_marker(app_dir, name, value):
    """Writes app_dir/name as JSON, all at once (a half-written marker is never seen)."""
    path = os.path.join(app_dir, name)
    with open(path + ".tmp", "w", encoding="utf-8") as f:
        json.dump(value, f)
        f.flush()
        os.fsync(f.fileno())
    os.replace(path + ".tmp", path)


def last_update_error(app_dir):
    """The sentence the wrapper left when a new version would not start and the old one was put back; None otherwise."""
    marker = read_marker(app_dir, "update-failed")
    message = marker.get("message") if marker else None
    return message if isinstance(message, str) and message else None


def stage_update(app_dir, data):
    """Writes the new files into app_dir/update/ (replacing any leftovers), checks that every .py compiles there, and
    records the plan in update/journal.json, last. Raises UpdateRejected when a file does not compile; nothing outside
    update/ is touched."""
    import py_compile
    stage = os.path.join(app_dir, "update")
    shutil.rmtree(stage, ignore_errors=True)
    os.makedirs(os.path.join(stage, "compiled"))
    try:
        for name, blob in sorted(data.items()):
            with open(os.path.join(stage, name), "wb") as f:
                f.write(blob)
                f.flush()
                os.fsync(f.fileno())
            if name.endswith(".py"):
                try:
                    py_compile.compile(os.path.join(stage, name), cfile=os.path.join(stage, "compiled", name + "c"), doraise=True)
                except py_compile.PyCompileError:
                    raise UpdateRejected("A file in the update does not compile.")
        shutil.rmtree(os.path.join(stage, "compiled"), ignore_errors=True)
        return stage
    except BaseException:
        shutil.rmtree(stage, ignore_errors=True)
        raise


def commit_update(app_dir, journal):
    """The swap. For each file in the journal, in order: keep the file that runs now as NAME.prev, then put the staged file
    in its place (an atomic rename). Safe to run again after a crash at any point: a file whose staged copy is gone was done
    before, so the same run finishes the rest. Last it marks the update as unproven (update-pending: the wrapper may put
    the .prev files back while the new version has not yet run for a while) and removes the journal and the staging folder."""
    stage = os.path.join(app_dir, "update")
    for name in journal["files"]:
        staged, final = os.path.join(stage, name), os.path.join(app_dir, name)
        if not os.path.exists(staged):
            continue
        if os.path.exists(final):
            shutil.copyfile(final, final + ".prev.tmp")
            os.replace(final + ".prev.tmp", final + ".prev")
        os.replace(staged, final)
    write_marker(app_dir, "update-pending", {"from": journal["from"], "to": journal["to"]})
    try:
        os.remove(os.path.join(app_dir, "update-failed"))
    except OSError:
        pass
    os.remove(os.path.join(stage, "journal.json"))
    shutil.rmtree(stage, ignore_errors=True)


def apply_update(app_dir, data, old_version, new_version):
    """Stage the files, write the journal, swap. After this returns the new set is in place; the caller replies and exits so
    that the service manager starts the new version."""
    stage = stage_update(app_dir, data)
    journal = {"from": old_version, "to": new_version, "files": sorted(data)}
    write_marker(stage, "journal.json", journal)
    commit_update(app_dir, journal)


def finish_interrupted_update(app_dir):
    """A crash between the first and the last rename left a journal: finish the swap. True when there was one."""
    journal = read_marker(os.path.join(app_dir, "update"), "journal.json")
    if not journal or not isinstance(journal.get("files"), list) or not isinstance(journal.get("from"), str) or not isinstance(journal.get("to"), str):
        return False
    if not all(isinstance(n, str) and re.fullmatch(r"[A-Za-z0-9_]+\.py", n) for n in journal["files"]):
        return False                                           # only plain .py names in the install folder, whatever the file says
    commit_update(app_dir, journal)
    return True


_PRIVATE_NETWORKS = None


def peer_is_private(address):
    """True for a loopback, private (10/8, 172.16/12, 192.168/16, fc00::/7) or link-local (169.254/16, fe80::/10) address:
    the local network only. An IPv4 address written as IPv6 counts as the IPv4 address."""
    import ipaddress
    global _PRIVATE_NETWORKS
    if _PRIVATE_NETWORKS is None:
        _PRIVATE_NETWORKS = [ipaddress.ip_network(n) for n in ("127.0.0.0/8", "10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16",
                                                              "169.254.0.0/16", "::1/128", "fc00::/7", "fe80::/10")]
    try:
        ip = ipaddress.ip_address(address.split("%")[0])
    except ValueError:
        return False
    if ip.version == 6 and ip.ipv4_mapped:
        ip = ip.ipv4_mapped
    return any(ip.version == net.version and ip in net for net in _PRIVATE_NETWORKS)
# ===== END helper-update core =====


# ---- key authentication (docs/api.md, "Key")
MIN_KEY_LENGTH = 32
TIME_WINDOW_S = 120          # a signature is good while |now - X-PBL-Time| <= this
FAIL_LIMIT = 10              # this many bad signatures from one address ...
FAIL_WINDOW_S = 60           # ... within this many seconds ...
BLOCK_S = 60                 # ... and that address gets 429 for this long
FAIL_TABLE_MAX = 512         # addresses remembered at once
LOG_ADDR_EVERY_S = 30        # one "rejected request" log line per address per this many seconds
LOG_GLOBAL_MAX = 20          # ... and at most this many lines per FAIL_WINDOW_S in total


def _now():
    return time.time()       # tests replace this to move the clock

SCALE_NAMES = ("quarters", "eighths", "sixteenths", "accents")


# ---------------------------------------------------------------- small helpers
def core_time(x):
    """Core Data seconds -> Unix seconds (None stays None)."""
    return None if x is None else float(x) + CORE_DATA_EPOCH


def iso_utc(unix):
    return None if unix is None else time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(unix))


def local_date(unix):
    return None if unix is None else time.strftime("%Y-%m-%d", time.localtime(unix))


def r3(x):
    return None if x is None else round(float(x), 3)


def norm(row):
    """sqlite3.Row -> dict with Playback's Z prefix removed and lower-case names (ZSETLISTSONGID -> setlistsongid)."""
    out = {}
    for k in row.keys():
        out[k[1:].lower() if k.startswith("Z") and not k.startswith("Z_") else k.lower()] = row[k]
    return out


def json_blob(x, default=None):
    """Parsed JSON from a text or blob column (None or garbage gives the default)."""
    try:
        if isinstance(x, (bytes, bytearray)):
            x = bytes(x).decode("utf-8", "replace")
        return json.loads(x) if x else default
    except ValueError:
        return default


def text(x, n=200):
    return x[:n] if isinstance(x, str) else x


MAX_ARTWORK_BYTES = 2_000_000       # the real covers are 7 to 77 KB; anything near this is not a cover


def image_type(data):
    """'image/jpeg' or 'image/png' when the bytes start like one, else None (never serve anything else)."""
    if not isinstance(data, (bytes, bytearray)) or not 8 < len(data) <= MAX_ARTWORK_BYTES:
        return None
    if data[:3] == b"\xff\xd8\xff":
        return "image/jpeg"
    if data[:8] == b"\x89PNG\r\n\x1a\n":
        return "image/png"
    return None


class Blob:
    """A binary answer (a song's cover picture): bytes, content type and a tag that changes when the bytes do."""

    def __init__(self, content_type, data):
        self.content_type, self.data = content_type, bytes(data)
        self.etag = '"%s"' % hashlib.sha1(self.data).hexdigest()[:16]


def interface_addrs():
    """{interface: ipv4} for every interface that has an address (parsed from ifconfig)."""
    out = {}
    try:
        txt = subprocess.run(["/sbin/ifconfig", "-a"], capture_output=True, text=True, timeout=5).stdout
    except Exception:
        return out
    cur = None
    for line in txt.splitlines():
        m = re.match(r"^([a-z0-9]+):", line)
        if m:
            cur = m.group(1)
            continue
        m = re.match(r"^\s+inet (\d+\.\d+\.\d+\.\d+)", line)
        if m and cur:
            out.setdefault(cur, m.group(1))
    return out


# ---------------------------------------------------------------- arrangement -> performed timeline
def performed_segments(sections):
    """Turn the arrangement's ordered sections into the timeline that Playback plays.

    Each section plays original-audio seconds [orig_start + trim_start, orig_end - trim_end). The pieces are laid
    end to end from 0 (the arranged song's own clock). Playback's clock ends where the sections end: measured on
    the real Playback, one song whose stored end time was 24.7 s earlier still ran to the end of its sections.
    Loop counts are NOT applied (their meaning is not verified); sections that carry one are flagged in the output.
    """
    segs, t = [], 0.0
    for i, s in enumerate(sections):
        a = (s.get("orig_start") or 0.0) + (s.get("trim_start") or 0.0)
        b = (s.get("orig_end") or 0.0) - (s.get("trim_end") or 0.0)
        if b <= a:
            continue
        segs.append({"index": i, "start": a, "end": b, "perf_start": t, "perf_end": t + (b - a)})
        t += b - a
    return segs, t


def to_performed(segs, t_original):
    """Every performed time at which original-audio second t_original is played (a repeated section gives several)."""
    return [g["perf_start"] + (t_original - g["start"]) for g in segs if g["start"] <= t_original < g["end"]]


def effective_performed(segs, t_original):
    """When a value that holds from original second t_original starts to count in the played song. A value set inside
    a trimmed-away part (a tempo set at 0 s under a trimmed count-off) counts from the next second that is played."""
    p = to_performed(segs, t_original)
    if p:
        return p[0]
    later = [g for g in segs if g["start"] >= t_original]
    return later[0]["perf_start"] if later else None


# ---------------------------------------------------------------- NSKeyedArchiver float lists (the beat grid)
def keyed_floats(blob):
    """Seconds stored by Playback as an archived NSArray of numbers. Falls back to every number in the archive."""
    if not isinstance(blob, (bytes, bytearray)) or not blob:
        return []
    try:
        arc = plistlib.loads(bytes(blob))
        objs = arc["$objects"]
    except Exception:
        return []
    vals = None
    for o in objs:
        if isinstance(o, dict) and "NS.objects" in o:
            vals = []
            for u in o["NS.objects"]:
                v = objs[u.data] if isinstance(u, plistlib.UID) else u
                if isinstance(v, (int, float)) and not isinstance(v, bool):
                    vals.append(float(v))
            if vals:
                break
    if not vals:
        vals = [float(o) for o in objs if isinstance(o, float)]
    return sorted(vals)


# ---------------------------------------------------------------- waveform pictures (PNG, standard library only)
_NZ = bytes([0] + [1] * 255)


def _add_mod256(x, y):
    n = len(x)
    a, b = int.from_bytes(x, "little"), int.from_bytes(y, "little")
    m7, m8 = int.from_bytes(b"\x7f" * n, "little"), int.from_bytes(b"\x80" * n, "little")
    return (((a & m7) + (b & m7)) ^ ((a ^ b) & m8)).to_bytes(n, "little")


def png_ink_columns(path):
    """(width, height, per-column count of visible pixels). Reads only the alpha channel of an 8-bit RGBA or
    grey+alpha PNG: the PNG filters work on each channel on its own, so the picture is not decoded in colour."""
    with open(path, "rb") as f:
        data = f.read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("not a PNG")
    pos, idat, ihdr = 8, [], None
    while pos + 8 <= len(data):
        n, kind = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + n]
        pos += 12 + n
        if kind == b"IHDR":
            ihdr = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            idat.append(body)
        elif kind == b"IEND":
            break
    if ihdr is None:
        raise ValueError("PNG without a header")
    w, h, depth, ctype, _, _, interlace = ihdr
    if depth != 8 or interlace or ctype not in (4, 6):
        raise ValueError("unsupported PNG (need 8-bit RGBA or grey+alpha, not interlaced)")
    bpp = 4 if ctype == 6 else 2
    raw = zlib.decompress(b"".join(idat))
    stride = 1 + w * bpp
    if len(raw) < h * stride:
        raise ValueError("PNG data is short")
    if h > 255:
        raise ValueError("picture too tall (%d rows)" % h)
    prev = bytes(w)
    acc = 0
    for r in range(h):
        f = raw[r * stride]
        row = raw[r * stride + 1:(r + 1) * stride]
        a = row[bpp - 1::bpp]
        if f == 0:
            cur = a
        elif f == 1:
            cur = bytes(itertools.accumulate(a, lambda x, y: (x + y) & 255))
        elif f == 2:
            cur = _add_mod256(a, prev)
        elif f == 3:
            b = bytearray(a)
            left = 0
            for i in range(w):
                left = b[i] = (b[i] + ((left + prev[i]) >> 1)) & 255
            cur = bytes(b)
        elif f == 4:
            b = bytearray(a)
            left = ul = 0
            for i in range(w):
                up = prev[i]
                p = left + up - ul
                pa, pb, pc = abs(p - left), abs(p - up), abs(p - ul)
                pred = left if (pa <= pb and pa <= pc) else (up if pb <= pc else ul)
                left = b[i] = (b[i] + pred) & 255
                ul = up
            cur = bytes(b)
        else:
            raise ValueError("bad PNG filter %d" % f)
        prev = cur
        acc += int.from_bytes(cur.translate(_NZ), "little")     # one count per column; at most 255 rows, so no carry between columns
    return w, h, acc.to_bytes(w, "little")


def columns_per_second(name):
    """Waveform<scale>x.<n>.png holds n * scale^2 / 2 columns per second (checked on two songs: 1x.32 = 16, 2x.32 = 64)."""
    m = re.match(r"Waveform(\d)x\.(\d+)\.png$", name)
    return None if not m else int(m.group(2)) * int(m.group(1)) ** 2 / 2.0


def bucket(values, per_bucket):
    """(peaks, means) of a column list cut into buckets of per_bucket columns (a float)."""
    n = len(values)
    count = max(1, int(round(n / per_bucket)))
    peaks, means = [], []
    for i in range(count):
        lo, hi = int(i * n / count), max(int(i * n / count) + 1, int((i + 1) * n / count))
        part = values[lo:hi]
        if not part:
            break
        peaks.append(max(part))
        means.append(sum(part) / len(part))
    return peaks, means


# ---------------------------------------------------------------- the library
# What the arrangement's sections add up to (kept pieces only), the same sum as performed_segments(): the arranged song's length.
_KEPT_END = "(x.ZENDTIMEORIGINAL - coalesce(x.ZSTOREDENDTIMETRIMOFFSET, 0))"
_KEPT_START = "(x.ZSTARTTIMEORIGINAL + coalesce(x.ZSTOREDSTARTTIMETRIMOFFSET, 0))"
SECTIONS_TOTAL_SQL = ("(select sum(case when %s > %s then %s - %s else 0 end) from ZSONGSECTION x where x.ZARRANGEMENT = a.Z_PK)"
                      % (_KEPT_END, _KEPT_START, _KEPT_END, _KEPT_START))


class Library:
    """A throttled read-only copy of Playback's database, plus every question the API asks of it."""

    def __init__(self):
        self.lock = threading.RLock()
        self.src = None            # the original AppData.storedata
        self.copy = None           # our copy
        self.sig = None
        self.checked = 0.0
        self.changed_at = None
        self.error = None          # None | "missing" | "denied" | "unreadable"
        self.detail = ""
        self.names = {}            # section type id -> metadata row
        self.keys = {}
        self.track_types = {}
        self.handled = 0           # database generation the watch loop has indexed
        self.history_last = {}
        self.prefetch_done = set()
        self.generation = 0

    # -- paths
    def songs_dir(self):
        return os.path.join(CFG["container"], "Documents", "Songs")

    def find_source(self):
        base = os.path.join(CFG["container"], "Library")
        try:
            for root, _dirs, files in os.walk(base):
                if "AppData.storedata" in files:
                    return os.path.join(root, "AppData.storedata")
        except PermissionError:
            raise
        return None

    def data_path(self, *parts):
        return os.path.join(CFG["data_dir"], *parts)

    # -- refresh
    def refresh(self, force=False):
        """Copy the database again when Playback changed it. Returns True when the copy is new."""
        with self.lock:
            now = time.time()
            if not force and self.copy and now - self.checked < CFG["refresh_min_interval_s"]:
                return False
            self.checked = now
            try:
                if self.src is None or not os.path.exists(self.src):
                    self.src = self.find_source()
                if self.src is None:
                    self.error, self.detail = "missing", "Playback's database was not found under %s" % CFG["container"]
                    return False
                parts = [self.src, self.src + "-wal", self.src + "-shm"]
                sig = tuple((os.path.getmtime(p), os.path.getsize(p)) if os.path.exists(p) else None for p in parts)
                if sig == self.sig and self.copy and os.path.exists(self.copy):
                    self.error = None
                    return False
                dest = self.data_path("db")
                tmp = dest + ".new"
                shutil.rmtree(tmp, ignore_errors=True)
                os.makedirs(tmp)
                for p in parts:
                    if os.path.exists(p):
                        shutil.copy2(p, os.path.join(tmp, os.path.basename(p)))
                shutil.rmtree(dest, ignore_errors=True)
                os.rename(tmp, dest)
                self.copy = os.path.join(dest, "AppData.storedata")
                self._load_lookup()          # also proves that the copy opens (a copy made mid-write does not)
                self.sig, self.error, self.detail = sig, None, ""
                self.changed_at = now
                self.generation += 1
                return True
            except PermissionError as e:
                self.copy, self.sig = None, None
                self.error, self.detail = "denied", "macOS did not let the agent read Playback's data (%s)" % e
            except (OSError, sqlite3.Error) as e:
                self.copy, self.sig = None, None
                self.error, self.detail = "unreadable", str(e)
            return False

    def connect(self):
        db = sqlite3.connect("file:%s?mode=ro" % self.copy, uri=True, check_same_thread=False)
        db.row_factory = sqlite3.Row
        return db

    def query(self, sql, *args):
        self.refresh()
        if not self.copy:
            raise LookupError(self.detail or "the Playback library is not readable")
        with self.lock:
            db = self.connect()
            try:
                return [norm(r) for r in db.execute(sql, args).fetchall()]
            finally:
                db.close()

    def _load_lookup(self):
        db = self.connect()
        try:
            self.names = {}
            for r in db.execute("select * from ZMETADATASECTION"):
                n = norm(r)
                self.names[n["partid"]] = n
            self.keys = {}
            for r in db.execute("select * from ZMETADATAKEY"):
                n = norm(r)
                self.keys[n["id"]] = n
            self.track_types = {}
            for r in db.execute("select * from ZMETADATATRACK"):
                n = norm(r)
                self.track_types[n["partid"]] = n
        finally:
            db.close()

    # -- keys, sections
    def key_info(self, key_id):
        k = self.keys.get(key_id)
        if not k:
            return {"id": key_id, "name": None}
        return {"id": key_id, "name": k.get("title"), "scale": k.get("storedscale"), "accidental": k.get("storedaccidental")}

    def section_name(self, type_id):
        n = self.names.get(type_id)
        return n.get("title") if n else None

    # -- setlists
    def api_data(self, row):
        d = json_blob(row.get("sourceapisetlistdata"), {})
        return d if isinstance(d, dict) else {}

    def setlist_summary(self, r, count, api=None):
        perf, loaded = core_time(r.get("performancedate")), core_time(r.get("lastloadeddate"))
        api = api if api is not None else self.api_data(r)
        return {
            "setlist_id": r.get("setlistid"), "title": r.get("title"),
            "performance_utc": iso_utc(perf), "performance_date": local_date(perf),
            "loaded_utc": iso_utc(loaded), "service_type": r.get("servicetypetitle"), "service_type_id": r.get("servicetypeid"),
            "songs": count, "pinned": bool(r.get("pinned")), "planning_center_auto_update": bool(r.get("planningcenterautoupdate")),
            "foreign_id": api.get("foreignID") or None, "foreign_type_id": api.get("foreignTypeID") or None,       # 0 = not linked to Planning Center
            "times": [{"start": t.get("startTime"), "end": t.get("endTime")} for t in api.get("times", []) if isinstance(t, dict)],
            "notes": text(r.get("notes"), 500), "comment": text(api.get("comment"), 500),
        }

    def setlists(self, limit=30):
        counts = {r["setlist"]: r["n"] for r in self.query("select ZSETLIST as setlist, count(*) as n from ZSONG group by ZSETLIST")}
        rows = self.query("select * from ZSETLIST order by ZPERFORMANCEDATE desc, Z_PK desc limit ?", limit)
        return [self.setlist_summary(r, counts.get(r["z_pk"], 0)) for r in rows]

    def find_setlist(self, which):
        """which: a Playback setlist id, 'current' (most recently opened), 'latest' (newest performance date) or YYYY-MM-DD."""
        if which == "current":
            rows = self.query("select * from ZSETLIST order by ZLASTLOADEDDATE desc, Z_PK desc limit 1")
        elif which == "latest":
            rows = self.query("select * from ZSETLIST order by ZPERFORMANCEDATE desc, Z_PK desc limit 1")
        elif re.match(r"^\d{4}-\d\d-\d\d$", which):
            rows = [r for r in self.query("select * from ZSETLIST order by ZPERFORMANCEDATE desc") if local_date(core_time(r.get("performancedate"))) == which][:1]
        elif which.isdigit():
            rows = self.query("select * from ZSETLIST where ZSETLISTID = ? order by Z_PK desc limit 1", int(which))
        else:
            rows = []
        return rows[0] if rows else None

    def song_rows(self, where, *args):
        sql = ("select s.*, a.ZTITLE as arr_title, " + SECTIONS_TOTAL_SQL + " as arr_total "
               "from ZSONG s left join ZSONGARRANGEMENT a on a.ZID = s.ZWORKINGCOPYARRANGEMENTID and a.ZSONG = s.Z_PK "
               + where)
        return self.query(sql, *args)

    def song_summary(self, s, api_item=None):
        api_item = api_item or {}
        return {
            "setlist_song_id": s.get("setlistsongid"), "order": s.get("order"), "title": s.get("title"), "artist": s.get("artistname"),
            "album": api_item.get("album"), "key": self.key_info(s.get("keyid")), "key_original": self.key_info(s.get("keyidoriginal")),
            "bpm": r3(s.get("bpm")), "bpm_original": r3(s.get("bpmoriginal")),
            "length_s": r3(s.get("arr_total")), "arrangement": s.get("arr_title"),
            "planning_center_song_id": api_item.get("pcSongID") or None, "content_id": s.get("contentid"), "content_type_id": s.get("contenttypeid"),
            "rental_expires_utc": iso_utc(core_time(s.get("rentalexpirationdate"))), "free_rental": bool(s.get("freerental")), "unowned": bool(s.get("unowned")),
            "content_status": s.get("storedcontentstatus"), "artwork": self.artwork_info(s),
        }

    def artwork_info(self, s):
        """Where this song's cover picture is served from, or None when Playback holds none (or something that is not a picture)."""
        data = s.get("artworkdata")
        kind = image_type(data)
        if kind is None or s.get("setlistsongid") is None:
            return None
        return {"path": "/songs/%s/artwork" % s["setlistsongid"], "type": kind, "bytes": len(data), "etag": Blob(kind, data).etag}

    def artwork(self, sid):
        """The cover picture of one song as a Blob; None when the song is unknown or holds no picture."""
        s = self.song_row(sid)
        kind = image_type(s.get("artworkdata")) if s else None
        return Blob(kind, s["artworkdata"]) if kind else None

    def setlist(self, which):
        r = self.find_setlist(which)
        if r is None:
            return None
        api = self.api_data(r)
        items = {i.get("setlistSongID"): i for i in api.get("items", []) if isinstance(i, dict)}
        songs = self.song_rows("where s.ZSETLIST = ? order by s.ZORDER", r["z_pk"])
        out = self.setlist_summary(r, len(songs), api)
        out["items"] = []
        for s in songs:
            item = self.song_summary(s, items.get(s.get("setlistsongid")))
            played = self.played(s)
            item["scan_length_s"] = self.scan_length(s, played[2], played[3]) if played else None
            out["items"].append(item)
        return out

    def library(self, q=None, limit=500):
        rows = self.song_rows("order by s.ZTITLE collate nocase")
        out, seen = [], set()
        for s in rows:
            key = (s.get("contenttypeid"), s.get("contentid"))
            if key in seen:
                continue
            seen.add(key)
            if q and q.lower() not in (s.get("title") or "").lower():
                continue
            out.append({"title": s.get("title"), "artist": s.get("artistname"), "content_id": s.get("contentid"),
                        "key": self.key_info(s.get("keyid"))["name"], "bpm": r3(s.get("bpm")),
                        "setlist_song_id": s.get("setlistsongid"), "length_s": r3(s.get("arr_total"))})
            if len(out) >= limit:
                break
        return out

    # -- one song
    def song_row(self, sid):
        rows = self.song_rows("left join ZSETLIST l on l.Z_PK = s.ZSETLIST where s.ZSETLISTSONGID = ? order by l.ZPERFORMANCEDATE desc, s.Z_PK desc", sid)
        return rows[0] if rows else None

    def arrangement(self, s):
        rows = self.query("select * from ZSONGARRANGEMENT where ZID = ? and ZSONG = ?", s.get("workingcopyarrangementid"), s["z_pk"])
        if not rows:
            rows = self.query("select * from ZSONGARRANGEMENT where ZID = ?", s.get("workingcopyarrangementid"))
        return rows[0] if rows else None

    def sections(self, arr):
        out = []
        for i, x in enumerate(self.query("select * from ZSONGSECTION where ZARRANGEMENT = ? order by Z_FOK_ARRANGEMENT", arr["z_pk"])):
            t = x.get("storedsectiontype")
            out.append({"order": i, "type_id": t, "name": self.section_name(t),
                        "edited_type_id": x.get("editedsectiontypeid"), "edited_name": self.section_name(x.get("editedsectiontypeid")),
                        "orig_start": x.get("starttimeoriginal"), "orig_end": x.get("endtimeoriginal"),
                        "trim_start": x.get("storedstarttimetrimoffset"), "trim_end": x.get("storedendtimetrimoffset"),
                        "loops": x.get("loopcount"), "loop_infinite": bool(x.get("loopinfinite")),
                        "playback_section_id": x.get("storedsetlistsongsectionid"), "content_section_id": x.get("contentsectionid"),
                        "original": bool(x.get("isoriginalsection")), "count_in": x.get("storedplaycountin"), "guide_cue": x.get("storedplayguidecue"),
                        "_pk": x["z_pk"]})
        return out

    def played(self, s):
        """(arrangement row, sections, segments, total length) of a song, or None when it has no arrangement."""
        arr = self.arrangement(s)
        if arr is None:
            return None
        secs = self.sections(arr)
        segs, total = performed_segments(secs)
        return arr, secs, segs, total

    def scan_length(self, s, segs, total):
        """What StagePilot's quick scan reports for a song (seek far past the end, read where Playback stops): the start of
        the song's last bar on the played clock. It matched Playback to the millisecond on 6 of 6 songs (2026-11-16)."""
        rows = self.query("select ZSTOREDACCENTS from ZSONGCLICKS where ZSONG = ?", s["z_pk"])
        if not rows:
            return None
        bars = sorted(p for t in keyed_floats(rows[0].get("storedaccents")) for p in to_performed(segs, t))
        below = [b for b in bars if b < total - 0.01]
        return r3(below[-1]) if below else None

    def song_folder(self, s):
        base = self.songs_dir()
        try:
            names = os.listdir(base)
        except OSError:
            return None
        kind, cid = s.get("contenttypeid"), s.get("contentid")
        wants = ["%s-%s-" % (kind, cid)]
        if kind == FOLDER_PAIRED_FROM:
            wants.append("%s-%s-" % (FOLDER_PAIRED_TO, cid))     # no exact folder: Playback may keep this song under the other number
        for want in wants:                                         # the exact folder always wins; the pairing is only a fallback
            hits = sorted(n for n in names if n.startswith(want))
            if hits:
                return os.path.join(base, hits[0])
        return None

    def song(self, sid):
        s = self.song_row(sid)
        if s is None:
            return None
        arr = self.arrangement(s)
        api_item = {}
        sl = self.query("select * from ZSETLIST where Z_PK = ?", s.get("setlist"))
        setlist_info = None
        if sl:
            api = self.api_data(sl[0])
            api_item = next((i for i in api.get("items", []) if i.get("setlistSongID") == sid), {})
            setlist_info = {"setlist_id": sl[0].get("setlistid"), "title": sl[0].get("title"), "performance_date": local_date(core_time(sl[0].get("performancedate")))}
        out = self.song_summary(s, api_item)
        out["setlist"] = setlist_info
        out["artwork_url"] = s.get("artworkurlstring")
        out["tempo"] = {"bpm": r3(s.get("bpm")), "bpm_original": r3(s.get("bpmoriginal")), "bpm_for_shift": r3(s.get("bpmforshift")),
                        "key_for_shift": self.key_info(s.get("keyidforshift")), "key_changes": s.get("keychanges"),
                        "measures": s.get("clicksongmeasurecount")}
        out["status"] = {"status": s.get("status"), "content_status": s.get("storedcontentstatus"), "audio_bytes": s.get("audiofilesbytes"),
                         "downloaded_bytes": s.get("audiofilesbytesdownloaded"), "rental_expires_utc": iso_utc(core_time(s.get("rentalexpirationdate"))),
                         "expires_utc": iso_utc(core_time(s.get("expiration"))), "free_rental": bool(s.get("freerental")), "unowned": bool(s.get("unowned")),
                         "stems_version": s.get("storedstemsversion"), "metadata_version": s.get("metadataversion"),
                         "has_click": bool(s.get("hasbuiltclicklocally"))}
        out["links"] = {"planning_center_song_id": api_item.get("pcSongID") or None,       # 0 in Playback means "not linked"
                        "propresenter_playlist": s.get("midipropresenterplaylist") or None, "propresenter_playlist_item": s.get("midipropresenterplaylistitem") or None,
                        "api_propresenter_playlist": api_item.get("ppPlaylist") or None, "api_propresenter_item": api_item.get("ppPlaylistItem") or None}
        out["api"] = {k: api_item.get(k) for k in ("contentType", "multiTracksType", "libraryID", "artistID", "albumID", "clickID", "bankID", "padID", "arrangementID",
                                                    "arrangementVersion", "version", "stemsVersion", "songStatus", "fileSize", "duration", "displayDuration", "rehearsalMixOnly")
                      if k in api_item}
        folder = self.song_folder(s)
        out["folder"] = os.path.basename(folder) if folder else None
        if arr is None:
            out["arrangement_detail"] = None
            return out
        _arr, secs, segs, total = self.played(s)
        stored = (arr.get("storedendtime") or 0.0) - (arr.get("storedstarttime") or 0.0)
        by_index = {g["index"]: g for g in segs}
        has_loops = any((x["loops"] or 0) > 0 or x["loop_infinite"] for x in secs)
        for x in secs:
            g = by_index.get(x["order"])
            x["performed_start"] = r3(g["perf_start"]) if g else None
            x["performed_end"] = r3(g["perf_end"]) if g else None
            x["length_s"] = r3(g["perf_end"] - g["perf_start"]) if g else 0.0
            x.pop("_pk", None)
        out["arrangement_detail"] = {
            "title": arr.get("title"), "saved_utc": iso_utc(core_time(arr.get("lastsaveddate"))), "length_s": r3(total),
            "stored_start_s": r3(arr.get("storedstarttime")), "stored_end_s": r3(arr.get("storedendtime")),
            "stored_end_gap_s": r3(stored - total),     # not 0 when Playback's stored end time disagrees with the sections (it follows the sections)
            "has_loops": has_loops, "master_volume": r3(arr.get("mastervolumelevel")),
            "end_fade": bool(arr.get("endfade")), "pad": {"include": bool(arr.get("includepad")), "use_default": bool(arr.get("usedefaultpad")),
                                                           "intro_outro_only": bool(arr.get("padintrooutroonly"))},
            "saved_arrangements": s.get("savedarrangementcount"), "sections": secs}
        out["length_s"] = r3(total)
        out["scan_length_s"] = self.scan_length(s, segs, total)
        out["tempo"]["changes"] = []
        for t in self.query("select * from ZSONGTEMPO where ZSONG = ? order by ZSTOREDTIMECODE", s["z_pk"]):
            tc = t.get("storedtimecode")
            p = effective_performed(segs, tc or 0.0)
            out["tempo"]["changes"].append({"original_s": r3(tc), "performed_s": r3(p), "bpm": r3(t.get("storedbpm"))})
        sigs = json_blob(s.get("timesignaturesdata"), [])
        out["time_signatures"] = [{"original_s": r3(t.get("time")), "performed_s": r3(effective_performed(segs, t.get("time") or 0.0)),
                                   "beats_per_bar": t.get("beatsPerBar"), "beat_unit": t.get("beatUnit")} for t in sigs if isinstance(t, dict)]
        out["stems"] = self.stems(arr)
        out["buses"] = [{"bus_id": b.get("id"), "volume": r3(b.get("volume")), "muted": bool(b.get("muted"))}
                        for b in self.query("select * from ZSONGBUS where ZARRANGEMENT = ? order by ZID", arr["z_pk"])]
        out["midi"] = [{"time_s": r3(m.get("sequencetimecode")), "note": m.get("note"), "velocity": m.get("velocity"), "channel": m.get("eventchannel"),
                        "type": m.get("storedtype"), "bus_id": m.get("busid"), "duration_s": r3(m.get("duration")), "description": text(m.get("eventdescription"))}
                       for m in self.query("select * from ZMIDIEVENT where ZSONG = ? order by ZSEQUENCETIMECODE", s["z_pk"])]
        out["cues"] = self.cues(arr, segs)
        out["pad_tracks"] = [{"title": p.get("title"), "volume": r3(p.get("volume")), "muted": bool(p.get("muted")), "pad_content_id": p.get("padcontentid")}
                             for p in self.query("select * from ZSONGPADTRACK where ZARRANGEMENT = ?", arr["z_pk"])]
        return out

    def stems(self, arr):
        out = []
        for t in self.query("select * from ZSONGTRACK where ZARRANGEMENT = ? order by ZORDERBY", arr["z_pk"]):
            meta = self.track_types.get(t.get("partid"), {})
            out.append({"order": t.get("orderby"), "title": t.get("title"), "abbreviation": t.get("storedabbreviation"), "file": t.get("filename"),
                        "muted": bool(t.get("ismuted")), "volume": r3(t.get("volume")), "click_or_guide": bool(t.get("isclickorguide")),
                        "vocal": bool(t.get("isvocal")), "included": bool(t.get("included")), "part_id": t.get("partid"), "stem_id": t.get("stemid"),
                        "bus_id": meta.get("storedbusid")})
        return out

    def cues(self, arr, segs):
        out = []
        sql = ("select c.*, x.ZSTOREDSETLISTSONGSECTIONID as sec_api from ZAUTOMATIONCUE c join ZSONGSECTION x on c.ZSONGSECTION = x.Z_PK "
               "where x.ZARRANGEMENT = ? order by c.ZSTOREDSEQUENCETIMECODE")
        for c in self.query(sql, arr["z_pk"]):
            t = c.get("storedsequencetimecode")
            p = to_performed(segs, t or 0.0)
            out.append({"name": c.get("name"), "original_s": r3(t), "performed_s": r3(p[0]) if p else None, "details": text(c.get("details")),
                        "transition_beats": c.get("transitionbeats"), "playback_section_id": c.get("sec_api")})
        return out

    def beats(self, sid, level, clock):
        s = self.song_row(sid)
        if s is None:
            return None
        rows = self.query("select * from ZSONGCLICKS where ZSONG = ?", s["z_pk"])
        if not rows:
            return {"setlist_song_id": sid, "level": level, "clock": clock, "times": [], "note": "no beat grid stored for this song"}
        times = keyed_floats(rows[0].get("stored" + level))
        arr = self.arrangement(s)
        if clock == "original" or arr is None:
            return {"setlist_song_id": sid, "level": level, "clock": "original", "count": len(times), "times": [r3(t) for t in times]}
        segs, _total = performed_segments(self.sections(arr))
        out = sorted(p for t in times for p in to_performed(segs, t))
        sig = [t for t in json_blob(s.get("timesignaturesdata"), []) if isinstance(t, dict)]
        return {"setlist_song_id": sid, "level": level, "clock": "performed", "count": len(out), "times": [r3(t) for t in out],
                "beats_per_bar": sig[0].get("beatsPerBar") if sig else None, "beat_unit": sig[0].get("beatUnit") if sig else None}

    # -- waveform
    def waveform(self, sid, pps, clock):
        s = self.song_row(sid)
        if s is None:
            return None
        folder = self.song_folder(s)
        if folder is None:
            raise LookupError("this song's folder was not found in Playback's Songs folder")
        files = []
        for n in os.listdir(folder):
            cps = columns_per_second(n)
            if cps:
                files.append((cps, n))
        if not files:
            raise LookupError("no waveform pictures stored for this song")
        pps = max(1.0, min(float(pps), max(c for c, _ in files)))
        fit = sorted(f for f in files if f[0] >= pps) or sorted(files)[-1:]
        cps, name = sorted(fit, key=lambda f: (f[0], 0 if "1x" in f[1] else 1))[0]
        w, h, cols = self.ink(os.path.join(folder, name))
        arr = self.arrangement(s)
        used, total = "original", None
        if clock == "performed" and arr is not None:
            segs, total = performed_segments(self.sections(arr))
            cols = b"".join(bytes(cols[int(round(g["start"] * cps)):int(round(g["end"] * cps))]) for g in segs)
            used = "performed"
        peaks, means = bucket(list(cols), cps / pps)
        step = len(cols) / cps / len(peaks) if peaks else None
        return {"setlist_song_id": sid, "title": s.get("title"), "clock": used, "source": name, "columns_per_second": cps, "points_per_second": r3(1.0 / step) if step else None,
                "step_s": r3(step), "length_s": r3(len(cols) / cps), "count": len(peaks), "image_height": h,
                "scale": "share of the picture's height that is drawn, 0 to 1: Playback's own display scale, not calibrated to dB",
                "peak": [round(v / h, 3) for v in peaks], "mean": [round(v / h, 3) for v in means]}

    def ink(self, path):
        """Per-column counts of a waveform picture, cached on disk next to the database copy."""
        st = os.stat(path)
        key = hashlib.sha1(("%s|%d|%d" % (path, st.st_mtime, st.st_size)).encode()).hexdigest()[:20]
        cache = self.data_path("cache", "waveform", key + ".json")
        try:
            with open(cache) as f:
                d = json.load(f)
            return d["w"], d["h"], base64.b64decode(d["cols"])
        except (OSError, ValueError, KeyError):
            pass
        w, h, cols = png_ink_columns(path)
        cols = bytes(cols)
        os.makedirs(os.path.dirname(cache), exist_ok=True)
        with open(cache + ".tmp", "w") as f:
            json.dump({"w": w, "h": h, "cols": base64.b64encode(cols).decode(), "source": os.path.basename(path)}, f)
        os.replace(cache + ".tmp", cache)
        return w, h, cols

    # -- outputs (how the stems are routed to the sound card)
    def outputs(self):
        devices = [{"id": d["z_pk"], "name": d.get("name"), "channels": d.get("channelcount")} for d in self.query("select * from ZAUDIODEVICE")]
        chan = {c["z_pk"]: c for c in self.query("select * from ZOUTPUTCHANNEL")}
        buses = []
        for b in self.query("select * from ZGLOBALBUS order by ZAUDIODEVICE, ZORDER"):
            c = chan.get(b.get("outputchannel"), {})
            buses.append({"device_id": b.get("audiodevice"), "type": b.get("type"), "order": b.get("order"), "volume": r3(b.get("volume")),
                          "muted": bool(b.get("ismuted")), "output_channel": c.get("name"), "output_number": (c.get("number") + 1) if c.get("number") is not None else None})
        return {"devices": devices, "buses": buses}

    # -- history of changes to this week's songs
    def history_file(self):
        return self.data_path("history.jsonl")

    def load_history(self):
        last = {}
        try:
            with open(self.history_file()) as f:
                for line in f:
                    try:
                        r = json.loads(line)
                    except ValueError:
                        continue
                    last[(r.get("setlist_id"), r.get("setlist_song_id"))] = r.get("digest")
        except OSError:
            pass
        self.history_last = last

    def record_history(self):
        if not CFG["history"]:
            return 0
        written, now = 0, time.time()
        for sl in self.query("select * from ZSETLIST"):
            perf = core_time(sl.get("performancedate"))
            if perf is None or abs(now - perf) > CFG["history_days"] * 86400:
                continue
            for s in self.song_rows("where s.ZSETLIST = ? order by s.ZORDER", sl["z_pk"]):
                arr = self.arrangement(s)
                secs = [[x["type_id"], x["orig_start"], x["orig_end"], x["trim_start"], x["trim_end"], x["loops"]] for x in self.sections(arr)] if arr else []
                state = {"key": self.key_info(s.get("keyid"))["name"], "key_original": self.key_info(s.get("keyidoriginal"))["name"], "bpm": r3(s.get("bpm")),
                         "arrangement": arr.get("title") if arr else None, "stored_end_s": r3(arr.get("storedendtime")) if arr else None,
                         "length_s": r3(performed_segments(self.sections(arr))[1]) if arr else None, "sections": secs}
                digest = hashlib.sha1(json.dumps(state, sort_keys=True).encode()).hexdigest()[:16]
                ident = (sl.get("setlistid"), s.get("setlistsongid"))
                if self.history_last.get(ident) == digest:
                    continue
                rec = {"t": iso_utc(now), "setlist_id": ident[0], "setlist_title": sl.get("title"), "performance_date": local_date(perf),
                       "setlist_song_id": ident[1], "title": s.get("title"), "digest": digest}
                rec.update(state)
                os.makedirs(CFG["data_dir"], exist_ok=True)
                with open(self.history_file(), "a") as f:
                    f.write(json.dumps(rec, separators=(",", ":")) + "\n")
                self.history_last[ident] = digest
                written += 1
        return written

    def history(self, setlist_id=None, song_id=None):
        out = []
        try:
            with open(self.history_file()) as f:
                for line in f:
                    try:
                        r = json.loads(line)
                    except ValueError:
                        continue
                    if setlist_id is not None and r.get("setlist_id") != setlist_id:
                        continue
                    if song_id is not None and r.get("setlist_song_id") != song_id:
                        continue
                    out.append(r)
        except OSError:
            pass
        return out

    # -- background work
    def prefetch(self):
        """Decode the waveform pictures of the newest setlists once, so the first request is quick."""
        if not CFG["prefetch_waveforms"]:
            return
        for sl in self.query("select * from ZSETLIST order by ZPERFORMANCEDATE desc limit ?", CFG["prefetch_setlists"]):
            for s in self.song_rows("where s.ZSETLIST = ?", sl["z_pk"]):
                key = (s.get("setlistsongid"), self.generation)
                if key in self.prefetch_done:
                    continue
                self.prefetch_done.add(key)
                try:
                    self.waveform(s.get("setlistsongid"), 32, "performed")
                except (LookupError, ValueError, OSError, zlib.error):
                    pass

    def watch_loop(self):
        self.load_history()
        while True:
            try:
                self.refresh(force=True)
                if self.copy and self.handled != self.generation:
                    self.handled = self.generation
                    n = self.record_history()
                    if n:
                        log.info("history: %d song state(s) changed", n)
                    self.prefetch()
            except Exception:
                log.exception("watch loop")
            time.sleep(CFG["watch_interval_s"])

    # -- health
    def health(self):
        # While macOS holds back the first read (its data-access question is open), a copy can wait a long time for the lock:
        # answer at once with "waiting" so the installer and StagePilot see an agent that is up and only waiting for the answer.
        if not self.lock.acquire(timeout=1.0):
            return dict({"ok": True, "version": __version__, "auth": auth_label(), "database": "waiting",
                         "detail": "waiting for macOS to answer the data-access question, or for a copy of Playback's database to finish"}, **update_health())
        try:
            return self._health()
        finally:
            self.lock.release()

    def _health(self):
        self.refresh()
        body = {"ok": True, "version": __version__, "auth": auth_label(), "database": self.error or ("ok" if self.copy else "not read yet"), "detail": self.detail}
        body.update(update_health())
        if self.copy:
            body["database_updated_utc"] = iso_utc(self.changed_at)
            # an opaque token: it differs whenever Playback's database changed since the last time, so a client can ask /health
            # cheaply and read the library again only when the token is new (compare it for equality, it is not a number to order)
            body["library_revision"] = hashlib.sha1(repr(self.sig).encode()).hexdigest()[:12]
            try:
                body["setlists"] = self.query("select count(*) as n from ZSETLIST")[0]["n"]
                body["songs"] = self.query("select count(*) as n from ZSONG")[0]["n"]
            except Exception as e:
                body["detail"] = str(e)
        try:
            body["playback_running"] = subprocess.run(["/usr/bin/pgrep", "-x", CFG["process_name"]], capture_output=True, timeout=3).returncode == 0
        except Exception:
            body["playback_running"] = None
        body["songs_folder"] = os.path.isdir(self.songs_dir())
        return body


def update_health():
    """What /health says about self-updates: that this helper can do it, and why the last one did not stay (when it did not)."""
    body = {"updater": UPDATER_LEVEL}
    error = last_update_error(APP_DIR)
    if error:
        body["last_update_error"] = error
    return body


LIB = Library()


# ---------------------------------------------------------------- key authentication
def auth_mode():
    """'none' (no key set: the 0.2.0 behaviour), 'key' (requests must be signed) or 'locked' (a key is set but unusable:
    every request but the bare /health is refused)."""
    key = CFG.get("key")
    if key is None or key == "":
        return "none"
    return "key" if isinstance(key, str) and len(key) >= MIN_KEY_LENGTH else "locked"


def auth_label():
    """What /health says about authentication."""
    return {"none": "none", "key": "key", "locked": "key_too_short"}[auth_mode()]


def sign(key, target, stamp, method="GET", body=None):
    """The signature: hex HMAC-SHA256, keyed with the key, of "GET\\n" + path-with-query + "\\n" + time. A request with a body
    (POST /update) signs "POST\\n" + path-with-query + "\\n" + time + "\\n" + the hex SHA-256 of the body, so the body cannot be
    swapped under a good signature."""
    message = "%s\n%s\n%s" % (method, target, stamp)
    if body is not None:
        message += "\n" + hashlib.sha256(body).hexdigest()
    return hmac.new(key.encode("utf-8"), message.encode("utf-8"), hashlib.sha256).hexdigest()


def signature_ok(key, target, stamp, sig, now, method="GET", body=None):
    """True only for a numeric time inside the window and a signature that matches (constant-time compare)."""
    if not isinstance(stamp, str) or not isinstance(sig, str) or not re.fullmatch(r"[0-9]{1,12}", stamp):
        return False
    if abs(now - int(stamp)) > TIME_WINDOW_S:
        return False
    return hmac.compare_digest(sign(key, target, stamp, method, body).encode(), sig.strip().lower().encode("utf-8", "replace"))


class FailureThrottle:
    """Counts bad signatures per client address: FAIL_LIMIT within FAIL_WINDOW_S blocks that address for BLOCK_S.
    The table holds at most FAIL_TABLE_MAX addresses (expired ones go first, then the oldest idle one), so a flood
    of made-up addresses cannot grow it. It also decides which rejections may be written to the log."""

    def __init__(self, clock=None):
        self.clock = clock or (lambda: _now())
        self.lock = threading.Lock()
        self.table = collections.OrderedDict()      # address -> {"times": [...], "until": float, "logged": float}
        self.logged = collections.deque()

    def blocked_for(self, addr):
        """Seconds left of the block on this address (0 when it is not blocked)."""
        now = self.clock()
        with self.lock:
            e = self.table.get(addr)
            if not e or not e["until"]:
                return 0
            left = e["until"] - now
            if 0 < left <= BLOCK_S:
                return left
            e["until"] = 0                          # over (or the clock moved back): let the address try again
            if not e["times"]:
                del self.table[addr]
            return 0

    def failed(self, addr):
        """Note one bad signature. Returns True when it may be written to the log."""
        now = self.clock()
        with self.lock:
            e = self.table.pop(addr, None) or {"times": [], "until": 0, "logged": None}
            self.table[addr] = e                    # newest last
            e["times"] = [t for t in e["times"] if 0 <= now - t < FAIL_WINDOW_S] + [now]
            if len(e["times"]) >= FAIL_LIMIT:
                e["until"], e["times"] = now + BLOCK_S, []
            self._trim(now)
            while self.logged and now - self.logged[0] >= FAIL_WINDOW_S:
                self.logged.popleft()
            recent = e["logged"] is not None and 0 <= now - e["logged"] < LOG_ADDR_EVERY_S
            if recent or len(self.logged) >= LOG_GLOBAL_MAX:
                return False
            e["logged"] = now
            self.logged.append(now)
            return True

    def _trim(self, now):
        if len(self.table) <= FAIL_TABLE_MAX:
            return
        for addr in [a for a, e in self.table.items() if (e["logged"] is None or now - e["logged"] >= FAIL_WINDOW_S) and (not e["until"] or e["until"] <= now)
                     and not any(0 <= now - t < FAIL_WINDOW_S for t in e["times"])]:
            del self.table[addr]
        while len(self.table) > FAIL_TABLE_MAX:     # still full: drop the oldest address that is not blocked, else the oldest
            victim = next((a for a, e in self.table.items() if not e["until"]), next(iter(self.table)))
            del self.table[victim]


THROTTLE = FailureThrottle()


def secure_config_file(path):
    """The key sits in config.json: when the file is readable by others, make it 600 (and say so)."""
    try:
        if os.stat(path).st_mode & 0o077:
            os.chmod(path, 0o600)
            log.warning("%s was readable by other users; set it to mode 600", path)
    except OSError:
        pass


# ---------------------------------------------------------------- HTTP
def _allowed_ips():
    ips = {"127.0.0.1"} | set(CFG["allowed_local_ips"])
    addrs = interface_addrs()
    for n in CFG["allow_interfaces"]:
        if n in addrs:
            ips.add(addrs[n])
    return ips


INDEX = {
    "name": "playback-library", "version": __version__,
    "endpoints": {
        "/health": "can the agent read Playback's library", "/setlists": "setlists, newest first (?limit=)",
        "/setlists/current": "the setlist Playback opened last", "/setlists/latest": "the setlist with the newest performance date",
        "/setlists/{id|YYYY-MM-DD}": "one setlist with its songs (key, BPM, length, links)",
        "/songs/{setlist_song_id}": "everything about one song: key, arrangement, sections, tempo, stems, cues, MIDI, links",
        "/songs/{id}/waveform": "waveform numbers (?points_per_second=4&clock=performed|original)",
        "/songs/{id}/beats": "beat times (?level=accents|quarters|eighths|sixteenths&clock=performed|original)",
        "/songs/{id}/artwork": "the song's cover picture (JPEG or PNG bytes, cacheable by its ETag)",
        "/library": "every song Playback holds (?q=title text)", "/outputs": "sound card and how the stems are routed",
        "/history": "recorded changes of key/arrangement for this week's songs (?setlist=&song=)",
        "POST /update": "install a new version of this script that the StagePilot project signed (docs/api.md, \"Updates\")"}}


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def _json(self, obj, code=200, headers=None):
        b = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Cache-Control", "no-store")
        for k, v in (headers or {}).items():
            self.send_header(k, v)
        self.send_header("Content-Length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def _blob(self, blob):
        if self.headers.get("If-None-Match") == blob.etag:
            self.send_response(304)
            self.send_header("ETag", blob.etag)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        self.send_response(200)
        self.send_header("Content-Type", blob.content_type)
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Cache-Control", "private, max-age=86400")
        self.send_header("ETag", blob.etag)
        self.send_header("Content-Length", str(len(blob.data)))
        self.end_headers()
        self.wfile.write(blob.data)

    def _permitted(self):
        # With a key the network limits are optional extras: they apply only when the config names something.
        limited = auth_mode() == "none" or CFG["allow_interfaces"] or CFG["allowed_local_ips"]
        if limited and self.connection.getsockname()[0] not in _allowed_ips():
            self._json({"error": "not available on this interface"}, 403)
            return False
        clients = CFG["allow_clients"]
        if clients and self.client_address[0] not in clients and self.client_address[0] != "127.0.0.1":
            self._json({"error": "not available to this address"}, 403)
            return False
        return True

    def do_POST(self):
        if self.path.partition("?")[0].rstrip("/") == "/update":
            return self._update()
        self._json({"error": "read-only"}, 405)

    def do_PUT(self):
        self._json({"error": "read-only"}, 405)

    do_DELETE = do_PUT

    def _authorized_update(self, body):
        """Key mode gate for POST /update: the signature must cover the method, the target, the time and the body. True: carry on.
        False: the answer (401 or 429) has been sent. A helper without a usable key takes no updates."""
        addr = self.client_address[0]
        left = THROTTLE.blocked_for(addr)
        if left:
            self._json({"error": "too many bad requests"}, 429, {"Retry-After": str(int(left) + 1), "Connection": "close"})
            return False
        stamp, sig = self.headers.get("X-PBL-Time"), self.headers.get("X-PBL-Sig")
        if auth_mode() == "key" and stamp is not None and sig is not None and signature_ok(CFG["key"], self._target(), stamp, sig, _now(), "POST", body):
            return True
        if THROTTLE.failed(addr):
            log.warning("rejected update request from %s: bad signature", addr)
        self._json({"error": "key required"}, 401, {"Connection": "close"})
        return False

    def _update(self):
        """POST /update: install a new version of this script that the StagePilot project signed. The checks run in this order and
        the first one that fails answers (and nothing has changed): the caller is on the local network; the request is signed with
        the key, over its body too; the signature on SHA256SUMS is the project's; every file matches SHA256SUMS; the files are
        exactly the ones this helper takes; the version is higher than the one that runs; every .py compiles."""
        self.close_connection = True                    # a refused request leaves its body unread, so the connection ends with the answer
        close = {"Connection": "close"}
        if not peer_is_private(self.client_address[0]):
            return self._json({"error": "updates are accepted from the local network only"}, 403, close)
        if not self._permitted():
            return
        try:
            length = int(self.headers.get("Content-Length") or "")
        except ValueError:
            return self._json({"error": "the request needs a Content-Length"}, 411, close)
        if length < 0 or length > UPDATE_MAX_BYTES:
            return self._json({"error": "the update is larger than %d bytes" % UPDATE_MAX_BYTES}, 413, close)
        body = self.rfile.read(length)
        if len(body) != length:
            return self._json({"error": "the request ended early"}, 400, close)
        if not self._authorized_update(body):
            return
        try:
            request = json.loads(body.decode("utf-8"))
        except ValueError:
            return self._json({"error": "the request is not JSON"}, 400, close)
        if not isinstance(request, dict):
            return self._json({"error": "the request is not in the expected shape"}, 400, close)
        if not UPDATE_LOCK.acquire(False):
            return self._json({"error": "another update is being installed"}, 409, close)
        try:
            version, data = check_update(request, UPDATE_FILES, "playback_library.py", __version__)
            failed = read_marker(APP_DIR, "update-failed")
            if failed and failed.get("version") == version:
                raise UpdateRejected("Version %s was installed here before and did not start. Run the setup command on this computer instead." % version, 409)
            apply_update(APP_DIR, data, __version__, version)
        except UpdateRejected as e:
            return self._json({"error": e.message}, e.status, close)
        except OSError:
            log.exception("update could not be written")
            return self._json({"error": "the update could not be written on this computer"}, 500, close)
        finally:
            UPDATE_LOCK.release()
        log.info("update %s -> %s installed; stopping so the new version starts", __version__, version)
        self._json({"updating": True, "from": __version__, "to": version}, 200, close)
        schedule_restart(self.server)

    def _target(self):
        """The request target exactly as the client sent it (path and query), which is what it signed."""
        words = getattr(self, "requestline", "").split()
        return words[1] if len(words) == 3 else self.path

    def _authorized(self, path):
        """Key mode gate. True: carry on with the normal answer. False: the answer (401 or 429) has been sent.
        Only the bare /health may go unsigned; it then answers the minimal body and nothing else."""
        addr = self.client_address[0]
        left = THROTTLE.blocked_for(addr)
        if left:
            self._json({"error": "too many bad requests"}, 429, {"Retry-After": str(int(left) + 1)})
            return False
        mode = auth_mode()
        stamp, sig = self.headers.get("X-PBL-Time"), self.headers.get("X-PBL-Sig")
        if mode == "key" and stamp is not None and sig is not None and signature_ok(CFG["key"], self._target(), stamp, sig, _now()):
            return True
        if path.rstrip("/") == "/health" and (mode == "locked" or (stamp is None and sig is None)):
            self._json({"ok": True, "name": "playback-library", "version": __version__, "auth": "required"})
            return False
        if THROTTLE.failed(addr):
            log.warning("rejected request from %s: bad signature", addr)
        self._json({"error": "key required"}, 401)
        return False

    def do_GET(self):
        if not self._permitted():
            return
        path, _, query = self.path.partition("?")
        if auth_mode() != "none" and not self._authorized(path):
            return
        q = {k: urllib.parse.unquote_plus(v) for k, v in (p.split("=", 1) for p in query.split("&") if "=" in p)}
        try:
            body = route(path.rstrip("/") or "/", q)
        except LookupError as e:
            return self._json({"error": str(e)}, 503)
        except ValueError as e:
            return self._json({"error": str(e)}, 400)
        except Exception:
            log.exception("request %s", self.path)
            return self._json({"error": "internal error, see the agent log"}, 500)
        if body is None:
            return self._json({"error": "not found"}, 404)
        if isinstance(body, Blob):
            return self._blob(body)
        self._json(body)


def _num(q, name, default, lo, hi):
    try:
        v = float(q.get(name, default))
    except ValueError:
        raise ValueError("%s must be a number" % name)
    return max(lo, min(hi, v))


def route(path, q):
    if path in ("/", "/index.json"):
        return INDEX
    if path == "/health":
        return LIB.health()
    if path == "/setlists":
        return {"setlists": LIB.setlists(int(_num(q, "limit", 30, 1, 200)))}
    m = re.match(r"^/setlists/([^/]+)$", path)
    if m:
        return LIB.setlist(urllib.parse.unquote(m.group(1)))
    if path == "/library":
        return {"songs": LIB.library(q.get("q"), int(_num(q, "limit", 500, 1, 2000)))}
    if path == "/outputs":
        return LIB.outputs()
    if path == "/history":
        sid = q.get("song")
        sl = q.get("setlist")
        return {"history": LIB.history(int(sl) if sl and sl.isdigit() else None, int(sid) if sid and sid.isdigit() else None)}
    m = re.match(r"^/songs/(\d+)(?:/(waveform|beats|artwork))?$", path)
    if m:
        sid, what = int(m.group(1)), m.group(2)
        clock = q.get("clock", "performed")
        if clock not in ("performed", "original"):
            raise ValueError("clock must be performed or original")
        if what == "artwork":
            return LIB.artwork(sid)
        if what == "waveform":
            return LIB.waveform(sid, _num(q, "points_per_second", 4, 1, 64), clock)
        if what == "beats":
            level = q.get("level", "accents")
            if level not in SCALE_NAMES:
                raise ValueError("level must be one of %s" % ", ".join(SCALE_NAMES))
            return LIB.beats(sid, level, clock)
        return LIB.song(sid)
    return None


class Server(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True


UPDATE_LOCK = threading.Lock()


def schedule_restart(server):
    """Stop serving half a second after the answer went out; run() then returns and the process exits 0, and the service
    manager (launchd, KeepAlive) starts the new version."""
    threading.Timer(0.5, server.shutdown).start()


# ---------------------------------------------------------------- command line
def load_config(path):
    global CONFIG_PATH
    CONFIG_PATH = path
    CFG.clear()
    CFG.update(DEFAULTS)
    if os.path.exists(path):
        with open(path) as f:
            CFG.update(json.load(f))


def setup_logging():
    os.makedirs(os.path.dirname(CFG["log_file"]), exist_ok=True)
    log.setLevel(logging.INFO)
    h = logging.handlers.RotatingFileHandler(CFG["log_file"], maxBytes=1_000_000, backupCount=3)
    h.setFormatter(logging.Formatter("%(asctime)s %(levelname)s %(message)s"))
    log.addHandler(h)


def run():
    setup_logging()
    shown = {k: v for k, v in CFG.items() if k not in ("log_file", "key")}
    shown["key"] = {"none": "not set", "key": "set", "locked": "set but too short"}[auth_mode()]       # never the key itself
    log.info("playback-library %s starting; config=%s", __version__, json.dumps(shown))
    if auth_mode() == "locked":
        log.error("The key in config.json is not usable (it must be text of at least %d characters). Refusing every request "
                  "except the bare /health until a proper key is installed: run the installer again with --key-file.", MIN_KEY_LENGTH)
    elif auth_mode() == "key":
        secure_config_file(CONFIG_PATH)
    os.makedirs(CFG["data_dir"], exist_ok=True)
    if finish_interrupted_update(APP_DIR):
        log.warning("an update was interrupted; it has been completed")
    threading.Thread(target=LIB.watch_loop, daemon=True).start()
    server = Server((CFG["bind"], CFG["port"]), Handler)
    try:
        server.serve_forever()
    finally:
        server.server_close()


def check():
    h = LIB.health()
    print(json.dumps(h, indent=2))
    return 0 if h["database"] == "ok" else 1


def dump(args):
    what = args[0] if args else "health"
    if what == "health":
        out = LIB.health()
    elif what == "setlists":
        out = route("/setlists", {})
    elif what in ("current", "latest"):
        out = LIB.setlist(what)
    elif what in ("library", "outputs", "history"):
        out = route("/" + what, {})
    elif what in ("song", "waveform", "beats") and len(args) > 1 and args[1].isdigit():
        out = route("/songs/%s%s" % (args[1], "" if what == "song" else "/" + what), {})
    else:
        print(__doc__)
        return 2
    print(json.dumps(out, indent=1))
    return 0


def main(argv):
    load_config(os.environ.get("PLAYBACK_LIBRARY_CONFIG", DEFAULT_CONFIG_PATH))
    cmd = argv[1] if len(argv) > 1 else "run"
    if cmd == "run":
        run()
    elif cmd == "check":
        return check()
    elif cmd == "dump":
        return dump(argv[2:])
    elif cmd in ("-V", "--version", "version"):
        print(__version__)
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
