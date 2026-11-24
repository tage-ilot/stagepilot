#!/usr/bin/env python3
"""Compare two recordings of Lightkey's raw sACN output (or one recording and a saved song), property by property.

    compare.py --fixtures fixtures.json --a original.pkl --b replay.pkl [--from S --to S] [--align auto]
    compare.py --fixtures fixtures.json --a replay.pkl --saved song_grid.json [--from S --to S]

Both sides are decoded with the Lightkey helper's own decoder (`Fixtures.osc_commands` of lightkey_api.py, imported
by path, never changed), so a value here is the value StagePilot would read. B is lined up with A by a constant
offset found by cross-correlating the decoded series, then every property is compared on a 25 ms grid.
Python 3.9+, standard library only. See README.md for the procedure and what the numbers mean.
"""
import argparse
import bisect
import collections
import importlib.util
import json
import math
import os
import pickle
import sys

sys.dont_write_bytecode = True          # never leave a __pycache__ in the helper folder: it is a bundle of plain files

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_HELPER = os.path.join(HERE, "..", "..", "crates", "stagepilot-lights", "assets", "lightkey-helper",
                              "lightkey_api.py")

GRID = 0.025            # seconds between compared samples
FINE = 0.005            # seconds, the resolution of the offset
DMX_STEP = 1.0 / 255.0  # one DMX step, as a share of a property's range
BIG = 0.02              # the second yardstick: 2% of the range
FLASH_BACK = 0.02       # a flash comes back to within this of where it was
FLASH_OUT = 0.05        # and was at least this far away in between
CLASSES = ["brightness", "colour", "pan/tilt", "zoom/focus", "other"]
MAX_OFFSET_SPREAD_MS = 25.0
INDISTINGUISHABLE_SHARE = 0.01   # at most 1% of samples may be more than one DMX step apart


class CompareError(Exception):
    pass


# ---------------------------------------------------------------- loading

