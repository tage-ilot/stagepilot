#!/usr/bin/env bash
# Rust server: build it, start it in demo mode on a loopback port, replay the golden transcripts, stop it.
# Usage (repo root): scripts/contract/run_rust.sh [PORT]
set -euo pipefail
cd "$(dirname "$0")/../.."
PORT="${1:-47313}"
cargo build -p stagepilot-server
LOG="$(mktemp)"
./target/debug/stagepilot-server --demo --host 127.0.0.1 --port "$PORT" >"$LOG" 2>&1 &
SERVER=$!
trap 'kill "$SERVER" 2>/dev/null || true; rm -f "$LOG"' EXIT
for _ in $(seq 1 60); do
  curl -sf "http://127.0.0.1:$PORT/api/v1/health/live" >/dev/null 2>&1 && break
  sleep 0.5
done
uv run --project backend --frozen python scripts/contract/contract.py replay "http://127.0.0.1:$PORT"
