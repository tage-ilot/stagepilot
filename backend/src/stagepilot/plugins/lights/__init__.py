"""Lighting MIDI-output integration exports."""

from stagepilot.plugins.lights.client import (
    MidiOutputBackendContract,
    MidiOutputBackendFactory,
    MidiOutputPortContract,
    MidoMidiOutputBackend,
)
from stagepilot.plugins.lights.osc import (
    OscOutputBackendContract,
    OscOutputBackendFactory,
    OscOutputContract,
    UdpOscOutputBackend,
)
from stagepilot.plugins.lights.plugin import LightsPlugin

__all__ = [
    "LightsPlugin",
    "MidiOutputBackendContract",
    "MidiOutputBackendFactory",
    "MidiOutputPortContract",
    "MidoMidiOutputBackend",
    "OscOutputBackendContract",
    "OscOutputBackendFactory",
    "OscOutputContract",
    "UdpOscOutputBackend",
]
