#!/usr/bin/env python3
"""REPORT.md from a run directory: tables for every check. Usage: report.py <out-dir>"""
from __future__ import annotations

import json
import re
import statistics
import subprocess
import sys
from pathlib import Path

out = Path(sys.argv[1])
res = out / "results"


def load(name):
    p = res / f"{name}.json"
    return json.loads(p.read_text()) if p.exists() else None


def f(x, n=1, unit=""):
    return "n/a" if x is None else f"{x:.{n}f}{unit}"


def table(head, rows):
    lines = ["| " + " | ".join(head) + " |", "|" + "|".join("---" for _ in head) + "|"]
    lines += ["| " + " | ".join(str(c) for c in r) + " |" for r in rows]
    return "\n".join(lines) + "\n"


def offline():
    p = out / "offline_replay_budget.txt"
    if not p.exists():
        return {}
    text = p.read_text()
    blocks = re.split(r"^=+ ", text, flags=re.M)[1:]
    o = {}
    for b in blocks:
        title = b.split(" (")[0]
        d = {}
        m = re.search(r"saved map: (\d+) rows \+ (\d+) effects = (\d+) items", b)
        if m:
            d["rows"], d["effects"], d["items"] = map(int, m.groups())
        m = re.search(r"simulated replay [^:]+: (\d+) packets, CPU time (\d+) ms \(([\d.]+) ms per song second", b)
        if m:
            d["packets"], d["cpu_ms"], d["cpu_ms_per_s"] = int(m.group(1)), int(m.group(2)), float(m.group(3))
        m = re.search(r"packets/s over \d+ s: mean ([\d.]+), p99 (\d+), max (\d+); seconds above 150/s: (\d+)", b)
        if m:
            d["mean"], d["p99"], d["max"], d["above150"] = float(m.group(1)), int(m.group(2)), int(m.group(3)), int(m.group(4))
        m = re.search(r"steps behind the pacing queue.*?median (\d+), p99 (\d+), max (\d+)", b)
        if m:
            d["step_wait"] = tuple(map(int, m.groups()))
        paced = b.split("as paced:")[-1] if "as paced:" in b else ""
        dev = {}
        place = None
        for line in paced.splitlines():
            s = line.strip()
            if s.startswith("outside effects"):
                place = "outside"
            elif s.startswith("inside effects"):
                place = "inside"
            else:
                m = re.match(r"(angle|colour|dimmer|other level)\s+(\d+) points\s+tolerance\s+[\d.]+\s+max\s+([\d.]+)\s+beyond tol\s+([\d.]+) %", s)
                if m and place:
                    dev[f"{m.group(1)}/{place}"] = (float(m.group(3)), float(m.group(4)))
        d["dev_paced"] = dev
        o[title] = d
    return o


OFF = offline()
md = []
w = md.append
sha = subprocess.run(["git", "-C", str(Path(__file__).parent), "rev-parse", "--short", "HEAD"], capture_output=True, text=True).stdout.strip()
w(f"# StagePilot lighting end-to-end test (fake relay + fake Lightkey, no rig)\n")
w(f"Run directory: `{out}`  \nRepository commit: `{sha}`  \nBackend: `target/release/stagepilot-server` headless, loopback, temp settings, demo service, Playback fake, simulated timer output.\n")
w("""All timing is **real time**: StagePilot's song clock is the wall clock, so there is no 4x mode. Every capture and replay below is a 1x run.
The fake relay speaks the lightkey-api relay protocol (docs/relay.md of that project); the fake Lightkey is a UDP sink on 21600 that timestamps every OSC datagram plus the `/time` and `/log` status API; it echoes every override back to the relay 20 ms later (quantised to DMX) the way the real DMX decode would. The relay's clock is 3.25 s ahead of the backend's, so the clock-offset path is exercised.
""")

