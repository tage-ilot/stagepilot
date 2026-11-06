#!/usr/bin/env python3
"""Pre-release end-to-end lighting test: fake relay + fake Lightkey + fake Playback, a real
StagePilot backend driven over HTTP only. See README.md. Stages write JSON under <out>/results."""
from __future__ import annotations

import argparse
import asyncio
import os
import gzip
import json
import re
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from lib import analysis as an  # noqa: E402
from lib.rig import DEMO, PB_ID, TAKE_FOR, Rig, iso, load_takes, log  # noqa: E402


# ------------------------------------------------------------------ helpers
class Probe:
    """Process counters around a stretch of the test."""

    def __init__(self, rig):
        self.rig = rig

    def __enter__(self):
        self.a = self.rig.backend.stats()
        self.t0 = time.time()
        return self

    def __exit__(self, *e):
        self.b = self.rig.backend.stats()

    def result(self, song_seconds):
        a, b = self.a, self.b
        return {
            "rss_before_kb": a["rss_kb"], "rss_after_kb": b["rss_kb"], "rss_delta_kb": b["rss_kb"] - a["rss_kb"],
            "threads_before": a["threads"], "threads_after": b["threads"],
            "fds_before": a["fds"], "fds_after": b["fds"],
            "cpu_s": round(b["cpu_s"] - a["cpu_s"], 3),
            "cpu_ms_per_song_second": round(1000 * (b["cpu_s"] - a["cpu_s"]) / max(1.0, song_seconds), 3),
        }


def save(out: Path, name: str, data):
    (out / "results").mkdir(parents=True, exist_ok=True)
    (out / "results" / f"{name}.json").write_text(json.dumps(data, indent=1, default=str))


def save_packets(out: Path, name: str, packets, origin):
    (out / "raw").mkdir(parents=True, exist_ok=True)
    with gzip.open(out / "raw" / f"{name}.packets.jsonl.gz", "wt") as f:
        for mono, wall, a, args in packets:
            f.write(json.dumps([round(wall - origin, 4), a, args]) + "\n")


def origin_now(rig):
    """The running timer's song-time zero on the wall clock, from the backend's own timer."""
    t = rig.state()["timer"]
    if t["status"] != "running" or not t.get("started_at"):
        return None
    return iso(t["started_at"]) - (t.get("position_seconds") or 0.0)


def summarize_cue_map(cm):
    if not cm:
        return None
    rows = [r for r in cm.get("osc_cues", []) if r.get("source") == "captured"]
    msgs = sum(len(r.get("messages") or []) for r in rows)
    return {
        "rows": len(rows), "effects": len(cm.get("effects") or []), "messages": msgs,
        "items": len(rows) + len(cm.get("effects") or []),
        "ramps": sum(1 for r in rows if r.get("fade_seconds", 0) > 0),
        "capture": cm.get("capture"), "json_bytes": len(json.dumps(cm)),
        "first_row": min((r["at_seconds"] for r in rows), default=None),
        "last_row": max((r["at_seconds"] for r in rows), default=None),
    }


def grab_log(rig):
    lines = rig.backend.log_lines()
    bad = [l for l in lines if re.search(r"\b(ERROR|WARN)\b", l) and "planning_center_oauth_refresh_unavailable" not in l
           and "settings_migration" not in l]
    return {"lines": len(lines), "error_warn": bad[:40], "error_warn_count": len(bad)}


async def prepare_relay(rig, take):
    rig.set_relay_state(take["initial"])
    await rig.wait_link()
    await asyncio.sleep(1.5)


async def settle(rig):
    """A settings change (Auto/Replay) makes the link restart its relay session: wait until the
    fixture stream is live again (keyframe received) before a song starts, or the capture's baseline
    is empty. Record is only a switch and needs no wait."""
    await asyncio.sleep(0.3)
    await rig.wait_link()
    await asyncio.sleep(float(os.environ.get("E2E_SETTLE_S", "40")))


async def status_message(rig):
    s = rig.lk_status()
    return s.get("message")


