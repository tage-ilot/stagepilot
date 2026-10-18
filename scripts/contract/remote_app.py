"""The "remote" and "diagnostics" contract profiles: the Python app with Remote wired in.

remote       demo app, a RemoteStore and RemoteFeature under DIR, the fixture dashboard in
             `web/`. Served on a local port, and the same app behind `RemoteIngress` (public origin
             `https://remote.contract.test`) on a second port. No tunnel, no network.
diagnostics  demo app, a RemoteStore under DIR and a scripted Remote manager (below) whose
             bootstrap points at the mock control plane. Nothing real is contacted.

Run standalone:  uv run --project backend python scripts/contract/serve_python_remote.py ...
"""

from __future__ import annotations

from pathlib import Path
from types import SimpleNamespace
from typing import Any

from stagepilot.core.config import Settings
from stagepilot.main import create_app
from stagepilot.remote_feature import RemoteFeature
from stagepilot.remote_provider import ProviderError
from stagepilot.services.remote_auth import RemoteStore

HERE = Path(__file__).parent
WEB_ROOT = HERE / "web"
PUBLIC_ORIGIN = "https://remote.contract.test"
INSTALLATION_ID = "abcd1234"
CREDENTIAL = "spi_abcd1234." + "s" * 43


class ContractBootstrap:
    """Enrolled against the mock control plane until the identity is reset."""

    def __init__(self, origin: str) -> None:
        self.origin = origin
        self.trusted_origins = frozenset({origin})
        self.enrolled = True

    def state(self) -> Any:
        active = (
            SimpleNamespace(control_plane_origin=self.origin, installation_id=INSTALLATION_ID)
            if self.enrolled
            else None
        )
        return SimpleNamespace(active=active)

    def credential(self, _metadata: object) -> str:
        return CREDENTIAL


class ContractManager:
    """A stateful stand-in for DesktopRemoteManager (the real one is not part of this profile)."""

    def __init__(self, origin: str) -> None:
        self.enabled = False
        self.bootstrap = ContractBootstrap(origin)

    def status(self) -> dict[str, Any]:
        return {
            "available": True,
            "enabled": self.enabled,
            "state": "connected" if self.enabled else "off",
            "url": "https://contract.stagepilot.example" if self.enabled else None,
            "message": None,
            "temporary_url": False,
        }

    def enable(self) -> None:
        if self.enabled:
            raise ValueError("already enabled")
        self.enabled = True

    def disable(self) -> None:
        self.enabled = False

    def reset_identity(self) -> None:
        self.bootstrap.enrolled = False

    def regenerate(self) -> None:
        if not self.enabled:
            raise ProviderError("Could not reach the service")


def create_remote(directory: Path) -> tuple[Any, str]:
    store = RemoteStore(directory / "identity.sqlite3")
    app = create_app(
        Settings(demo_mode=True),
        remote_store=store,
        dashboard_auth_enforced=False,
        web_root=WEB_ROOT,
    )
    app.state.remote_feature = RemoteFeature(directory / "remote" / "remote.json")
    return app, PUBLIC_ORIGIN


def create_diagnostics(directory: Path, control_plane_port: int) -> Any:
    app = create_app(
        Settings(demo_mode=True),
        remote_store=RemoteStore(directory / "identity.sqlite3"),
        dashboard_auth_enforced=False,
    )
    app.state.remote_manager = ContractManager(f"http://127.0.0.1:{control_plane_port}")
    return app
