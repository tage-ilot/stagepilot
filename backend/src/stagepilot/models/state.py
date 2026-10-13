"""Observable application state models."""

from __future__ import annotations

from datetime import UTC, date, datetime
from enum import StrEnum
from uuid import UUID

from pydantic import BaseModel, Field, computed_field, model_validator


class ApplicationStatus(StrEnum):
    STARTING = "starting"
    RUNNING = "running"
    STOPPING = "stopping"
    ERROR = "error"


class ConnectionStatus(StrEnum):
    CONNECTING = "connecting"
    CONNECTED = "connected"
    DISCONNECTED = "disconnected"
    ERROR = "error"


class TimerStatus(StrEnum):
    IDLE = "idle"
    RUNNING = "running"
    STOPPED = "stopped"
    ERROR = "error"


class PluginStatus(StrEnum):
    STOPPED = "stopped"
    STARTING = "starting"
    RUNNING = "running"
    STOPPING = "stopping"
    ERROR = "error"


class ServiceLoadStatus(StrEnum):
    IDLE = "idle"
    LOADING = "loading"
    LOADED = "loaded"
    NOT_FOUND = "not_found"
    AMBIGUOUS = "ambiguous"
    ERROR = "error"


class Song(BaseModel):
    id: str
    title: str = Field(min_length=1)
    duration_seconds: int | None = Field(default=None, ge=0)
    order: int = Field(ge=1)
    service_sequence: int | None = Field(default=None, ge=0)
    is_generic: bool = False
    source_song_id: str | None = None
    description: str | None = Field(default=None, max_length=10_000)

    @computed_field  # type: ignore[prop-decorator]
    @property
    def formatted_duration(self) -> str | None:
        if self.duration_seconds is None:
            return None
        hours, remainder = divmod(self.duration_seconds, 3600)
        minutes, seconds = divmod(remainder, 60)
        if hours:
            return f"{hours}:{minutes:02d}:{seconds:02d}"
        return f"{minutes}:{seconds:02d}"


class ServicePlan(BaseModel):
    id: str
    title: str
    date: date
    service_type: str
    service_type_id: str | None = None
    service_times: list[str] = Field(default_factory=list)
    duration_source: str = "Scheduled item length"
    songs: list[Song] = Field(default_factory=list)


class ServicePlanCandidate(BaseModel):
    id: str = Field(min_length=1, max_length=128)
    title: str = Field(min_length=1, max_length=500)
    service_type_id: str = Field(min_length=1, max_length=128)
    service_type_name: str = Field(min_length=1, max_length=500)
    target_date: date
    service_times: list[str] = Field(min_length=1)


class SkippedServiceItem(BaseModel):
    item_id: str = Field(min_length=1, max_length=128)
    title: str = Field(min_length=1, max_length=500)
    description: str | None = Field(default=None, max_length=10_000)
    item_type: str = Field(min_length=1, max_length=32)
    sequence: int = Field(ge=0)
    duration_seconds: int | None = Field(default=None, ge=0)
    reason: str = Field(min_length=1, max_length=64)


class ServiceLoadState(BaseModel):
    """Validated projection of the current plan-loading outcome."""

    status: ServiceLoadStatus = ServiceLoadStatus.IDLE
    target_date: date | None = None
    candidates: list[ServicePlanCandidate] = Field(default_factory=list)
    skipped_items: list[SkippedServiceItem] = Field(default_factory=list)
    message: str | None = None
    is_stale: bool = False
    last_attempt_at: datetime | None = None

    @model_validator(mode="after")
    def load_state_is_consistent(self) -> ServiceLoadState:
        if self.status is ServiceLoadStatus.IDLE:
            if self.target_date is not None:
                msg = "An idle service-load state cannot have a target date."
                raise ValueError(msg)
        elif self.target_date is None:
            msg = "A non-idle service-load state requires a target date."
            raise ValueError(msg)

        if self.status is ServiceLoadStatus.AMBIGUOUS:
            if len(self.candidates) < 2:
                msg = "An ambiguous service-load state requires at least two candidates."
                raise ValueError(msg)
            if any(candidate.target_date != self.target_date for candidate in self.candidates):
                msg = "Every service-plan candidate must match the service-load target date."
                raise ValueError(msg)
        elif self.candidates:
            msg = "Service-plan candidates are only valid for an ambiguous load state."
            raise ValueError(msg)

        stale_statuses = {
            ServiceLoadStatus.LOADING,
            ServiceLoadStatus.NOT_FOUND,
            ServiceLoadStatus.AMBIGUOUS,
            ServiceLoadStatus.ERROR,
        }
        if self.is_stale and self.status not in stale_statuses:
            msg = "An idle or successfully loaded service cannot retain a stale plan."
            raise ValueError(msg)
        return self


class TimerState(BaseModel):
    status: TimerStatus = TimerStatus.IDLE
    duration_seconds: int | None = Field(default=None, ge=0)
    started_at: datetime | None = None
    # Position in the song (seconds) when the countdown was last started
    # (`running`) or frozen (`stopped`). None when unknown or blanked.
    position_seconds: float | None = Field(default=None, ge=0)
    # Seconds left on the countdown at the moment it was stopped. None while
    # running (the UI derives it from duration_seconds and started_at), after
    # a reset, or when unknown.
    remaining_seconds: int | None = Field(default=None, ge=0)
    last_error: str | None = None


class PluginHealth(BaseModel):
    name: str
    version: str
    status: PluginStatus
    last_error: str | None = None
    last_activity_at: datetime | None = None


class EventSummary(BaseModel):
    id: UUID
    type: str
    timestamp: datetime
    source: str


class ErrorSummary(BaseModel):
    timestamp: datetime = Field(default_factory=lambda: datetime.now(UTC))
    component: str
    message: str
    event_id: UUID | None = None


class ApplicationState(BaseModel):
    revision: int = Field(default=0, ge=0)
    updated_at: datetime = Field(default_factory=lambda: datetime.now(UTC))
    application_status: ApplicationStatus = ApplicationStatus.STARTING
    plan: ServicePlan | None = None
    # The navigation CURSOR: Previous / Next move it without starting anything, and Playback and
    # MIDI map against it.
    current_song: Song | None = None
    next_song: Song | None = None
    current_song_index: int | None = Field(default=None, ge=0)
    # What the screen should say (service plan labels, Now playing, Up next): the song actually
    # started (its timer, ProPresenter and lights ran) and the song the next Start next will play.
    # They differ from the cursor after Previous.
    playing_song: Song | None = None
    playing_song_index: int | None = Field(default=None, ge=0)
    up_next_song: Song | None = None
    planning_center_status: ConnectionStatus = ConnectionStatus.DISCONNECTED
    midi_status: ConnectionStatus = ConnectionStatus.DISCONNECTED
    propresenter_status: ConnectionStatus = ConnectionStatus.DISCONNECTED
    lights_status: ConnectionStatus = ConnectionStatus.DISCONNECTED
    service_load: ServiceLoadState = Field(default_factory=ServiceLoadState)
    timer: TimerState = Field(default_factory=TimerState)
    plugins: dict[str, PluginHealth] = Field(default_factory=dict)
    recent_events: list[EventSummary] = Field(default_factory=list)
    recent_errors: list[ErrorSummary] = Field(default_factory=list)
    last_successful_plan_reload_at: datetime | None = None
    last_action: str | None = None
