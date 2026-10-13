"""Typed events exchanged by StagePilot services and plugins."""

from __future__ import annotations

from datetime import UTC, date, datetime
from enum import StrEnum
from typing import Annotated, Literal
from uuid import UUID, uuid4

from pydantic import BaseModel, Field

from stagepilot.models.state import (
    ConnectionStatus,
    PluginStatus,
    ServiceLoadStatus,
    ServicePlan,
    ServicePlanCandidate,
    SkippedServiceItem,
    Song,
)


class EventType(StrEnum):
    APPLICATION_STARTED = "application.started"
    APPLICATION_STOPPING = "application.stopping"
    ACTION_REQUESTED = "action.requested"
    SERVICE_RELOAD_REQUESTED = "service.reload_requested"
    SERVICE_PLAN_SELECTION_REQUESTED = "service.plan_selection_requested"
    SERVICE_LOAD_CHANGED = "service.load_changed"
    SERVICE_LOADED = "service.loaded"
    SONG_SELECTED = "song.selected"
    SONG_STARTED = "song.started"
    SONG_RESTARTED = "song.restarted"
    MIDI_NOTE_RECEIVED = "midi.note_received"
    TIMER_STOP_REQUESTED = "timer.stop_requested"
    TIMER_RESET_REQUESTED = "timer.reset_requested"
    TIMER_STARTED = "timer.started"
    TIMER_RESUMED = "timer.resumed"
    TIMER_SEEKED = "timer.seeked"
    TIMER_STOPPED = "timer.stopped"
    TIMER_FAILED = "timer.failed"
    CONNECTION_CHANGED = "connection.changed"
    PLUGIN_STATUS_CHANGED = "plugin.status_changed"
    PLUGIN_FAILED = "plugin.failed"


class ActionName(StrEnum):
    START_NEXT = "start_next"
    RESTART_CURRENT = "restart_current"
    PREVIOUS = "previous"
    NEXT = "next"
    STOP_TIMER = "stop_timer"
    RELOAD_PLAN = "reload_plan"
    RESET_POSITION = "reset_position"


class EmptyPayload(BaseModel):
    kind: Literal["empty"] = "empty"


class ActionPayload(BaseModel):
    kind: Literal["action"] = "action"
    action: ActionName
    request_id: UUID


class ServicePayload(BaseModel):
    kind: Literal["service"] = "service"
    plan: ServicePlan


class ServicePlanSelectionPayload(BaseModel):
    kind: Literal["service_plan_selection"] = "service_plan_selection"
    plan_id: str = Field(min_length=1, max_length=128)


class ServiceLoadPayload(BaseModel):
    kind: Literal["service_load"] = "service_load"
    status: ServiceLoadStatus
    target_date: date | None = None
    candidates: list[ServicePlanCandidate] = Field(default_factory=list)
    skipped_items: list[SkippedServiceItem] = Field(default_factory=list)
    message: str | None = None
    is_stale: bool = False


class SongPayload(BaseModel):
    kind: Literal["song"] = "song"
    song: Song
    index: int = Field(ge=0)


class MidiNotePayload(BaseModel):
    kind: Literal["midi_note"] = "midi_note"
    channel: int = Field(ge=1, le=16)
    note: int = Field(ge=0, le=127)
    velocity: int = Field(ge=1, le=127)
    action: ActionName
    song_position: int | None = Field(default=None, ge=1, le=99)
    connection_id: UUID | None = None
    simulated: bool = False


class TimerPayload(BaseModel):
    kind: Literal["timer"] = "timer"
    duration_seconds: int = Field(ge=0)
    song: Song | None = None
    started_at: datetime | None = None
    message: str | None = None
    # Song position (seconds) the countdown starts from (resumed/seeked) or
    # was frozen at (stopped). None when the emitter does not know it.
    position_seconds: float | None = Field(default=None, ge=0)
    # Seconds left at that position (plan duration minus position, clamped at 0).
    remaining_seconds: int | None = Field(default=None, ge=0)
    # timer.seeked only: True when the countdown keeps running after the seek.
    running: bool = False


class ConnectionPayload(BaseModel):
    kind: Literal["connection"] = "connection"
    integration: Literal["planning_center", "midi", "propresenter", "lights"]
    status: ConnectionStatus
    detail: str | None = None


class PluginPayload(BaseModel):
    kind: Literal["plugin"] = "plugin"
    name: str
    version: str
    status: PluginStatus
    error: str | None = None


EventPayload = Annotated[
    EmptyPayload
    | ActionPayload
    | ServicePayload
    | ServicePlanSelectionPayload
    | ServiceLoadPayload
    | SongPayload
    | MidiNotePayload
    | TimerPayload
    | ConnectionPayload
    | PluginPayload,
    Field(discriminator="kind"),
]


class StagePilotEvent(BaseModel):
    """Immutable event envelope with a typed, discriminated payload."""

    model_config = {"frozen": True}

    id: UUID = Field(default_factory=uuid4)
    type: EventType
    timestamp: datetime = Field(default_factory=lambda: datetime.now(UTC))
    source: str
    payload: EventPayload = Field(default_factory=EmptyPayload)


def new_event(
    event_type: EventType,
    *,
    source: str,
    payload: EventPayload | None = None,
) -> StagePilotEvent:
    """Build an event while keeping event creation consistent at call sites."""

    return StagePilotEvent(type=event_type, source=source, payload=payload or EmptyPayload())