async def finish_song(rig, origin, key):
    """Let the song run to its length, then report the end to StagePilot (Playback stopped at the end)."""
    length = DEMO[key][1]
    await rig.sleep_until_song(origin, length - 0.2)
    rig.pb.end(length)


# ------------------------------------------------------------------ stage: capture
async def capture_song(rig, take, key, out, tag, record=True):
    await prepare_relay(rig, take)
    rig.set_mode("record" if record else "auto")
    await settle(rig)
    start_status = rig.lk_status()
    with Probe(rig) as pr:
        origin = await rig.play(key)
        t_start = time.time()
        mid = []

        async def sample():
            while True:
                await asyncio.sleep(4)
                m = await status_message(rig)
                if m != (mid[-1]["message"] if mid else None) or len(mid) % 8 == 0:
                    mid.append({"t": round(time.time() - origin, 1), "message": m,
                                "rss_kb": rig.backend.stats()["rss_kb"]})
        sampler = asyncio.create_task(sample())
        sent = await rig.feed(take, origin)
        await finish_song(rig, origin, key)
        t_end = time.time()
        sampler.cancel()
        try:
            seen = []

            def save_message():
                m = rig.lk_status().get("message")
                if m and (not seen or seen[-1] != m):
                    seen.append(m)
                return m if m and (m.startswith("Saved") or m.startswith("Not saved")) else None
            saved = await rig.wait(save_message, 60, 0.1, "save message")
        except TimeoutError:
            log("no save message; status:", json.dumps(rig.lk_status())[:1500], "timer:", json.dumps(rig.state().get("timer")), "messages after the end:", json.dumps(seen), "samples:", json.dumps([x for x in mid if x["message"]])[:1200], "log:", json.dumps(grab_log(rig))[:800])
            raise
        save_latency = time.time() - t_end
    st = rig.lk_status()
    cm = rig.cue_map(key)
    res = {
        "tag": tag, "song": key, "take": take["title"], "events_sent": sent, "save_message": saved,
        "save_latency_s": round(save_latency, 3), "status_after": st, "status_during": mid,
        "start_status_message": start_status.get("message"), "cue_map": summarize_cue_map(cm),
        "lost_packets": st["fixtures"]["lost_packets"], "probe": pr.result(DEMO[key][1]),
        "log": grab_log(rig), "relay_subscribe_rx": sum(1 for _, a in rig.relay.rx_log if a == "/relay/subscribe"),
    }
    (out / "raw").mkdir(parents=True, exist_ok=True)
    (out / "raw" / f"cue_map_{key}_{tag}.json").write_text(json.dumps(cm, indent=1))
    log(f"capture {take['title']}: {saved} -> {res['cue_map']}")
    return res


async def stage_capture(rig, takes, out):
    results = []
    for title, key in TAKE_FOR.items():
        results.append(await capture_song(rig, takes[title], key, out, "capture"))
        rig.set_mode("auto")
        await asyncio.sleep(2)
    save(out, "capture", results)
    return results


# ------------------------------------------------------------------ stage: replay
async def replay_song(rig, take, key, out, tag, with_analysis=True):
    cm = rig.cue_map(key)
    rig.set_mode("replay")
    await rig.wait_link()
    await asyncio.sleep(1)
    mark = rig.lk.mark()
    with Probe(rig) as pr:
        origin = await rig.play(key)
        await finish_song(rig, origin, key)
        await asyncio.sleep(1.5)
    pk_all = rig.lk.packets[mark:]
    save_packets(out, f"replay_{tag}", pk_all, origin)
    pk = an.packets_in(pk_all, origin)
    dur = take["duration"]
    res = {
        "tag": tag, "song": key, "take": take["title"], "echoed": rig.echoed, "probe": pr.result(DEMO[key][1]),
        "status_after": rig.lk_status(), "log": grab_log(rig), "bad_osc": rig.lk.bad,
        "clears": sum(1 for _, a, _ in pk if a.endswith("/clear")),
    }
    if with_analysis:
        raw = an.Raw(take)
        rs = an.rate_stats(pk, DEMO[key][1])
        res["rate"] = {k: v for k, v in rs.items() if k != "per_second"}
        res["rate_per_second"] = rs["per_second"]
        res["rate_take_window"] = {k: v for k, v in an.rate_stats(pk, dur).items() if k != "per_second"}
        res["deviation"] = an.deviation(pk, raw, cm, dur)
        sl = an.step_lateness(pk, cm, 0.0, DEMO[key][1])
        res["steps"] = {k: v for k, v in sl.items() if k != "values"}
        res["step_late_ms_values"] = [round(1000 * x, 1) for x in sl["values"]]
        got = {a for _, a, _ in pk if not a.endswith("/clear")}
        res["addresses"] = {"in_take": len(list(raw.addresses())), "driven": len(got),
                            "driven_not_in_take": len(got - set(raw.addresses()))}
    log(f"replay {take['title']}: {res.get('rate_take_window')}")
    return res


