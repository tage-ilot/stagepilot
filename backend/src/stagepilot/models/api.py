"""Typed HTTP and WebSocket response models."""

from __future__ import annotations

from datetime import datetime
from typing import Literal
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field, SecretStr, field_validator, model_validator

from stagepilot.core.config import (
    LightingCue,
    LightingMethod,
    LightingPad,
    OscArgument,
    OscLightingCue,
    OscPreset,
    SongLightingCueMap,
    validate_osc_address,
)
from stagepilot.core.events import ActionName
from stagepilot.core.midi import MidiCueName, MidiMessageDisposition
from stagepilot.core.settings import PersistentPlanningCenterSettings, PersistentSettings
from stagepilot.models.state import (
    ApplicationState,
    ApplicationStatus,
    ConnectionStatus,
    PluginHealth,
    ServicePlanCandidate,
)


class HealthResponse(BaseModel):
    status: Literal["healthy", "degraded"]
    version: str
    application_status: ApplicationStatus
    plugins: list[PluginHealth]
    # When this backend process started (UTC). Read-only; shown as "Running since".
    started_at: datetime | None = None


class SettingsResponse(BaseModel):
    settings: PersistentSettings
    planning_center_secret_saved: bool
    warning: str | None = None
    restart_required: bool = False


class DashboardAuthStatusResponse(BaseModel):
    required: bool
    authenticated: bool


class DashboardAuthLoginRequest(BaseModel):
    pin: str = Field(min_length=4, max_length=64)


class DashboardAccessSettingsRequest(BaseModel):
    enabled: bool
    pin: SecretStr | None = Field(default=None, min_length=4, max_length=64)


class PlanningCenterSettingsUpdateRequest(PersistentPlanningCenterSettings):
    """Update non-secret settings and optionally replace or remove the saved PAT secret."""

    model_config = ConfigDict(extra="forbid", hide_input_in_errors=True)

    secret: SecretStr | None = None
    remove_secret: bool = False

    @model_validator(mode="after")
    def secret_action_is_unambiguous(self) -> PlanningCenterSettingsUpdateRequest:
        if self.secret is not None and self.remove_secret:
            raise ValueError("A credential cannot be replaced and removed in one request.")
        return self


class PlanningCenterStatusResponse(BaseModel):
    connection_status: ConnectionStatus
    configured: bool
    app_id: str | None = None
    service_type_id: str | None = None
    planning_center_secret_saved: bool
    connection_method: Literal["oauth", "manual"] = "manual"
    oauth_connected: bool = False
    oauth_needs_reconnect: bool = False
    detail: str | None = None


class PlanningCenterOAuthStartResponse(BaseModel):
    """Everything the desktop shell needs to open the consent page.

    `authorize_url` deliberately omits `redirect_uri`: only the desktop
    side knows which of the four pre-registered loopback ports was free,
    and it appends that itself before opening the browser.
    """

    authorize_url: str = Field(min_length=1)
    state: str = Field(min_length=1)


class PlanningCenterOAuthCallbackRequest(BaseModel):
    model_config = ConfigDict(extra="forbid", hide_input_in_errors=True)

    state: str = Field(min_length=1, max_length=512)
    code: SecretStr = Field(min_length=1)
    redirect_uri: str = Field(min_length=1, max_length=256)


class PlanningCenterOAuthStatusResponse(BaseModel):
    connection_method: Literal["oauth", "manual"]
    connected: bool
    needs_reconnect: bool
    expires_at: float | None = None
    scope: str | None = None


class PlanningCenterTestRequest(BaseModel):
    """Temporary credentials used only for one authentication test."""

    model_config = ConfigDict(extra="forbid", hide_input_in_errors=True)

    app_id: str | None = Field(default=None, min_length=1, max_length=255)
    secret: SecretStr | None = Field(default=None, min_length=1, max_length=255)


class PlanningCenterServiceTypeResponse(BaseModel):
    id: str = Field(min_length=1, max_length=128, pattern=r"^[A-Za-z0-9_-]+$")
    name: str = Field(min_length=1, max_length=500)


