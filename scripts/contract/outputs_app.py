"""The "outputs" contract profile: the Python app with fake MIDI and a mock ProPresenter.

MIDI (input and lights output) is an in-memory fake with fixed port names, ProPresenter is the
mock HTTP server in `mock_propresenter.py`, lights send OSC over loopback UDP (nothing listens, so
nothing is delivered). No MIDI, OSC or ProPresenter traffic leaves this machine.
"""

from __future__ import annotations

from typing import Any

from stagepilot.core.config import (
    DemoSettings,
    LightingMethod,
    LightsSettings,
    MidiSettings,
    ProPresenterSettings,
    Settings,
)
from stagepilot.main import create_app

MIDI_INPUTS = ["StagePilot Test In", "Second Test In"]
MIDI_OUTPUTS = ["StagePilot Test Out"]
INPUT_NAME = "StagePilot Test In"
OSC_PORT = 47999  # nothing listens here; UDP sends to a closed loopback port do not error


class _InPort:
    def __init__(self, name: str) -> None:
        self.name = name
        self.closed = False

    def close(self) -> None:
        self.closed = True


class FakeInBackend:
    def __init__(self) -> None:
        self.ports: list[_InPort] = []

    def list_input_names(self) -> list[str]:
        return list(MIDI_INPUTS)

    def open_input(self, name: str, _callback: Any) -> _InPort:
        port = _InPort(name)
        self.ports.append(port)
        return port


class _OutPort:
    def __init__(self) -> None:
        self.closed = False

    def send_note_on(self, channel: int, note: int, velocity: int) -> None:
        return None

    def send_note_off(self, channel: int, note: int) -> None:
        return None

    def close(self) -> None:
        self.closed = True


class FakeOutBackend:
    def list_output_names(self) -> list[str]:
        return list(MIDI_OUTPUTS)

    def open_output(self, name: str) -> _OutPort:
        if name not in MIDI_OUTPUTS:
            raise ValueError("Output unavailable.")
        return _OutPort()


def settings(propresenter_port: int) -> Settings:
    return Settings(
        demo_mode=True,
        demo=DemoSettings(simulate_midi=False, simulate_propresenter=False),
        midi=MidiSettings(enabled=True, input_name=INPUT_NAME),
        propresenter=ProPresenterSettings(
            enabled=True,
            host="127.0.0.1",
            port=propresenter_port,
            health_check_interval_seconds=300,
            reconnect_initial_seconds=0.5,
            reconnect_max_seconds=1,
        ),
        lights=LightsSettings(
            enabled=True, method=LightingMethod.OSC, osc_host="127.0.0.1", osc_port=OSC_PORT
        ),
    )


def create(propresenter_port: int) -> Any:
    in_backend = FakeInBackend()
    out_backend = FakeOutBackend()
    return create_app(
        settings(propresenter_port),
        midi_backend_factory=lambda: in_backend,
        lights_backend_factory=lambda: out_backend,
        dashboard_auth_enforced=False,
    )
