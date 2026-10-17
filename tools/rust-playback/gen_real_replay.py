"""Run the Python Normalizer over the redacted real captures and store its event streams.

The Rust test `tests/real_replay.rs` replays the same frames and must produce the same events.
Run from repo root:  cd backend && uv run python ../tools/rust-playback/gen_real_replay.py
Only `frame` records are fed (what a listening client receives); `sent` records are the commands
the test rig itself sent and are used by the wire-shape test instead.
"""

from __future__ import annotations

import dataclasses
import json
import sys
from pathlib import Path

from stagepilot.plugins.playback_api.normalizer import Normalizer

DIR = Path(__file__).resolve().parents[2] / "crates/stagepilot-playback/tests/fixtures/real"


def main() -> None:
    out = {}
    for path in sorted(DIR.glob("*.jsonl")):
        frames = [json.loads(line) for line in path.read_text().splitlines()]
        frames = [r for r in frames if r["kind"] == "frame"]
        for fast in (False, True):
            norm = Normalizer(fast_transport=fast)
            per_message = []
            for r in frames:
                raw = json.dumps(r["msg"], separators=(",", ":"))
                per_message.append(
                    [
                        {k: v for k, v in dataclasses.asdict(e).items() if v is not None and v is not False}
                        for e in norm.feed(raw, r["t"])
                    ]
                )
            out[f"{path.name}|fast={str(fast).lower()}"] = {
                "messages": len(frames),
                "events": per_message,
                "setlist_id": norm.setlist_id,
            }
            n = sum(len(m) for m in per_message)
            print(f"{path.name} fast={fast}: {len(frames)} frames -> {n} events", file=sys.stderr)
    (DIR / "expected_events.json").write_text(json.dumps(out, separators=(",", ":")))


main()
