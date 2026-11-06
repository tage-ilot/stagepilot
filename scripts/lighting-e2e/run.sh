#!/usr/bin/env bash
# Pre-release end-to-end lighting test (fake relay + fake Lightkey + fake Playback, no rig).
# One command:   scripts/lighting-e2e/run.sh            (about 50 minutes: every run is real time)
# Options:       --out DIR  --takes FILE  --stages capture,replay,control,soak,auto  --skip-build
# Needs: Rust toolchain, python3 (3.11+) with the `websockets` package, free ports
# 21600 (OSC), 21700 (relay), 8770 (status API), 18981 (fake Playback) and 18900 (backend).
# Never starts or touches the preview's services (ports 18766 / 8101) or any real settings file.
set -euo pipefail
cd "$(dirname "$0")/../.."
HERE=scripts/lighting-e2e
TAKES="${TAKES:-$HOME/AgentHub-data/projects/stagepilot-lightkey-smooth/data/takes-fixtures.json}"
OUT="${E2E_OUT:-$HOME/AgentHub-data/projects/stagepilot-lighting-e2e/run-$(date +%Y%m%d-%H%M%S)}"
STAGES="capture,replay,control,soak,auto"
BUILD=1
while [ $# -gt 0 ]; do
  case "$1" in
    --out) OUT="$2"; shift 2 ;;
    --takes) TAKES="$2"; shift 2 ;;
    --stages) STAGES="$2"; shift 2 ;;
    --skip-build) BUILD=0; shift ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done
[ -f "$TAKES" ] || { echo "takes file not found: $TAKES (real take data is never committed; see README)" >&2; exit 2; }
python3 -c "import websockets" 2>/dev/null || { echo "python3 needs the 'websockets' package (pip install websockets)" >&2; exit 2; }
for p in 21600 21700 8770 18981 18900; do
  if ss -lntu 2>/dev/null | awk '{print $5}' | grep -q ":$p\$"; then echo "port $p is in use; stop whatever holds it" >&2; exit 2; fi
done
mkdir -p "$OUT"
if [ "$BUILD" = 1 ]; then cargo build --release -p stagepilot-server; fi
echo "output: $OUT"
# the offline numbers the end-to-end run is compared with (S1 capture and S2 simulated replay)
cargo run --release -q -p stagepilot-lights --example replay_budget -- "$TAKES" > "$OUT/offline_replay_budget.txt" 2>&1 || true
python3 "$HERE/e2e.py" --out "$OUT" --takes "$TAKES" --stages "$STAGES" 2>&1 | tee "$OUT/run.log"
python3 "$HERE/report.py" "$OUT" > "$OUT/REPORT.md"
echo "report: $OUT/REPORT.md"
