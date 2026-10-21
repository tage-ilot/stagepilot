"""Serve the Python backend with the "outputs" contract profile on a loopback port.

Usage (repo root):  uv run --project backend python scripts/contract/serve_python_outputs.py PORT MOCK_PORT

Fake MIDI devices, ProPresenter at 127.0.0.1:MOCK_PORT (scripts/contract/mock_propresenter.py must
already be running there), OSC to a loopback port nothing listens on. In-memory settings.
"""

from __future__ import annotations

import sys
from pathlib import Path

import uvicorn

sys.path.insert(0, str(Path(__file__).parent))
import outputs_app  # noqa: E402


def main() -> None:
    port, mock_port = int(sys.argv[1]), int(sys.argv[2])
    uvicorn.run(outputs_app.create(mock_port), host="127.0.0.1", port=port, log_level="warning")


if __name__ == "__main__":
    main()
