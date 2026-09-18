"""Runtime configuration with environment-variable overrides."""

from __future__ import annotations

from enum import StrEnum
from functools import lru_cache
from uuid import UUID, uuid4
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

from pydantic import BaseModel, ConfigDict, Field, SecretStr, field_validator, model_validator

from stagepilot.core.midi import MidiCueName


class DemoSettings(BaseModel):
    """Control which integrations are simulated while using the demo service plan."""

    simulate_midi: bool = True
    simulate_propresenter: bool = True


class ServiceSource(StrEnum):
    """Source used to populate the weekly service plan."""

    DEMO = "demo"
    PLANNING_CENTER = "planning_center"


class MidiSource(StrEnum):
    """Source used to trigger StagePilot actions."""

    SIMULATED = "simulated"
    REAL = "real"


class TimerOutput(StrEnum):
    """Destination used for countdown timer output."""

    SIMULATED = "simulated"
    PROPRESENTER = "propresenter"


class IntegrationModes(BaseModel):
    """Independent integration modes used for safe mixed-mode testing."""

    service_source: ServiceSource = ServiceSource.DEMO
    midi_source: MidiSource = MidiSource.SIMULATED
    timer_output: TimerOutput = TimerOutput.SIMULATED


class MidiVelocityMappings(BaseModel):
    """Configurable note-on velocity mappings for Playback cues."""

    start_next: int | None = Field(default=100, ge=1, le=127)
    restart_current: int | None = Field(default=101, ge=1, le=127)
    previous: int | None = Field(default=102, ge=1, le=127)
    next: int | None = Field(default=103, ge=1, le=127)
    reload_plan: int | None = Field(default=104, ge=1, le=127)
    stop_timer: int | None = Field(default=105, ge=1, le=127)

    @model_validator(mode="after")
    def mapped_velocities_are_unique(self) -> MidiVelocityMappings:
        velocities = [velocity for _cue, velocity in self.configured()]
        if any(velocity < 100 for velocity in velocities):
            raise ValueError(
                "MIDI action velocities must be between 100 and 127; "
                "velocities 1 through 99 identify service-plan songs."
            )
        if len(velocities) != len(set(velocities)):
            raise ValueError("Every configured MIDI action must use a distinct velocity.")
        return self

    def configured(self) -> tuple[tuple[MidiCueName, int], ...]:
        values = (
            (MidiCueName.START_NEXT, self.start_next),
            (MidiCueName.RESTART_CURRENT, self.restart_current),
            (MidiCueName.PREVIOUS, self.previous),
            (MidiCueName.NEXT, self.next),
            (MidiCueName.RELOAD_PLAN, self.reload_plan),
            (MidiCueName.STOP_TIMER, self.stop_timer),
        )
        return tuple((cue, velocity) for cue, velocity in values if velocity is not None)

    def velocity_for(self, cue: MidiCueName) -> int | None:
        return dict(self.configured()).get(cue)

    def cue_for(self, velocity: int) -> MidiCueName | None:
        return next(
            (cue for cue, mapped_velocity in self.configured() if mapped_velocity == velocity),
            None,
        )


# Backwards-compatible import name for code that imported the old class.
# The values now represent velocities, not note numbers.
MidiNoteMappings = MidiVelocityMappings


class MidiSettings(BaseModel):
    """Validated runtime settings for the Playback MIDI input."""

    enabled: bool = False
    input_name: str | None = Field(default=None, max_length=512)
    channel: int = Field(default=1, ge=1, le=16)
    note: int = Field(default=112, ge=0, le=127)
    mappings: MidiVelocityMappings = Field(default_factory=MidiVelocityMappings)
    debounce_ms: int = Field(default=250, ge=0, le=2000)

    @field_validator("input_name", mode="before")
    @classmethod
    def empty_input_name_is_unset(cls, value: object) -> object:
        if isinstance(value, str):
            stripped = value.strip()
            return stripped or None
        return value


