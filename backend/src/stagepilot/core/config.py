"""Runtime configuration with environment-variable overrides."""

from __future__ import annotations

from collections.abc import Mapping
from datetime import datetime
from enum import StrEnum
from functools import lru_cache
from typing import Any, Literal, Self
from uuid import UUID, uuid4
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

from pydantic import (
    BaseModel,
    ConfigDict,
    Field,
    SecretStr,
    StrictInt,
    field_validator,
    model_validator,
)

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
    PLAYBACK_API = "playback_api"


class MidiTransport(StrEnum):
    """Physical transport used by a real MIDI integration."""

    LOCAL = "local"
    NETWORK = "network"


class TimerOutput(StrEnum):
    """Destination used for countdown timer output."""

    SIMULATED = "simulated"
    PROPRESENTER = "propresenter"


class IntegrationModes(BaseModel):
    """Independent integration modes used for safe mixed-mode testing."""

    service_source: ServiceSource = ServiceSource.DEMO
    midi_source: MidiSource = MidiSource.PLAYBACK_API
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


class PlaybackApiSettings(BaseModel):
    """Connection configuration for the primary Playback API input."""

    model_config = ConfigDict(extra="forbid")

    enabled: bool = True
    host: str | None = Field(default=None, max_length=255)
    port: int = Field(default=8080, ge=1, le=65535)
    auto_scan: bool = True
    fast_transport: bool = True
    planning_center_update_service_type_id: str | None = None
    song_order: list[StrictInt] = Field(default_factory=list, max_length=200)
    captured_version: StrictInt | None = None
    setlist_id: StrictInt | str | None = None
    scan_plan_key: str | None = None
    lengths_applied_key: str | None = None
    captured_at: datetime | None = None
    song_lengths: list[float | None] = Field(default_factory=list, max_length=200)
    # Songs left out of the pairing; order never changes (plugins/playback_api/arrangement.py).
    skipped_song_ids: list[StrictInt] = Field(default_factory=list, max_length=200)
    skipped_plan_song_ids: list[str] = Field(default_factory=list, max_length=200)
    discovery_duration_seconds: float | None = Field(default=None, gt=0)
    lengths_measured_at: datetime | None = None

    def model_copy(self, *, update: Mapping[str, Any] | None = None, deep: bool = False) -> Self:
        if update and "song_order" in update and update["song_order"] != self.song_order:
            # Discovery supplies a new aligned measurement explicitly.
            update = {
                "song_lengths": [],
                "lengths_measured_at": None,
                # A new scan is a new setlist: Playback skips no longer mean anything.
                "skipped_song_ids": [],
                **update,
            }
        return super().model_copy(update=update, deep=deep)

    @model_validator(mode="after")
    def lengths_match_order(self) -> PlaybackApiSettings:
        if self.song_lengths and len(self.song_lengths) != len(self.song_order):
            raise ValueError("Song lengths must have the same length as song order.")
        if any(length is not None and not 0 < length <= 86400 for length in self.song_lengths):
            raise ValueError("Song lengths must be finite positive seconds or null.")
        if not set(self.skipped_song_ids) <= set(self.song_order):
            raise ValueError("Skipped Playback songs must be songs in the saved order.")
        return self

    @field_validator("skipped_song_ids", "skipped_plan_song_ids")
    @classmethod
    def skips_are_unique(cls, value: list[Any]) -> list[Any]:
        if len(value) != len(set(value)):
            raise ValueError("Skipped songs must be unique.")
        return value

    @field_validator("song_order")
    @classmethod
    def song_ids_are_unique(cls, value: list[int]) -> list[int]:
        if len(value) != len(set(value)):
            raise ValueError("Discovered song IDs must be unique and in setlist order.")
        return value

    @field_validator("host", mode="before")
    @classmethod
    def empty_host_is_unset(cls, value: object) -> object:
        return value.strip() or None if isinstance(value, str) else value


class MidiSettings(BaseModel):
    """Validated runtime settings for the Playback MIDI input."""

    enabled: bool = False
    transport: MidiTransport = MidiTransport.LOCAL
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


