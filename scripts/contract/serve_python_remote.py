"""Serve the Python backend with the "remote" or "diagnostics" contract profile.

Usage (repo root):
  uv run --project backend python scripts/contract/serve_python_remote.py remote PORT INGRESS_PORT DIR
  uv run --project backend python scripts/contract/serve_python_remote.py diagnostics PORT CONTROL_PORT DIR

DIR is a scratch directory for the identity database and the Remote files. The mock control
plane (scripts/contract/mock_control.py) must already be running for `diagnostics`.
"""

from __future__ import annotations

import sys
import threading
import time
from pathlib import Path

import uvicorn

sys.path.insert(0, str(Path(__file__).parent))
import remote_app  # noqa: E402


def serve_ingress(app: object, port: int, origin: str) -> uvicorn.Server:
    from stagepilot.api.remote_ingress import RemoteIngress

    server = uvicorn.Server(
        uvicorn.Config(
            RemoteIngress(app, public_origin=origin),  # type: ignore[arg-type]
            host="127.0.0.1",
            port=port,
            log_level="warning",
        )
    )
    threading.Thread(target=server.run, daemon=True).start()
    for _ in range(100):
        if server.started:
            break
        time.sleep(0.1)
    return server


def main() -> None:
    mode, port, other, directory = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), Path(sys.argv[4])
    if mode == "remote":
        app, origin = remote_app.create_remote(directory)
        serve_ingress(app, other, origin)
    else:
        app = remote_app.create_diagnostics(directory, other)
    uvicorn.run(app, host="127.0.0.1", port=port, log_level="warning")


if __name__ == "__main__":
    main()
