"""Serve the Python backend in demo mode on a loopback port for contract recording/replay.

Usage (from repo root):  uv run --project backend python scripts/contract/serve_python.py PORT
In-memory settings (nothing is written to the user's StagePilot data directory), dashboard PIN
off, no Playback discovery, no MIDI/OSC/ProPresenter/Planning Center traffic.
"""

from __future__ import annotations

import sys

import uvicorn

from stagepilot.core.config import Settings
from stagepilot.main import create_app


def main() -> None:
    port = int(sys.argv[1])
    app = create_app(Settings(demo_mode=True), dashboard_auth_enforced=False)
    uvicorn.run(app, host="127.0.0.1", port=port, log_level="warning")


if __name__ == "__main__":
    main()