# ---------------------------------------------------------------- capture
cap = load("capture")
w("## 1. Capture (relay stream in, fit, save)\n")
if cap:
    rows = []
    for r in cap:
        cm = r["cue_map"] or {}
        off = OFF.get(r["take"], {})
        rows.append([r["take"], r["song"], r["save_message"].split(" Loudness")[0], cm.get("rows"), cm.get("effects"), cm.get("items"),
                     f"{off.get('rows', 'n/a')} + {off.get('effects', 'n/a')} = {off.get('items', 'n/a')}",
                     json.dumps(cm.get("capture")), r["lost_packets"], f"{r['save_latency_s']:.2f} s",
                     r["probe"]["cpu_ms_per_song_second"], r["log"]["error_warn_count"]])
    w(table(["take", "song", "saved message", "rows", "effects", "items", "offline S1 (rows + effects = items)", "capture status", "lost packets", "end to saved", "CPU ms / song s", "error/warn log lines"], rows))
    w("Status text during/after each capture:\n")
    for r in cap:
        w(f"- {r['take']}: start `{r['start_status_message']}`; mid-song `{[m['message'] for m in r['status_during']][:2]}`; end `{r['save_message']}`")
    w("")
else:
    w("not run\n")

# ---------------------------------------------------------------- replay
rep = load("replay")
w("## 2. Replay: what the fake Lightkey received, against the original take\n")
if rep:
    rows = []
    for r in rep:
        off = OFF.get(r["take"], {})
        rt = r["rate_take_window"]
        rows.append([r["take"], rt["packets"], f"{off.get('packets', 'n/a')}", f"{rt['mean']:.1f} / {rt['p99']} / {rt['max']}",
                     f"{off.get('mean', 'n/a')} / {off.get('p99', 'n/a')} / {off.get('max', 'n/a')}",
                     rt["seconds_above_150"], off.get("above150", "n/a"), r["clears"], r["bad_osc"]])
    w("### Packets and rate (take window)\n")
    w(table(["take", "packets (e2e)", "packets (S2 sim)", "pkts/s mean / p99 / max (e2e)", "(S2 sim)", "s above 150/s (e2e)", "(S2 sim)", "clears sent", "undecodable"], rows))
    w("### Deviation of the received series from the original take (held value, 50 ms grid; tolerance 0.03 level / 1.5 deg)\n")
    for r in rep:
        off = OFF.get(r["take"], {}).get("dev_paced", {})
        rows = []
        for k, v in sorted(r["deviation"].items()):
            o = off.get(k)
            rows.append([k, v["n"], f"{v['max']:.3f}", f"{v['p99']:.3f}", f"{v['beyond_pct']:.2f} %", v["worst"],
                         f"{o[0]:.3f} / {o[1]:.2f} %" if o else "n/a"])
        w(f"**{r['take']}**\n")
        w(table(["class/place", "points", "max dev", "p99 dev", "beyond tolerance", "worst point", "S2 sim 'as paced' max / beyond"], rows))
    w("### Step lateness against the saved row times (negative = early; design target = -45 ms) and stale packets\n")
    rows = []
    for r in rep:
        s = r["steps"]
        off = OFF.get(r["take"], {})
        rows.append([r["take"], s["considered"], s["matched"], s["unmatched"], f(s["min_ms"], 0), f(s["median_ms"], 0), f(s["p99_ms"], 0), f(s["max_ms"], 0),
                     s["late_over_100ms"], s["stale_packets"], "/".join(map(str, off.get("step_wait", ("n/a",))))])
    w(table(["take", "step messages", "matched", "unmatched", "min ms", "median ms", "p99 ms", "max ms", "later than target + 100 ms", "stale packets", "S2 sim queue wait median/p99/max ms"], rows))
    w("### Process cost, addresses and echo\n")
    rows = []
    for r in rep:
        p = r["probe"]
        rows.append([r["take"], r["addresses"]["in_take"], r["addresses"]["driven"], r["echoed"], p["rss_before_kb"], p["rss_after_kb"], p["threads_after"], p["cpu_ms_per_song_second"], r["log"]["error_warn_count"]])
    w(table(["take", "addresses in take", "addresses driven", "echoes fed back", "RSS before kB", "RSS after kB", "threads", "CPU ms / song s", "error/warn lines (whole run so far)"], rows))
else:
    w("not run\n")

