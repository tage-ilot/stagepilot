#!/usr/bin/env bash
# Baseline: start the Python backend (demo mode, loopback), replay the golden transcripts, stop it.
# Then the same for the "pco" profile (Planning Center and the control plane are a local mock).
# And the same for the "outputs" profile (fake MIDI, mock ProPresenter, dead-end OSC).
# And the same for the "remote" and "static" profiles (the trusted remote ingress is a second
# listener) and the "diagnostics" profile (uploads go to a mock control plane).
# Usage (repo root): scripts/contract/run_baseline.sh [PORT]
set -euo pipefail
cd "$(dirname "$0")/../.."
PORT="${1:-47312}"
PCO_PORT=$((PORT + 100))
OUT_PORT=$((PORT + 300))
OMOCK_PORT=$((PORT + 400))
REM_PORT=$((PORT + 500))
ING_PORT=$((PORT + 600))
DIAG_PORT=$((PORT + 700))
CTRL_PORT=$((PORT + 800))
MOCK_PORT=$((PORT + 200))
LOG="$(mktemp)"
PIDS=()
SCRATCH="$(mktemp -d)"
cleanup() { for p in "${PIDS[@]:-}"; do kill "$p" 2>/dev/null || true; done; rm -rf "$LOG" "$SCRATCH"; }
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

mkdir -p "$SCRATCH/remote" "$SCRATCH/diagnostics"
uv run --project backend --frozen python scripts/contract/serve_python_remote.py remote "$REM_PORT" "$ING_PORT" "$SCRATCH/remote" >>"$LOG" 2>&1 &
PIDS+=($!)
wait_for "http://127.0.0.1:$REM_PORT/api/v1/health/live"
uv run --project backend --frozen python scripts/contract/contract.py replay "http://127.0.0.1:$REM_PORT" \
  --profile remote --ingress-url "http://127.0.0.1:$ING_PORT"
uv run --project backend --frozen python scripts/contract/contract.py replay "http://127.0.0.1:$REM_PORT" --profile static

uv run --project backend --frozen python scripts/contract/mock_control.py "$CTRL_PORT" >>"$LOG" 2>&1 &
PIDS+=($!)
wait_for "http://127.0.0.1:$CTRL_PORT/__log"
uv run --project backend --frozen python scripts/contract/serve_python_remote.py diagnostics "$DIAG_PORT" "$CTRL_PORT" "$SCRATCH/diagnostics" >>"$LOG" 2>&1 &
PIDS+=($!)
wait_for "http://127.0.0.1:$DIAG_PORT/api/v1/health/live"
uv run --project backend --frozen python scripts/contract/contract.py replay "http://127.0.0.1:$DIAG_PORT" \
  --profile diagnostics --mock-url "http://127.0.0.1:$CTRL_PORT"
