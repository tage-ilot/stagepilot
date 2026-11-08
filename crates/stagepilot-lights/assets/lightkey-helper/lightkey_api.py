#!/usr/bin/env python3
"""lightkey-api: live fixture state of a running Lightkey (macOS), over HTTP and OSC.

Lightkey takes OSC and MIDI in but sends no state back. This agent reads the sACN (E1.31) DMX output Lightkey
emits and, with the fixture map from the project file, decodes it into color, dimmer, pan/tilt, gobo and so on.
It serves that state and its change log over HTTP, converts every change to Lightkey's own OSC commands, records
takes, and streams changes to subscribers over UDP (docs/relay.md). With the optional Shortcut it also reports
the active cue names.

Python 3.9+ standard library only. Not affiliated with the makers of Lightkey.

Commands:
  run                 start the agent (what the LaunchAgent runs)
  export-fixtures [PROJECT.lightkeyproj]
                      read the fixture map from the open (or given) project. Re-run after patching fixtures.
  check               verify Lightkey, the fixture map and (if "cues" is on) the Shortcut
  install-shortcut    optional (needs lightkey_cues.py): build the "Get Active Cues" Shortcut, turn "cues" on
  --version           print the version
"""
import hashlib
import json
import logging
import logging.handlers
import os
import plistlib
import re
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse
from collections import deque
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

__version__ = "0.4.3"

HOME = os.path.expanduser("~")
DEFAULT_CONFIG_PATH = os.path.join(HOME, "Library", "Application Support", "LightkeyAPI", "config.json")

DEFAULTS = {
    "cues": False,                   # read the active cue names through the optional Shortcut (see install-shortcut)
    "port": 8770,
    "bind": "",                      # "" = all interfaces, filtered by the allow-list below
    "allow_interfaces": [],          # extra interfaces (e.g. ["en5"]) whose address may be queried; loopback always may
    "allowed_local_ips": [],         # extra server-side addresses that may be queried
    "sacn_interfaces": "auto",       # "auto" (all IPv4 interfaces except loopback/link-local) or a list like ["en5"]
    "sacn_universes": [1, 2, 3, 4],  # universes to watch for change detection
    "process_name": "Lightkey",
    "log_file": os.path.join(HOME, "Library", "Logs", "lightkey-api.log"),
    "fixtures_file": os.path.join(HOME, "Library", "Application Support", "LightkeyAPI", "fixtures.json"),
    "levels_log_size": 5000,         # decoded fixture changes kept in memory (takes and the relay hold the history)
    "takes_dir": os.path.join(HOME, "Library", "Application Support", "LightkeyAPI", "takes"),
    "relay": True,                   # stream decoded fixture changes to subscribers over OSC/UDP (docs/relay.md)
    "relay_port": 21700,
    "relay_targets": [],             # fixed receivers "ip:port" that get the stream without subscribing
    "relay_ttl_s": 15.0,             # a subscriber that has not renewed in this long is dropped
    "relay_hello_s": 2.0,
    "relay_keyframe_s": 10.0,        # full state this often, so a receiver that lost a packet catches up
    "relay_mtu": 1400,
    "relay_max_subscribers": 8,
    "interface_check_s": 5.0,        # how often to look for network interface changes (re-joins sACN when they change)
    "osc_lead_s": 0.045,             # OSC command -> first changed DMX frame (measured 40-50 ms)
    "take_max_events": 200000,       # a recording keeps going past this, but is marked truncated
}

log = logging.getLogger("lightkey-api")
CFG = dict(DEFAULTS)

now_ms = lambda: int(time.time() * 1000)

# ---------------------------------------------------------------- state
cv = threading.Condition()
S = {
    "online": False,        # Lightkey process is running
    "dmx_seen_at": 0,       # ms epoch of the last sACN frame
    "dmx_changed_at": 0,    # ms epoch of the last sACN frame that differed
    "error": None,
}
HOOKS = {"online": [], "dmx": [], "get": {}}   # the optional cue module registers here (lightkey_cues.py)
streak = [0]
dirty_at = [0.0]
wake = threading.Event()


def snapshot():
    with cv:
        d = dict(S)
    d["version"] = __version__
    d["cues_enabled"] = bool(CFG["cues"])
    d["server_time"] = now_ms()
    return d


# ---------------------------------------------------------------- Lightkey process
def lightkey_running():
    try:
        return subprocess.run(["/usr/bin/pgrep", "-x", CFG["process_name"]],
                              capture_output=True, timeout=5).returncode == 0
    except Exception:
        return S["online"]


def process_loop():
    while True:
        up = lightkey_running()
        if up != S["online"]:
            log.info("Lightkey %s", "started" if up else "stopped")
            with cv:
                S["online"] = up
                if up:
                    S["error"] = None
                cv.notify_all()
            for fn in HOOKS["online"]:
                fn(up)
        time.sleep(2.0)


# ---------------------------------------------------------------- network helpers
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


def sacn_ips():
    addrs = interface_addrs()
    want = CFG["sacn_interfaces"]
    if want == "auto":
        return sorted({ip for n, ip in addrs.items() if n != "lo0" and not ip.startswith("169.254.")})
    return sorted({addrs[n] for n in want if n in addrs})


# ---------------------------------------------------------------- sACN change detector
def sacn_loop():
    sock = None
    joined = None
    last = {}
    ips, ips_at = [], float("-inf")
    while True:
        # Interfaces rarely change: look again every few seconds, not on every frame (that spawned ifconfig ~80x/s).
        if time.monotonic() - ips_at >= CFG["interface_check_s"]:
            ips, ips_at = sacn_ips(), time.monotonic()
        if ips != joined:
            if sock:
                sock.close()
                sock = None
            joined = ips
            if ips:
                try:
                    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM, socket.IPPROTO_UDP)
                    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                    try:
                        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEPORT, 1)
                    except Exception:
                        pass
                    s.bind(("", 5568))
                    for ip in ips:
                        for u in CFG["sacn_universes"]:
                            g = socket.inet_aton("239.255.%d.%d" % (u >> 8, u & 255))
                            try:
                                s.setsockopt(socket.IPPROTO_IP, socket.IP_ADD_MEMBERSHIP, g + socket.inet_aton(ip))
                            except OSError:
                                pass
                    s.settimeout(1.0)
                    sock = s
                    log.info("watching sACN universes %s on %s", CFG["sacn_universes"], ips)
                except Exception as e:
                    log.warning("sACN listen failed: %s", e)
                    sock = None
                    joined = None
                    time.sleep(3)
            else:
                log.info("no usable interface for sACN yet; relying on the %ss backstop read", CFG["backstop_s"])
        if not sock:
            time.sleep(3)
            continue
        try:
            d, _ = sock.recvfrom(1500)
        except socket.timeout:
            continue
        except OSError:
            joined = None
            continue
        if len(d) < 126 or d[4:16] != b"ASC-E1.17\0\0\0":
            continue
        uni = struct.unpack(">H", d[113:115])[0]
        h = hashlib.blake2b(d[125:], digest_size=8).digest()   # start code + slots; excludes the sequence number
        t = now_ms()
        S["dmx_seen_at"] = t
        if last.get(uni) != h:
            first = uni not in last
            last[uni] = h
            slots = bytes(d[126:126 + 512])
            with cv:
                LEVELS[uni] = slots
                if TRACKER[0] is not None:
                    try:
                        TRACKER[0].frame(uni, slots, t)
                    except Exception as e:             # a bad fixture definition must never stop change detection
                        S["levels_error"] = "%s: %s" % (type(e).__name__, e)
                        log.warning("decoding universe %s failed: %s", uni, e)
                    cv.notify_all()
            if not first:
                S["dmx_changed_at"] = t
                for fn in HOOKS["dmx"]:
                    fn(t, uni)


