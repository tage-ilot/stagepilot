"""Serve the Python backend with the "pco" contract profile on a loopback port.

Usage (repo root):  uv run --project backend python scripts/contract/serve_python_pco.py PORT MOCK_PORT

Planning Center and the control plane are the mock at 127.0.0.1:MOCK_PORT
(scripts/contract/mock_pco.py must already be running there). In-memory settings and tokens.
"""

from __future__ import annotations

import sys
from pathlib import Path

import uvicorn

sys.path.insert(0, str(Path(__file__).parent))
import pco_app  # noqa: E402


def main() -> None:
    port, mock_port = int(sys.argv[1]), int(sys.argv[2])
    uvicorn.run(pco_app.create(mock_port), host="127.0.0.1", port=port, log_level="warning")


if __name__ == "__main__":
    main()