async def stage_replay(rig, takes, out):
    results = []
    for title, key in TAKE_FOR.items():
        results.append(await replay_song(rig, takes[title], key, out, f"replay-{key}"))
    save(out, "replay", results)
    return results


# ------------------------------------------------------------------ stage: control (pause/seek/take-over/release/stop)
def fixtures_of(pk):
    return {an.fixture(a) for _, a, _ in pk if not a.endswith("/clear")}


async def stage_control(rig, takes, out):
    title = os.environ.get("E2E_CONTROL_SONG", "Able")
    key = TAKE_FOR[title]
    take = takes[title]
    cm = rig.cue_map(key)
    raw = an.Raw(take)
    rig.set_mode("replay")
    await rig.wait_link()
    res = {"song": key}
    mark = rig.lk.mark()
    origin = await rig.play(key)
    T = lambda p: origin + p  # song position -> wall

    # ---- operator take-over: as soon as some fixture property is being driven steadily, inject an
    # unexpected DMX change for it into the relay
    target_addr = None
    for sec in range(8, 150):
        await rig.sleep_until_song(origin, float(sec))
        recent = [x for x in an.packets_in(rig.lk.packets[mark:], origin, sec - 5.0, sec)
                  if not x[1].endswith("/clear") and all(isinstance(v, float) for v in x[2])]
        counts = {}
        for p, a, args in recent:
            counts[a] = counts.get(a, 0) + 1
        if counts and max(counts.values()) >= 12:
            target_addr = max(counts, key=counts.get)
            last = [x for x in recent if x[1] == target_addr][-1][2]
            break
    assert target_addr, "nothing was driven steadily in the first 150 s"
    target = an.fixture(target_addr)
    inj = [(0.2 if abs(v - 0.2) > 0.3 else 0.9) if not an.is_angle(target_addr) else float(v) + 25.0 for v in last]
    t_inj = time.time() - origin
    rig.relay.emit([(target_addr, inj)], tag=time.time() + rig.relay.skew)
    await asyncio.sleep(2.0)
    st_after = rig.lk_status()
    res["takeover"] = {"fixture": target, "address": target_addr, "injected": inj, "injected_at_s": round(t_inj, 3),
                       "status_2s_after": st_after, "packets_before_inj_5s": counts.get(target_addr),
                       "operator_state": rig.state().get("lights_status")}
    # no further operator change: take back expected 30 s after the injection
    await rig.sleep_until_song(origin, t_inj + 33.0)
    pk = an.packets_in(rig.lk.packets[mark:], origin)
    pk = [x for x in pk if not x[1].endswith("/clear")]
    f_after = [p for p, a, _ in pk if an.fixture(a) == target and p > t_inj + 0.3]
    f_last_before = max([p for p, a, _ in pk if an.fixture(a) == target and p <= t_inj + 0.3], default=None)
    others_rate_before = sum(1 for p, a, _ in pk if an.fixture(a) != target and t_inj - 5 <= p < t_inj) / 5.0
    others_rate_during = sum(1 for p, a, _ in pk if an.fixture(a) != target and t_inj + 1 <= p < t_inj + 29) / 28.0
    first_back = min(f_after, default=None)
    res["takeover"].update({
        "target_last_packet_before_s": f_last_before,
        "target_packets_in_quiet_window": sum(1 for p in f_after if p < t_inj + 29.8),
        "target_first_packet_after_s": first_back,
        "taken_back_after_s": None if first_back is None else round(first_back - t_inj, 2),
        "other_fixtures_pkts_per_s_before": round(others_rate_before, 1),
        "other_fixtures_pkts_per_s_during": round(others_rate_during, 1),
        "status_33s": rig.lk_status(),
    })
    if first_back is not None:
        v = [x for x in pk if x[0] == first_back][0]
        res["takeover"]["first_back_value"] = v[2]
        res["takeover"]["raw_value_then"] = raw.at(v[1], v[0] + an.LEAD)

    save_packets(out, "control_takeover", rig.lk.packets[mark:], origin)
    # ---- pause for 8 s
    mark_p = rig.lk.mark()
    rig.pb.pause()
    t_pause = time.time()
    await asyncio.sleep(8.0)
    t_resume_cmd = time.time()
    held = [w for _, w, a, _ in rig.lk.packets[mark_p:] if t_pause + 0.4 <= w < t_resume_cmd]
    clears_pause = [a for _, w, a, _ in rig.lk.packets[mark_p:] if a.endswith("/clear") and t_pause <= w < t_resume_cmd]
    res["pause"] = {"paused_s": round(t_resume_cmd - t_pause, 2), "packets_while_paused_after_0.4s": len(held),
                    "packets_in_first_0.4s_after_pause": sum(1 for _, w, a, _ in rig.lk.packets[mark_p:] if t_pause <= w < t_pause + 0.4),
                    "clears_while_paused": len(clears_pause), "status_paused": rig.lk_status()}
    mark2 = rig.lk.mark()
    rig.pb.play()
    await asyncio.sleep(0.3)
    org2 = origin_now(rig)
    await asyncio.sleep(1.2)
    pk2 = an.packets_in(rig.lk.packets[mark2:], org2)
    res["resume"] = {"origin_shift_s": round(org2 - origin, 3), "packets_first_1.5s": len(pk2),
                     "addresses_first_1.5s": len({a for _, a, _ in pk2 if not a.endswith('/clear')}),
                     "max_dev_at_resume+1.5s": resync_dev(rig.lk.packets[mark:], raw, rig_pos(org2), cm)}

    # ---- seek forward to 200 s while playing
    await rig.sleep_until_song(org2, rig_pos(org2) + 10.0)
    seek_to = 200.0 if rig_pos(org2) < 150 else 250.0
    mark3 = rig.lk.mark()
    rig.pb.seek(seek_to)
    await asyncio.sleep(0.4)
    org3 = origin_now(rig)
    await asyncio.sleep(1.6)
    pk3 = an.packets_in(rig.lk.packets[mark3:], org3, seek_to - 1.0, seek_to + 2.5)
    first3 = min((p for p, a, _ in an.packets_in(rig.lk.packets[mark3:], org3) if p >= seek_to - 0.2), default=None)
    res["seek_forward"] = {"to_s": seek_to, "packets_around": len(pk3), "addresses": len({a for _, a, _ in pk3 if not a.endswith('/clear')}),
                           "first_packet_song_s": first3,
                           "max_dev_1.5s_after": resync_dev(rig.lk.packets[mark3:], raw, rig_pos(org3), cm)}
    # ---- seek back to 2 s: fixtures the song has not touched yet are given back
    await rig.sleep_until_song(org3, seek_to + 5.0)
    mark4 = rig.lk.mark()
    rig.pb.seek(2.0)
    await asyncio.sleep(0.5)
    org4 = origin_now(rig)
    await asyncio.sleep(1.5)
    pk4 = an.packets_in(rig.lk.packets[mark4:], org4)
    clears = [a for _, a, _ in pk4 if a.endswith("/clear")]
    res["seek_back"] = {"to_s": 2.0, "clears": len(clears), "fixtures_cleared": len({an.fixture(a) for a in clears}),
                        "packets_first_2s": len(pk4)}
    # ---- pause, Release lights while paused
    await asyncio.sleep(6.0)
    rig.pb.pause()
    await asyncio.sleep(1.0)
    mark5 = rig.lk.mark()
    t0 = time.time()
    held_status = rig.lk_status()
    rel = await asyncio.to_thread(rig.backend.post, "/api/v1/lights/lightkey/release")  # the call returns after the clears: do not block the sink's loop
    await asyncio.sleep(3.0)
    pk5 = [(w, a) for _, w, a, _ in rig.lk.packets[mark5:]]
    cl = [(w, a) for w, a in pk5 if a.endswith("/clear")]
    per = {}
    for _, a in cl:
        per[an.fixture(a)] = per.get(an.fixture(a), 0) + 1
    spans = max(w for w, _ in cl) - min(w for w, _ in cl) if cl else 0
    all_fix = {an.fixture(a) for a in raw.addresses()}
    res["release"] = {"accepted": rel.get("accepted"), "message": rel.get("message"),
                      "clears": len(cl), "fixtures_cleared": len(per),
                      "each_twice": all(v == 2 for v in per.values()),
                      "rate_per_s": round(len(cl) / spans, 1) if spans else None,
                      "fixtures_in_take": len(all_fix), "fixtures_in_take_not_cleared": len(all_fix - set(per)),
                      "non_clear_packets": len(pk5) - len(cl), "status_before": held_status, "status_after": rig.lk_status()}
    # one more release: nothing held -> still names every saved fixture (documented)
    res["log"] = grab_log(rig)

    save(out, "control", res)
    return res