PadColor = Literal["rose", "teal", "amber", "blue", "violet", "green"]
"""Named pad colours. The UI maps each name to a design token; no raw colours are stored."""


def _round_tenths(value: object) -> object:
    """Cue times are kept to 0.1 s (the Live capture and timeline resolution)."""

    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return round(float(value), 1)
    return value


class LightingCue(BaseModel):
    """One elapsed-time MIDI pulse sent to the lighting application."""

    id: UUID = Field(default_factory=uuid4)
    at_seconds: float = Field(ge=0, le=86_399)
    note: int = Field(ge=0, le=127)
    velocity: int = Field(default=127, ge=1, le=127)
    label: str = Field(default="", max_length=120)
    color: PadColor | None = None

    @field_validator("at_seconds", mode="before")
    @classmethod
    def at_seconds_is_tenths(cls, value: object) -> object:
        return _round_tenths(value)

    @field_validator("label", mode="before")
    @classmethod
    def label_is_trimmed(cls, value: object) -> object:
        return value.strip() if isinstance(value, str) else value


class LightingMethod(StrEnum):
    """How Lighting reaches the lighting application. Only the saved method fires."""

    OSC = "osc"
    MIDI = "midi"


class OscArgument(BaseModel):
    """One typed OSC argument. The type is explicit so 1.0 stays a float and 1 an int."""

    type: Literal["int", "float", "string", "bool"]
    value: int | float | str | bool

    @model_validator(mode="after")
    def value_matches_type(self) -> OscArgument:
        value = self.value
        if self.type == "bool":
            ok = isinstance(value, bool)
        elif self.type == "int":
            ok = isinstance(value, int) and not isinstance(value, bool)
            ok = ok and -(2**31) <= int(value) < 2**31
        elif self.type == "float":
            ok = isinstance(value, (int, float)) and not isinstance(value, bool)
            if ok:
                self.value = float(value)
        else:
            ok = isinstance(value, str) and len(value) <= 512
        if not ok:
            raise ValueError(f"OSC argument value does not match type {self.type}.")
        return self


def validate_osc_address(value: str) -> str:
    """OSC address rule shared by cues and test sends: leading slash, no whitespace."""

    if not value.startswith("/"):
        raise ValueError("An OSC address must start with /.")
    if any(character.isspace() or ord(character) < 32 for character in value):
        raise ValueError("An OSC address cannot contain spaces.")
    if len(value) > 256:
        raise ValueError("An OSC address can be at most 256 characters.")
    return value


class OscLightingCue(BaseModel):
    """One elapsed-time one-shot OSC message. OSC has no note length or pulse."""

    id: UUID = Field(default_factory=uuid4)
    at_seconds: float = Field(ge=0, le=86_399)
    address: str = Field(min_length=1, max_length=256)
    args: list[OscArgument] = Field(default_factory=list, max_length=16)
    label: str = Field(default="", max_length=120)
    color: PadColor | None = None

    @field_validator("at_seconds", mode="before")
    @classmethod
    def at_seconds_is_tenths(cls, value: object) -> object:
        return _round_tenths(value)

    @field_validator("address", mode="before")
    @classmethod
    def address_is_trimmed(cls, value: object) -> object:
        return value.strip() if isinstance(value, str) else value

    @field_validator("address")
    @classmethod
    def address_is_valid(cls, value: str) -> str:
        return validate_osc_address(value)

    @field_validator("label", mode="before")
    @classmethod
    def label_is_trimmed(cls, value: object) -> object:
        return value.strip() if isinstance(value, str) else value