class LightingCue(BaseModel):
    """One elapsed-time MIDI pulse sent to the lighting application."""

    id: UUID = Field(default_factory=uuid4)
    at_seconds: int = Field(ge=0, le=86_399)
    note: int = Field(ge=0, le=127)
    velocity: int = Field(default=127, ge=1, le=127)
    label: str = Field(default="", max_length=120)

    @field_validator("label", mode="before")
    @classmethod
    def label_is_trimmed(cls, value: object) -> object:
        return value.strip() if isinstance(value, str) else value


class SongLightingCueMap(BaseModel):
    """Persistent lighting timeline for one stable Planning Center song."""

    song_key: str = Field(min_length=1, max_length=256)
    song_title: str = Field(min_length=1, max_length=500)
    cues: list[LightingCue] = Field(default_factory=list, max_length=2_000)

    @field_validator("song_key", "song_title", mode="before")
    @classmethod
    def values_are_trimmed(cls, value: object) -> object:
        if isinstance(value, str):
            stripped = value.strip()
            if not stripped:
                raise ValueError("Value cannot be empty.")
            return stripped
        return value

    @model_validator(mode="after")
    def cue_ids_are_unique(self) -> SongLightingCueMap:
        cue_ids = [cue.id for cue in self.cues]
        if len(cue_ids) != len(set(cue_ids)):
            raise ValueError("Every lighting cue must have a unique ID.")
        self.cues.sort(key=lambda cue: (cue.at_seconds, str(cue.id)))
        return self


class LightsSettings(BaseModel):
    """Validated MIDI output and per-song cue timelines for Lights."""

    enabled: bool = False
    output_name: str | None = Field(default=None, max_length=512)
    channel: int = Field(default=1, ge=1, le=15)
    pulse_ms: int = Field(default=100, ge=10, le=2_000)
    cue_maps: dict[str, SongLightingCueMap] = Field(default_factory=dict)

    @field_validator("output_name", mode="before")
    @classmethod
    def empty_output_name_is_unset(cls, value: object) -> object:
        if isinstance(value, str):
            stripped = value.strip()
            return stripped or None
        return value

    @model_validator(mode="after")
    def cue_map_keys_match_song_keys(self) -> LightsSettings:
        for key, cue_map in self.cue_maps.items():
            if key != cue_map.song_key:
                raise ValueError("Every lighting cue-map key must match its song key.")
        return self


class ProPresenterSettings(BaseModel):
    """Validated runtime settings for ProPresenter's local HTTP API."""

    enabled: bool = False
    host: str = Field(default="127.0.0.1", min_length=1, max_length=255)
    port: int = Field(default=1025, ge=1, le=65535)
    timer_name: str = Field(default="Song Countdown", min_length=1, max_length=255)
    look_id: str | None = Field(default=None, max_length=256)
    request_timeout_seconds: float = Field(default=3.0, gt=0, le=60.0)
    reconnect_initial_seconds: float = Field(default=1.0, gt=0, le=60.0)
    reconnect_max_seconds: float = Field(default=30.0, gt=0, le=300.0)
    health_check_interval_seconds: float = Field(default=10.0, gt=0, le=300.0)

    @model_validator(mode="after")
    def reconnect_window_is_valid(self) -> ProPresenterSettings:
        if self.reconnect_max_seconds < self.reconnect_initial_seconds:
            raise ValueError(
                "ProPresenter reconnect maximum must be greater than or equal to the initial delay."
            )
        return self

    @field_validator("host", "timer_name", mode="before")
    @classmethod
    def values_are_trimmed(cls, value: object) -> object:
        if isinstance(value, str):
            stripped = value.strip()
            if not stripped:
                raise ValueError("Value cannot be empty.")
            return stripped
        return value

    @field_validator("look_id", mode="before")
    @classmethod
    def empty_look_id_is_unset(cls, value: object) -> object:
        if isinstance(value, str):
            return value.strip() or None
        return value

    @property
    def base_url(self) -> str:
        return f"http://{self.host}:{self.port}"