# ---------------------------------------------------------------- fixtures: project map, decoder, level tracker
# Lightkey keeps no live state outside its DMX output, so the full picture (colour, intensity, pan/tilt, gobos...)
# is rebuilt from that output. The channel layout of every fixture comes from the Lightkey project file.

SKIP_COMPONENTS = {"red", "green", "blue"}   # folded into one "color" value
KIND_NAMES = {   # capability class -> property name
    "Intensity": "dimmer", "ShutterStrobe": "strobe", "Zoom": "zoom", "Focus": "focus", "Frost": "frost", "Fog": "fog",
    "Prism": "prism", "PrismRotation": "prismRotation", "PrismAngle": "prismAngle", "Gobo": "gobo",
    "GoboRotation": "goboRotation", "GoboAngle": "goboAngle", "PanTiltSpeed": "panTiltSpeed", "Command": "command",
}


COMPONENT_OSC = {"warmwhite": "warmWhite", "coolwhite": "coolWhite", "amber": "amber", "lime": "lime",
                 "ultraviolet": "ultraviolet", "uv": "ultraviolet"}


def _rng(st):
    """A setting's DMX range as (lo, hi) 0..255. The project stores bytes as signed values (-128 is 128, -1 is 255)."""
    lo, hi = st["lo"], st["hi"]
    return (lo + 256 if lo < 0 else lo), (hi + 256 if hi < 0 else hi)


def _camel(text):
    words = re.sub(r"[^A-Za-z0-9]+", " ", text).split()
    return (words[0].lower() + "".join(w.capitalize() for w in words[1:])) if words else ""


def parse_project(path):
    """Read a .lightkeyproj (an NSKeyedArchiver binary plist) into a plain fixture map. Read-only."""
    with open(path, "rb") as f:
        objs = plistlib.load(f)["$objects"]

    def cls(o):
        c = o.get("$class") if isinstance(o, dict) else None
        return objs[c.data].get("$classname") if c is not None else None

    def val(x):
        return objs[x.data] if isinstance(x, plistlib.UID) else x

    def items(x):
        x = val(x)
        if isinstance(x, dict):
            return [val(i) for i in x.get("NS.objects", [])]
        return []

    def plain(x, depth=0):
        x = val(x)
        if depth > 6:
            return None
        if isinstance(x, dict):
            c = cls(x)
            if c in ("NSArray", "NSMutableArray", "NSSet", "NSMutableSet"):
                return [plain(i, depth + 1) for i in x.get("NS.objects", [])]
            if c in ("NSDictionary", "NSMutableDictionary"):
                return {str(plain(k, depth + 1)): plain(v, depth + 1) for k, v in zip(x["NS.keys"], x["NS.objects"])}
            return {k: plain(v, depth + 1) for k, v in x.items() if k != "$class"}
        if isinstance(x, plistlib.UID):
            return None
        if isinstance(x, (bytes, bytearray)):
            return list(x) if len(x) <= 16 else None   # short blobs are component arrays (RGB); long ones are images
        if isinstance(x, float) and (x != x or x in (float("inf"), float("-inf"))):
            return None
        return x

    def uid_key(x):
        x = val(x)
        b = x.get("NS.uuidbytes") if isinstance(x, dict) else None
        return val(b).hex() if isinstance(val(b), bytes) else str(x)

    profiles = {}
    for o in objs:
        if cls(o) != "LXFixtureProfile":
            continue
        pers = []
        for per in items(o["personalities"]):
            caps = []
            for cap in items(per["capabilities"]):
                kind = (cls(cap) or "")[2:-len("Capability")]
                settings = []
                for st in items(cap["settings"]):
                    rng = plain(st.get("$0")) if isinstance(st, dict) else None
                    lo = rng.get("$0", 0) if isinstance(rng, dict) else 0
                    hi = rng.get("$1", -1) if isinstance(rng, dict) else -1
                    settings.append({"lo": lo, "hi": hi, "params": plain(st.get("params")) or {}})
                beams = plain(cap.get("beamIndexes"))
                cname = val(cap.get("customName"))
                caps.append({"kind": kind, "channel": cap["channel"], "settings": settings,
                             "beams": beams if isinstance(beams, list) else None,
                             "name": cname if isinstance(cname, str) and cname != "$null" else ""})
            pname = val(per.get("customName"))
            pers.append({"name": pname if isinstance(pname, str) and pname != "$null" else "",
                         "footprint": per.get("footprint", 0), "capabilities": caps})
        lay = plain(o.get("beamLayout")) or {}
        if isinstance(lay.get("beamsByRing"), list):
            nbeams = sum(int(x) for x in lay["beamsByRing"] if isinstance(x, (int, float)))
        elif isinstance(lay.get("beamLocations"), list):
            nbeams = len(lay["beamLocations"])
        elif isinstance(lay.get("length"), (int, float)):
            nbeams = int(lay["length"])
        else:
            nbeams = 1
        profiles[uid_key(o["UUID"])] = {
            "beams": nbeams,
            "name": val(o["name"]), "manufacturer": val(o["manufacturer"]),
            "panRange": float(o.get("panRange") or 0), "tiltRange": float(o.get("tiltRange") or 0),
            "tiltCenter": float(o.get("tiltCenter") or 0), "personalities": pers}
    fixtures = []
    for o in objs:
        if cls(o) != "LXDMXFixture":
            continue
        prof = profiles.get(uid_key(o["fixtureProfileUUID"]))
        uni = val(o["universe"])
        if not prof or not isinstance(uni, dict):
            continue
        idx = o.get("personalityIndex", 0)
        per = prof["personalities"][idx] if idx < len(prof["personalities"]) else None
        if per is None:
            continue
        fixtures.append({"name": val(o["shortName"]), "profile": f'{prof["manufacturer"]} {prof["name"]}',
                         "personality": per["name"], "universe": uni["index"], "address": o["address"], "beams": prof["beams"],
                         "footprint": per["footprint"], "panRange": prof["panRange"], "tiltRange": prof["tiltRange"],
                         "capabilities": per["capabilities"]})
    fixtures.sort(key=lambda f: (f["universe"], f["address"]))
    return {"version": 1, "project": os.path.basename(path), "fixtures": fixtures}


def open_project_path():
    """Path of the project Lightkey currently has open (from its saved document bookmark), or None."""
    try:
        raw = subprocess.run(["/usr/bin/defaults", "export", "de.monospc.Lightkey", "-"], capture_output=True, timeout=10).stdout
        mark = plistlib.loads(raw).get("OpenDocumentURLBookmark")
        m = re.search(rb"(/Users/[^\x00]+?\.lightkeyproj)", mark or b"")
        return m.group(1).decode() if m else None
    except Exception:
        return None


UNUSED_PARAMS = ("default", "rotates", "rotating", "style")      # in the project file, never read here
LAYOUT_KEYS = ("profile", "personality", "beams", "footprint", "panRange", "tiltRange", "capabilities")


def compact_map(fmap):
    """Version 2: each distinct channel layout once, and one short row per fixture. Same data, about a quarter of the size."""
    layouts, index, rows = [], {}, []
    for f in fmap["fixtures"]:
        caps = []
        for c in f["capabilities"]:
            caps.append(dict(c, settings=[dict(st, params={k: v for k, v in st["params"].items() if k not in UNUSED_PARAMS})
                                          for st in c["settings"]]))
        layout = {k: (caps if k == "capabilities" else f[k]) for k in LAYOUT_KEYS if k == "capabilities" or k in f}
        key = json.dumps(layout, sort_keys=True, separators=(",", ":"))
        if key not in index:
            index[key] = len(layouts)
            layouts.append(layout)
        rows.append([f["name"], f["universe"], f["address"], index[key]])
    return {"version": 2, "project": fmap["project"], "layouts": layouts, "fixtures": rows}