class LightingPad(BaseModel):
    """A saved, reusable cue on the cue board. Global (not per song).

    A pad holds the payload for one method. A pad whose method is not the saved method is
    kept but disabled in the UI, like the per-song cues of the inactive method.
    """

    id: UUID = Field(default_factory=uuid4)
    label: str = Field(min_length=1, max_length=60)
    color: PadColor | None = None
    method: LightingMethod
    address: str | None = Field(default=None, max_length=256)
    args: list[OscArgument] = Field(default_factory=list, max_length=16)
    note: int | None = Field(default=None, ge=0, le=127)
    velocity: int = Field(default=127, ge=1, le=127)

    @field_validator("label", "address", mode="before")
    @classmethod
    def text_is_trimmed(cls, value: object) -> object:
        return value.strip() if isinstance(value, str) else value

    @model_validator(mode="after")
    def payload_matches_method(self) -> LightingPad:
        if self.method is LightingMethod.OSC:
            if self.address is None:
                raise ValueError("An OSC pad needs an OSC address.")
            validate_osc_address(self.address)
            self.note = None
        else:
            if self.note is None:
                raise ValueError("A MIDI pad needs a MIDI note.")
            self.address = None
            self.args = []
        return self


class SongLightingCueMap(BaseModel):
    """Persistent lighting timeline for one stable Planning Center song.

    ``cues`` are the MIDI cues (unchanged, so every existing cue map keeps working) and
    ``osc_cues`` the OSC cues. Both lists are kept; only the saved method's list fires.
    """

    song_key: str = Field(min_length=1, max_length=256)
    song_title: str = Field(min_length=1, max_length=500)
    cues: list[LightingCue] = Field(default_factory=list, max_length=2_000)
    osc_cues: list[OscLightingCue] = Field(default_factory=list, max_length=2_000)

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
        for cues in (self.cues, self.osc_cues):
            cue_ids = [cue.id for cue in cues]
            if len(cue_ids) != len(set(cue_ids)):
                raise ValueError("Every lighting cue must have a unique ID.")
        self.cues.sort(key=lambda cue: (cue.at_seconds, str(cue.id)))
        self.osc_cues.sort(key=lambda cue: (cue.at_seconds, str(cue.id)))
        return self


DEFAULT_LIGHTKEY_API_PORT = 8770


class LightkeySettings(BaseModel):
    """Saved Lightkey link settings (part of ``LightsSettings``)."""

    # Empty means "derive from the Lighting OSC host": http://<osc_host>:8770.
    api_url: str = Field(default="", max_length=300)
    # Replay is on by default and only ever suspended by this switch (or by recording).
    replay: bool = True

    @field_validator("api_url", mode="before")
    @classmethod
    def api_url_is_clean(cls, value: object) -> object:
        if not isinstance(value, str):
            return value
        text = value.strip().rstrip("/")
        if not text:
            return ""
        if not text.startswith(("http://", "https://")):
            text = f"http://{text}"
        rest = text.split("://", 1)[1]
        if not rest or any(character.isspace() for character in text) or "/" in rest:
            raise ValueError(
                "Enter the Lightkey API address as host or host:port, e.g. 10.30.0.211:8770."
            )
        return text


class LightsSettings(BaseModel):
    """Validated lighting output (OSC or MIDI) and per-song cue timelines for Lighting."""

    enabled: bool = False
    method: LightingMethod = LightingMethod.OSC
    transport: MidiTransport = MidiTransport.LOCAL
    output_name: str | None = Field(default=None, max_length=512)
    channel: int = Field(default=1, ge=1, le=15)
    pulse_ms: int = Field(default=100, ge=10, le=2_000)
    osc_host: str = Field(default="127.0.0.1", min_length=1, max_length=253)
    osc_port: int = Field(default=21_600, ge=1, le=65_535)
    cue_maps: dict[str, SongLightingCueMap] = Field(default_factory=dict)
    pads: list[LightingPad] = Field(default_factory=list, max_length=64)
    lightkey: LightkeySettings = Field(default_factory=LightkeySettings)

    @model_validator(mode="before")
    @classmethod
    def existing_lighting_settings_stay_midi(cls, data: object) -> object:
        """Settings saved before OSC existed have no method and keep sending MIDI.

        A fresh install (no saved lighting block) gets OSC.
        """

        if not isinstance(data, dict) or "method" in data:
            return data
        if data:  # a saved lighting block from before OSC existed
            return {**data, "method": LightingMethod.MIDI}
        return data

    @field_validator("output_name", mode="before")
    @classmethod
    def empty_output_name_is_unset(cls, value: object) -> object:
        if isinstance(value, str):
            stripped = value.strip()
            return stripped or None
        return value

    @field_validator("osc_host", mode="before")
    @classmethod
    def osc_host_is_trimmed(cls, value: object) -> object:
        if isinstance(value, str):
            stripped = value.strip()
            if not stripped or any(character.isspace() for character in stripped):
                raise ValueError("Enter an OSC host name or IP address without spaces.")
            return stripped
        return value

    @model_validator(mode="after")
    def cue_map_keys_match_song_keys(self) -> LightsSettings:
        for key, cue_map in self.cue_maps.items():
            if key != cue_map.song_key:
                raise ValueError("Every lighting cue-map key must match its song key.")
        pad_ids = [pad.id for pad in self.pads]
        if len(pad_ids) != len(set(pad_ids)):
            raise ValueError("Every lighting pad must have a unique ID.")
        return self