class PlanningCenterTestResponse(BaseModel):
    authenticated: Literal[True] = True
    message: str
    service_types: list[PlanningCenterServiceTypeResponse]


class ActionResponse(BaseModel):
    action: ActionName
    accepted: bool
    message: str
    state: ApplicationState


class PlanSelectionRequest(BaseModel):
    plan_id: str = Field(
        min_length=1,
        max_length=128,
        pattern=r"^[A-Za-z0-9_-]+$",
    )


class PlanSelectionResponse(BaseModel):
    accepted: bool
    message: str
    state: ApplicationState


class PendingPlanSelectionResponse(BaseModel):
    pending: bool
    target_date: str | None = None
    candidates: list[ServicePlanCandidate]
    message: str | None = None


class MidiInputResponse(BaseModel):
    id: str = Field(min_length=64, max_length=64, pattern=r"^[a-f0-9]+$")
    name: str = Field(min_length=1, max_length=512)
    ambiguous: bool
    selected: bool
    connected: bool


class MidiInputsResponse(BaseModel):
    enabled: bool
    channel: int = Field(ge=1, le=16)
    note: int = Field(ge=0, le=127)
    configured_input_name: str | None = None
    selected_input_name: str | None = None
    inputs: list[MidiInputResponse]
    mappings: dict[MidiCueName, int]


class MidiMonitorMessageResponse(BaseModel):
    timestamp: datetime
    input_name: str | None = None
    message_type: Literal["note_on", "note_off"]
    channel: int = Field(ge=1, le=16)
    note: int = Field(ge=0, le=127)
    note_name: str = Field(min_length=2, max_length=4)
    velocity: int = Field(ge=0, le=127)
    disposition: MidiMessageDisposition
    detail: str
    action: ActionName | None = None
    simulated: bool = False


class MidiMonitorResponse(BaseModel):
    messages: list[MidiMonitorMessageResponse]


class MidiInputSelectionRequest(BaseModel):
    input_id: str | None = Field(
        min_length=64,
        max_length=64,
        pattern=r"^[a-f0-9]+$",
    )


class MidiInputSelectionResponse(BaseModel):
    accepted: bool
    message: str
    midi: MidiInputsResponse


class MidiCueSimulationRequest(BaseModel):
    cue: MidiCueName


class MidiCueSimulationResponse(BaseModel):
    cue: MidiCueName
    action: ActionName
    accepted: bool
    message: str
    state: ApplicationState


class ProPresenterTimerResponse(BaseModel):
    id: str = Field(min_length=1, max_length=256)
    name: str = Field(min_length=1, max_length=255)
    index: int = Field(ge=0)
    is_countdown: bool
    state: str | None = None


class ProPresenterLookResponse(BaseModel):
    id: str = Field(min_length=1, max_length=256)
    name: str = Field(min_length=1, max_length=255)
    index: int = Field(ge=0)


class ProPresenterStatusResponse(BaseModel):
    enabled: bool
    host: str = Field(min_length=1, max_length=255)
    port: int = Field(ge=1, le=65535)
    timer_name: str = Field(min_length=1, max_length=255)
    look_id: str | None = Field(default=None, max_length=256)
    request_timeout_seconds: float = Field(gt=0, le=60.0)
    connection_status: ConnectionStatus
    detail: str | None = None
    timers: list[ProPresenterTimerResponse] = Field(default_factory=list)
    selected_timer_id: str | None = None
    timer_found: bool
    looks: list[ProPresenterLookResponse] = Field(default_factory=list)
    current_look_id: str | None = None
    look_found: bool
    last_checked_at: datetime | None = None


class ProPresenterSettingsRequest(BaseModel):
    host: str = Field(min_length=1, max_length=255)
    port: int = Field(ge=1, le=65535)
    timer_name: str = Field(min_length=1, max_length=255)
    look_id: str | None = Field(default=None, max_length=256)
    request_timeout_seconds: float = Field(gt=0, le=60.0)