def load_helper(path=None):
    """The helper module, imported by path from the repository (or `path`)."""
    path = os.path.abspath(path or DEFAULT_HELPER)
    if not os.path.exists(path):
        raise CompareError("cannot find the helper at %s (pass --helper)" % path)
    spec = importlib.util.spec_from_file_location("lightkey_api_for_fidelity", path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = mod
    spec.loader.exec_module(mod)
    return mod


class _PlainUnpickler(pickle.Unpickler):
    """A capture holds only tuples, numbers and bytes; it never needs a global, so it may not name one."""

    def find_class(self, module, name):
        raise pickle.UnpicklingError("a capture file may not contain %s.%s" % (module, name))


def load_capture(path):
    """[(wall seconds, universe, sequence, slots bytes), ...] in the order they were heard."""
    try:
        with open(path, "rb") as f:
            frames = _PlainUnpickler(f).load()
    except (OSError, pickle.UnpicklingError, EOFError) as e:
        raise CompareError("cannot read capture %s: %s" % (path, e))
    ok = isinstance(frames, list) and frames and all(
        isinstance(x, tuple) and len(x) == 4 and isinstance(x[3], (bytes, bytearray)) for x in frames)
    if not ok:
        raise CompareError("%s is not a capture (a non-empty list of (wall, universe, sequence, slots))" % path)
    return frames


class Source:
    """Decoded properties of one side: series[address] = (times, values), hold-last between events. Times are
    seconds on the side's own clock (a capture counts from its first frame; a saved song from the song's 0)."""

    def __init__(self, name, series, start, end, info):
        self.name, self.series, self.start, self.end, self.info = name, series, start, end, info

    def value(self, address, t):
        s = self.series.get(address)
        if s is None or t < self.start - 1e-9 or t > self.end + 1e-9:
            return None
        i = bisect.bisect_right(s[0], t + 1e-9) - 1
        return s[1][i] if i >= 0 else None


def decode_capture(fx, frames, name):
    """Decode every frame with the helper's decoder: only the fixtures whose slots changed are decoded again."""
    t0 = frames[0][0]
    levels = {}
    times, vals = collections.defaultdict(list), collections.defaultdict(list)
    for wall, universe, _seq, slots in frames:
        slots = bytes(slots)
        old = levels.get(universe)
        levels[universe] = slots
        if old is None:
            idx = [i for i, f in enumerate(fx.fixtures) if f["universe"] == universe]
        elif old == slots:
            continue
        else:
            idx = sorted(fx.affected(universe, [s for s in range(min(len(old), len(slots))) if old[s] != slots[s]]))
        for i in idx:
            try:
                cmds, _notes = fx.osc_commands(i, levels)
            except Exception:           # a fixture the decoder cannot read is left out, as the helper leaves it out
                continue
            for c in cmds:
                t, v = times[c["a"]], vals[c["a"]]
                if not v or v[-1] != c["v"]:
                    t.append(wall - t0)
                    v.append(c["v"])
    series = {a: (times[a], vals[a]) for a in times}
    end = frames[-1][0] - t0
    return Source(name, series, 0.0, end, {"frames": len(frames), "universes": sorted(levels), "seconds": end})


def load_song_grid(path, name):
    """The 25 ms grid `examples/dump_song.rs` writes for a saved song."""
    try:
        with open(path) as f:
            d = json.load(f)
        step, start = float(d["step_seconds"]), float(d["from_seconds"])
        end = float(d["to_seconds"])
        series = {}
        for a, column in d["addresses"].items():
            t, v = [], []
            for k, args in enumerate(column):
                if args is not None and (not v or v[-1] != args):
                    t.append(start + k * step)
                    v.append(args)
            if t:
                series[a] = (t, v)
    except (OSError, ValueError, KeyError, TypeError) as e:
        raise CompareError("cannot read saved song grid %s: %s" % (path, e))
    return Source(name, series, start, end, {"title": d.get("song_title"), "seconds": end - start})


# ---------------------------------------------------------------- properties

def prop_of(address):
    return address.rsplit("/", 1)[-1]


def fixture_of(address):
    parts = address.split("/")
    return parts[2] if len(parts) > 2 else address


def class_of(address):
    p = prop_of(address)
    if p in ("dimmer", "innerDimmer"):
        return "brightness"
    if p == "color":
        return "colour"
    if p in ("panAngle", "tiltAngle"):
        return "pan/tilt"
    if p in ("zoomAngle", "zoom", "focus"):
        return "zoom/focus"
    return "other"


def range_of(address, fixtures_by_name):
    """The span a property's value moves over, so that a difference is a share of the range (angles in degrees
    against the fixture's own range)."""
    p = prop_of(address)
    fx = fixtures_by_name.get(fixture_of(address)) or {}
    if p == "panAngle":
        return float(fx.get("panRange") or 1.0)
    if p == "tiltAngle":
        return float(fx.get("tiltRange") or 1.0)
    if p == "zoomAngle":
        for c in fx.get("capabilities", []):
            if c.get("kind") == "Zoom" and c.get("settings"):
                prm = c["settings"][0].get("params", {})
                span = abs(prm.get("maxAngle", 0) - prm.get("minAngle", 0))
                if span:
                    return float(span)
        return 1.0
    if p.startswith(("gobo", "prism")) and "Angle" in p:
        return 360.0
    if p == "strobeSpeed":
        return 20.0
    return 1.0


def _is_number(x):
    return isinstance(x, (int, float)) and not isinstance(x, bool)


def distance(x, y, rng):
    """How far apart two argument lists are, as a share of the range: the largest difference of any number,
    1.0 for any other argument (a name, an option) that differs."""
    if x is None or y is None:
        return None
    if len(x) != len(y):
        return 1.0
    e = 0.0
    for p, q in zip(x, y):
        d = abs(p - q) / rng if _is_number(p) and _is_number(q) else (0.0 if p == q else 1.0)
        if d > e:
            e = d
    return min(e, 1.0)


def scalar(args, rng):
    """One number for the alignment: the arguments' sum as a share of the range, or None."""
    if not args or not all(_is_number(a) for a in args):
        return None
    return sum(args) / rng


# ---------------------------------------------------------------- alignment

def _fine_array(source, address, rng, origin, count):
    """The scalar of a series on the FINE grid from `origin`, None where there is no value."""
    s = source.series.get(address)
    out = [None] * count
    if s is None:
        return out
    times, vals = s
    lo = max(0, int(math.ceil((source.start - origin) / FINE - 1e-9)))
    hi = min(count - 1, int(math.floor((source.end - origin) / FINE + 1e-9)))
    e = 0
    cur = None
    for j in range(lo, hi + 1):
        t = origin + j * FINE + 1e-9
        while e < len(times) and times[e] <= t:
            cur = scalar(vals[e], rng)
            e += 1
        out[j] = cur
    return out


def _cost(a_idx, a_val, b, lag):
    total, n = 0.0, 0
    size = len(b)
    for k in range(len(a_idx)):
        j = a_idx[k] + lag
        if 0 <= j < size:
            y = b[j]
            if y is not None:
                d = a_val[k] - y
                total += d if d >= 0 else -d
                n += 1
    return total, n


def _ties(costs, tolerance, slack):
    """The lags whose cost is as good as the best (within `tolerance` of it, plus `slack`)."""
    best = min(c for c, _ in costs)
    return sorted(lag for c, lag in costs if c <= best * (1.0 + tolerance) + slack)


def _plateau_centre(costs, near, tolerance=0.02, slack=1e-9):
    """The middle of the run of lags whose cost is within 2% of the best (frames arrive every 25 ms, so a value
    that steps matches over a stretch of lags, not at one). If there are several runs, the one closest to `near`."""
    ok = _ties(costs, tolerance, slack)
    runs, cur = [], [ok[0]]
    for lag in ok[1:]:
        if lag == cur[-1] + 1:
            cur.append(lag)
        else:
            runs.append(cur)
            cur = [lag]
    runs.append(cur)
    run = min(runs, key=lambda r: (min(abs(x - near) for x in r), r[0]))
    return run[(len(run) - 1) // 2] if len(run) % 2 else (run[len(run) // 2 - 1] + run[len(run) // 2]) // 2


def _exact_costs(a, b, picks, lo, hi, lags, limit):
    """(share of samples more than one DMX step apart, lag) for each lag in FINE steps, over the given
    (address, range) pairs; B read at A's time plus the lag. Samples are spread over the window at an interval
    that is not a multiple of the 25 ms frame period, so every phase of a frame is judged."""
    n = int(math.floor((hi - lo) / FINE + 1e-9)) + 1
    stride = max(1, int(math.ceil(n / float(limit))))
    if stride % 5 == 0:
        stride += 1
    times = [lo + k * FINE for k in range(0, n, stride)]
    a_vals = [[a.value(addr, t) for t in times] for addr, _rng in picks]
    out = []
    for lag in lags:
        over = total = 0
        for (addr, rng), column in zip(picks, a_vals):
            for t, x in zip(times, column):
                d = distance(x, b.value(addr, t + lag * FINE), rng)
                if d is not None:
                    total += 1
                    over += d > DMX_STEP + 1e-9
        out.append((over / total if total else float("inf"), lag))
    return out


def align(a, b, rng_of, lo, hi, guess, max_offset, signals=32):
    """Offset (seconds, B is later than A by this) so that B(s + offset) matches A(s), and the largest spread of
    the per-property offsets around it. Returns (offset, spread, spread_address, note)."""
    common = [x for x in a.series if x in b.series]
    n_a = int(math.floor((hi - lo) / FINE)) + 1
    movers = []
    for addr in common:
        rng = rng_of(addr)
        arr = _fine_array(a, addr, rng, lo, n_a)
        prev, move = None, 0.0
        for v in arr[::5]:                  # every 25 ms
            if v is not None:
                if prev is not None:
                    move += abs(v - prev)
                prev = v
        if move > 2 * DMX_STEP:
            movers.append((move, addr, arr, rng))
    movers.sort(key=lambda m: (-m[0], m[1]))
    movers = movers[:signals]
    if not movers:
        return 0.0, 0.0, None, "nothing moves in the window, so there is nothing to line up; offset taken as 0"
    # B on the same fine grid, from where the search can reach
    reach = int(round(max_offset / FINE))
    centre = int(round(guess / FINE))
    b_origin = lo + (centre - reach) * FINE
    b_count = n_a + 2 * reach + 2
    for k, m in enumerate(movers):
        movers[k] = m + (_fine_array(b, m[1], m[3], b_origin, b_count),)
    picked = {}

    def evaluate(idx_stride, lags, use):
        """Mean absolute difference (A against B shifted by `lag` FINE steps) for each lag. B index of A index i
        is i + lag - (centre - reach)."""
        costs = []
        for lag in lags:
            tot, n = 0.0, 0
            for mover in use:
                arr, barr = mover[2], mover[4]
                key = (mover[1], idx_stride)
                if key not in picked:
                    idx = [i for i in range(0, n_a, idx_stride) if arr[i] is not None]
                    picked[key] = (idx, [arr[i] for i in idx])
                c, m = _cost(picked[key][0], picked[key][1], barr, lag - (centre - reach))
                tot += c
                n += m
            costs.append((tot / n if n >= 10 else float("inf"), lag))
        return costs

    stride_c = max(10, int(math.ceil(n_a / 300.0)))
    step_c = 10
    coarse = evaluate(stride_c, range(centre - reach, centre + reach + 1, step_c), movers)
    best_cost = min(c for c, _ in coarse)
    if best_cost == float("inf"):
        return 0.0, 0.0, None, "the two recordings do not overlap, so they cannot be lined up; offset taken as 0"
    # The best few local minima, and the ones nearest the guess, are candidates (a looping show matches itself one loop later too).
    # They are tried on the real 25 ms grid: the offset with the fewest samples more than one DMX step apart wins
    # (frames come every 25 ms, so a stretch of lags ties; the middle of it is taken), and of the ones within half
    # a percent of each other (a flash or two) the offset nearest the guess.
    picks = [(m[1], m[3]) for m in movers]
    minima = [(c, lag) for k, (c, lag) in enumerate(coarse)
              if c <= best_cost * 1.5 + 0.02 and (k == 0 or c <= coarse[k - 1][0]) and (k == len(coarse) - 1 or c <= coarse[k + 1][0])]
    cands = [lag for _c, lag in sorted(minima)[:4]]
    cands += [lag for _c, lag in sorted(minima, key=lambda m: (abs(m[1] - centre), m[1])) if lag not in cands][:2]
    tried = []
    for lag in cands:
        costs = _exact_costs(a, b, picks, lo, hi, range(lag - step_c, lag + step_c + 1), 600 if len(cands) > 1 else 1500)
        tried.append((min(c for c, _ in costs), _plateau_centre(costs, lag, 0.0, 0.0005)))
    top = min(c for c, _ in tried)
    lag0 = min((lag for c, lag in tried if c <= top + 0.005), key=lambda lag: (abs(lag - centre), lag))
    if len(cands) > 1:
        lag1 = _plateau_centre(_exact_costs(a, b, picks, lo, hi, range(lag0 - 10, lag0 + 11), 1500), lag0, 0.0, 0.0005)
    else:
        lag1 = lag0
    # each moving property on its own, within +-100 ms of that
    spread, where = 0, None
    for pick in picks:
        res = _exact_costs(a, b, [pick], lo, hi, range(lag1 - 20, lag1 + 21), 800)
        ok = _ties(res, 0.0, 0.0005)       # an address whose best lags include the global one is not an outlier
        off = 0 if lag1 in ok else min(abs(x - lag1) for x in ok)
        if off > spread:
            spread, where = off, pick[0]
    return lag1 * FINE, spread * FINE, where, None


# ---------------------------------------------------------------- comparison

def percentile(sorted_values, p):
    if not sorted_values:
        return 0.0
    return sorted_values[max(0, min(len(sorted_values) - 1, int(math.ceil(p * len(sorted_values))) - 1))]


def count_flashes(samples, rng):
    """One-frame flashes: a value leaves where it was by more than FLASH_OUT for a single sample and comes back."""
    n = 0
    for i in range(2, len(samples)):
        x0, x1, x2 = samples[i - 2], samples[i - 1], samples[i]
        if x0 is None or x1 is None or x2 is None:
            continue
        if distance(x2, x0, rng) <= FLASH_BACK and distance(x1, x0, rng) > FLASH_OUT:
            n += 1
    return n


def compare_sources(a, b, fixtures_by_name, lo, hi, offset):
    """Per class and per address statistics over the 25 ms grid samples of A's window [lo, hi]."""
    steps = int(math.floor((hi - lo) / GRID + 1e-9)) + 1
    grid = [lo + k * GRID for k in range(steps)]
    per_class = {c: {"errors": [], "fixtures": set(), "flash_a": 0, "flash_b": 0, "addresses": 0} for c in CLASSES}
    per_address = {}
    common = sorted(x for x in a.series if x in b.series)
    for addr in common:
        rng = range_of(addr, fixtures_by_name)
        cls = class_of(addr)
        sa, sb, errs = [], [], []
        for t in grid:
            x, y = a.value(addr, t), b.value(addr, t + offset)
            sa.append(x)
            sb.append(y)
            if x is not None and y is not None:
                errs.append((distance(x, y, rng), t, x, y))
        if not errs:
            continue
        c = per_class[cls]
        c["errors"].extend(e[0] for e in errs)
        c["fixtures"].add(fixture_of(addr))
        c["addresses"] += 1
        c["flash_a"] += count_flashes(sa, rng)
        c["flash_b"] += count_flashes(sb, rng)
        values = [e[0] for e in errs]
        per_address[addr] = {
            "samples": len(errs),
            "over_step": sum(1 for v in values if v > DMX_STEP + 1e-9) / len(errs),
            "over_002": sum(1 for v in values if v > BIG) / len(errs),
            "max": max(values),
            "worst": errs,
        }
    return per_class, per_address, common


def summarise(per_class, minutes):
    rows = []
    for cls in CLASSES:
        c = per_class[cls]
        errs = sorted(c["errors"])
        n = len(errs)
        if not n:
            rows.append({"class": cls, "fixtures": 0, "addresses": 0, "samples": 0})
            continue
        fx = max(len(c["fixtures"]), 1)
        rows.append({
            "class": cls, "fixtures": len(c["fixtures"]), "addresses": c["addresses"], "samples": n,
            "over_step": sum(1 for e in errs if e > DMX_STEP + 1e-9) / n,
            "over_002": sum(1 for e in errs if e > BIG) / n,
            "p50": percentile(errs, 0.50), "p90": percentile(errs, 0.90), "p99": percentile(errs, 0.99),
            "max": errs[-1],
            "flash_a": c["flash_a"] / fx / minutes, "flash_b": c["flash_b"] / fx / minutes,
            "flashes_a_total": c["flash_a"], "flashes_b_total": c["flash_b"],
        })
    return rows


def examples(info, count=3):
    """The largest differences of one address, at least 0.2 s apart: (time, A value, B value)."""
    out = []
    for e, t, x, y in sorted(info["worst"], key=lambda r: -r[0]):
        if e <= DMX_STEP:
            break
        if all(abs(t - u[0]) >= 0.2 for u in out):
            out.append((t, x, y))
        if len(out) == count:
            break
    return out


def judge(rows, offset):
    """(indistinguishable?, [reasons it is not]). One DMX step for 99% of samples, no flashes B has and A has
    not, a constant offset under one grid step (25 ms)."""
    why = []
    for r in rows:
        if not r.get("samples"):
            continue
        if r["over_step"] > INDISTINGUISHABLE_SHARE:
            why.append("%s: %.1f%% of samples are more than one DMX step apart (limit 1%%)" % (r["class"], 100 * r["over_step"]))
        if r["flashes_b_total"] > r["flashes_a_total"]:
            why.append("%s: B has %d one-frame flashes, A has %d" % (r["class"], r["flashes_b_total"], r["flashes_a_total"]))
    if abs(offset) * 1000.0 >= 25.0:
        why.append("the constant offset is %.0f ms (limit 25 ms)" % (offset * 1000.0))
    return (not why), why


def analyse(fixtures_path, a_path, b_path=None, saved_path=None, helper=None, lo=None, hi=None, align_mode="auto",
            max_offset=10.0, guess=None):
    """Everything compare.py reports, as data. A is always a capture, or the saved song when `saved_path` is
    given (then B is the capture: the song is what was meant, the capture is what Lightkey did)."""
    lk = load_helper(helper)
    try:
        with open(fixtures_path) as f:
            fmap = lk.expand_map(json.load(f))
        fx = lk.Fixtures(fmap)
    except (OSError, ValueError, KeyError) as e:
        raise CompareError("cannot read the fixture map %s: %s" % (fixtures_path, e))
    by_name = {f["name"]: f for f in fx.fixtures}
    if saved_path:
        a = load_song_grid(saved_path, os.path.basename(saved_path))
        b = decode_capture(fx, load_capture(a_path), os.path.basename(a_path))
    else:
        a = decode_capture(fx, load_capture(a_path), os.path.basename(a_path))
        b = decode_capture(fx, load_capture(b_path), os.path.basename(b_path))
    lo = a.start if lo is None else max(lo, a.start)
    hi = a.end if hi is None else min(hi, a.end)
    if hi - lo < 1.0:
        raise CompareError("the window is under one second long (A covers %.1f to %.1f s)" % (a.start, a.end))
    rng_of = lambda addr: range_of(addr, by_name)
    note = None
    if align_mode == "auto":
        g = b.start - lo if guess is None else guess
        offset, spread, spread_addr, note = align(a, b, rng_of, lo, hi, g, max_offset)
    elif align_mode == "none":
        offset, spread, spread_addr = 0.0, 0.0, None
    else:
        try:
            offset, spread, spread_addr = float(align_mode), 0.0, None
        except ValueError:
            raise CompareError("--align is auto, none, or a number of seconds")
    per_class, per_address, common = compare_sources(a, b, by_name, lo, hi, offset)
    rows = summarise(per_class, (hi - lo) / 60.0)
    ok, why = judge(rows, offset)
    return {
        "a": a, "b": b, "saved": bool(saved_path), "window": (lo, hi), "offset": offset, "spread": spread,
        "spread_address": spread_addr, "note": note, "rows": rows, "addresses": per_address,
        "only_a": len([x for x in a.series if x not in b.series]), "only_b": len([x for x in b.series if x not in a.series]),
        "indistinguishable": ok, "reasons": why, "compared": len(per_address),
    }


# ---------------------------------------------------------------- report

def describe(src):
    i = src.info
    if "frames" in i:
        return "%s (%d frames, %.1f s, universes %s)" % (src.name, i["frames"], i["seconds"], ",".join(map(str, i["universes"])))
    return "%s (saved song%s, %.1f s)" % (src.name, ": " + i["title"] if i.get("title") else "", i["seconds"])


def fmt_args(v):
    return "[" + ", ".join(("%.3f" % x) if isinstance(x, float) else str(x) for x in v) + "]"


def report(r, out=None):
    out = out or sys.stdout
    p = lambda s="": print(s, file=out)
    a, b = r["a"], r["b"]
    lo, hi = r["window"]
    p("A: " + describe(a))
    p("B: " + describe(b))
    p("Window (A seconds): %.1f to %.1f, compared on a %d ms grid; %d addresses in both (only in A %d, only in B %d)" % (
        lo, hi, round(GRID * 1000), r["compared"], r["only_a"], r["only_b"]))
    if r["note"]:
        p("Alignment: " + r["note"])
    else:
        later = r["offset"] * 1000.0
        p("Alignment: B at (t %+.0f ms) matches A at t: B is %s by %.0f ms. Largest per-property offset spread: %.0f ms%s" % (
            later, "later" if later >= 0 else "earlier", abs(later), r["spread"] * 1000.0,
            " (%s)" % r["spread_address"] if r["spread_address"] else ""))
    p()
    p("Differences are shares of each property's range; 'steps' = 1/255 of it (angles in degrees against the fixture range).")
    head = "%-11s %5s %8s %8s %8s %7s %7s %7s %8s  %s" % (
        "class", "fixt", "samples", ">1 step", ">0.02", "p50", "p90", "p99", "max", "flashes/fixture/min A | B")
    p(head)
    p("-" * len(head))
    for row in r["rows"]:
        if not row.get("samples"):
            p("%-11s %5d   (nothing to compare)" % (row["class"], 0))
            continue
        p("%-11s %5d %8d %7.2f%% %7.2f%% %7.1f %7.1f %7.1f %8.1f  %.1f | %.1f" % (
            row["class"], row["fixtures"], row["samples"], 100 * row["over_step"], 100 * row["over_002"],
            row["p50"] / DMX_STEP, row["p90"] / DMX_STEP, row["p99"] / DMX_STEP, row["max"] / DMX_STEP,
            row["flash_a"], row["flash_b"]))
    p("(p50/p90/p99/max in DMX steps)")
    worst = sorted(r["addresses"].items(), key=lambda kv: (-kv[1]["over_step"], -kv[1]["max"], kv[0]))
    worst = [w for w in worst if w[1]["over_step"] > 0][:5]
    p()
    if worst:
        p("Worst addresses (share of samples more than one step apart):")
        for addr, info in worst:
            p("  %s: %.1f%% of %d samples, max %.1f steps" % (addr, 100 * info["over_step"], info["samples"], info["max"] / DMX_STEP))
            for t, x, y in examples(info):
                p("      at %.3f s  A %s  B %s" % (t, fmt_args(x), fmt_args(y)))
    else:
        p("No address differs by more than one DMX step.")
    p()
    if r["indistinguishable"]:
        p("INDISTINGUISHABLE: at most one DMX step apart for 99%% of samples in every class, no flashes B has and A has not, offset %.0f ms (< 25 ms)." % (abs(r["offset"]) * 1000.0))
    else:
        p("NOT INDISTINGUISHABLE:")
        for why in r["reasons"]:
            p("  - " + why)


def main(argv=None):
    ap = argparse.ArgumentParser(description="Compare the raw sACN output of two Lightkey runs, or one run and a saved song.")
    ap.add_argument("--fixtures", required=True, help="the helper's fixture map (fixtures.json)")
    ap.add_argument("--a", required=True, help="raw capture A (sniff.py output); the original run")
    ap.add_argument("--b", help="raw capture B (sniff.py output); the replay")
    ap.add_argument("--saved", metavar="SONG_GRID", help="instead of --b: a saved song's grid (examples/dump_song.rs); "
                    "the song is then the reference and --a the capture of its replay")
    ap.add_argument("--from", dest="lo", type=float, help="window start, seconds on the reference side's clock")
    ap.add_argument("--to", dest="hi", type=float, help="window end, seconds on the reference side's clock")
    ap.add_argument("--align", default="auto", help="auto (default), none, or a fixed offset in seconds (B later than A)")
    ap.add_argument("--max-offset", type=float, default=10.0, help="seconds either side of the guess that auto searches (default 10)")
    ap.add_argument("--align-guess", type=float, help="where auto starts searching, seconds (default: lines the window start up with B's start)")
    ap.add_argument("--helper", help="path of lightkey_api.py (default: the one in this repository)")
    ap.add_argument("--check", action="store_true", help="exit 1 when the runs are not indistinguishable")
    args = ap.parse_args(argv)
    if bool(args.b) == bool(args.saved):
        ap.error("give exactly one of --b and --saved")
    try:
        r = analyse(args.fixtures, args.a, args.b, args.saved, args.helper, args.lo, args.hi, args.align,
                    args.max_offset, args.align_guess)
    except CompareError as e:
        print("error: %s" % e, file=sys.stderr)
        return 2
    report(r)
    return 0 if (r["indistinguishable"] or not args.check) else 1


if __name__ == "__main__":
    sys.exit(main())