def expand_map(fmap):
    """Back to one dict per fixture. Reads version 1 (written by 0.4.2 and earlier) and version 2."""
    if fmap.get("version", 1) < 2:
        return fmap
    out = []
    for name, universe, address, n in fmap["fixtures"]:
        f = dict(fmap["layouts"][n])
        f.update(name=name, universe=universe, address=address)
        out.append(f)
    return {"version": 1, "project": fmap.get("project"), "fixtures": out}


def export_fixtures(project=None, out=None):
    project = project or open_project_path()
    if not project:
        raise SystemExit("Pass the project file: lightkey_api.py export-fixtures /path/to/Show.lightkeyproj")
    fmap = parse_project(os.path.expanduser(project))
    out = out or CFG["fixtures_file"]
    os.makedirs(os.path.dirname(out), exist_ok=True)
    tmp = out + ".tmp"
    with open(tmp, "w") as f:
        json.dump(compact_map(fmap), f, separators=(",", ":"))
    os.replace(tmp, out)
    print("wrote %d fixtures from %s to %s" % (len(fmap["fixtures"]), fmap["project"], out))


class Fixtures:
    """Fixture map plus a decoder from raw DMX slots to named, normalised properties."""

    def __init__(self, fmap):
        self.fixtures = fmap["fixtures"]
        self.project = fmap.get("project")
        self.groups = [self._color_groups(f) for f in self.fixtures]   # per fixture: [[(name, channel), ...], ...]
        self.by_slot = {}                 # (universe, slot) -> [fixture index]
        for i, fx in enumerate(self.fixtures):
            for cap in fx["capabilities"]:
                self.by_slot.setdefault((fx["universe"], fx["address"] + cap["channel"]), []).append(i)

    @staticmethod
    def _color_groups(fx):
        """Colour channels in order, split into one group per beam: a new group starts when a component repeats."""
        groups, cur, names = [], [], set()
        for c in sorted(fx["capabilities"], key=lambda c: c["channel"]):
            if c["kind"] != "ColorComponent":
                continue
            n = _camel((c["settings"][0]["params"] if c["settings"] else {}).get("componentName", "") or "color")
            if n in names:
                groups.append(cur)
                cur, names = [], set()
            cur.append((n, c["channel"]))
            names.add(n)
        if cur:
            groups.append(cur)
        beams = fx.get("beams")
        if beams and len(groups) > beams:
            groups = groups[len(groups) - beams:]      # leading groups beyond the beam count are master colour channels
        return groups

    def summary(self):
        out = []
        for i, f in enumerate(self.fixtures):
            names = set()
            for c in f["capabilities"]:
                if c["kind"].endswith("Fine"):
                    continue
                if c["kind"] == "ColorComponent":
                    n = _camel((c["settings"][0]["params"] if c["settings"] else {}).get("componentName", "") or "color")
                    names.add("color" if n in SKIP_COMPONENTS else COMPONENT_OSC.get(n, n))
                elif c["kind"] == "ColorWheel":
                    names.add("colorWheel")
                elif c["kind"] in ("Pan", "Tilt"):
                    names.update((c["kind"].lower(), c["kind"].lower() + "Angle"))
                elif c["kind"] == "Custom":
                    st = c["settings"][0]["params"] if c["settings"] else {}
                    names.add(_camel(st.get("name", "")) or _camel(c["name"]) or "ch%d" % (c["channel"] + 1))
                else:
                    names.add(KIND_NAMES.get(c["kind"], c["kind"].lower()))
            out.append({"name": f["name"], "profile": f["profile"], "personality": f["personality"],
                        "universe": f["universe"], "address": f["address"] + 1, "channels": f["footprint"],
                        "properties": sorted(names | {"lit"})})
        return out

    def affected(self, universe, slots):
        out = set()
        for s in slots:
            out.update(self.by_slot.get((universe, s), ()))
        return out

    @staticmethod
    def _setting_index(cap, v):
        for k, st in enumerate(cap["settings"]):
            lo, hi = _rng(st)
            if lo <= v <= hi:
                return k
        return None

    @classmethod
    def _setting(cls, cap, v):
        k = cls._setting_index(cap, v)
        return cap["settings"][k] if k is not None else None

    # ---- OSC: the Lightkey commands that reproduce a fixture's current DMX state
    def osc_state(self, i, levels):
        """Ordered Lightkey OSC commands that reproduce fixture i's DMX state, as (property, args, exact) tuples,
        plus notes for anything OSC cannot express. Inverts Lightkey's own DMX mapping (measured on a real rig).
        Send them in order: colour first, then named components, then the rest."""
        fx = self.fixtures[i]
        data = levels.get(fx["universe"])
        if data is None:
            return [], []
        base = fx["address"]

        def raw(ch):
            return data[base + ch] if 0 <= base + ch < len(data) else 0

        caps = sorted(fx["capabilities"], key=lambda c: (c["channel"], c["kind"]))
        fine = {}
        for c in caps:
            if c["kind"].endswith("Fine"):
                fine[(c["kind"][:-4], tuple(c["beams"] or ()))] = c["channel"]

        def frac(c):
            f = fine.get((c["kind"], tuple(c["beams"] or ())))
            return ((raw(c["channel"]) << 8) | raw(f)) / 65535.0 if f is not None else raw(c["channel"]) / 255.0

        def inverted(c):
            return bool(c["settings"]) and c["settings"][0]["params"].get("transformation") == 1

        out, notes = [], []
        add = lambda prop, *args, exact=True: out.append((prop, list(args), exact))
        r4 = lambda x: round(x, 4)
        r6 = lambda x: round(x, 6)
        STEP16 = 0.5 / 65535.0           # Lightkey truncates to the DMX step: aim for the middle of it

        def has_fine(c):
            return (c["kind"], tuple(c["beams"] or ())) in fine

        # intensity (a fixture without an intensity channel gets a virtual dimmer: colour is scaled, so use full)
        ints = [c for c in caps if c["kind"] == "Intensity"]
        groups = self.groups[i]
        if ints:
            f = frac(ints[0])
            if has_fine(ints[0]):
                f = min(1.0, f + STEP16) if not inverted(ints[0]) else max(0.0, f - STEP16)
            add("dimmer", r6(1.0 - f if inverted(ints[0]) else f))
        elif groups:
            add("dimmer", 1.0)
        # colour
        if groups:
            comps = [{n: raw(ch) / 255.0 for n, ch in g} for g in groups]
            avg = lambda n: sum(g.get(n, 0.0) for g in comps) / len(comps)
            if any("red" in g or "green" in g or "blue" in g for g in comps):
                add("color", r4(avg("red")), r4(avg("green")), r4(avg("blue")))
            for n in sorted({n for g in comps for n in g} - SKIP_COMPONENTS):
                add(COMPONENT_OSC.get(n, n), r4(avg(n)))
            if len(comps) > 1 and any(g != comps[0] for g in comps):
                notes.append("beams differ in colour; OSC addresses the whole fixture, so the average is sent")
        wheels = {"ColorWheel": 0, "Gobo": 0, "Prism": 0}
        for c in caps:
            kind, v = c["kind"], raw(c["channel"])
            if kind.endswith("Fine") or kind in ("ColorComponent", "Intensity"):
                continue
            if kind in wheels:
                wheels[kind] += 1
            w = wheels.get(kind, 0)
            k = self._setting_index(c, v)
            st = c["settings"][k] if k is not None else None
            prm = st["params"] if st else {}
            if kind in ("Pan", "Tilt"):
                rng = fx["panRange"] if kind == "Pan" else fx["tiltRange"]
                f = frac(c) + (STEP16 if has_fine(c) else 0.0)
                add(kind.lower() + "Angle", round((f - 0.5) * rng, 4) if rng else 0.0)
            elif kind == "Zoom":
                lo_a, hi_a = prm.get("minAngle", 0), prm.get("maxAngle", 0)
                f = frac(c)
                add("zoomAngle", round(hi_a - f * (hi_a - lo_a) if inverted(c) else lo_a + f * (hi_a - lo_a), 2))
            elif kind == "Focus":
                add("focus", r4(frac(c)))
            elif kind == "Frost":
                add("frostAmount", r4(frac(c)))
            elif kind == "Fog":
                add("fogAmount", r4(frac(c)))
            elif kind == "ColorWheel":
                if k is not None:
                    add("color%d" % w, k + 1, exact=False)
            elif kind == "Gobo":
                if k is not None:
                    # Lightkey numbers gobos without the "shaking" duplicates: the index counts plain entries only.
                    plain = [x for x in c["settings"] if not x["params"].get("shaking")]
                    ident = prm.get("goboIdentifier")
                    shaking = bool(prm.get("shaking"))
                    if shaking:
                        n = next((j for j, x in enumerate(plain) if x["params"].get("goboIdentifier") == ident), 0)
                    else:
                        n = plain.index(st)
                    add("gobo%d" % w, n + 1, exact=False)
                    if shaking:
                        lo, hi = _rng(st)
                        add("goboShakeMode%d" % w, "shaking")
                        add("goboShakeSpeed%d" % w, round(1.0 + (v - lo) / max(hi - lo, 1) * 9.0, 2), exact=False)
                    else:
                        add("goboShakeMode%d" % w, "stopped")
            elif kind in ("GoboRotation", "PrismRotation"):
                # shares its channel with the matching *Angle capability: whichever range holds the value wins
                name = "gobo" if kind == "GoboRotation" else "prism"
                wi = 1 + sum(1 for a in caps if a["kind"] == kind and a["channel"] < c["channel"])
                ang = [a for a in caps if a["kind"] == kind.replace("Rotation", "Angle") and a["channel"] == c["channel"]]
                if ang and self._setting_index(ang[0], v) is not None:
                    continue               # an angle value; handled by the Angle capability
                if k is None:
                    continue
                lo, hi = _rng(st)
                if prm.get("mode") == 0:
                    add(name + "RotationMode%d" % wi, "stopped")
                else:
                    cw = prm.get("direction") == 0
                    upper = lo >= 128 and lo > 190
                    spd = (v - lo + 0.0) / max(hi - lo, 1) if upper else (hi - v + 0.0) / max(hi - lo, 1)
                    add(name + "RotationMode%d" % wi, "rotatingCW" if cw else "rotatingCCW")
                    add(name + "RotationSpeed%d" % wi, round(spd * 2.0, 3), exact=False)
            elif kind in ("GoboAngle", "PrismAngle", "GoboAngleFine"):
                name = "gobo" if kind == "GoboAngle" else "prism"
                rot = [a for a in caps if a["kind"] == kind.replace("Angle", "Rotation") and a["channel"] == c["channel"]]
                wi = 1 + sum(1 for a in caps if a["kind"] == kind and a["channel"] < c["channel"])
                if k is not None:
                    add(name + "RotationMode%d" % wi, "indexed")
                    lo, hi = _rng(st)
                    add(name + "Angle%d" % wi, round((v - lo) / max(hi - lo, 1) * 360.0, 1))
            elif kind == "Prism":
                if k is not None:
                    t = prm.get("type", 0)
                    names = ["none", "twoFacet", "threeFacet", "fourFacet", "fiveFacet", "sixFacet", "eightFacet", "sixteenFacet"]
                    add("prismType%d" % w, names[t] if 0 <= t < len(names) else "fourFacet", exact=(t == 0))
            elif kind == "ShutterStrobe":
                if k is None:
                    continue
                lo, hi = _rng(st)
                width = max(hi - lo, 1)
                mode = prm.get("mode", 1)
                if mode == 0:
                    add("shutterState", "closed")
                elif mode == 1:
                    add("shutterState", "open", exact=(v == (lo + hi) // 2 or lo == hi))
                elif mode == 2:
                    add("shutterState", "strobe")
                    add("strobeSpeed", round((v - lo + 0.3) * 20.0 / width, 2))
                elif mode == 5:
                    add("shutterState", "pulseAlternating")
                    add("shutterPulseSpeed", round(1.0 + (v - lo) / width * 9.0, 2), exact=False)
                elif mode == 10:
                    add("shutterState", "randomStrobe")
                    add("strobeSpeed", round((v - lo) / width, 3))
                else:
                    add("shutterState", "lightningStrobe", exact=False)
                    add("strobeSpeed", round((v - lo) / width, 3), exact=False)
            elif kind == "Custom":
                nm = (c["name"] or prm.get("name") or "").replace(" ", "_")
                if not nm:
                    notes.append("channel %d has no name" % (c["channel"] + 1))
                    continue
                if len(c["settings"]) <= 1:
                    add(nm, r4(frac(c)))
                elif k is not None:
                    opt = (prm.get("name") or "").replace(" ", "_")
                    lo, hi = _rng(st)
                    if prm.get("continuous"):
                        add(nm, opt, r4((v - lo) / max(hi - lo, 1)), exact=False)
                    else:
                        add(nm, opt, exact=False)
            elif kind in ("PanTiltSpeed", "Command"):
                if raw(c["channel"]) not in (0,) and not (kind == "Command" and k is not None and prm.get("name") is None):
                    notes.append("%s on channel %d (value %d) has no OSC property" % (kind, c["channel"] + 1, v))
        return out, notes

    def osc_commands(self, i, levels):
        """osc_state as JSON-ready {"a": address, "v": args} commands."""
        name = self.fixtures[i]["name"]
        cmds, notes = self.osc_state(i, levels)
        return [{"a": "/fixture/%s/overrides/%s" % (name, p), "v": a, **({} if ex else {"approx": True})} for p, a, ex in cmds], notes

    def decode(self, i, levels):
        """Properties of fixture i from {universe: bytes}. Values are 0..1 unless noted."""
        fx = self.fixtures[i]
        data = levels.get(fx["universe"])
        if data is None:
            return None
        base = fx["address"]

        def raw(ch):
            return data[base + ch] if 0 <= base + ch < len(data) else 0

        caps = fx["capabilities"]
        fine = {}
        for c in caps:
            if c["kind"].endswith("Fine"):
                fine[(c["kind"][:-4], tuple(c["beams"] or ()))] = c["channel"]
        props = {}
        for c in caps:
            kind, v = c["kind"], raw(c["channel"])
            if kind.endswith("Fine") or kind == "ColorComponent":
                continue
            beam = tuple(c["beams"] or ())
            fch = fine.get((kind, beam))
            frac = ((v << 8) | raw(fch)) / 65535.0 if fch is not None else v / 255.0
            tag = ("@" + ",".join(str(b) for b in beam)) if beam else ""
            if kind == "ColorWheel":
                st = self._setting(c, v)
                p = (st or {}).get("params", {})
                col = p.get("color") if isinstance(p, dict) else None
                entry = {"dmx": v}
                if isinstance(col, dict) and isinstance(col.get("Cmps"), list):
                    entry["color"] = [round(x / 255.0, 3) for x in col["Cmps"]]
                elif isinstance(col, dict) and "temperature" in col:
                    entry["temperature"] = col["temperature"]
                props["colorWheel" + tag] = entry
            elif kind in ("Pan", "Tilt"):
                rng = fx["panRange"] if kind == "Pan" else fx["tiltRange"]
                props[kind.lower() + tag] = round(frac, 5)
                if rng:
                    # Lightkey's own angle: 0 is the middle of the range (the OSC panAngle/tiltAngle value).
                    props[kind.lower() + "Angle" + tag] = round((frac - 0.5) * rng, 2)
            elif kind == "Custom":
                st = c["settings"][0]["params"] if c["settings"] else {}
                props[(_camel(st.get("name", "")) or _camel(c["name"]) or "ch%d" % (c["channel"] + 1)) + tag] = round(frac, 4)
            else:
                props[KIND_NAMES.get(kind, kind.lower()) + tag] = round(frac, 4)
        groups = []
        for g in self.groups[i]:
            groups.append({n: raw(ch) / 255.0 for n, ch in g})
        if groups:
            rgb = [[round(g.get(n, 0.0), 4) for n in ("red", "green", "blue")] for g in groups]
            if len(groups) == 1:
                props["color"] = rgb[0]
            else:
                props["color"] = [round(sum(c[k] for c in rgb) / len(rgb), 4) for k in range(3)]
                props["beams"] = len(groups)
                if any(c != rgb[0] for c in rgb):
                    props["beamColors"] = rgb
            for n in sorted({n for g in groups for n in g} - SKIP_COMPONENTS):
                vals = [g[n] for g in groups if n in g]
                props[COMPONENT_OSC.get(n, n)] = round(sum(vals) / len(vals), 4)
        dim = props.get("dimmer")
        cols = [v for k, v in props.items() if k.startswith("color") and isinstance(v, list)]
        comps = [v for k, v in props.items() if k in ("white", "amber", "warmWhite", "coolWhite", "lime", "ultraviolet")]
        if dim is not None:
            props["lit"] = bool(dim > 0)
        else:
            props["lit"] = bool(any(any(c) for c in cols) or any(comps))
        return props


class LevelTracker:
    """Latest DMX per universe, plus a log of decoded fixture changes with timestamps."""

    def __init__(self, fixtures, size):
        self.fx = fixtures
        self.levels = {}
        self.last = {}                    # fixture index -> last decoded props
        self.osc = {}                     # fixture index -> {address: command} last emitted
        self.log = deque(maxlen=size)
        self.seq = 0

    def frame(self, universe, data, t):
        """Called with the lock held for every sACN frame that differed. Returns True if anything decoded changed."""
        prev = self.levels.get(universe)
        self.levels[universe] = data
        if prev is None:
            for i, fx in enumerate(self.fx.fixtures):
                if fx["universe"] == universe:
                    try:
                        self.last[i] = self.fx.decode(i, self.levels)
                        self.osc[i] = {c["a"]: c for c in self.fx.osc_commands(i, self.levels)[0]}
                    except Exception as e:
                        log.warning("cannot decode fixture %s: %s", fx["name"], e)
            return False
        diff = [s for s in range(min(len(prev), len(data))) if prev[s] != data[s]]
        changes = {}
        osc_out = []
        for i in self.fx.affected(universe, diff):
            try:
                now = self.fx.decode(i, self.levels)
            except Exception as e:
                log.warning("cannot decode fixture %s: %s", self.fx.fixtures[i]["name"], e)
                continue
            before = self.last.get(i) or {}
            delta = {k: v for k, v in now.items() if before.get(k) != v}
            self.last[i] = now
            try:
                cmds, _notes = self.fx.osc_commands(i, self.levels)
            except Exception as e:
                log.warning("cannot convert fixture %s to OSC: %s", self.fx.fixtures[i]["name"], e)
                cmds = []
            old_cmds = self.osc.get(i, {})
            self.osc[i] = {c["a"]: c for c in cmds}
            changed = [c for c in cmds if old_cmds.get(c["a"]) != c]
            if delta or changed:
                changes[self.fx.fixtures[i]["name"]] = delta
                osc_out.extend(changed)
        if not changes:
            return False
        self.seq += 1
        ev = {"seq": self.seq, "t": t, "universe": universe, "changes": changes, "osc": osc_out}
        self.log.append(ev)
        if REC.active:
            REC.feed(ev)
        if osc_out:
            RELAY.push(t, osc_out)
        return True

    def state(self):
        return {self.fx.fixtures[i]["name"]: p for i, p in sorted(self.last.items()) if p is not None}


# ---------------------------------------------------------------- recorder: a take = the OSC to replay, with times
class Recorder:
    """One recording at a time. A take is a JSON file: the starting OSC state, then timed OSC commands.

    Times are seconds from the start, measured on the Lightkey Mac from the first changed DMX frame, so the file is
    independent of any clock offset. Replay: send "initial" at once, then each "events" entry at its "at".
    """

    def __init__(self):
        self.active = None            # dict while recording
        self.lock = threading.Lock()

    @staticmethod
    def _dir():
        return CFG["takes_dir"]

    @staticmethod
    def _safe(name):
        name = re.sub(r"[^A-Za-z0-9._ -]+", "_", name or "").strip(" .")[:80]
        return name or time.strftime("take-%Y%m%d-%H%M%S")

    def start(self, name=None, fixtures=None, scope="changed"):
        if scope not in ("changed", "all"):
            raise ValueError("scope must be 'changed' or 'all'")
        tr = TRACKER[0]
        if tr is None or FIXTURES[0] is None:
            raise ValueError("no fixture map; run: lightkey_api.py export-fixtures")
        with self.lock, cv:
            if self.active:
                raise ValueError("already recording %r" % self.active["name"])
            fx = FIXTURES[0][0]
            want = set(fixtures or [])
            initial = []
            for i, f in enumerate(fx.fixtures):
                if want and f["name"] not in want:
                    continue
                try:
                    cmds, _n = fx.osc_commands(i, tr.levels)
                except Exception:
                    cmds = []
                initial.extend(cmds)
            self.active = {"name": self._safe(name), "t0": now_ms(), "seq0": tr.seq, "fixtures": sorted(want), "initial": initial, "scope": scope,
                           "events": [], "last": {c["a"]: c for c in initial}}
        return {"name": self.active["name"], "started_ms": self.active["t0"], "scope": scope}

    def feed(self, event):
        """Called with the lock held (from the sACN thread) for every decoded event while recording."""
        a = self.active
        if not a:
            return
        want = a["fixtures"]
        cmds = [c for c in event["osc"] if not want or c["a"].split("/")[2] in want]
        cmds = [c for c in cmds if a["last"].get(c["a"]) != c]
        if not cmds:
            return
        for c in cmds:
            a["last"][c["a"]] = c
        a["events"].append({"at": round((event["t"] - a["t0"]) / 1000.0, 3), "osc": cmds})
        if len(a["events"]) > CFG["take_max_events"]:
            self.active["overflow"] = True

    def stop(self):
        with self.lock:
            a, self.active = self.active, None
        if not a:
            raise ValueError("not recording")
        end = now_ms()
        initial = a["initial"]
        if a["scope"] == "changed":
            # only fixtures that change during the take: everything else stays under the Lightkey operator's control
            moved = {c["a"].split("/")[2] for e in a["events"] for c in e["osc"]}
            initial = [c for c in initial if c["a"].split("/")[2] in moved]
        take = {"format": 1, "name": a["name"], "created": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
                "project": FIXTURES[0][0].project if FIXTURES[0] else None, "duration": round((end - a["t0"]) / 1000.0, 3),
                "scope": a["scope"], "fixtures": a["fixtures"], "initial": initial, "events": a["events"],
                # "at" is when the change appeared on the DMX output. OSC reaches the lights about this much later,
                # so send each command this many seconds before its "at".
                "lead": CFG["osc_lead_s"], "truncated": bool(a.get("overflow"))}
        os.makedirs(self._dir(), exist_ok=True)
        path = os.path.join(self._dir(), a["name"] + ".take.json")
        tmp = path + ".tmp"
        with open(tmp, "w") as f:
            json.dump(take, f)
        os.replace(tmp, path)
        return {"name": a["name"], "path": path, "duration": take["duration"], "events": len(take["events"]),
                "commands": len(take["initial"]) + sum(len(e["osc"]) for e in take["events"]), "truncated": take["truncated"]}

    def status(self):
        a = self.active
        if not a:
            return {"recording": False}
        return {"recording": True, "name": a["name"], "seconds": round((now_ms() - a["t0"]) / 1000.0, 1),
                "events": len(a["events"]), "fixtures": a["fixtures"]}

    def list(self):
        out = []
        if os.path.isdir(self._dir()):
            for f in sorted(os.listdir(self._dir())):
                if f.endswith(".take.json"):
                    try:
                        with open(os.path.join(self._dir(), f)) as fh:
                            t = json.load(fh)
                        out.append({"name": t["name"], "created": t.get("created"), "duration": t.get("duration"),
                                    "events": len(t.get("events", [])), "fixtures": t.get("fixtures", [])})
                    except Exception:
                        continue
        return out

    def get(self, name):
        path = os.path.join(self._dir(), self._safe(name) + ".take.json")
        if not os.path.exists(path):
            raise KeyError(name)
        with open(path) as f:
            return json.load(f)

    def delete(self, name):
        path = os.path.join(self._dir(), self._safe(name) + ".take.json")
        if not os.path.exists(path):
            raise KeyError(name)
        os.remove(path)


REC = Recorder()


FIXTURES = [None]
TRACKER = [None]


def load_fixtures():
    path = CFG["fixtures_file"]
    try:
        mtime = os.path.getmtime(path)
    except OSError:
        return
    if FIXTURES[0] is not None and FIXTURES[0][1] == mtime:
        return
    try:
        with open(path) as f:
            fx = Fixtures(expand_map(json.load(f)))
    except Exception as e:
        log.warning("fixture map %s unreadable: %s", path, e)
        return
    with cv:
        FIXTURES[0] = (fx, mtime)
        TRACKER[0] = LevelTracker(fx, CFG["levels_log_size"])
        for u, data in list(LEVELS.items()):
            TRACKER[0].frame(u, data, now_ms())
    log.info("loaded %d fixtures from %s (%s)", len(fx.fixtures), path, fx.project)


LEVELS = {}   # universe -> latest 512 slots (bytes), kept even before a fixture map exists


# ---------------------------------------------------------------- OSC codec
NTP_EPOCH = 2208988800


def _pad4(b):
    return b + b"\0" * (4 - len(b) % 4)


def ntp_pack(t):
    """Unix seconds (float) -> 8-byte OSC time tag."""
    sec = int(t)
    return struct.pack(">II", sec + NTP_EPOCH, int((t - sec) * 4294967296.0) & 0xFFFFFFFF)


def ntp_unpack(b):
    sec, frac = struct.unpack(">II", b)
    return sec - NTP_EPOCH + frac / 4294967296.0


def osc_message(addr, args=()):
    """Encode one OSC message. int -> i, float -> f, str -> s, bytes -> b, bool -> T/F, ("t", unix_s) -> t, ("d", x) -> d."""
    tags, body = ",", b""
    for a in args:
        if isinstance(a, bool):
            tags += "T" if a else "F"
        elif isinstance(a, int):
            tags += "i"
            body += struct.pack(">i", a)
        elif isinstance(a, float):
            tags += "f"
            body += struct.pack(">f", a)
        elif isinstance(a, str):
            tags += "s"
            body += _pad4(a.encode())
        elif isinstance(a, (bytes, bytearray)):
            tags += "b"
            body += struct.pack(">i", len(a)) + bytes(a) + b"\0" * (-len(a) % 4)
        elif isinstance(a, tuple) and a[0] == "t":
            tags += "t"
            body += ntp_pack(a[1])
        elif isinstance(a, tuple) and a[0] == "d":
            tags += "d"
            body += struct.pack(">d", a[1])
        else:
            raise TypeError("cannot encode %r" % (a,))
    return _pad4(addr.encode()) + _pad4(tags.encode()) + body


def osc_bundle(t, elements):
    out = b"#bundle\0" + ntp_pack(t)
    for e in elements:
        out += struct.pack(">i", len(e)) + e
    return out


def _cstr(b, i):
    j = b.index(b"\0", i)
    return b[i:j].decode(), ((j + 4) // 4) * 4


def osc_parse(b):
    """Decode a packet. Returns [("msg", addr, args)] or [("bundle", unix_s, [elements...])] (elements parsed recursively)."""
    if b.startswith(b"#bundle\0"):
        t = ntp_unpack(b[8:16])
        i, els = 16, []
        while i + 4 <= len(b):
            n = struct.unpack(">i", b[i:i + 4])[0]
            i += 4
            els.extend(osc_parse(b[i:i + n]))
            i += n
        return [("bundle", t, els)]
    addr, i = _cstr(b, 0)
    tags, i = _cstr(b, i)
    args = []
    for c in tags[1:]:
        if c == "i":
            args.append(struct.unpack(">i", b[i:i + 4])[0]); i += 4
        elif c == "f":
            args.append(round(struct.unpack(">f", b[i:i + 4])[0], 6)); i += 4
        elif c == "d":
            args.append(struct.unpack(">d", b[i:i + 8])[0]); i += 8
        elif c == "h":
            args.append(struct.unpack(">q", b[i:i + 8])[0]); i += 8
        elif c == "t":
            args.append(ntp_unpack(b[i:i + 8])); i += 8
        elif c == "s":
            v, i = _cstr(b, i); args.append(v)
        elif c == "b":
            n = struct.unpack(">i", b[i:i + 4])[0]; args.append(bytes(b[i + 4:i + 4 + n])); i += 4 + ((n + 3) // 4) * 4
        elif c == "T":
            args.append(True)
        elif c == "F":
            args.append(False)
        elif c == "N":
            args.append(None)
        else:
            raise ValueError("unsupported OSC type %r" % c)
    return [("msg", addr, args)]


# ---------------------------------------------------------------- relay: stream fixture changes to other computers
import queue
import random

RELAY_VERSION = 1


class Relay:
    """Sends the decoded fixture changes to subscribers as OSC over UDP, each with the time it appeared on the DMX output.

    Wire format (docs/relay.md). Every UDP packet is one OSC bundle whose time tag is the event time in Unix seconds
    on THIS computer's clock. The first element is /relay/seq seq kind part parts; the rest are Lightkey fixture
    commands (/fixture/<name>/overrides/<property> args...), the same ones the HTTP API and the take files use.
        kind "event"    commands that changed (one bundle per DMX change; split in parts when large)
        kind "keyframe" the full state, sent on subscribe and every relay_keyframe_s
        kind "hello"    /relay/hello version stream_id project fixture_count map_id, every relay_hello_s
    Subscribers send /relay/subscribe [port] (renew inside relay_ttl_s), /relay/unsubscribe, /relay/keyframe and
    /relay/ping t1 (answered with /relay/pong t1 t2 t3 for the clock offset). Commands to the relay are refused
    from addresses that cannot reach the HTTP API.
    """

    def __init__(self):
        self.subs = {}                   # (ip, port) -> {"sock", "seq", "seen", "static"}
        self.lock = threading.Lock()
        self.q = queue.Queue(maxsize=2000)
        self.socks = []
        self.stream = random.getrandbits(31)
        self.dropped = 0
        self.sent = 0
        self.kf_wanted = set()
        self.stop_flag = False
        self.map_id = None

    # -- producer side (sACN thread, lock cv held)
    def push(self, t_ms, cmds):
        if not self.socks or not self.subs:
            return
        try:
            self.q.put_nowait((t_ms / 1000.0, cmds))
        except queue.Full:
            self.dropped += 1

    # -- lifecycle
    def start(self, ips=None, port=None):
        ips = sorted(ips or _allowed_ips())
        port = CFG["relay_port"] if port is None else port
        for ip in ips:
            try:
                s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
                s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                s.bind((ip, port))
                s.settimeout(0.2)
                self.socks.append(s)
            except OSError as e:
                log.warning("relay: cannot listen on %s:%s (%s)", ip, port, e)
        for t in CFG["relay_targets"]:
            host, _, prt = t.rpartition(":")
            if self.socks:
                self.subs[(host, int(prt))] = {"sock": self.socks[0], "seq": 0, "seen": 0.0, "static": True}
                self.kf_wanted.add((host, int(prt)))
        self.stop_flag = False
        for s in self.socks:
            threading.Thread(target=self._rx, args=(s,), daemon=True).start()
        threading.Thread(target=self._tx, daemon=True).start()
        log.info("relay listening on %s udp %s", [s.getsockname()[0] for s in self.socks], port)

    def stop(self):
        self.stop_flag = True
        for s in self.socks:
            try:
                s.close()
            except OSError:
                pass
        self.socks = []
        self.subs.clear()

    # -- packet building
    def _hello(self):
        proj, n = (None, 0)
        if FIXTURES[0]:
            proj, n = FIXTURES[0][0].project, len(FIXTURES[0][0].fixtures)
        return osc_message("/relay/hello", [RELAY_VERSION, self.stream, proj or "", n, int(FIXTURES[0][1]) if FIXTURES[0] else 0])

    def _groups(self, cmds):
        """Split command messages so every packet stays under relay_mtu bytes."""
        msgs = [osc_message(c["a"], c["v"]) for c in cmds]
        room = CFG["relay_mtu"] - 16 - 4 - 40
        groups, cur, size = [], [], 0
        for m in msgs:
            if cur and size + len(m) + 4 > room:
                groups.append(cur)
                cur, size = [], 0
            cur.append(m)
            size += len(m) + 4
        if cur or not groups:
            groups.append(cur)
        return groups

    def _send(self, key, kind, t, groups):
        sub = self.subs.get(key)
        if not sub:
            return
        n = len(groups)
        for i, g in enumerate(groups):
            sub["seq"] = (sub["seq"] + 1) & 0x7FFFFFFF
            pkt = osc_bundle(t, [osc_message("/relay/seq", [sub["seq"], kind, i + 1, n])] + g)
            try:
                sub["sock"].sendto(pkt, key)
                self.sent += 1
            except OSError:
                pass

    def _keyframe_groups(self):
        with cv:
            if not (FIXTURES[0] and TRACKER[0]):
                return None, []
            fx, lv = FIXTURES[0][0], dict(TRACKER[0].levels)
            cmds = []
            for i in range(len(fx.fixtures)):
                try:
                    cmds.extend(fx.osc_commands(i, lv)[0])
                except Exception:
                    pass
            t = now_ms() / 1000.0
        return t, self._groups(cmds)

    # -- threads
    def _rx(self, sock):
        while not self.stop_flag:
            try:
                data, addr = sock.recvfrom(2048)
            except socket.timeout:
                continue
            except OSError:
                return
            t2 = time.time()
            try:
                els = osc_parse(data)
            except Exception:
                continue
            if not els or els[0][0] != "msg":
                continue
            _, a, args = els[0]
            ip, sport = addr
            if a == "/relay/ping":
                t1 = args[0] if args else 0.0
                try:
                    sock.sendto(osc_message("/relay/pong", [("d", float(t1)), ("d", t2), ("d", time.time())]), addr)
                except OSError:
                    pass
                continue
            port = int(args[0]) if args and isinstance(args[0], int) and 0 < args[0] < 65536 else sport
            key = (ip, port)
            if a == "/relay/subscribe":
                with self.lock:
                    if len(self.subs) >= CFG["relay_max_subscribers"] and key not in self.subs:
                        continue
                    new = key not in self.subs
                    sub = self.subs.setdefault(key, {"sock": sock, "seq": 0, "seen": 0.0, "static": False})
                    sub["seen"] = time.monotonic()
                    sub["sock"] = sock
                    if new:
                        self.kf_wanted.add(key)
                try:
                    sock.sendto(osc_bundle(time.time(), [osc_message("/relay/seq", [0, "hello", 1, 1]), self._hello()]), key)
                except OSError:
                    pass
            elif a == "/relay/unsubscribe":
                with self.lock:
                    if key in self.subs and not self.subs[key]["static"]:
                        del self.subs[key]
            elif a == "/relay/keyframe":
                with self.lock:
                    if key in self.subs:
                        self.kf_wanted.add(key)

    def _tx(self):
        next_hello = next_kf = 0.0
        map_seen = None
        while not self.stop_flag:
            try:
                t, cmds = self.q.get(timeout=0.2)
            except queue.Empty:
                t = None
            now = time.monotonic()
            with self.lock:
                for k in [k for k, v in self.subs.items() if not v["static"] and now - v["seen"] > CFG["relay_ttl_s"]]:
                    del self.subs[k]
                keys = list(self.subs)
                wanted, self.kf_wanted = set(self.kf_wanted) & set(keys), set()
            if t is not None and keys and cmds:
                groups = self._groups(cmds)
                for k in keys:
                    self._send(k, "event", t, groups)
            mid = FIXTURES[0][1] if FIXTURES[0] else None
            if mid != map_seen:
                map_seen = mid
                if keys:
                    wanted = set(keys)
            if keys and now >= next_hello:
                next_hello = now + CFG["relay_hello_s"]
                for k in keys:
                    self._send(k, "hello", time.time(), [[self._hello()]])
            if now >= next_kf:
                next_kf = now + CFG["relay_keyframe_s"]
                wanted = set(keys)
            if wanted:
                kt, groups = self._keyframe_groups()
                if groups:
                    for k in wanted:
                        self._send(k, "keyframe", kt, groups)

    def status(self):
        with self.lock:
            subs = [{"address": "%s:%d" % k, "static": v["static"], "packets": v["seq"],
                     "idle_s": round(time.monotonic() - v["seen"], 1) if v["seen"] else None} for k, v in self.subs.items()]
        return {"enabled": bool(self.socks), "port": CFG["relay_port"], "stream": self.stream, "subscribers": subs,
                "packets_sent": self.sent, "dropped": self.dropped}


RELAY = Relay()


# ---------------------------------------------------------------- HTTP
INDEX = {
    "name": "lightkey-api", "version": __version__,
    "endpoints": {
        "/time": "server clock in ms, for measuring the clock offset",
        "/fixtures": "fixtures from the Lightkey project: name, profile, universe, address, properties",
        "/fixtures/state": "every fixture's decoded properties now (colour, dimmer, pan/tilt, gobo...)",
        "/fixtures/log?since=N": "timestamped fixture property changes with seq > N, each with the Lightkey OSC commands ('osc')",
        "/fixtures/osc": "OSC commands that reproduce the current state (?fixture=MH1&fixture=P1 to select)",
        "/levels": "raw DMX slots per universe (?u=1&u=2)",
        "POST /record/start": "begin a take: {\"name\": \"Song 1\", \"fixtures\": [\"MH1\"], \"scope\": \"changed\"}; fixtures optional (default all); scope \"changed\" (default) starts only fixtures that change, \"all\" starts every fixture",
        "POST /record/stop": "end the take and save it",
        "/record": "recording status",
        "/relay": "OSC relay status: port, subscribers, packets sent (docs/relay.md)",
        "/takes": "saved takes; /takes/<name> returns one (initial OSC + timed OSC commands)",
        "/health": "liveness and last error",
    },
}


def _allowed_ips():
    ips = {"127.0.0.1"} | set(CFG["allowed_local_ips"])
    addrs = interface_addrs()
    for n in CFG["allow_interfaces"]:
        if n in addrs:
            ips.add(addrs[n])
    return ips


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def _json(self, obj, code=200):
        b = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def _fixtures(self, path, query, q):
        load_fixtures()
        if path == "/levels":
            want = [int(v) for k, v in (p.split("=", 1) for p in query.split("&") if "=" in p) if k == "u" and v.isdigit()]
            with cv:
                lv = {str(u): list(d) for u, d in LEVELS.items() if not want or u in want}
            return self._json({"levels": lv, "server_time": now_ms()})
        if FIXTURES[0] is None:
            return self._json({"error": "no fixture map yet; run: lightkey_api.py export-fixtures"}, 409)
        fx, tr = FIXTURES[0][0], TRACKER[0]
        if path == "/fixtures":
            return self._json({"project": fx.project, "count": len(fx.fixtures), "fixtures": fx.summary()})
        if path == "/fixtures/osc":
            want = {v.replace("%20", " ") for k, v in (p.split("=", 1) for p in query.split("&") if "=" in p) if k == "fixture"}
            cmds, notes = [], {}
            with cv:
                lv = dict(tr.levels)
                for i, f in enumerate(fx.fixtures):
                    if want and f["name"] not in want:
                        continue
                    try:
                        c, n = fx.osc_commands(i, lv)
                    except Exception as e:
                        c, n = [], ["cannot convert: %s" % e]
                    cmds.extend(c)
                    if n:
                        notes[f["name"]] = n
            return self._json({"commands": cmds, "notes": notes, "server_time": now_ms()})
        if path == "/fixtures/state":
            with cv:
                st = tr.state()
                seq = tr.seq
            return self._json({"fixtures": st, "seq": seq, "server_time": now_ms()})
        try:
            since = int(q.get("since", 0))
        except ValueError:
            return self._json({"error": "since must be an integer"}, 400)
        with cv:
            evs = [e for e in tr.log if e["seq"] > since]
            seq = tr.seq
        return self._json({"events": evs, "seq": seq, "server_time": now_ms()})

    def do_POST(self):
        if self.connection.getsockname()[0] not in _allowed_ips():
            return self._json({"error": "not available on this interface"}, 403)
        path = self.path.partition("?")[0]
        try:
            n = int(self.headers.get("Content-Length") or 0)
            body = json.loads(self.rfile.read(n) or b"{}") if n else {}
            if not isinstance(body, dict):
                raise ValueError("body must be a JSON object")
            load_fixtures()
            if path == "/record/start":
                fx = body.get("fixtures")
                if fx is not None and not (isinstance(fx, list) and all(isinstance(x, str) for x in fx)):
                    raise ValueError("fixtures must be a list of names")
                return self._json(REC.start(body.get("name"), fx, body.get("scope", "changed")))
            if path == "/record/stop":
                return self._json(REC.stop())
        except ValueError as e:
            return self._json({"error": str(e)}, 409 if "recording" in str(e) else 400)
        self._json({"error": "not found"}, 404)

    def do_DELETE(self):
        if self.connection.getsockname()[0] not in _allowed_ips():
            return self._json({"error": "not available on this interface"}, 403)
        path = self.path.partition("?")[0]
        if path.startswith("/takes/"):
            try:
                REC.delete(urllib.parse.unquote(path[7:]))
                return self._json({"deleted": True})
            except KeyError:
                return self._json({"error": "no such take"}, 404)
        self._json({"error": "not found"}, 404)

    def do_GET(self):
        if self.connection.getsockname()[0] not in _allowed_ips():
            return self._json({"error": "not available on this interface"}, 403)
        path, _, query = self.path.partition("?")
        q = dict(p.split("=", 1) for p in query.split("&") if "=" in p)
        if path in ("/", "/index.json"):
            return self._json(INDEX)
        if path == "/time":
            return self._json({"t": now_ms()})
        hook = HOOKS["get"].get(path)
        if hook:
            return hook(self, q)
        if path == "/record":
            return self._json(REC.status())
        if path == "/relay":
            return self._json(RELAY.status())
        if path == "/takes":
            return self._json({"takes": REC.list()})
        if path.startswith("/takes/"):
            try:
                return self._json(REC.get(urllib.parse.unquote(path[7:])))
            except KeyError:
                return self._json({"error": "no such take"}, 404)
        if path in ("/fixtures", "/fixtures/state", "/fixtures/log", "/fixtures/osc", "/levels"):
            return self._fixtures(path, query, q)
        if path == "/health":
            sn = snapshot()
            body = {"ok": True, "lightkey": sn["online"], "error": sn["error"],
                    "dmx_age_ms": (sn["server_time"] - sn["dmx_seen_at"]) if sn["dmx_seen_at"] else None}
            if CFG["cues"]:
                body["reads"] = S.get("reads", 0)
            return self._json(body)
        self._json({"error": "not found"}, 404)


class Server(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True


# ---------------------------------------------------------------- optional cue names
def load_cues():
    """Import lightkey_cues.py (next to this file) and hand it the agent's state. None if it is not installed."""
    try:
        import importlib.util
        path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "lightkey_cues.py")
        spec = importlib.util.spec_from_file_location("lightkey_cues", path)
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        mod.attach(sys.modules[__name__])
        return mod
    except (OSError, ImportError, AttributeError) as e:
        log.warning("cue names unavailable: %s", e)
        return None


# ---------------------------------------------------------------- check
def check():
    ok = True

    def line(good, msg):
        nonlocal ok
        ok = ok and good
        print(("OK    " if good else "FAIL  ") + msg)

    line(sys.version_info >= (3, 9), "python %d.%d.%d (need 3.9+)" % sys.version_info[:3])
    line(os.path.isdir("/Applications/Lightkey.app") or os.path.isdir(os.path.join(HOME, "Applications", "Lightkey.app")),
         "Lightkey.app installed")
    running = lightkey_running()
    line(running, "Lightkey is running" + ("" if running else " (open it, with a project loaded)"))
    if not CFG["cues"]:
        print("      active cue names are off (optional). To turn them on: python3 lightkey_api.py install-shortcut")
    else:
        cues = load_cues()
        if cues:
            cues.check_cues(line, running)
        else:
            line(False, "lightkey_cues.py is missing next to lightkey_api.py (the installer puts it there)")
    if os.path.exists(CFG["fixtures_file"]):
        try:
            with open(CFG["fixtures_file"]) as f:
                m = expand_map(json.load(f))
            line(True, "fixture map: %d fixtures from %s" % (len(m["fixtures"]), m.get("project")))
        except Exception as e:
            line(False, "fixture map unreadable (%s); run: lightkey_api.py export-fixtures" % e)
    else:
        print("      no fixture map yet (optional): run  lightkey_api.py export-fixtures  to decode colour, dimmer, pan/tilt")
    print("sACN interfaces: %s" % (sacn_ips() or "none found; change detection falls back to the %ss backstop" % CFG["backstop_s"]))
    return 0 if ok else 1


# ---------------------------------------------------------------- main
def load_config(path):
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
    log.info("lightkey-api %s starting; config=%s", __version__,
             json.dumps({k: v for k, v in CFG.items() if k != "log_file"}))
    S["online"] = lightkey_running()
    load_fixtures()
    loops = [process_loop, sacn_loop]
    if CFG["cues"]:
        cues = load_cues()
        if cues:
            loops.append(cues.reader_loop)
        else:
            log.warning("\"cues\" is true but lightkey_cues.py was not found; cue names are off")
    for fn in loops:
        threading.Thread(target=fn, daemon=True).start()
    if CFG["relay"]:
        RELAY.start()
    Server((CFG["bind"], CFG["port"]), Handler).serve_forever()


def main(argv):
    path = os.environ.get("LIGHTKEY_API_CONFIG", DEFAULT_CONFIG_PATH)
    load_config(path)
    cmd = argv[1] if len(argv) > 1 else "run"
    if cmd == "run":
        run()
    elif cmd == "check":
        return check()
    elif cmd == "install-shortcut":
        cues = load_cues()
        if not cues:
            print("lightkey_cues.py is missing next to lightkey_api.py. Install it from the same release.")
            return 1
        return cues.install_shortcut()
    elif cmd == "export-fixtures":
        return export_fixtures(argv[2] if len(argv) > 2 else None)
    elif cmd in ("-V", "--version", "version"):
        print(__version__)
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
