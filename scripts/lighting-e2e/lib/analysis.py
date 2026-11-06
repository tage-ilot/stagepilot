"""Numbers from what the fake Lightkey received, against the original take. Pure Python."""
from __future__ import annotations

import bisect
import statistics

LEAD = 0.045
LEVEL_TOL = 0.03
ANGLE_TOL = 1.5


def prop(addr):
    return addr.rsplit("/", 1)[-1]


def fixture(addr):
    return addr.split("/")[2]


def is_angle(addr):
    p = prop(addr).lower()
    return any(k in p for k in ("pan", "tilt", "angle", "rotat"))


def klass(addr, dim):
    p = prop(addr).lower()
    if is_angle(addr):
        return "angle"
    if "dimmer" in p or "intensity" in p:
        return "dimmer"
    if "color" in p and dim >= 3:
        return "colour"
    return "other level"


def tol(addr):
    return ANGLE_TOL if is_angle(addr) else LEVEL_TOL


def pct(xs, p):
    if not xs:
        return None
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(round(p / 100.0 * (len(xs) - 1))))]


def numeric(v):
    return [x for x in v if isinstance(x, (int, float)) and not isinstance(x, bool)]


class Raw:
    """The original take as a sample-and-hold series per address."""

    def __init__(self, take):
        self.t, self.v = {}, {}
        for a, v in take["initial"]:
            self.t.setdefault(a, []).append(-1.0)
            self.v.setdefault(a, []).append(v)
        for at, cmds in take["events"]:
            for a, v in cmds:
                self.t.setdefault(a, []).append(at)
                self.v.setdefault(a, []).append(v)

    def at(self, a, t):
        ts = self.t.get(a)
        if not ts:
            return None
        i = bisect.bisect_right(ts, t) - 1
        return self.v[a][max(i, 0)]

    def addresses(self):
        return self.t.keys()


def packets_in(packets, origin, lo=None, hi=None):
    """[(pos, addr, args)] for packets whose song position lies in [lo, hi)."""
    out = []
    for mono, wall, a, args in packets:
        p = wall - origin
        if (lo is None or p >= lo) and (hi is None or p < hi):
            out.append((p, a, args))
    return out


def rate_stats(pk, seconds):
    """Packets per song second: mean / p99 / max / seconds above 150, over [0, seconds)."""
    buckets = [0] * int(seconds)
    for p, a, _ in pk:
        if a.endswith("/clear"):
            continue
        i = int(p)
        if 0 <= i < len(buckets):
            buckets[i] += 1
    return {
        "packets": sum(buckets), "mean": sum(buckets) / max(1, len(buckets)),
        "p99": pct(buckets, 99), "max": max(buckets) if buckets else 0,
        "seconds_above_150": sum(1 for b in buckets if b > 150),
        "per_second": buckets,
    }


def effect_index(cue_map):
    """{(fixture, property): [(start, end)]} from the saved effects."""
    idx = {}
    for e in (cue_map or {}).get("effects") or []:
        props = {s["property"] for s in e.get("cycle", [])}
        for m in e.get("members", []):
            for pr in props:
                idx.setdefault((m["fixture"], pr), []).append((e["start_seconds"], e["end_seconds"]))
    return idx


def in_effect(idx, a, t):
    for s, e in idx.get((fixture(a), prop(a)), ()):
        if s <= t <= e:
            return True
    return False


def deviation(pk, raw, cue_map, duration, grid=0.05):
    """Held received series (a value takes effect LEAD after it arrives) against the raw take on a
    grid. Returns per class and place (outside/inside effects): n, max, p99, share beyond the
    tolerance."""
    eidx = effect_index(cue_map)
    series = {}
    for p, a, args in pk:
        if a.endswith("/clear"):
            continue
        series.setdefault(a, []).append((p + LEAD, args))
    res = {}
    worst = {}
    steps = int(duration / grid)
    for a, ser in series.items():
        if a not in raw.t:
            continue
        times = [s[0] for s in ser]
        for k in range(steps):
            t = k * grid
            i = bisect.bisect_right(times, t) - 1
            if i < 0:
                continue
            got = numeric(ser[i][1])
            want = numeric(raw.at(a, t) or [])
            if not got or len(got) != len(want):
                continue
            d = max(abs(x - y) for x, y in zip(got, want))
            key = (klass(a, len(got)), "inside" if in_effect(eidx, a, t) else "outside")
            r = res.setdefault(key, {"n": 0, "devs": [], "beyond": 0})
            r["n"] += 1
            r["devs"].append(d)
            if d > tol(a):
                r["beyond"] += 1
            if d > worst.get(key, (0, ""))[0]:
                worst[key] = (d, f"{a} @ {t:.2f}s")
    out = {}
    for key, r in res.items():
        out[f"{key[0]}/{key[1]}"] = {
            "n": r["n"], "max": max(r["devs"]), "p99": pct(r["devs"], 99),
            "beyond_pct": 100.0 * r["beyond"] / r["n"], "worst": worst[key][1],
        }
    return out