async def stage_stop(rig, takes, out):
    """Stop StagePilot mid-song (SIGTERM, then SIGKILL). Last stage: a restarted backend has to
    learn the Playback pairing again, so the song that plays afterwards is whatever resolves."""
    res = {}
    key = TAKE_FOR["Able"]
    rig.set_mode("replay")
    await rig.wait_link()
    # ---- stop StagePilot mid-song: SIGTERM, then SIGKILL
    rig.pb.stop()
    await asyncio.sleep(1)
    rig.pb.play(101, 0.0)  # a (re)started backend resolves the first Playback song it sees to Up next
    await asyncio.sleep(1.5)
    rig.pb.seek(30.0)
    await asyncio.sleep(1.5)
    org = origin_now(rig)
    await asyncio.sleep(10)
    markT = rig.lk.mark()
    tT = time.time()
    playing_t = (rig.state().get("playing_song") or {}).get("title")
    rig.backend.stop()  # SIGTERM -> graceful shutdown
    await asyncio.sleep(2.0)
    pT = rig.lk.packets[markT:]
    clT = [a for _, w, a, _ in pT if a.endswith("/clear")]
    res["sigterm_mid_song"] = {"playing": playing_t, "packets_after_term": len(pT), "clears": len(clT),
                               "fixtures_cleared": len({an.fixture(a) for a in clT}), "each_twice": all(clT.count(a) == 2 for a in set(clT))}
    await rig.restart_stopped()
    # the restarted backend resolves the first Playback song it sees to Up next
    rig.pb.play(101, 0.0)
    await asyncio.sleep(1.5)
    rig.pb.seek(30.0)
    await asyncio.sleep(12)
    markK = rig.lk.mark()
    playing_k = (rig.state().get("playing_song") or {}).get("title")
    sent_before_kill = sum(1 for p in rig.lk.packets[max(0, markK - 400):markK] if not p[2].endswith("/clear"))
    rig.backend.stop(kill=True)  # SIGKILL
    await asyncio.sleep(2.0)
    pK = rig.lk.packets[markK:]
    res["sigkill_mid_song"] = {"playing": playing_k, "packets_in_400_before_kill": sent_before_kill, "packets_after_kill": len(pK), "clears": sum(1 for _, _, a, _ in pK if a.endswith("/clear"))}
    await rig.restart_stopped()
    res["log_after_restarts"] = grab_log(rig)
    save(out, "stop", res)
    return res


