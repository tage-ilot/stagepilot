"""Generate replay fixtures: synthetic wire traffic + the Python Normalizer's output.

Run from repo root:  cd backend && uv run python ../spikes/rust-playback/tools/gen_replay.py
All data is synthetic (seeded random walk over the message kinds the normalizer
understands); no private setlist or rig data is used.
"""

from __future__ import annotations

import dataclasses
import json
import random
import sys
from pathlib import Path

from stagepilot.plugins.playback_api.normalizer import Normalizer

OUT = Path(__file__).resolve().parents[1] / "crates/stagepilot-playback/tests/fixtures/replay.json"
SONGS = [101, 102, 103]


def hb(song, pos, playing, pad, ver):
    return json.dumps({"heartbeat": {"stateData": {
        "setlistSongID": song, "sequenceTime": pos,
        "sequencerPlayState": {"playing" if playing else "stopped": {}},
        "padPlayerPlayState": {"playing" if pad else "stopped": {}},
        "setlistCloudVersion": ver}}})


def scenario(seed: int, n: int):
    r = random.Random(seed)
    song, pos, playing, pad, ver = 101, r.choice([0, 0, 5.5]), False, False, 1
    t, msgs = 10.0, []
    for _ in range(n):
        t = round(t + r.choice([0.05, 0.3, 1.0, 1.0, 1.0, 2.5, 4.0]), 3)
        k = r.random()
        if k < 0.45:
            if playing:
                pos = round(pos + r.choice([1.0, 1.0, 1.0, 0.5, 9.0]), 3)
            msgs.append((t, hb(song, pos, playing, pad, ver)))
        elif k < 0.55:
            playing = not playing
            msgs.append((t, json.dumps({"transportPlay": {"playing": playing}})))
        elif k < 0.60:
            song = r.choice(SONGS); pos = 0; playing = False if r.random() < .7 else playing
            msgs.append((t, json.dumps({"setlistSelectSong": {"setlistSongID": song}})))
        elif k < 0.65:
            pos = r.choice([0, 0, 1.0, 30.0, 86400.0])
            msgs.append((t, json.dumps({"waveformSeek": {"sequenceTime": pos}})))
        elif k < 0.70:
            pos = 0; playing = False
            msgs.append((t, '{"transportReturnToStart":{}}'))
        elif k < 0.73:
            pad = not pad
            msgs.append((t, json.dumps({"transportPad": {"playing": pad}})))
        elif k < 0.76:
            msgs.append((t, json.dumps({"transportFade": {"direction": r.choice([0, 1, 2])}})))
        elif k < 0.79:
            msgs.append((t, json.dumps({r.choice(["waveformDoubleTap", "waveformLoop"]): {
                "setlistSongSectionID": r.choice([7, 8]), "active": r.choice([True, False, "x"])}})))
        elif k < 0.82:
            ver += 1
        elif k < 0.85:
            song = r.choice(SONGS)  # auto-advance / navigation
        elif k < 0.88:
            msgs.append((t, json.dumps({"contentLoadSetlist": {"setlistData": {
                "setlistID": r.choice([6047773, "abc", "", None])}}})))
        elif k < 0.91:
            msgs.append((t, json.dumps({r.choice(["transportNextSong", "mixerLoop",
                "transportNavigateToSongMapElementIndex", "mixerTrackMute", "weird"]): {}})))
        elif k < 0.94:
            msgs.append((t, r.choice(["not json", "[]", "{}", '{"a":{},"b":{}}', '{"a":5}',
                                       hb("x", 1, True, False, 1),
                                       hb(1, -1, True, False, 1),
                                       '{"heartbeat":{"stateData":{"setlistSongID":true}}}'])))
        else:
            msgs.append((t, hb(song, pos, playing, pad, ver)))
    return msgs


def main() -> None:
    cases = []
    for seed in range(20):
        for fast in (True, False):
            msgs = scenario(seed, 120)
            norm = Normalizer(fast_transport=fast)
            events = []
            for ts, raw in msgs:
                events.append([
                    {k: v for k, v in dataclasses.asdict(e).items() if v is not None and v is not False}
                    for e in norm.feed(raw, ts)
                ])
            cases.append({"seed": seed, "fast": fast, "messages": msgs, "events": events, "setlist_id": norm.setlist_id})
    OUT.write_text(json.dumps(cases, separators=(",", ":")))
    total = sum(sum(len(m) for m in c["events"]) for c in cases)
    print(f"{len(cases)} cases, {total} events -> {OUT} ({OUT.stat().st_size} bytes)", file=sys.stderr)


main()