# ---------------------------------------------------------------- control
c = load("control")
w("## 3. Pause, seek, operator take-over, Release lights, stopping StagePilot (Able on Holy Forever)\n")
if c:
    t = c["takeover"]
    w("### Operator take-over (one fixture, an unexpected DMX change injected into the relay at 15 s)\n")
    w(table(["item", "result"], [
        ["fixture / address", f"{t['fixture']} / {t['address']}"], ["injected value", t["injected"]],
        ["packets for that address in the 6.9 s before", t["packets_before_inj_8_14.9s"]],
        ["last packet to the fixture before the change (song s)", t["target_last_packet_before_s"]],
        ["packets to the fixture in the 29.8 s of quiet after", t["target_packets_in_quiet_window"]],
        ["fixture taken back after (s, expect ~30)", t["taken_back_after_s"]],
        ["value sent at take-back vs the take then", f"{t.get('first_back_value')} vs {t.get('raw_value_then')}"],
        ["other fixtures pkts/s before vs during", f"{t['other_fixtures_pkts_per_s_before']} vs {t['other_fixtures_pkts_per_s_during']}"],
        ["status text 2 s after", json.dumps({k: v for k, v in t['status_2s_after'].items() if k in ('message', 'detail', 'replay_blocked', 'replaying_song', 'holding', 'yielded')})],
    ]))
    p, rs = c["pause"], c["resume"]
    sf, sb = c["seek_forward"], c["seek_back"]
    w("### Pause, resume, seek\n")
    w(table(["check", "result"], [
        [f"pause {p['paused_s']} s: packets after the first 0.4 s", p["packets_while_paused_after_0.4s"]],
        ["pause: packets in the first 0.4 s (in flight)", p["packets_in_first_0.4s_after_pause"]],
        ["pause: clears sent", p["clears_while_paused"]],
        ["resume in place: packets / addresses in the first 1.5 s", f"{rs['packets_first_1.5s']} / {rs['addresses_first_1.5s']}"],
        ["resume: addresses beyond 2x tolerance vs the take, max dev", json.dumps(rs["max_dev_at_resume+1.5s"])],
        ["seek forward to 200 s: packets 199-202.5 s / addresses", f"{sf['packets_199_202.5']} / {sf['addresses']}"],
        ["seek forward: re-sync check (last value per address vs take)", json.dumps(sf["max_dev_1.5s_after"])],
        ["seek back to 2 s: clears / fixtures given back / packets in 2 s", f"{sb['clears']} / {sb['fixtures_cleared']} / {sb['packets_first_2s']}"],
    ]))
    r = c["release"]
    w("### Release lights (paused)\n")
    w(table(["check", "result"], [
        ["accepted / message", f"{r['accepted']} / {r['message']}"], ["clears sent", r["clears"]], ["fixtures cleared", r["fixtures_cleared"]],
        ["each fixture exactly twice", r["each_twice"]], ["clear rate per s (limit 200)", r["rate_per_s"]],
        ["fixtures in the take / not cleared", f"{r['fixtures_in_take']} / {r['fixtures_in_take_not_cleared']}"],
        ["non-clear packets in the 3 s after", r["non_clear_packets"]],
    ]))
stp = load("stop")
if stp:
    a, b = stp["sigterm_mid_song"], stp["sigkill_mid_song"]
    w("### Stopping StagePilot mid-song\n")
    w(table(["how", "packets in the 2 s after", "clears", "fixtures cleared", "each twice"], [
        ["SIGTERM (graceful: documented 'StagePilot shuts down' gives the lights back)", a["packets_after_term"], a["clears"], a["fixtures_cleared"], a["each_twice"]],
        ["SIGKILL (crash: nothing can run)", b["packets_after_kill"], b["clears"], "-", "-"],
    ]))
else:
    w("not run\n")

