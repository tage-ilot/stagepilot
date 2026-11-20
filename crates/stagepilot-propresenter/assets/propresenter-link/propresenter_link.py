#!/usr/bin/env python3
"""propresenter-link: the files and the screen of the ProPresenter computer, for StagePilot on another computer (macOS).

ProPresenter keeps its songs as files in ~/Documents/ProPresenter/Libraries/<library>/<song>.pro. This agent lets StagePilot,
running on another computer on your network, look at those files, add a new rearranged copy of a song to the library named
Playback, and (with the Accessibility permission) ask ProPresenter's own Search to import one song. It never edits, renames or
deletes a file, never writes outside Libraries/Playback (a new library folder is the one other thing it can make) and never
sends anything but what the commands below name.

Every request except the bare /health must be signed with the key StagePilot made (X-PPL-Time, X-PPL-Sig: HMAC-SHA256 of
METHOD, path and query, time and, for PUT and POST, the SHA-256 of the body). Without a usable key it answers nothing but the bare
/health. It answers only the one StagePilot computer named at install (--allow-for); loopback only when the installer was told so.

Python 3.9+ standard library only. A copy of the Playback Library Link's design (huntrw6/playback-library) with its own code:
the two helpers are independent.

Commands:
  run                 start the agent (what the LaunchAgent runs)
  check               print what this computer can do (folder, libraries, Accessibility) and exit
  --version           print the version
"""
import base64
import collections
import hashlib
import hmac
import json
import logging
import logging.handlers
import os
import platform
import re
import secrets
import shutil
import subprocess
import sys
import threading
import time
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

__version__ = "0.1.1"

HOME = os.path.expanduser("~")
APP_SUPPORT = os.path.join(HOME, "Library", "Application Support", "ProPresenterLink")
DEFAULT_CONFIG_PATH = os.path.join(APP_SUPPORT, "config.json")
DEFAULT_DATA_FOLDER = os.path.join(HOME, "Documents", "ProPresenter")

DEFAULTS = {
    "port": 8790,
    "bind": "",                      # "" = all interfaces; who may ask is decided by allow_clients and the key
    "key": "",                       # the shared key (32+ characters). Without a usable one only the bare /health answers
    "allow_clients": [],             # the only client addresses that are answered (the StagePilot computer). Empty: nobody
    "allow_loopback": False,         # loopback is refused unless the installer was told otherwise (--allow-loopback)
    "data_folder": "",               # "" = ~/Documents/ProPresenter
    "process_name": "ProPresenter",
    "log_file": os.path.join(HOME, "Library", "Logs", "propresenter-link.log"),
}
CFG = dict(DEFAULTS)
CONFIG_PATH = DEFAULT_CONFIG_PATH
log = logging.getLogger("propresenter-link")
APP_DIR = os.path.dirname(os.path.abspath(__file__))     # where the installer put this file; an update replaces the file here
UPDATE_FILES = ("propresenter_link.py",)                  # the only file an update may carry


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


# ---- key authentication
MIN_KEY_LENGTH = 32
TIME_WINDOW_S = 120          # a signature is good while |now - X-PPL-Time| <= this
FAIL_LIMIT = 10              # this many bad signatures from one address ...
FAIL_WINDOW_S = 60           # ... within this many seconds ...
BLOCK_S = 60                 # ... and that address gets 429 for this long
FAIL_TABLE_MAX = 512         # addresses remembered at once
LOG_ADDR_EVERY_S = 30        # one "rejected request" log line per address per this many seconds
LOG_GLOBAL_MAX = 20          # ... and at most this many lines per FAIL_WINDOW_S in total


def _now():
    return time.time()


def auth_mode():
    """'key' (requests must be signed) or 'locked' (no usable key: every request but the bare /health is refused)."""
    key = CFG.get("key")
    return "key" if isinstance(key, str) and len(key) >= MIN_KEY_LENGTH else "locked"


