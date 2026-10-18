#!/usr/bin/env bash
# Rust server: build it, start it in demo mode on a loopback port, replay the golden transcripts, stop it.
# Then the same for the "outputs" profile (fake MIDI, mock ProPresenter, dead-end OSC).
# Usage (repo root): scripts/contract/run_rust.sh [PORT]
set -euo pipefail
cd "$(dirname "$0")/../.."
PORT="${1:-47313}"
OUT_PORT=$((PORT + 100))
MOCK_PORT=$((PORT + 200))
cargo build -p stagepilot-server
LOG="$(mktemp)"
PIDS=()
cleanup() { for p in "${PIDS[@]:-}"; do kill "$p" 2>/dev/null || true; done; rm -f "$LOG"; }
trap cleanup EXIT

wait_for() {
  for _ in $(seq 1 60); do
    curl -sf "$1" >/dev/null 2>&1 && return 0
    sleep 0.5
  done
  echo "server did not become ready: $1" >&2
  cat "$LOG" >&2 || true
  return 1
}

./target/debug/stagepilot-server --demo --host 127.0.0.1 --port "$PORT" >"$LOG" 2>&1 &
PIDS+=($!)
wait_for "http://127.0.0.1:$PORT/api/v1/health/live"
uv run --project backend --frozen python scripts/contract/contract.py replay "http://127.0.0.1:$PORT"

uv run --project backend --frozen python scripts/contract/mock_propresenter.py "$MOCK_PORT" >>"$LOG" 2>&1 &
PIDS+=($!)
wait_for "http://127.0.0.1:$MOCK_PORT/__log"
./target/debug/stagepilot-server --contract-outputs --propresenter-port "$MOCK_PORT" \
  --host 127.0.0.1 --port "$OUT_PORT" >>"$LOG" 2>&1 &
PIDS+=($!)
wait_for "http://127.0.0.1:$OUT_PORT/api/v1/health/live"
uv run --project backend --frozen python scripts/contract/contract.py replay "http://127.0.0.1:$OUT_PORT" \
  --profile outputs --mock-url "http://127.0.0.1:$MOCK_PORT"
