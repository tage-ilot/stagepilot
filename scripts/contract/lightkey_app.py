"""The "lightkey" contract profile: the Python app with Lighting on OSC and a fake lightkey-api.

The demo plan runs with simulated MIDI and a simulated timer output (so the countdown publishes the
timer events the link follows). Lighting sends OSC over loopback UDP to a
port nothing listens on, and the Lightkey link polls the fake agent in `mock_lightkey.py`. One take
is seeded for the first demo song so replay and clearing can be recorded. No MIDI, OSC or Lightkey
traffic leaves this machine.
"""

from __future__ import annotations

from datetime import UTC, datetime
from typing import Any

from stagepilot.core.config import (
    IntegrationModes,
    LightingMethod,
    LightkeySettings,
    LightsSettings,
    MidiSource,
    ServiceSource,
    Settings,
    TimerOutput,
)
from stagepilot.core.lightkey import LightkeyEdge, LightkeyTake
from stagepilot.main import create_app
from stagepilot.plugins.lights import MemoryTakeStore

OSC_PORT = 47999  # nothing listens here; UDP sends to a closed loopback port do not error
SEED_KEY = "demo-battle-belongs"


def seed_take() -> LightkeyTake:
    return LightkeyTake(
        song_key=SEED_KEY,
        song_title="Battle Belongs",
        recorded_at=datetime(2026, 10, 20, 20, 0, tzinfo=UTC),
        duration_seconds=281,
        initial=["Warm"],
        edges=[
            LightkeyEdge(at_seconds=10, added=["Stars"]),
            LightkeyEdge(at_seconds=40, added=["Strobe"], removed=["Stars"]),
        ],
    )


def settings(agent_port: int) -> Settings:
    return Settings(
        integration_modes=IntegrationModes(
            service_source=ServiceSource.DEMO,
            midi_source=MidiSource.SIMULATED,
            timer_output=TimerOutput.SIMULATED,
        ),
        lights=LightsSettings(
            enabled=True,
            method=LightingMethod.OSC,
            osc_host="127.0.0.1",
            osc_port=OSC_PORT,
            lightkey=LightkeySettings(api_url=f"127.0.0.1:{agent_port}"),
        ),
    )


def create(agent_port: int) -> Any:
    store = MemoryTakeStore()
    store.save({SEED_KEY: seed_take()})
    return create_app(settings(agent_port), lightkey_store=store, dashboard_auth_enforced=False)