class ProPresenterOperationResponse(BaseModel):
    accepted: bool
    message: str
    propresenter: ProPresenterStatusResponse


class LightingOutputResponse(BaseModel):
    name: str = Field(min_length=1, max_length=512)
    ambiguous: bool
    selected: bool
    connected: bool


class LightsStatusResponse(BaseModel):
    enabled: bool
    method: LightingMethod = LightingMethod.OSC
    osc_preset: OscPreset = OscPreset.LIGHTKEY
    osc_host: str = "127.0.0.1"
    osc_port: int = Field(default=21_600, ge=1, le=65_535)
    output_name: str | None = Field(default=None, max_length=512)
    channel: int = Field(ge=1, le=15)
    pulse_ms: int = Field(ge=10, le=2_000)
    connection_status: ConnectionStatus
    detail: str | None = None
    outputs: list[LightingOutputResponse] = Field(default_factory=list)
    last_cue: LightingCue | None = None
    last_osc_cue: OscLightingCue | None = None
    last_cue_at: datetime | None = None


class LightsSettingsRequest(BaseModel):
    enabled: bool = True
    # A request without a method is a pre-OSC client and means MIDI.
    method: LightingMethod = LightingMethod.MIDI
    osc_preset: OscPreset | None = None
    osc_host: str | None = Field(default=None, min_length=1, max_length=253)
    osc_port: int | None = Field(default=None, ge=1, le=65_535)
    output_name: str | None = Field(default=None, max_length=512)
    channel: int = Field(default=1, ge=1, le=15)
    pulse_ms: int = Field(default=100, ge=10, le=2_000)


class LightingCueTestRequest(BaseModel):
    note: int = Field(ge=0, le=127)
    velocity: int = Field(default=127, ge=1, le=127)


class LightingOscCueTestRequest(BaseModel):
    address: str = Field(min_length=1, max_length=256)
    args: list[OscArgument] = Field(default_factory=list, max_length=16)

    @field_validator("address")
    @classmethod
    def address_is_valid(cls, value: str) -> str:
        return validate_osc_address(value.strip())


class LightingCueMapRequest(SongLightingCueMap):
    """Replaces only ``method``'s cue list; the other method's cues are kept untouched."""

    method: LightingMethod = LightingMethod.MIDI


class LightsOperationResponse(BaseModel):
    accepted: bool
    message: str
    lights: LightsStatusResponse


class LightingPadsRequest(BaseModel):
    """Replaces the whole cue board (create, edit, rename, reorder and delete in one save)."""

    pads: list[LightingPad] = Field(default_factory=list, max_length=64)

    @model_validator(mode="after")
    def pad_ids_are_unique(self) -> LightingPadsRequest:
        ids = [pad.id for pad in self.pads]
        if len(ids) != len(set(ids)):
            raise ValueError("Every lighting pad must have a unique ID.")
        return self


class LightingCaptureRequest(BaseModel):
    """Fires a pad and, when ``song_key`` and ``at_seconds`` are given, adds it to the song."""

    pad_id: UUID
    song_key: str | None = Field(default=None, min_length=1, max_length=256)
    song_title: str | None = Field(default=None, min_length=1, max_length=500)
    at_seconds: float | None = Field(default=None, ge=0, le=86_399)

    @model_validator(mode="after")
    def capture_is_complete(self) -> LightingCaptureRequest:
        capturing = (self.song_key, self.song_title, self.at_seconds)
        if any(item is not None for item in capturing) and any(item is None for item in capturing):
            raise ValueError("A capture needs song_key, song_title and at_seconds together.")
        return self


class LightingCaptureResponse(LightsOperationResponse):
    cue_id: UUID | None = None
    at_seconds: float | None = None


class LightingCaptureUndoRequest(BaseModel):
    song_key: str = Field(min_length=1, max_length=256)
    cue_id: UUID


class StateEnvelope(BaseModel):
    type: Literal["state.snapshot"] = "state.snapshot"
    data: ApplicationState
