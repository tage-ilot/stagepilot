#!/usr/bin/env bash
# Baseline: start the Python backend (demo mode, loopback), replay the golden transcripts, stop it.
# Then the same for the "pco" profile (Planning Center and the control plane are a local mock).
# And the same for the "outputs" profile (fake MIDI, mock ProPresenter, dead-end OSC).
# Usage (repo root): scripts/contract/run_baseline.sh [PORT]
set -euo pipefail
cd "$(dirname "$0")/../.."
PORT="${1:-47312}"
PCO_PORT=$((PORT + 100))
OUT_PORT=$((PORT + 300))
OMOCK_PORT=$((PORT + 400))
MOCK_PORT=$((PORT + 200))
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

uv run --project backend --frozen python scripts/contract/serve_python.py "$PORT" >"$LOG" 2>&1 &
PIDS+=($!)
wait_for "http://127.0.0.1:$PORT/api/v1/health/live"
uv run --project backend --frozen python scripts/contract/contract.py replay "http://127.0.0.1:$PORT"

uv run --project backend --frozen python scripts/contract/mock_pco.py "$MOCK_PORT" >>"$LOG" 2>&1 &
PIDS+=($!)
wait_for "http://127.0.0.1:$MOCK_PORT/__log"
uv run --project backend --frozen python scripts/contract/serve_python_pco.py "$PCO_PORT" "$MOCK_PORT" >>"$LOG" 2>&1 &
PIDS+=($!)
wait_for "http://127.0.0.1:$PCO_PORT/api/v1/health/live"
uv run --project backend --frozen python scripts/contract/contract.py replay "http://127.0.0.1:$PCO_PORT" \
  --profile pco --mock-url "http://127.0.0.1:$MOCK_PORT"

uv run --project backend --frozen python scripts/contract/mock_propresenter.py "$OMOCK_PORT" >>"$LOG" 2>&1 &
PIDS+=($!)
wait_for "http://127.0.0.1:$OMOCK_PORT/__log" # (this read clears the log; the server starts after)
uv run --project backend --frozen python scripts/contract/serve_python_outputs.py "$OUT_PORT" "$OMOCK_PORT" >>"$LOG" 2>&1 &
PIDS+=($!)
wait_for "http://127.0.0.1:$OUT_PORT/api/v1/health/live"
uv run --project backend --frozen python scripts/contract/contract.py replay "http://127.0.0.1:$OUT_PORT" \
  --profile outputs --mock-url "http://127.0.0.1:$OMOCK_PORT"