def rig_pos(origin):
    return time.time() - origin


def resync_dev(pk_all, raw, at, cm):
    """After a resume/seek: the last value received per address vs the take at song position `at`
    (addresses an effect drives at that moment are skipped: they move by design). How many differ
    by more than twice the tolerance, and the largest difference."""
    eidx = an.effect_index(cm)
    last = {}
    for mono, wall, a, args in pk_all:
        if a.endswith("/clear"):
            continue
        last[a] = (0, args)
    bad, worst, n = 0, 0.0, 0
    for a, (p, args) in last.items():
        if a not in raw.t or an.in_effect(eidx, a, at):
            continue
        want = an.numeric(raw.at(a, at) or [])
        got = an.numeric(args)
        if not got or len(want) != len(got):
            continue
        n += 1
        d = max(abs(x - y) for x, y in zip(got, want))
        worst = max(worst, d)
        bad += d > an.tol(a) * 2
    return {"addresses_compared": n, "beyond_2x_tolerance": bad, "max": round(worst, 3)}


# ------------------------------------------------------------------ stage: soak
async def stage_soak(rig, takes, out):
    key_able = TAKE_FOR["Able"]
    key_ug = TAKE_FOR["Unstoppable God"]
    run = []
    base = rig.backend.stats()
    seq = [("replay", "Able"), ("capture", "Unstoppable God"), ("replay", "Able"), ("replay", "Able")]
    for i, (kind, title) in enumerate(seq, 1):
        key = TAKE_FOR[title]
        if kind == "replay":
            r = await replay_song(rig, takes[title], key, out, f"soak{i}", with_analysis=True)
            r["rate_per_second"] = None
            r["step_late_ms_values"] = None
        else:
            r = await capture_song(rig, takes[title], key, out, f"soak{i}")
            rig.set_mode("auto")
        r["kind"] = kind
        r["stats_end"] = rig.backend.stats()
        run.append(r)
        await asyncio.sleep(2)
    await asyncio.sleep(30)
    end = rig.backend.stats()
    res = {"runs": run, "baseline": base, "end_after_30s_idle": end, "log": grab_log(rig)}
    save(out, "soak", res)
    return res


