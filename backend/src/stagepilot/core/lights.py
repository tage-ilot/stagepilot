"""Typed boundary for the Lighting MIDI-output integration."""

from __future__ import annotations

from datetime import datetime
from typing import Protocol

from pydantic import BaseModel, Field

from stagepilot.core.actions import ActionOutcome
from stagepilot.core.config import (
    LightingCue,
    LightingMethod,
    LightsSettings,
    OscArgument,
    OscLightingCue,
    OscPreset,
    SongLightingCueMap,
)
from stagepilot.models.state import ConnectionStatus


class LightingOutputSummary(BaseModel):
    name: str = Field(min_length=1, max_length=512)
    ambiguous: bool = False
    selected: bool = False
    connected: bool = False


class LightsSnapshot(BaseModel):
    enabled: bool
    method: LightingMethod = LightingMethod.OSC
    osc_preset: OscPreset = OscPreset.LIGHTKEY
    osc_host: str = "127.0.0.1"
    osc_port: int = 21_600
    output_name: str | None = None
    channel: int = Field(ge=1, le=15)
    pulse_ms: int = Field(ge=10, le=2_000)
    connection_status: ConnectionStatus
    detail: str | None = None
    outputs: list[LightingOutputSummary] = Field(default_factory=list)
    last_cue: LightingCue | None = None
    last_osc_cue: OscLightingCue | None = None
    last_cue_at: datetime | None = None


class LightsController(Protocol):
    async def snapshot(self, *, refresh: bool = False) -> LightsSnapshot: ...

    async def reconfigure(self, settings: LightsSettings) -> ActionOutcome: ...

    async def test_osc_cue(self, address: str, args: list[OscArgument]) -> ActionOutcome: ...

    async def test_cue(self, note: int, velocity: int) -> ActionOutcome: ...

    async def replace_cue_map(self, cue_map: SongLightingCueMap) -> None: ...
