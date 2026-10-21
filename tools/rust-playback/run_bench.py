"""Run the same scripted scenario against Rust and Python clients on one fake Playback.

Usage (repo root): cd backend && uv run python ../tools/rust-playback/run_bench.py [runs]
Needs `cargo build --release --examples` in repo root first.
"""

from __future__ import annotations

import json
import os
import resource
import statistics
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
EX = ROOT / "target/release/examples"
RUNS = int(sys.argv[1]) if len(sys.argv) > 1 else 5


def run(cmd: list[str]) -> dict:
    fake = subprocess.Popen([str(EX / "fake_playback"), "200", "5"], stdout=subprocess.PIPE, text=True)
    port = int(fake.stdout.readline().split()[1])
    try:
        before = resource.getrusage(resource.RUSAGE_CHILDREN)
        t0 = time.time() * 1000
        proc = subprocess.run([*cmd, str(port)], capture_output=True, text=True, timeout=60)
        wall = time.time() * 1000 - t0
        after = resource.getrusage(resource.RUSAGE_CHILDREN)
        if proc.returncode:
            raise SystemExit(proc.stderr)
        out = json.loads(proc.stdout.strip().splitlines()[-1])
        out["cold_start_ms"] = out.pop("connected_at_ms") - t0
        out["cpu_s"] = (after.ru_utime - before.ru_utime) + (after.ru_stime - before.ru_stime)
        out["wall_ms"] = wall
        return out
    finally:
        fake.kill()


def pct(v: list[float], p: float) -> float:
    return v[min(len(v) - 1, int(round(p * (len(v) - 1))))]


results = {}
for name, cmd in {"rust": [str(EX / "bench")], "python": [sys.executable, str(ROOT / "tools/rust-playback/bench_py.py")]}.items():
    rs = [run(cmd) for _ in range(RUNS)]
    lat = sorted(x for r in rs for x in r["lat_ms"])
    results[name] = {
        "cold_start_ms_median": round(statistics.median(r["cold_start_ms"] for r in rs), 1),
        "rss_mb_median": round(statistics.median(r["rss_kb"] for r in rs) / 1024, 1),
        "cpu_s_median": round(statistics.median(r["cpu_s"] for r in rs), 3),
        "lat_ms_median": round(statistics.median(lat), 2),
        "lat_ms_p95": round(pct(lat, 0.95), 2),
        "lat_ms_max": round(lat[-1], 2),
        "runs": RUNS,
    }
rust_bin = EX / "bench"
results["rust"]["binary_bytes_example_bench"] = rust_bin.stat().st_size
print(json.dumps(results, indent=2))