# ------------------------------------------------------------------ stage: auto mode logic
async def stage_auto(rig, takes, out):
    """Fresh settings: nothing captured yet. Run with its own rig (see main)."""
    res = {}
    take = takes["Unstoppable God"]
    quiet = take["initial"]
    key = "demo-battle-belongs"
    other = "demo-holy-forever"
    rig.set_relay_state(quiet)
    await rig.wait_link()
    rig.set_mode("auto")
    await settle(rig)
    # 1. quiet song (no lighting change in the whole song): stays "not captured"
    origin = await rig.play(key)
    st0 = rig.lk_status()
    res["quiet_start"] = {"auto": st0.get("auto"), "message": st0.get("message")}
    await finish_song(rig, origin, key)
    msg = await rig.wait(lambda: rig.lk_status().get("message"), 20)
    await asyncio.sleep(2)
    res["quiet_end"] = {"message": rig.lk_status().get("message"), "cue_map": summarize_cue_map(rig.cue_map(key))}
    rig.pb.stop()
    await asyncio.sleep(1)
    origin = await rig.play(key)
    st1 = rig.lk_status()
    res["quiet_next_play"] = {"auto": st1.get("auto"), "message": st1.get("message")}
    rig.pb.pause()
    await asyncio.sleep(1)
    rig.pb.stop()
    # 2. a complete capture, then its next play replays
    try:
        cap = await capture_song(rig, take, key, out, "auto", record=False)
        res["auto_capture"] = {"save_message": cap["save_message"], "cue_map": cap["cue_map"],
                               "status_start": cap["start_status_message"]}
    except TimeoutError:
        # the capture ended without a Saved / Not saved message (seen: "too many rows"); keep going
        st_end = rig.lk_status()
        res["auto_capture"] = {"save_message": "NO SAVE MESSAGE (see run.log and backend.log: lightkey_looks_too_large)",
                               "cue_map": summarize_cue_map(rig.cue_map(key)), "status_start": None,
                               "status_end": {k: st_end.get(k) for k in ("auto", "message", "mode")}}
    rig.pb.stop()
    await asyncio.sleep(2)
    # NOTE (finding, see README): the demo service source publishes TimerStarted without
    # `started_at`, and the link caches its per-song decision by (song, started_at), so a second
    # play of the same song reuses the first decision. A different song in between clears it
    # (as a real timer output, which sets started_at, does on every start).
    await flush_decision(rig, other)
    origin = await rig.play(key)
    await asyncio.sleep(1.5)
    st2 = rig.lk_status()
    res["captured_next_play"] = {"auto": st2.get("auto"), "message": st2.get("message"), "replay_blocked": st2.get("replay_blocked"),
                                 "replaying_song": st2.get("replaying_song")}
    rig.pb.pause()
    await asyncio.sleep(1)
    rig.pb.stop()
    await asyncio.sleep(1)
    # 3. link drop under 60 s: the next uncaptured song still captures
    res["drop_30s"] = await drop_case(rig, other, drop_seconds=30)
    await asyncio.sleep(3)
    # 4. link drop over 60 s: it replays (the captured song is played first to clear the cached decision)
    await flush_decision(rig, key)
    res["drop_70s"] = await drop_case(rig, other, drop_seconds=70)
    res["log"] = grab_log(rig)
    save(out, "auto", res)
    return res