# ---------------------------------------------------------------- soak
s = load("soak")
w("## 4. Soak: Able replay x3 with a capture of Unstoppable God between\n")
if s:
    rows = [["start", "-", s["baseline"]["rss_kb"], s["baseline"]["threads"], s["baseline"]["fds"], "-", "-"]]
    prev = s["baseline"]
    for r in s["runs"]:
        e = r["stats_end"]
        rows.append([f"{r['kind']} {r['take']}", r["tag"], e["rss_kb"], e["threads"], e["fds"], r["probe"]["cpu_ms_per_song_second"], r["log"]["error_warn_count"]])
    e = s["end_after_30s_idle"]
    rows.append(["30 s idle after", "-", e["rss_kb"], e["threads"], e["fds"], "-", s["log"]["error_warn_count"]])
    w(table(["step", "tag", "RSS kB", "threads", "open fds", "CPU ms / song s", "error/warn lines (cumulative)"], rows))
    rss = [x[2] for x in rows]
    first_replay = rows[1][2]
    w(f"RSS growth from the end of the first replay to the end: {rss[-1] - first_replay} kB; from start: {rss[-1] - rss[0]} kB. Tokio tasks cannot be read from outside the process: threads and open file descriptors are the leak proxy.\n")
    rows = []
    for r in s["runs"]:
        if r["kind"] == "replay":
            rt = r["rate_take_window"]
            rows.append([r["tag"], rt["packets"], f"{rt['mean']:.1f} / {rt['p99']} / {rt['max']}", r["steps"]["stale_packets"], f(r["steps"]["median_ms"], 0)])
        else:
            rows.append([r["tag"] + " (capture)", r["cue_map"]["items"], r["save_message"].split(" Loudness")[0], "-", "-"])
    w(table(["run", "packets / items", "pkts/s mean / p99 / max | message", "stale", "step median ms"], rows))
else:
    w("not run\n")

# ---------------------------------------------------------------- auto
a = load("auto")
w("## 5. Auto mode logic\n")
if a:
    def au(x):
        return json.dumps({k: x.get(k) for k in ("auto", "message", "replay_blocked", "replaying_song", "mode") if k in x})
    rows = [
        ["quiet song (no lighting change), at its start", au(a["quiet_start"])],
        ["... after the whole song", f"{a['quiet_end']['message']} | cue map: {a['quiet_end']['cue_map']}"],
        ["... next play of the same song", au(a["quiet_next_play"])],
        ["complete capture (Auto)", f"{a['auto_capture']['status_start']} -> {a['auto_capture']['save_message'].split(' Loudness')[0]}"],
        ["next play of the captured song", au(a["captured_next_play"])],
    ]
    for k in ("drop_30s", "drop_70s"):
        d = a[k]
        rows.append([f"link down {d['drop_s']} s, then a not-captured song starts", f"before: {json.dumps(d['status_before_start'])}; at start: {json.dumps(d['status_at_start'])}"])
    w(table(["check", "status"], rows))
else:
    w("not run\n")

w("""## 6. What this does NOT prove

- **Real Lightkey behaviour**: whether Lightkey applies these overrides, how it truncates values, how it treats a flood of OSC, the 37-51 ms OSC-to-DMX delay (the fake applies nothing). The rig test (S4) decides.
- **A real network**: everything is loopback UDP. No loss, reordering, jitter, Wi-Fi stalls or multicast. The sink timestamps arrival, not Lightkey output.
- **DMX decode**: the fake relay hands over the take's values as OSC; the real agent's sACN decode (quantisation, `approx` flags, frame timing, the 1-2 ms "DMX change to packet") is not exercised. The echo quantises levels to 8 bit as a stand-in.
- The real Playback (WebSocket protocol is faked with heartbeats only), the real timer clock drift over a long service, other songs/takes than the two recorded ones, audio loudness capture (no audio input in this build), or the UI.
- The relay wire format is read from lightkey-api's docs/relay.md and StagePilot's decoder; if the real agent deviates from its own spec this test cannot know.

## 7. Raw logs

`run.log` (harness), `rig-*/backend/backend.log` (StagePilot's own log), `raw/replay_*.packets.jsonl.gz` (every OSC datagram the fake Lightkey saw: song position, address, args), `raw/cue_map_*.json` (the saved cue maps), `results/*.json`, `offline_replay_budget.txt` (the S1/S2 offline numbers compared with).
""")
print("\n".join(md))
