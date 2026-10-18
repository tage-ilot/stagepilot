"""The "pco" contract profile: the Python app with Planning Center as the Services source, talking
to the deterministic mock in `mock_pco.py` instead of Planning Center and the control-plane Worker.

* Every Planning Center API request and every control-plane OAuth request is rewritten to the mock
  at transport level, so nothing can reach the real services.
* OAuth tokens and the settings live in memory (no keychain, no files).
* "Today" is pinned to 2026-10-18 so the mock's plans are always in range.
* MIDI and the timer output are simulated; no Playback traffic.

Run standalone:  uv run --project backend python scripts/contract/serve_python_pco.py PORT MOCK_PORT
"""

from __future__ import annotations

import os
from datetime import date
from typing import Any

import httpx

from stagepilot.core.config import (
    IntegrationModes,
    MidiSource,
    PlanningCenterSettings,
    ServiceSource,
    Settings,
    TimerOutput,
)
from stagepilot.core.settings import MemoryCredentialStore
from stagepilot.main import create_app
from stagepilot.planning_center_oauth import ControlPlaneOAuthClient
from stagepilot.plugins.planning_center.client import PlanningCenterClient

TODAY = date(2026, 10, 18)
CLIENT_ID = "client-id-1"


class ToMock(httpx.AsyncBaseTransport):
    """Send every request to the mock on loopback, whatever host it was addressed to."""

    def __init__(self, port: int) -> None:
        self._inner = httpx.AsyncHTTPTransport()
        self._port = port

    async def handle_async_request(self, request: httpx.Request) -> httpx.Response:
        request.url = request.url.copy_with(scheme="http", host="127.0.0.1", port=self._port)
        return await self._inner.handle_async_request(request)

    async def aclose(self) -> None:
        await self._inner.aclose()


def create(mock_port: int) -> Any:
    os.environ["STAGEPILOT_PCO_CLIENT_ID"] = CLIENT_ID
    settings = Settings(
        integration_modes=IntegrationModes(
            service_source=ServiceSource.PLANNING_CENTER,
            midi_source=MidiSource.SIMULATED,
            timer_output=TimerOutput.SIMULATED,
        ),
        # A personal access token for one service type, so the plugin connects at boot.
        planning_center=PlanningCenterSettings(
            app_id="app-1", secret="secret-1", service_type_id="111", request_timeout_seconds=5
        ),
    )
    return create_app(
        settings,
        planning_center_client_factory=lambda s: PlanningCenterClient(
            s, transport=ToMock(mock_port)
        ),
        planning_center_today_provider=lambda _zone: TODAY,
        oauth_credential_store=MemoryCredentialStore(),
        oauth_control_plane=ControlPlaneOAuthClient(transport=ToMock(mock_port)),
        dashboard_auth_enforced=False,
    )