def step_rows(cue_map):
    """Instant (step) look messages of the saved rows: [(row_time, addr, [values])]. A step row
    that is only the start value of a ramp (a ramp row for the same address at the same time)
    is left out: replay evaluates the ramp's curve instead of sending that start value."""
    def values(m):
        return [x.get("value") if isinstance(x, dict) else x for x in (m.get("args") or [])]

    def messages(r):
        return r.get("messages") or ([{"address": r["address"], "args": r.get("args", [])}] if r.get("address") else [])

    rows = (cue_map or {}).get("osc_cues") or []
    ramp_starts = {(round(r["at_seconds"], 3), m["address"]) for r in rows if (r.get("fade_seconds") or 0) > 0 for m in messages(r)}
    out = []
    for r in rows:
        if (r.get("fade_seconds") or 0) > 0:
            continue
        for m in messages(r):
            if (round(r["at_seconds"], 3), m["address"]) in ramp_starts:
                continue
            out.append((r["at_seconds"], m["address"], values(m)))
    return out


def close(a, b, eps=2e-3):
    na, nb = numeric(a), numeric(b)
    return len(na) == len(nb) and all(abs(x - y) <= eps for x, y in zip(na, nb))


FIRST_LOOK_S = 0.05  # rows at the very start of the song leave as one burst; they cannot start early


def step_lateness(pk, cue_map, lo, hi):
    """For every saved step message with time in [lo, hi): the first packet for that address with
    that value within [t-0.25, t+3] -> lateness = packet position - row time (negative = early).
    The design target is -0.045. Also counts regressions: a packet carrying the value of an older
    step arriving after a packet carrying a newer step's value for that address (a stale value)."""
    by_addr_pk = {}
    for p, a, args in pk:
        by_addr_pk.setdefault(a, []).append((p, args))
    rows = {}
    for t, a, vals in step_rows(cue_map):
        rows.setdefault(a, []).append((t, vals))
    # ramps and effects send interpolated values that can equal an old step's value by chance
    # (a ramp ending where an earlier step began): packets inside them are not step values
    moving = {}
    for r in (cue_map or {}).get("osc_cues") or []:
        if (r.get("fade_seconds") or 0) > 0:
            for m in r.get("messages") or []:
                moving.setdefault(m["address"], []).append((r["at_seconds"] - 0.1, r["at_seconds"] + r["fade_seconds"] + 0.3))
    eidx = effect_index(cue_map)
    late, late_rest, missing, stale, considered = [], [], 0, 0, 0
    for a, lst in rows.items():
        lst.sort(key=lambda x: x[0])
        packets = by_addr_pk.get(a, [])
        times = [x[0] for x in packets]
        last_matched = -1
        for idx, (t, vals) in enumerate(lst):
            if not (lo <= t < hi):
                continue
            considered += 1
            i = bisect.bisect_left(times, t - 0.25)
            found = None
            while i < len(packets) and packets[i][0] <= t + 3.0:
                if close(packets[i][1], vals):
                    found = (i, packets[i][0])
                    break
                i += 1
            if found is None:
                missing += 1
                continue
            late.append(found[1] - t)
            if t >= FIRST_LOOK_S:
                late_rest.append(found[1] - t)
        # stale: packets that equal an older row's value after a newer row was due
        row_times = [x[0] for x in lst]
        for pi, (p, args) in enumerate(packets):
            if not (lo <= p < hi):
                continue
            if any(a0 <= p <= b0 for a0, b0 in moving.get(a, ())) or in_effect(eidx, a, p):
                continue
            due = bisect.bisect_right(row_times, p + LEAD + 0.005) - 1
            if due < 1:
                continue
            for back in range(max(0, due - 8), due):
                if close(args, lst[back][1]) and not close(args, lst[due][1]) and p > lst[due][0] - LEAD + 0.05:
                    # carried an older step's value while a different, newer step was already due
                    # (and no ramp/effect value can be mistaken for it: exact match required)
                    stale += 1
                    break
    return {
        "considered": considered, "matched": len(late), "unmatched": missing,
        "median_ms": 1000 * statistics.median(late) if late else None,
        "p99_ms": 1000 * pct(late, 99) if late else None,
        "max_ms": 1000 * max(late) if late else None,
        "min_ms": 1000 * min(late) if late else None,
        "rest_matched": len(late_rest),
        "rest_median_ms": 1000 * statistics.median(late_rest) if late_rest else None,
        "rest_p99_ms": 1000 * pct(late_rest, 99) if late_rest else None,
        "rest_max_ms": 1000 * max(late_rest) if late_rest else None,
        "rest_min_ms": 1000 * min(late_rest) if late_rest else None,
        "rest_late_over_100ms": sum(1 for x in late_rest if x > 0.1 - LEAD),
        "first_look_max_ms": 1000 * max((x for x in late if x not in late_rest), default=0.0),
        "stale_packets": stale,
        "late_over_100ms": sum(1 for x in late if x > 0.1 - LEAD),
        "values": late,
    }