async def flush_decision(rig, key):
    """Play another song for a moment so the link forgets its cached per-song decision."""
    await rig.play(key)
    await asyncio.sleep(1.5)
    rig.pb.pause()
    await asyncio.sleep(0.5)
    rig.pb.stop()
    await asyncio.sleep(1.0)


async def drop_case(rig, key, drop_seconds):
    rig.lk.api_down = True
    rig.relay.muted = True
    t0 = time.time()
    await asyncio.sleep(drop_seconds)
    pre = rig.lk_status()
    origin = await rig.play(key)
    st = rig.lk_status()
    r = {"drop_s": drop_seconds, "status_before_start": {"link": pre["link"], "auto": pre.get("auto"), "mode": pre.get("mode")},
         "status_at_start": {"auto": st.get("auto"), "message": st.get("message"), "link": st["link"], "mode": st.get("mode")}}
    rig.pb.pause()
    await asyncio.sleep(0.5)
    rig.pb.stop()
    rig.lk.api_down = False
    rig.relay.muted = False
    await rig.wait_link(60)
    r["recovered_after_s"] = round(time.time() - t0, 1)
    await asyncio.sleep(1)
    return r


# ------------------------------------------------------------------ main
async def amain(args):
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    takes = load_takes(args.takes)
    stages = args.stages.split(",")
    # one rig for capture..soak (cue maps persist in its temp settings); a fresh one for auto
    main_stages = [s for s in stages if s != "auto"]
    if main_stages:
        rig = Rig(out / "rig-main", Path(args.binary) if args.binary else None, backend_port=args.port)
        await rig.up()
        try:
            for s in main_stages:
                log("=== stage", s)
                await {"capture": stage_capture, "replay": stage_replay, "control": stage_control, "soak": stage_soak, "stop": stage_stop}[s](rig, takes, out)
        finally:
            await rig.down()
    if "auto" in stages:
        await asyncio.sleep(5)  # let the first rig's sockets go
        rig = Rig(out / "rig-auto", Path(args.binary) if args.binary else None, backend_port=args.port)
        await rig.up()
        try:
            log("=== stage auto")
            await stage_auto(rig, takes, out)
        finally:
            await rig.down()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--takes", default=None)
    ap.add_argument("--binary", default=None)
    ap.add_argument("--port", type=int, default=18900)
    ap.add_argument("--stages", default="capture,replay,control,soak,stop,auto")
    asyncio.run(amain(ap.parse_args()))


if __name__ == "__main__":
    main()
