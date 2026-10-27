"""Redact a raw Playback capture (JSON lines: {t, kind, raw|msg}) into a committable fixture.

Raw captures stay out of git (~/AgentHub-data/projects/stagepilot-rust-migration/captures/).
Redaction rules (see docs/archive/rust-migration/captures.md):
  - setlistName                         -> "Redacted setlist <n>"
  - setlistID                           -> 7000001, 7000002, ... (stable per distinct id)
  - setlistSongID                       -> 101, 102, ... (stable per distinct id)
  - setlistSongSectionID                -> 9000 + n (stable per distinct id)
  - contentID / mappingIDs / other numbers inside rentalData, liveData, modularClickSongData
    are dropped (the normalizer only uses the *count* of rentalData / modularClickSongData items)
  - mixerTrackVolume: only every 20th frame is kept (they never produce events by default)
  - timestamps are rebased so the first kept record is t=0.000; wall-clock `iso` is dropped
Everything else (heartbeats, positions, transport states, loop/mute/pad flags) is kept verbatim.

Usage: redact_capture.py RAW.jsonl OUT.jsonl [START_SEC END_SEC]   (window relative to first frame)
"""

from __future__ import annotations

import json
import sys

ids: dict[str, dict[int, int]] = {"song": {}, "setlist": {}, "section": {}}
BASE = {"song": 101, "setlist": 7000001, "section": 9000}


def remap(kind: str, value: object) -> object:
    if not isinstance(value, int) or isinstance(value, bool):
        return value
    table = ids[kind]
    if value not in table:
        table[value] = BASE[kind] + len(table)
    return table[value]


def scrub(node: object, parent_key: str = "") -> object:
    if isinstance(node, dict):
        out = {}
        for k, v in node.items():
            if k == "setlistName":
                out[k] = "Redacted setlist"
            elif k == "setlistID":
                out[k] = remap("setlist", v)
            elif k == "setlistSongID":
                out[k] = remap("song", v)
            elif k == "setlistSongSectionID":
                out[k] = remap("section", v)
            elif k in ("rentalData", "modularClickSongData", "rentalsData") and isinstance(v, list):
                out[k] = [{} for _ in v]
            elif k == "liveData":
                out[k] = {}
            else:
                out[k] = scrub(v, k)
        return out
    if isinstance(node, list):
        return [scrub(v, parent_key) for v in node]
    return node


def main() -> None:
    src, dst = sys.argv[1], sys.argv[2]
    lo, hi = (float(sys.argv[3]), float(sys.argv[4])) if len(sys.argv) > 4 else (0.0, 1e12)
    rows = [json.loads(line) for line in open(src, encoding="utf-8")]
    frames = [r for r in rows if r["kind"] in ("frame", "sent")]
    t0 = frames[0]["t"]
    kept, vol = [], 0
    for r in frames:
        rel = r["t"] - t0
        if not lo <= rel <= hi:
            continue
        msg = json.loads(r["raw"]) if r["kind"] == "frame" else r["msg"]
        if "mixerTrackVolume" in msg:
            vol += 1
            if vol % 20:
                continue
        kept.append({"t": r["t"], "kind": r["kind"], "msg": scrub(msg)})
    base = kept[0]["t"]
    with open(dst, "w", encoding="utf-8") as f:
        for r in kept:
            r["t"] = round(r["t"] - base, 3)
            f.write(json.dumps(r, separators=(",", ":")) + "\n")
    print(f"{len(kept)} records -> {dst}", file=sys.stderr)


main()