class NetworkMidiSettings(BaseModel):
    """Private IPC settings shared by server-side network MIDI adapters."""

    socket_path: str = Field(
        default="/run/stagepilot-midi/midi.sock",
        min_length=1,
        max_length=1024,
    )
    request_timeout_seconds: float = Field(default=2.0, gt=0, le=30.0)

    @field_validator("socket_path", mode="before")
    @classmethod
    def socket_path_is_trimmed(cls, value: object) -> object:
        if isinstance(value, str):
            stripped = value.strip()
            if not stripped:
                raise ValueError("Network MIDI socket path cannot be empty.")
            return stripped
        return value


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
    # "manual" is the existing Personal Access Token (App ID + Secret) path
    # and stays the default so existing installations are unaffected.
    # "oauth" selects the Bearer-token path fed by planning_center_oauth.
    connection_method: Literal["oauth", "manual"] = "manual"
    access_token: SecretStr | None = None
    service_type_id: str | None = Field(default=None, min_length=1)
    plan_title_preference: str | None = Field(default=None, max_length=255)
    preferred_service_time: str | None = Field(default=None, pattern=r"^([01]\d|2[0-3]):[0-5]\d$")
    upcoming_lookahead_days: int = Field(default=30, ge=0, le=365)
    request_timeout_seconds: float = Field(default=10.0, ge=1.0, le=60.0)
    user_agent: str = Field(
        default="StagePilot/2.42.0 (https://github.com/tage-ilot/stagepilot)",
        min_length=1,
        max_length=256,
    )

    @field_validator("app_id", "secret", "access_token", mode="before")
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

        if self.connection_method == "oauth":
            return self.access_token is not None
        return self.app_id is not None and self.secret is not None

    def credentials(self) -> tuple[str, str]:
        """Return credentials only to trusted backend integrations."""

        if self.app_id is None or self.secret is None:
            msg = "Planning Center credentials are not configured."
            raise ValueError(msg)
        return self.app_id.get_secret_value(), self.secret.get_secret_value()

    def bearer_token(self) -> str:
        """Return the OAuth access token for the Bearer auth path."""

        if self.connection_method != "oauth" or self.access_token is None:
            msg = "Planning Center credentials are not configured."
            raise ValueError(msg)
        return self.access_token.get_secret_value()


class Settings(BaseModel):
    """Validated runtime settings; integration secrets remain server-side only."""

    app_name: str = "StagePilot"
    version: str = "2.42.0"
    bind_host: str = "127.0.0.1"
    bind_port: int = Field(default=8765, ge=1, le=65535)
    log_level: str = "INFO"
    integration_modes: IntegrationModes = Field(default_factory=IntegrationModes)
    demo_mode: bool | None = Field(default=None, exclude=True, repr=False)
    demo: DemoSettings = Field(default_factory=DemoSettings, exclude=True, repr=False)
    timezone: str = "America/Los_Angeles"
    planning_center: PlanningCenterSettings = Field(default_factory=PlanningCenterSettings)
    midi: MidiSettings = Field(default_factory=MidiSettings)
    playback_api: PlaybackApiSettings = Field(default_factory=PlaybackApiSettings)
    lights: LightsSettings = Field(default_factory=LightsSettings)
    network_midi: NetworkMidiSettings = Field(default_factory=NetworkMidiSettings)
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