def sign(key, target, stamp, method="GET", body=None):
    """The signature: hex HMAC-SHA256, keyed with the key, of "GET\\n" + path-with-query + "\\n" + time. A request with a body
    (PUT, POST) signs METHOD + "\\n" + path-with-query + "\\n" + time + "\\n" + the hex SHA-256 of the body, so the body cannot
    be swapped under a good signature."""
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
    The table holds at most FAIL_TABLE_MAX addresses, so a flood of made-up addresses cannot grow it. It also decides
    which rejections may be written to the log."""

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


def client_allowed(address):
    """Who is answered at all: loopback only when the config says so, and any other address only when it is named in
    allow_clients and is on the local network. An IPv4 address written as IPv6 counts as the IPv4 address."""
    import ipaddress
    try:
        ip = ipaddress.ip_address(address.split("%")[0])
    except ValueError:
        return False
    if ip.version == 6 and ip.ipv4_mapped:
        ip = ip.ipv4_mapped
    if ip.is_loopback:
        return bool(CFG.get("allow_loopback"))
    return str(ip) in [str(c) for c in CFG.get("allow_clients") or []] and peer_is_private(str(ip))


# ---- the ProPresenter folder
PLAYBACK_LIBRARY = "Playback"        # the one library this agent may add files to
SONGS_LIBRARY = "Songs"              # ProPresenter's own; never made here, never written here
MAX_FILE_BYTES = 40 * 1024 * 1024
MAX_SUB_MESSAGE = 64 * 1024
IMPORT_WAIT_S = 30
_ENSURE_NAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9 -]{0,63}$")
_SHA256_HEX = re.compile(r"^[0-9a-f]{64}$")


class Refused(Exception):
    """A request that cannot be done. `message` is one plain sentence; `code` a short word; `status` the HTTP code."""

    def __init__(self, status, code, message):
        Exception.__init__(self, message)
        self.status, self.code, self.message = status, code, message


def _plain_component(name, what):
    """One path component, nothing else: no separators, no '..', no hidden names, no control characters."""
    if (not isinstance(name, str) or not name or len(name) > 200 or name in (".", "..") or name.startswith(".")
            or "/" in name or "\\" in name or any(ord(c) < 32 or ord(c) == 127 for c in name)):
        raise Refused(400, "bad_path", "That %s name is not allowed." % what)
    return name


def library_name(name):
    return _plain_component(name, "library")


def song_file_name(name):
    name = _plain_component(name, "file")
    if not name.endswith(".pro") or len(name) <= 4:
        raise Refused(400, "bad_path", "Only .pro files can be used.")
    return name


def import_name(name):
    """The name a song is imported under: ProPresenter cuts a name at the first dot, so it has none."""
    if (not isinstance(name, str) or not name.strip() or len(name) > 120 or "." in name or "/" in name or "\\" in name
            or any(ord(c) < 32 or ord(c) == 127 for c in name)):
        raise Refused(400, "bad_name", "The song name must be 1 to 120 characters, without dots or slashes.")
    return name.strip()


def is_songs(name):
    return name.strip().lower() == SONGS_LIBRARY.lower()


def multitracks_id(path):
    """The MultiTracks song number of a .pro file (Presentation.multi_tracks_licensing.song_identifier: field 21, then field 1),
    found by reading the protobuf wire format only, never the whole file: other fields are skipped by seeking. None for a file
    without one, a file that is not a protobuf, a truncated one and anything else that does not parse."""
    try:
        size = os.path.getsize(path)
        if size > MAX_FILE_BYTES:
            return None
        with open(path, "rb") as f:
            while f.tell() < size:
                tag = _varint(f)
                if tag is None:
                    return None
                field, wire = tag >> 3, tag & 7
                if wire == 0:
                    if _varint(f) is None:
                        return None
                elif wire == 1 or wire == 5:
                    f.seek(8 if wire == 1 else 4, 1)
                elif wire == 2:
                    length = _varint(f)
                    if length is None or f.tell() + length > size:
                        return None
                    if field != 21:
                        f.seek(length, 1)
                        continue
                    if length > MAX_SUB_MESSAGE:
                        return None
                    return _licence_song_id(f.read(length))
                else:
                    return None
                if field == 0:
                    return None
    except (OSError, ValueError):
        return None
    return None


def _varint(f):
    value = shift = 0
    for _ in range(10):
        b = f.read(1)
        if not b:
            return None
        value |= (b[0] & 0x7F) << shift
        if not b[0] & 0x80:
            return value
        shift += 7
    return None


def _licence_song_id(data):
    pos = 0
    while pos < len(data):
        tag, pos = _varint_at(data, pos)
        if tag is None:
            return None
        field, wire = tag >> 3, tag & 7
        if wire == 0:
            value, pos = _varint_at(data, pos)
            if value is None:
                return None
            if field == 1:
                return value if 0 < value < 2 ** 63 else None
        elif wire == 1:
            pos += 8
        elif wire == 5:
            pos += 4
        elif wire == 2:
            length, pos = _varint_at(data, pos)
            if length is None:
                return None
            pos += length
        else:
            return None
    return None


def _varint_at(data, pos):
    value = shift = 0
    for _ in range(10):
        if pos >= len(data):
            return None, pos
        b = data[pos]
        pos += 1
        value |= (b & 0x7F) << shift
        if not b & 0x80:
            return value, pos
        shift += 7
    return None, pos


class Folder:
    """The ProPresenter data folder. Every path is built here from a validated library name and file name and is checked,
    after symlinks are resolved, to lie inside the folder."""

    def __init__(self, root):
        self.root = root
        self.cache = {}                  # (path, size, mtime_ns) -> id or None
        self.cache_lock = threading.Lock()

    def real_root(self):
        return os.path.realpath(self.root)

    def libraries_dir(self):
        path = os.path.join(self.root, "Libraries")
        if not os.path.isdir(path):
            raise Refused(503, "folder_missing", "ProPresenter's folder was not found on this computer (%s). Open ProPresenter once." % self.root)
        return path

    def inside(self, path):
        real, root = os.path.realpath(path), self.real_root()
        if os.path.commonpath([real, root]) != root:
            raise Refused(400, "bad_path", "That path leads outside ProPresenter's folder.")
        return real

    def library_path(self, name):
        path = os.path.join(self.libraries_dir(), library_name(name))
        self.inside(path)
        return path

    def library_names(self):
        base = self.libraries_dir()
        names = []
        for entry in sorted(os.listdir(base)):
            path = os.path.join(base, entry)
            if not entry.startswith(".") and os.path.isdir(path):
                try:
                    self.inside(path)
                except Refused:
                    continue
                names.append(entry)
        return names

    def list_library(self, name):
        path = self.library_path(name)
        if not os.path.isdir(path):
            raise Refused(404, "not_found", "That library is not in ProPresenter's folder.")
        out = []
        for entry in sorted(os.listdir(path)):
            if not entry.endswith(".pro") or entry.startswith("."):
                continue
            try:
                file_path = os.path.join(path, entry)
                self.inside(file_path)
                st = os.stat(file_path)
            except (Refused, OSError):
                continue
            if os.path.isfile(file_path):
                out.append({"name": entry, "size": st.st_size, "mtime": int(st.st_mtime)})
        return out

    def read_file(self, library, file):
        path = os.path.join(self.library_path(library), song_file_name(file))
        self.inside(path)
        if not os.path.isfile(path):
            raise Refused(404, "not_found", "That file is not in the library.")
        if os.path.getsize(path) > MAX_FILE_BYTES:
            raise Refused(413, "too_large", "That file is larger than 40 MB.")
        with open(path, "rb") as f:
            return f.read()

    def ensure_library(self, name):
        """Make the folder Libraries/<name>. True when it was made, False when it was there. Never `Songs`."""
        if not isinstance(name, str) or not _ENSURE_NAME.match(name) or name != name.strip() or is_songs(name):
            raise Refused(400, "bad_name", "A library name can have letters, digits, spaces and dashes, and it cannot be Songs.")
        path = os.path.join(self.libraries_dir(), name)
        self.inside(path)
        if os.path.isdir(path):
            return False
        os.mkdir(path)
        return True

    def create_file(self, library, file, data, sha256):
        """Add a new file to the library named Playback. Create-only: a name that is taken is refused, whatever it holds.
        The bytes go to a temporary file in the same folder, are written out to disk, the hash is checked, and the file is
        put in place with a hard link (it fails when the name exists, so nothing is ever replaced) and the temporary one removed."""
        if library != PLAYBACK_LIBRARY:
            raise Refused(403, "not_allowed", "Files can only be added to the library named %s." % PLAYBACK_LIBRARY)
        file = song_file_name(file)
        if not isinstance(sha256, str) or not _SHA256_HEX.match(sha256):
            raise Refused(400, "bad_hash", "The checksum of the file is missing or not valid.")
        if len(data) > MAX_FILE_BYTES:
            raise Refused(413, "too_large", "That file is larger than 40 MB.")
        folder = self.library_path(library)
        if not os.path.isdir(folder):
            raise Refused(404, "not_found", "The %s library does not exist yet." % PLAYBACK_LIBRARY)
        final = os.path.join(folder, file)
        if os.path.lexists(final):
            raise Refused(409, "exists", "A file with that name is already in the library. Files are never replaced.")
        temp = os.path.join(folder, ".%s.%s.tmp" % (file, secrets.token_hex(6)))
        try:
            fd = os.open(temp, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
            with os.fdopen(fd, "wb") as f:
                f.write(data)
                f.flush()
                os.fsync(f.fileno())
            if _file_sha256(temp) != sha256:
                raise Refused(422, "hash_mismatch", "The file arrived changed (its checksum does not match). Nothing was added.")
            try:
                os.link(temp, final)
            except FileExistsError:
                raise Refused(409, "exists", "A file with that name is already in the library. Files are never replaced.")
        finally:
            try:
                os.remove(temp)
            except OSError:
                pass
        if _file_sha256(final) != sha256:
            raise Refused(500, "hash_mismatch", "The file on disk does not match its checksum.")
        return {"name": file, "size": len(data)}

    def index(self):
        """Every .pro file under Libraries/*, with its MultiTracks number when it has one. One file is read at a time."""
        out = []
        for lib in self.library_names():
            base = os.path.join(self.root, "Libraries", lib)
            for entry in sorted(os.listdir(base)):
                path = os.path.join(base, entry)
                if not entry.endswith(".pro") or entry.startswith(".") or not os.path.isfile(path):
                    continue
                try:
                    self.inside(path)
                    st = os.stat(path)
                except (Refused, OSError):
                    continue
                key = (path, st.st_size, st.st_mtime_ns)
                with self.cache_lock:
                    known = key in self.cache
                    found = self.cache.get(key)
                if not known:
                    found = multitracks_id(path)
                    with self.cache_lock:
                        if len(self.cache) > 20000:
                            self.cache.clear()
                        self.cache[key] = found
                out.append({"library": lib, "file": entry, "multitracks_id": found})
        return out


def _file_sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def data_folder():
    return os.path.expanduser(CFG.get("data_folder") or DEFAULT_DATA_FOLDER)


FOLDER = Folder(data_folder())


def app_version():
    """ProPresenter's own version, from its Info.plist, when it can be found. None otherwise."""
    import plistlib
    best = None
    try:
        for entry in sorted(os.listdir("/Applications")):
            if entry.startswith("ProPresenter") and entry.endswith(".app"):
                with open(os.path.join("/Applications", entry, "Contents", "Info.plist"), "rb") as f:
                    best = str(plistlib.load(f).get("CFBundleShortVersionString") or "") or best
    except (OSError, ValueError, plistlib.InvalidFileException):
        pass
    return best


# ---- the screen (macOS Accessibility). The scripts are the files of crates/stagepilot-propresenter/assets/gui/; a test keeps
# these copies equal to them.
# ===== BEGIN screen scripts =====
CHECK_ACCESS_APPLESCRIPT = r'''-- ProPresenter Link: can this program control other programs on screen (macOS Accessibility)?
-- Asks System Events one harmless question and changes nothing. Prints OK, or ERR|check|<reason>.
on run argv
	try
		tell application "System Events" to set n to count of processes
		return "OK|" & n
	on error msg number code
		return "ERR|check|" & msg
	end try
end run
'''
IMPORT_SONG_APPLESCRIPT = r'''-- ProPresenter Link: import ONE song through ProPresenter's own Search (MultiTracks source) on screen.
-- Arguments: 1 the song's MultiTracks number (digits only), 2 the name to give the song (no dots), 3 the playlist
-- to add it to ("" for none). The values are only ever typed into ProPresenter's own fields.
-- It touches only: the Search button, the Search panel (its MultiTracks choice, its text field, its one result row,
-- its close button) and the Import dialog (Name, Library read only, Playlist, Import, Cancel). Nothing else.
-- At any unexpected thing it stops, presses Cancel on an Import dialog if one is open, closes the Search panel
-- and prints ERR|<step>|<reason>. On success it prints OK|<name>.pro once the Import dialog has gone.
-- Nothing is typed blind: before EVERY keystroke the script checks that ProPresenter is the front app and that the
-- field meant to receive it has the keyboard (assertFocus); after typing it reads the field back and it must hold exactly
-- what was typed (the song number; the name), the Import dialog's other text fields must still hold what they held before
-- and its Library must still read Songs. Any mismatch stops with ERR (Cancel is pressed if the Import dialog is open,
-- the Search panel is closed) and nothing more is typed or pressed. At most 40 characters of what a field held are printed.

on nameOf(e)
	set t to ""
	repeat with a in {"AXTitle", "AXDescription", "AXLabel", "AXValue"}
		try
			set v to (value of attribute (a as text) of e) as text
			if v is not "" and v is not "missing value" then set t to t & "|" & v
		end try
	end repeat
	return t
end nameOf

-- The text of a field (its value), "" when it has none.
on textOf(e)
	try
		tell application "System Events" to set v to value of e
		if v is missing value then return ""
		return v as text
	on error
		return ""
	end try
end textOf

-- Exactly the same text: letters in the same case (AppleScript compares without case unless told to).
on sameText(a, b)
	considering case
		return ((a as text) is equal to (b as text))
	end considering
end sameText

-- At most 40 characters, on one line, safe to print: control characters become ? and | becomes /.
on shortText(v)
	set t to v as text
	set out to ""
	set n to count of characters of t
	if n > 40 then set n to 40
	repeat with i from 1 to n
		set c to character i of t
		set charCode to id of c
		if charCode < 32 or charCode is 127 then
			set out to out & "?"
		else if c is "|" then
			set out to out & "/"
		else
			set out to out & c
		end if
	end repeat
	if (count of characters of t) > 40 then set out to out & "..."
	return out
end shortText

-- Before every keystroke: ProPresenter is the front app and `target` is the element that has the keyboard. Else an error,
-- so nothing is typed anywhere.
on assertFocus(procName, target, what)
	set isFront to false
	set hasFocus to false
	tell application "System Events"
		try
			set isFront to (frontmost of process procName)
		end try
		try
			set hasFocus to (focused of target)
		end try
	end tell
	if isFront is not true then error "ProPresenter is not the front app, so nothing was typed."
	if hasFocus is not true then error "The " & what & " does not have the keyboard, so nothing was typed."
end assertFocus

-- After typing the name: the name field holds exactly the name, the other text fields hold what they held before, and the
-- Library still reads Songs.
on verifyDialog(textFields, heldBefore, nameIndex, songName, libraryPopup)
	set held to my textOf(item nameIndex of textFields)
	if not my sameText(held, songName) then error "The Name field held \"" & my shortText(held) & "\" instead of the name. Nothing was imported."
	repeat with i from 1 to (count of textFields)
		if i is not nameIndex then
			set nowHeld to my textOf(item i of textFields)
			if not my sameText(nowHeld, item i of heldBefore) then error "Another field of the Import dialog changed to \"" & my shortText(nowHeld) & "\". Nothing was imported."
		end if
	end repeat
	if not my sameText(my textOf(libraryPopup), "Songs") then error "The Import dialog's Library no longer reads Songs. Nothing was imported."
end verifyDialog

on findWindow(procName, wantedSubrole)
	tell application "System Events"
		tell process procName
			repeat with w in windows
				try
					if subrole of w is wantedSubrole then return w
				end try
			end repeat
		end tell
	end tell
	return missing value
end findWindow

on waitForWindow(procName, wantedSubrole, seconds)
	repeat (seconds * 4) times
		set w to my findWindow(procName, wantedSubrole)
		if w is not missing value then return w
		delay 0.25
	end repeat
	return missing value
end waitForWindow

on resultTable(fw)
	tell application "System Events"
		try
			return table 1 of scroll area 1 of group 3 of group 1 of fw
		end try
		repeat with e in (entire contents of fw)
			try
				if role of e is "AXTable" then return e
			end try
		end repeat
	end tell
	return missing value
end resultTable

on closeEverything(procName)
	tell application "System Events"
		tell process procName
			set dlg to my findWindow(procName, "AXDialog")
			if dlg is not missing value then
				try
					click button "Cancel" of dlg
					delay 0.5
				end try
			end if
			set fw to my findWindow(procName, "AXFloatingWindow")
			if fw is not missing value then
				try
					repeat with b in (buttons of fw)
						if subrole of b is "AXCloseButton" then
							click b
							exit repeat
						end if
					end repeat
				end try
			end if
		end tell
	end tell
end closeEverything

on run argv
	set procName to "ProPresenter"
	set stepName to "start"
	try
		if (count of argv) is not 3 then error "The script needs the song number, the name and the playlist."
		set songId to item 1 of argv
		set songName to item 2 of argv
		set playlistName to item 3 of argv
		if songId is "" or songId contains " " then error "The song number is not valid."
		repeat with ch in (characters of songId)
			if "0123456789" does not contain (ch as text) then error "The song number is not valid."
		end repeat
		if songName is "" or songName contains "." then error "The song name is not valid."

		set stepName to "find ProPresenter"
		tell application "System Events"
			if not (exists process procName) then error "ProPresenter is not running."
		end tell

		set stepName to "open the Search panel"
		tell application "System Events"
			tell process procName
				set clicked to false
				try
					click button "Search" of toolbar 1 of window "ProPresenter"
					set clicked to true
				end try
				if not clicked then
					repeat with e in (entire contents of window "ProPresenter")
						try
							if role of e is "AXButton" and ((title of e as text) is "Search" or (description of e as text) is "Search") then
								click e
								set clicked to true
								exit repeat
							end if
						end try
					end repeat
				end if
				if not clicked then error "The Search button was not found."
			end tell
		end tell
		set fw to my waitForWindow(procName, "AXFloatingWindow", 6)
		if fw is missing value then error "The Search panel did not open."

		set stepName to "choose the MultiTracks source"
		tell application "System Events"
			tell process procName
				set chosen to false
				repeat with e in (entire contents of fw)
					try
						set r to role of e
						set n to my nameOf(e)
						if n contains "MultiTracks" then
							if r is "AXRadioButton" or r is "AXButton" or r is "AXMenuItem" or r is "AXTab" then
								click e
								set chosen to true
								exit repeat
							else if r is "AXPopUpButton" then
								if (value of e as text) contains "MultiTracks" then
									set chosen to true
								end if
								exit repeat
							end if
						end if
					end try
				end repeat
				if not chosen then
					repeat with e in (entire contents of fw)
						try
							if role of e is "AXPopUpButton" then
								click e
								delay 0.4
								click (first menu item of menu 1 of e whose name contains "MultiTracks")
								set chosen to true
								exit repeat
							end if
						end try
					end repeat
				end if
				if not chosen then error "The MultiTracks source was not found in the Search panel."
			end tell
		end tell
		delay 0.4

		set stepName to "type the song number"
		tell application "System Events"
			tell process procName
				set field to missing value
				repeat with e in (entire contents of fw)
					try
						if role of e is "AXTextField" or role of e is "AXSearchField" or role of e is "AXComboBox" then
							set field to e
							exit repeat
						end if
					end try
				end repeat
				if field is missing value then error "The Search panel has no text field."
				set focused of field to true
				delay 0.3
				my assertFocus(procName, field, "Search field")
				keystroke "a" using command down
				my assertFocus(procName, field, "Search field")
				keystroke songId
				delay 0.3
				set held to my textOf(field)
				if not my sameText(held, songId) then error "The Search field held \"" & my shortText(held) & "\" instead of the song number. Nothing else was typed or pressed."
			end tell
		end tell

		set stepName to "read the search result"
		set rowCount to 0
		set stable to 0
		repeat 24 times
			delay 0.5
			set t to my resultTable(fw)
			if t is not missing value then
				tell application "System Events" to set n to count of (rows of t)
				if n is rowCount and n > 0 then
					set stable to stable + 1
				else
					set stable to 0
				end if
				set rowCount to n
				if stable >= 3 then exit repeat
			end if
		end repeat
		if rowCount is 0 then error "The search found no song with this number."
		if rowCount is not 1 then error "The search found " & rowCount & " songs for this number, not exactly one."

		set stepName to "open the Import dialog"
		tell application "System Events"
			tell process procName
				select row 1 of (my resultTable(fw))
				delay 0.6
				try
					set focused of (my resultTable(fw)) to true
				end try
				delay 0.2
				my assertFocus(procName, (my resultTable(fw)), "results list")
				key code 36
			end tell
		end tell
		set dlg to my waitForWindow(procName, "AXDialog", 8)
		if dlg is missing value then error "The Import dialog did not open."

		set stepName to "fill in the Import dialog"
		tell application "System Events"
			tell process procName
				set nameField to missing value
				set firstField to missing value
				set libraryPopup to missing value
				set playlistPopup to missing value
				set textFields to {}
				set nameIndex to 0
				repeat with e in (entire contents of dlg)
					try
						set r to role of e
						set n to my nameOf(e)
						if r is "AXTextField" then
							set end of textFields to e
							if firstField is missing value then set firstField to e
							if nameField is missing value and (n contains "|Name") then
								set nameField to e
								set nameIndex to count of textFields
							end if
						else if r is "AXPopUpButton" then
							if libraryPopup is missing value and (n contains "Library") then set libraryPopup to e
							if playlistPopup is missing value and (n contains "Playlist") then set playlistPopup to e
						end if
					end try
				end repeat
				if nameField is missing value then
					set nameField to firstField
					set nameIndex to 1
				end if
				if nameField is missing value then error "The Import dialog has no Name field."
				if libraryPopup is missing value then error "The Import dialog has no Library choice."
				if (value of libraryPopup as text) is not "Songs" then error "The Import dialog's Library is not Songs."
				-- What every text field holds before anything is typed.
				set heldBefore to {}
				repeat with f in textFields
					set end of heldBefore to my textOf(f)
				end repeat
				set focused of nameField to true
				delay 0.3
				my assertFocus(procName, nameField, "Name field")
				keystroke "a" using command down
				my assertFocus(procName, nameField, "Name field")
				keystroke songName
				delay 0.3
				my verifyDialog(textFields, heldBefore, nameIndex, songName, libraryPopup)
				if playlistName is not "" then
					if playlistPopup is missing value then error "The Import dialog has no Playlist choice."
					click playlistPopup
					delay 0.5
					click (first menu item of menu 1 of playlistPopup whose name is playlistName)
					delay 0.4
				else if playlistPopup is not missing value then
					try
						if (value of playlistPopup as text) is not "None" then
							click playlistPopup
							delay 0.5
							click (first menu item of menu 1 of playlistPopup whose name is "None")
							delay 0.4
						end if
					end try
				end if
			end tell
		end tell

		set stepName to "press Import"
		tell application "System Events"
			tell process procName
				-- Once more, after the Playlist choice: what is about to be imported is what was asked for.
				my verifyDialog(textFields, heldBefore, nameIndex, songName, libraryPopup)
				click button "Import" of dlg
			end tell
		end tell

		set stepName to "wait for the import to finish"
		repeat 120 times
			delay 0.25
			if my findWindow(procName, "AXDialog") is missing value then exit repeat
		end repeat
		if my findWindow(procName, "AXDialog") is not missing value then error "The Import dialog stayed open."

		set stepName to "close the Search panel"
		my closeEverything(procName)
		return "OK|" & songName & ".pro"
	on error msg
		try
			my closeEverything(procName)
		end try
		return "ERR|" & stepName & "|" & msg
	end try
end run
'''
SELECT_LIBRARY_APPLESCRIPT = r'''-- ProPresenter Link: find the row of one library in ProPresenter's library list (the left side of its main window)
-- and say where it is on screen. It clicks nothing itself: the program that runs it clicks that spot once with a real
-- mouse event (click.swift), because AppleScript's own `click at` did not land on this list.
-- Argument 1: the library's name. Prints FOUND|x|y (the middle of the row's name), or ERR|<step>|<reason>.
on run argv
	set stepName to "start"
	try
		set target to item 1 of argv
		set stepName to "find ProPresenter"
		tell application "System Events"
			if not (exists process "ProPresenter") then error "ProPresenter is not running."
			tell process "ProPresenter"
				set stepName to "find the library row"
				set w to window "ProPresenter"
				repeat with e in (entire contents of w)
					try
						if role of e is "AXStaticText" and (value of e as text) is target then
							set p to position of e
							set s to size of e
							set cx to (item 1 of p) + (item 1 of s) / 2
							set cy to (item 2 of p) + (item 2 of s) / 2
							return "FOUND|" & (round cx) & "|" & (round cy)
						end if
					end try
				end repeat
			end tell
		end tell
		error "The library " & target & " is not in ProPresenter's list."
	on error msg
		return "ERR|" & stepName & "|" & msg
	end try
end run
'''
CLICK_SWIFT = r'''// ProPresenter Link: one left click at the screen position given as two numbers (x y), the way a mouse does.
// AppleScript's own `click at` did not land on ProPresenter's library list; a real mouse event does.
// Run as `xcrun swift click.swift X Y`. It clicks once and prints "clicked X Y". Nothing else.
import Foundation
import CoreGraphics

let args = CommandLine.arguments
guard args.count == 3, let x = Double(args[1]), let y = Double(args[2]), x >= 0, y >= 0, x < 20000, y < 20000 else {
    print("usage: click.swift X Y")
    exit(2)
}
let point = CGPoint(x: x, y: y)
CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: point, mouseButton: .left)?.post(tap: .cghidEventTap)
usleep(200_000)
for kind in [CGEventType.leftMouseDown, CGEventType.leftMouseUp] {
    CGEvent(mouseEventSource: nil, mouseType: kind, mouseCursorPosition: point, mouseButton: .left)?.post(tap: .cghidEventTap)
    usleep(80_000)
}
print("clicked", x, y)
'''
# ===== END screen scripts =====
SCRIPTS = {"check_access.applescript": CHECK_ACCESS_APPLESCRIPT, "import_song.applescript": IMPORT_SONG_APPLESCRIPT,
           "select_library.applescript": SELECT_LIBRARY_APPLESCRIPT, "click.swift": CLICK_SWIFT}
SCRIPT_DIR = os.path.join(APP_SUPPORT, "scripts")
OSASCRIPT = "/usr/bin/osascript"
SWIFT = ["/usr/bin/xcrun", "swift"]
IMPORT_TIMEOUT_S = 90
NO_MAC = "Importing a song needs a Mac; this computer is not one."
NO_ACCESS = "Allow the ProPresenter Link in System Settings > Privacy & Security > Accessibility, then try again."
ACCESS = {"at": 0.0, "ok": False, "blocker": None}
ACCESS_LOCK = threading.Lock()
IMPORT_LOCK = threading.Lock()


def is_mac():
    return sys.platform == "darwin"


def script_path(name):
    """The file of one bundled script on disk (written from the copy above, only when it differs; private folder)."""
    if name not in SCRIPTS:
        raise Refused(500, "internal", "Unknown script.")
    os.makedirs(SCRIPT_DIR, mode=0o700, exist_ok=True)
    path = os.path.join(SCRIPT_DIR, name)
    data = SCRIPTS[name].encode("utf-8")
    try:
        with open(path, "rb") as f:
            same = f.read() == data
    except OSError:
        same = False
    if not same:
        tmp = path + ".new"
        with open(tmp, "wb") as f:
            f.write(data)
        os.replace(tmp, path)
    return path


def run_script(name, args, timeout):
    """Run one bundled AppleScript with its values as arguments (never built into a command line or a script text).
    Returns its last line of output."""
    try:
        done = subprocess.run([OSASCRIPT, script_path(name)] + [str(a) for a in args], stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              timeout=timeout, universal_newlines=True)
    except subprocess.TimeoutExpired:
        raise Refused(502, "screen", "ProPresenter did not answer on screen in time.")
    except OSError:
        raise Refused(502, "screen", "The screen scripts could not be started on this computer.")
    lines = [line for line in (done.stdout or "").splitlines() if line.strip()]
    if lines:
        return lines[-1].strip()
    err = (done.stderr or "").strip()
    if "assistive" in err or "not allowed" in err or "-25211" in err or "-1719" in err:
        return "ERR|check|" + NO_ACCESS
    return "ERR|run|The screen script stopped without an answer."


def accessibility():
    """(can drive the screen, why not). Asked of macOS at most every 30 seconds."""
    if not is_mac():
        return False, NO_MAC
    with ACCESS_LOCK:
        now = time.time()
        if now - ACCESS["at"] > 30:
            try:
                line = run_script("check_access.applescript", [], 10)
            except Refused as e:
                line = "ERR|check|" + e.message
            ACCESS.update(at=now, ok=line.startswith("OK"), blocker=None if line.startswith("OK") else NO_ACCESS)
        return ACCESS["ok"], ACCESS["blocker"]


def search_import(song_id, name, playlist):
    """Import one song through ProPresenter's own Search, into the library Songs. Returns the new file's name."""
    if not isinstance(song_id, str) or not re.fullmatch(r"[0-9]{1,18}", song_id):
        raise Refused(400, "bad_id", "The song number must be digits only.")
    name = import_name(name)
    if playlist is None:
        playlist = ""
    if not isinstance(playlist, str) or len(playlist) > 120 or any(ord(c) < 32 or ord(c) == 127 for c in playlist):
        raise Refused(400, "bad_name", "The playlist name is not valid.")
    ok, blocker = accessibility()
    if not ok:
        raise Refused(409, "cannot_import", blocker)
    songs = FOLDER.library_path(SONGS_LIBRARY)
    if not os.path.isdir(songs):
        raise Refused(404, "not_found", "ProPresenter's Songs library was not found.")
    target = os.path.join(songs, name + ".pro")
    if os.path.lexists(target):
        raise Refused(409, "exists", "A song with that name is already in the Songs library.")
    if not IMPORT_LOCK.acquire(False):
        raise Refused(409, "busy", "A song is being imported right now.")
    try:
        line = run_script("import_song.applescript", [song_id, name, playlist], IMPORT_TIMEOUT_S)
        if line.startswith("ERR|"):
            _, step, message = (line.split("|", 2) + ["", ""])[:3]
            raise Refused(502, "screen", "The import stopped at the step \"%s\": %s" % (step, message))
        if not line.startswith("OK|"):
            raise Refused(502, "screen", "The screen script gave an answer that was not understood.")
        deadline = time.time() + IMPORT_WAIT_S
        while time.time() < deadline:
            if os.path.isfile(target) and os.path.getsize(target) > 0:
                time.sleep(0.5)                         # let ProPresenter finish writing it
                return {"file": name + ".pro"}
            time.sleep(0.25)
        raise Refused(504, "timeout", "ProPresenter did not write the song within %d seconds." % IMPORT_WAIT_S)
    finally:
        IMPORT_LOCK.release()


def select_library(name):
    """Best effort: click the library's row in ProPresenter's list so that it loads the files added there."""
    library_name(name)
    ok, why = accessibility()
    if not ok:
        return {"tried": False, "detail": why}
    try:
        line = run_script("select_library.applescript", [name], 30)
        parts = line.split("|")
        if parts[0] != "FOUND" or len(parts) != 3 or not (parts[1].lstrip("-").isdigit() and parts[2].lstrip("-").isdigit()):
            return {"tried": False, "detail": "The library row was not found on screen."}
        done = subprocess.run(SWIFT + [script_path("click.swift"), parts[1], parts[2]], stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              timeout=90, universal_newlines=True)
        return {"tried": done.returncode == 0, "detail": "" if done.returncode == 0 else "The click could not be made."}
    except (Refused, subprocess.TimeoutExpired, OSError):
        return {"tried": False, "detail": "The library row could not be clicked."}


def health():
    mac = is_mac()
    ok, blocker = accessibility() if mac else (False, NO_MAC)
    exists = os.path.isdir(os.path.join(FOLDER.root, "Libraries"))
    try:
        libs = FOLDER.library_names() if exists else []
    except (Refused, OSError):
        libs = []
    body = {"ok": True, "name": "propresenter-link", "version": __version__, "auth": "key", "os": "macOS" if mac else platform.system(),
            "data_folder": FOLDER.root, "data_folder_exists": exists, "libraries": libs, "playback_library_exists": PLAYBACK_LIBRARY in libs,
            "can_import": ok, "propresenter_version": app_version() if mac else None}
    if blocker:
        body["import_blocker"] = blocker
    body.update(update_health())
    return body


def update_health():
    """What /health says about self-updates: that this helper can do it, and why the last one did not stay (when it did not)."""
    body = {"updater": UPDATER_LEVEL}
    error = last_update_error(APP_DIR)
    if error:
        body["last_update_error"] = error
    return body


# ---------------------------------------------------------------- HTTP
INDEX = {
    "name": "propresenter-link", "version": __version__,
    "endpoints": {
        "/health": "the version, the folder, the libraries, whether a song can be imported on screen",
        "/libraries/{name}/files": "the .pro files of one library (name, size, modified time)",
        "/libraries/{name}/files/{file}": "one .pro file's bytes (at most 40 MB)",
        "/index": "every .pro file with its MultiTracks number, when it has one",
        "PUT /libraries/Playback/files/{file}": "add a new file to the library Playback (create-only; header X-Sha256)",
        "POST /libraries/{name}": "make a library folder when it is not there",
        "POST /import": "import one song through ProPresenter's Search {id, name, playlist}",
        "POST /select-library": "select a library row on screen {name}",
        "POST /update": "install a new version of this script that the StagePilot project signed"}}
SMALL_BODY = 64 * 1024


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def _send(self, code, data, content_type, headers=None):
        self.send_response(code)
        self.send_header("Content-Type", content_type)
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        for k, v in (headers or {}).items():
            self.send_header(k, v)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(data)

    def _json(self, obj, code=200, headers=None):
        self._send(code, json.dumps(obj).encode(), "application/json", headers)

    def _target(self):
        """The request target exactly as the client sent it (path and query), which is what it signed."""
        words = getattr(self, "requestline", "").split()
        return words[1] if len(words) == 3 else self.path

    def _read_body(self, limit):
        """The request body, or None when the answer (411, 413 or 400) has been sent."""
        self.close_connection = True                       # a refused request leaves its body unread
        close = {"Connection": "close"}
        try:
            length = int(self.headers.get("Content-Length") or "")
        except ValueError:
            self._json({"error": "The request needs a Content-Length.", "code": "length_required"}, 411, close)
            return None
        if length < 0 or length > limit:
            self._json({"error": "The request is larger than %d bytes." % limit, "code": "too_large"}, 413, close)
            return None
        data = self.rfile.read(length)
        if len(data) != length:
            self._json({"error": "The request ended early.", "code": "bad_request"}, 400, close)
            return None
        self.close_connection = False
        return data

    def _authorize(self, method, body):
        """The key gate: the signature covers the method, the target, the time and (PUT, POST) the body. True: carry on.
        False: the answer (401 or 429) has been sent."""
        addr = self.client_address[0]
        left = THROTTLE.blocked_for(addr)
        if left:
            self._json({"error": "too many bad requests"}, 429, {"Retry-After": str(int(left) + 1), "Connection": "close"})
            return False
        stamp, sig = self.headers.get("X-PPL-Time"), self.headers.get("X-PPL-Sig")
        if auth_mode() == "key" and stamp is not None and sig is not None and signature_ok(CFG["key"], self._target(), stamp, sig, _now(), method, body):
            return True
        if THROTTLE.failed(addr):
            log.warning("rejected request from %s: bad signature", addr)
        self._json({"error": "key required"}, 401, {"Connection": "close"})
        return False

    def _handle(self, method):
        addr = self.client_address[0]
        if not client_allowed(addr):
            self.close_connection = True
            return self._json({"error": "not available to this address"}, 403, {"Connection": "close"})
        path, _, query = self.path.partition("?")
        path = path.rstrip("/") or "/"
        left = THROTTLE.blocked_for(addr)
        if left:
            self.close_connection = True
            return self._json({"error": "too many bad requests"}, 429, {"Retry-After": str(int(left) + 1), "Connection": "close"})
        body = None
        if method in ("PUT", "POST"):
            limit = UPDATE_MAX_BYTES if path == "/update" else (MAX_FILE_BYTES if method == "PUT" else SMALL_BODY)
            if path == "/update" and (method != "POST" or not peer_is_private(addr)):
                self.close_connection = True
                return self._json({"error": "updates are accepted from the local network only"}, 403, {"Connection": "close"})
            body = self._read_body(limit)
            if body is None:
                return
        if method == "GET" and path == "/health" and self.headers.get("X-PPL-Time") is None and self.headers.get("X-PPL-Sig") is None:
            return self._json({"ok": True, "name": "propresenter-link", "version": __version__, "auth": "required"})
        if not self._authorize(method, body):
            return
        try:
            result = route(method, path, query, self.headers, body, self.server)
        except Refused as e:
            return self._json({"error": e.message, "code": e.code}, e.status)
        except UpdateRejected as e:
            return self._json({"error": e.message, "code": "update_rejected"}, e.status, {"Connection": "close"})
        except OSError:
            log.exception("request %s", method)
            return self._json({"error": "The folder could not be read or written on this computer.", "code": "io"}, 500)
        except Exception:
            log.exception("request %s", method)
            return self._json({"error": "internal error, see the agent log", "code": "internal"}, 500)
        if result is None:
            return self._json({"error": "not found", "code": "not_found"}, 404)
        if isinstance(result, tuple):                       # (bytes, headers) for a file
            return self._send(200, result[0], "application/octet-stream", result[1])
        self._json(result)

    def do_GET(self):
        self._handle("GET")

    def do_PUT(self):
        self._handle("PUT")

    def do_POST(self):
        self._handle("POST")

    def do_DELETE(self):
        self.close_connection = True
        self._json({"error": "not allowed", "code": "method"}, 405, {"Connection": "close"})

    do_PATCH = do_DELETE


UPDATE_LOCK = threading.Lock()


def _json_body(body):
    try:
        value = json.loads((body or b"{}").decode("utf-8") or "{}")
    except ValueError:
        raise Refused(400, "bad_request", "The request is not JSON.")
    if not isinstance(value, dict):
        raise Refused(400, "bad_request", "The request is not in the expected shape.")
    return value


def route(method, path, query, headers, body, server=None):
    parts = [urllib.parse.unquote(p) for p in path.strip("/").split("/")] if path != "/" else []
    if method == "GET":
        if not parts or parts == ["index.json"]:
            return INDEX
        if parts == ["health"]:
            return health()
        if parts == ["index"]:
            return {"songs": FOLDER.index()}
        if len(parts) == 3 and parts[0] == "libraries" and parts[2] == "files":
            return {"files": FOLDER.list_library(parts[1])}
        if len(parts) == 4 and parts[0] == "libraries" and parts[2] == "files":
            data = FOLDER.read_file(parts[1], parts[3])
            return data, {"X-Sha256": hashlib.sha256(data).hexdigest()}
        return None
    if method == "PUT":
        if len(parts) == 4 and parts[0] == "libraries" and parts[2] == "files":
            info = FOLDER.create_file(parts[1], parts[3], body, (headers.get("X-Sha256") or "").strip().lower())
            return {"created": info["name"], "size": info["size"]}
        return None
    if method == "POST":
        if len(parts) == 2 and parts[0] == "libraries":
            return {"created": FOLDER.ensure_library(parts[1])}
        if parts == ["import"]:
            request = _json_body(body)
            return search_import(request.get("id"), request.get("name"), request.get("playlist"))
        if parts == ["select-library"]:
            return select_library(_json_body(body).get("name"))
        if parts == ["update"]:
            return _update(_json_body(body), server)
    return None


def _update(request, server):
    """POST /update: install a new version of this script that the StagePilot project signed. The caller is on the local network
    and the request is signed with the key over its body (both checked before this); here: the signature on SHA256SUMS is the
    project's; every file matches SHA256SUMS; the files are exactly the ones this helper takes; the version is higher than the one
    that runs; every .py compiles."""
    if not UPDATE_LOCK.acquire(False):
        raise UpdateRejected("another update is being installed", 409)
    try:
        version, data = check_update(request, UPDATE_FILES, "propresenter_link.py", __version__)
        failed = read_marker(APP_DIR, "update-failed")
        if failed and failed.get("version") == version:
            raise UpdateRejected("Version %s was installed here before and did not start. Run the setup command on this computer instead." % version, 409)
        apply_update(APP_DIR, data, __version__, version)
    finally:
        UPDATE_LOCK.release()
    log.info("update %s -> %s installed; stopping so the new version starts", __version__, version)
    if server is not None:
        schedule_restart(server)
    return {"updating": True, "from": __version__, "to": version}


class Server(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True


def schedule_restart(server):
    """Stop serving half a second after the answer went out; run() then returns and the process exits 0, and the service
    manager (launchd, KeepAlive) starts the new version."""
    threading.Timer(0.5, server.shutdown).start()


# ---------------------------------------------------------------- command line
def load_config(path):
    global CONFIG_PATH, FOLDER
    CONFIG_PATH = path
    CFG.clear()
    CFG.update(DEFAULTS)
    if os.path.exists(path):
        with open(path) as f:
            loaded = json.load(f)
        if isinstance(loaded, dict):
            CFG.update(loaded)
    FOLDER = Folder(data_folder())


def setup_logging():
    os.makedirs(os.path.dirname(CFG["log_file"]), exist_ok=True)
    log.setLevel(logging.INFO)
    h = logging.handlers.RotatingFileHandler(CFG["log_file"], maxBytes=1_000_000, backupCount=3)
    h.setFormatter(logging.Formatter("%(asctime)s %(levelname)s %(message)s"))
    log.addHandler(h)


def run():
    setup_logging()
    shown = {k: v for k, v in CFG.items() if k not in ("log_file", "key")}
    shown["key"] = {"key": "set", "locked": "not set or too short"}[auth_mode()]       # never the key itself
    log.info("propresenter-link %s starting; config=%s", __version__, json.dumps(shown))
    if auth_mode() == "locked":
        log.error("There is no usable key in config.json (it must be text of at least %d characters). Refusing every request "
                  "except the bare /health until the installer is run again with --key-file.", MIN_KEY_LENGTH)
    else:
        secure_config_file(CONFIG_PATH)
    if finish_interrupted_update(APP_DIR):
        log.warning("an update was interrupted; it has been completed")
    server = Server((CFG["bind"], CFG["port"]), Handler)
    try:
        server.serve_forever()
    finally:
        server.server_close()


def check():
    body = health()
    print(json.dumps(body, indent=2))
    return 0 if body["data_folder_exists"] else 1


def main(argv):
    load_config(os.environ.get("PROPRESENTER_LINK_CONFIG", DEFAULT_CONFIG_PATH))
    cmd = argv[1] if len(argv) > 1 else "run"
    if cmd == "run":
        run()
    elif cmd == "check":
        return check()
    elif cmd in ("-V", "--version", "version"):
        print(__version__)
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
