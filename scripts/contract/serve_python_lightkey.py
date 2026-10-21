"""Serve the Python backend with the "lightkey" contract profile on a loopback port.

Usage (repo root):  uv run --project backend python scripts/contract/serve_python_lightkey.py PORT AGENT_PORT

The fake lightkey-api (scripts/contract/mock_lightkey.py) must already be running on AGENT_PORT.
Lighting sends OSC to a loopback port nothing listens on. In-memory settings and takes.
"""

from __future__ import annotations

import sys
from pathlib import Path

import uvicorn

sys.path.insert(0, str(Path(__file__).parent))
import lightkey_app  # noqa: E402


def main() -> None:
    port, agent_port = int(sys.argv[1]), int(sys.argv[2])
    uvicorn.run(lightkey_app.create(agent_port), host="127.0.0.1", port=port, log_level="warning")


if __name__ == "__main__":
    main()