class PlanningCenterSettings(BaseModel):
    """Validated server-side settings for Planning Center Personal Access Tokens."""

    model_config = ConfigDict(hide_input_in_errors=True)

    app_id: SecretStr | None = None
    secret: SecretStr | None = None
    service_type_id: str | None = Field(default=None, min_length=1)
    plan_title_preference: str | None = Field(default=None, max_length=255)
    preferred_service_time: str | None = Field(default=None, pattern=r"^([01]\d|2[0-3]):[0-5]\d$")
    upcoming_lookahead_days: int = Field(default=30, ge=0, le=365)
    request_timeout_seconds: float = Field(default=10.0, ge=1.0, le=60.0)
    user_agent: str = Field(
        default="StagePilot/1.1.103 (https://github.com/tage-ilot/stagepilot)",
        min_length=1,
        max_length=256,
    )

    @field_validator("app_id", "secret", mode="before")
    @classmethod
    def empty_secrets_are_unset(cls, value: object) -> object:
        """Treat empty environment values as absent credentials."""

        if isinstance(value, str):
            stripped = value.strip()
            return stripped or None
        return value

    @field_validator(
        "service_type_id",
        "plan_title_preference",
        "preferred_service_time",
        mode="before",
    )
    @classmethod
    def empty_service_type_is_unset(cls, value: object) -> object:
        if isinstance(value, str):
            stripped = value.strip()
            return stripped or None
        return value

    @property
    def is_configured(self) -> bool:
        """Return a safe credential-presence flag without revealing either value."""

        return self.app_id is not None and self.secret is not None

    def credentials(self) -> tuple[str, str]:
        """Return credentials only to trusted backend integrations."""

        if self.app_id is None or self.secret is None:
            msg = "Planning Center credentials are not configured."
            raise ValueError(msg)
        return self.app_id.get_secret_value(), self.secret.get_secret_value()


class Settings(BaseModel):
    """Validated runtime settings; integration secrets remain server-side only."""

    app_name: str = "StagePilot"
    version: str = "1.1.103"
    bind_host: str = "127.0.0.1"
    bind_port: int = Field(default=8765, ge=1, le=65535)
    log_level: str = "INFO"
    integration_modes: IntegrationModes = Field(default_factory=IntegrationModes)
    demo_mode: bool | None = Field(default=None, exclude=True, repr=False)
    demo: DemoSettings = Field(default_factory=DemoSettings, exclude=True, repr=False)
    timezone: str = "America/Los_Angeles"
    planning_center: PlanningCenterSettings = Field(default_factory=PlanningCenterSettings)
    midi: MidiSettings = Field(default_factory=MidiSettings)
    lights: LightsSettings = Field(default_factory=LightsSettings)
    propresenter: ProPresenterSettings = Field(default_factory=ProPresenterSettings)
    recent_event_limit: int = Field(default=100, ge=1, le=1000)
    recent_error_limit: int = Field(default=50, ge=1, le=500)

    @model_validator(mode="after")
    def legacy_demo_settings_map_to_independent_modes(self) -> Settings:
        """Keep v0.4 constructors working while v0.5 adopts independent modes."""

        if self.demo_mode is None:
            return self
        midi_source = (
            MidiSource.REAL
            if (not self.demo_mode and self.midi.enabled) or not self.demo.simulate_midi
            else MidiSource.SIMULATED
        )
        timer_output = (
            TimerOutput.PROPRESENTER
            if (not self.demo_mode and self.propresenter.enabled)
            or not self.demo.simulate_propresenter
            else TimerOutput.SIMULATED
        )
        self.integration_modes = IntegrationModes(
            service_source=(
                ServiceSource.DEMO if self.demo_mode else ServiceSource.PLANNING_CENTER
            ),
            midi_source=midi_source,
            timer_output=timer_output,
        )
        return self

    @property
    def uses_demo_service(self) -> bool:
        return self.integration_modes.service_source is ServiceSource.DEMO

    @field_validator("timezone")
    @classmethod
    def timezone_must_be_valid(cls, value: str) -> str:
        try:
            ZoneInfo(value)
        except (ZoneInfoNotFoundError, ValueError) as exc:
            msg = f"Unknown IANA timezone: {value}"
            raise ValueError(msg) from exc
        return value


@lru_cache
def get_settings() -> Settings:
    """Resolve defaults, saved settings, credentials, and environment overrides."""

    from stagepilot.core.settings import load_runtime_settings

    return load_runtime_settings()
