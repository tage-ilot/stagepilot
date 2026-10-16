"""Persistent, validated settings and secure Planning Center credential storage."""

from __future__ import annotations

import json
import logging
import os
import sys
from collections.abc import Callable, Mapping
from contextlib import suppress
from datetime import datetime
from pathlib import Path
from typing import Literal, Protocol, cast
from uuid import uuid4
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

import keyring
from keyring.errors import KeyringError, PasswordDeleteError
from pydantic import BaseModel, ConfigDict, Field, SecretStr, field_validator, model_validator

from stagepilot.core.config import (
    IntegrationModes,
    LightsSettings,
    MidiSettings,
    MidiSource,
    NetworkMidiSettings,
    PlanningCenterSettings,
    PlaybackApiSettings,
    ProPresenterSettings,
    ServiceSource,
    Settings,
    TimerOutput,
)

SETTINGS_SCHEMA_VERSION: Literal[2] = 2
KEYRING_SERVICE = "StagePilot"
KEYRING_ACCOUNT = "planning-center-secret"
# The OAuth token blob (access token, refresh token, expiry, reconnect
# marker) lives under a second account in the SAME keyring service, so the
# existing PAT secret is untouched and both auth methods can coexist for an
# installation that switches between them.
KEYRING_ACCOUNT_OAUTH = "planning-center-oauth"


class SettingsFileError(RuntimeError):
    """A saved settings file could not be safely read or written."""


class CredentialStoreError(RuntimeError):
    """The OS credential backend could not complete a safe credential operation."""


class PersistentPlanningCenterSettings(BaseModel):
    """Non-secret Planning Center settings safe to write to settings.json."""

    model_config = ConfigDict(extra="forbid")

    app_id: str | None = Field(default=None, max_length=255)
    # Additive, defaults to the existing manual PAT flow so saved settings
    # written before OAuth existed keep validating and behaving identically.
    connection_method: Literal["oauth", "manual"] = "manual"
    service_type_id: str | None = Field(default=None, max_length=128)
    plan_title_preference: str | None = Field(default=None, max_length=255)
    preferred_service_time: str | None = Field(
        default=None,
        pattern=r"^([01]\d|2[0-3]):[0-5]\d$",
    )
    upcoming_lookahead_days: int = Field(default=30, ge=0, le=365)
    request_timeout_seconds: float = Field(default=10.0, ge=1.0, le=60.0)

    @field_validator(
        "app_id",
        "service_type_id",
        "plan_title_preference",
        "preferred_service_time",
        mode="before",
    )
    @classmethod
    def empty_values_are_unset(cls, value: object) -> object:
        if isinstance(value, str):
            stripped = value.strip()
            return stripped or None
        return value


class OnboardingSettings(BaseModel):
    """Small persisted markers that cannot be inferred from validated defaults."""

    model_config = ConfigDict(extra="forbid")

    general_completed: bool = False


class PlanningCenterLengthUndo(BaseModel):
    """Write-ahead undo entry; never contains credentials."""

    service_type_id: str
    plan_id: str
    item_id: str
    title: str
    old_length: int
    new_length: int
    written_at: datetime
    pending_from_length: int | None = None


class PersistentSettings(BaseModel):
    """Versioned ordinary settings persisted outside the repository."""

    model_config = ConfigDict(extra="forbid")

    planning_center_length_undo: list[PlanningCenterLengthUndo] = Field(default_factory=list)
    schema_version: Literal[2] = SETTINGS_SCHEMA_VERSION
    onboarding: OnboardingSettings = Field(default_factory=OnboardingSettings)
    integration_modes: IntegrationModes = Field(default_factory=IntegrationModes)
    timezone: str = "America/Los_Angeles"
    log_level: Literal["DEBUG", "INFO", "WARNING", "ERROR", "CRITICAL"] = "INFO"
    server_port: int = Field(default=8765, ge=1, le=65535)
    lan_access: bool = False
    web_dashboard_pin_enabled: bool = True
    web_dashboard_pin_hash: str | None = Field(default=None, exclude=True)
    release_channel: Literal["STABLE", "BETA"] = "BETA"
    planning_center: PersistentPlanningCenterSettings = Field(
        default_factory=PersistentPlanningCenterSettings
    )
    midi: MidiSettings = Field(default_factory=MidiSettings)
    playback_api: PlaybackApiSettings = Field(default_factory=PlaybackApiSettings)
    network_midi: NetworkMidiSettings = Field(default_factory=NetworkMidiSettings)
    lights: LightsSettings = Field(default_factory=LightsSettings)
    propresenter: ProPresenterSettings = Field(default_factory=ProPresenterSettings)

    @model_validator(mode="before")
    @classmethod
    def migrate_v1(cls, value: object) -> object:
        """Move untouched legacy installs to Playback; keep explicit MIDI choices."""
        if not isinstance(value, dict):
            return value
        version = value.get("schema_version", SETTINGS_SCHEMA_VERSION)
        if type(version) is int and version == 1:
            migrated = dict(value)
            modes = value.get("integration_modes", {})
            if not isinstance(modes, dict):
                return value
            midi = value.get("midi", {})
            chose_midi = (
                modes.get("midi_source") == MidiSource.REAL
                and isinstance(midi, dict)
                and bool(midi.get("input_name"))
            )
            # An install the user explicitly set up with a MIDI input keeps MIDI; the
            # UI offers Scan Network to switch. Untouched old defaults move to Playback.
            if not chose_midi:
                migrated["integration_modes"] = {**modes, "midi_source": MidiSource.PLAYBACK_API}
            migrated["schema_version"] = SETTINGS_SCHEMA_VERSION
            logging.getLogger(__name__).warning(
                "settings_migration schema=1->2 chose_midi=%s midi_source=%s",
                chose_midi,
                migrated["integration_modes"].get("midi_source")
                if isinstance(migrated.get("integration_modes"), dict)
                else None,
            )
            return migrated
        return value

    @field_validator("timezone")
    @classmethod
    def timezone_must_be_valid(cls, value: str) -> str:
        try:
            ZoneInfo(value)
        except (ZoneInfoNotFoundError, ValueError) as exc:
            raise ValueError(f"Unknown IANA timezone: {value}") from exc
        return value

    @classmethod
    def from_runtime(cls, settings: Settings) -> PersistentSettings:
        app_id = settings.planning_center.app_id
        return cls(
            integration_modes=settings.integration_modes,
            timezone=settings.timezone,
            log_level=cast(
                Literal["DEBUG", "INFO", "WARNING", "ERROR", "CRITICAL"],
                settings.log_level.upper(),
            ),
            server_port=settings.bind_port,
            lan_access=settings.bind_host == "0.0.0.0",
            planning_center=PersistentPlanningCenterSettings(
                app_id=app_id.get_secret_value() if app_id is not None else None,
                connection_method=settings.planning_center.connection_method,
                service_type_id=settings.planning_center.service_type_id,
                plan_title_preference=settings.planning_center.plan_title_preference,
                preferred_service_time=settings.planning_center.preferred_service_time,
                upcoming_lookahead_days=settings.planning_center.upcoming_lookahead_days,
                request_timeout_seconds=settings.planning_center.request_timeout_seconds,
            ),
            midi=settings.midi.model_copy(deep=True),
            network_midi=settings.network_midi,
            lights=settings.lights,
            playback_api=settings.playback_api,
            propresenter=settings.propresenter.model_copy(
                update={
                    "enabled": (
                        settings.integration_modes.timer_output is TimerOutput.PROPRESENTER
                    ),
                }
            ),
        )

    def to_runtime(self, secret: str | None, access_token: str | None = None) -> Settings:
        planning_center = self.planning_center
        return Settings(
            bind_host="0.0.0.0" if self.lan_access else "127.0.0.1",
            bind_port=self.server_port,
            log_level=self.log_level,
            integration_modes=self.integration_modes,
            timezone=self.timezone,
            planning_center=PlanningCenterSettings(
                app_id=(SecretStr(planning_center.app_id) if planning_center.app_id else None),
                secret=SecretStr(secret) if secret else None,
                connection_method=planning_center.connection_method,
                access_token=SecretStr(access_token) if access_token else None,
                service_type_id=planning_center.service_type_id,
                plan_title_preference=planning_center.plan_title_preference,
                preferred_service_time=planning_center.preferred_service_time,
                upcoming_lookahead_days=planning_center.upcoming_lookahead_days,
                request_timeout_seconds=planning_center.request_timeout_seconds,
            ),
            midi=self.midi.model_copy(
                update={"enabled": self.integration_modes.midi_source is not MidiSource.SIMULATED}
            ),
            lights=self.lights,
            playback_api=self.playback_api,
            network_midi=self.network_midi,
            propresenter=self.propresenter.model_copy(
                update={"enabled": self.integration_modes.timer_output is TimerOutput.PROPRESENTER}
            ),
        )


class SettingsStore(Protocol):
    def load(self) -> PersistentSettings | None: ...

    def save(self, settings: PersistentSettings) -> None: ...


class CredentialStore(Protocol):
    def get_secret(self) -> str | None: ...

    def set_secret(self, secret: str) -> None: ...

    def remove_secret(self) -> None: ...


class SettingsFileStore:
    """Read and atomically replace one validated local settings file."""

    def __init__(self, path: Path) -> None:
        self.path = path

    def load(self) -> PersistentSettings | None:
        if not self.path.exists():
            return None
        try:
            payload = json.loads(self.path.read_text(encoding="utf-8"))
            if isinstance(payload, dict):
                payload.setdefault("schema_version", 1)
            settings = PersistentSettings.model_validate(payload)
            if isinstance(payload, dict) and payload.get("schema_version") == 1:
                self.save(settings)
            return settings
        except (OSError, ValueError, TypeError) as exc:
            raise SettingsFileError(
                "Saved settings are corrupt or invalid; built-in defaults were used."
            ) from exc

    def save(self, settings: PersistentSettings) -> None:
        temporary = self.path.with_name(f".{self.path.name}.{uuid4().hex}.tmp")
        try:
            self.path.parent.mkdir(parents=True, exist_ok=True)
            with temporary.open("x", encoding="utf-8", newline="\n") as handle:
                payload = settings.model_dump(mode="json")
                if settings.web_dashboard_pin_hash is not None:
                    payload["web_dashboard_pin_hash"] = settings.web_dashboard_pin_hash
                handle.write(json.dumps(payload, indent=2))
                handle.write("\n")
                handle.flush()
                os.fsync(handle.fileno())
            os.replace(temporary, self.path)
        except OSError as exc:
            with suppress(OSError):
                temporary.unlink(missing_ok=True)
            raise SettingsFileError("Settings could not be saved atomically.") from exc


class MemorySettingsStore:
    """Non-persistent settings store used by explicitly configured app instances."""

    def __init__(self, settings: PersistentSettings | None = None) -> None:
        self.settings = settings

    def load(self) -> PersistentSettings | None:
        return self.settings.model_copy(deep=True) if self.settings is not None else None

    def save(self, settings: PersistentSettings) -> None:
        self.settings = settings.model_copy(deep=True)


class KeyringCredentialStore:
    """Store the Planning Center PAT secret in the operating-system credential backend."""

    def __init__(self, account: str = KEYRING_ACCOUNT) -> None:
        self._account = account
        # Each Keychain read can raise a macOS password prompt, so read at most once
        # per launch and keep the (in-memory only) result current across our own writes.
        self._cached = False
        self._value: str | None = None

    def get_secret(self) -> str | None:
        if self._cached:
            return self._value
        try:
            self._value = keyring.get_password(KEYRING_SERVICE, self._account)
        except KeyringError:
            raise CredentialStoreError("The secure credential store is unavailable.") from None
        self._cached = True
        return self._value

    def set_secret(self, secret: str) -> None:
        try:
            keyring.set_password(KEYRING_SERVICE, self._account, secret)
        except KeyringError:
            raise CredentialStoreError("The secure credential could not be saved.") from None
        self._cached, self._value = True, secret

    def remove_secret(self) -> None:
        try:
            keyring.delete_password(KEYRING_SERVICE, self._account)
        except PasswordDeleteError:
            self._cached, self._value = True, None
            return
        except KeyringError:
            raise CredentialStoreError("The secure credential could not be removed.") from None
        self._cached, self._value = True, None


def default_oauth_credential_store() -> CredentialStore:
    """The keyring account holding the Planning Center OAuth token blob."""

    return KeyringCredentialStore(KEYRING_ACCOUNT_OAUTH)


class MemoryCredentialStore:
    """In-memory credential backend for tests and explicit application fixtures."""

    def __init__(self, secret: str | None = None) -> None:
        self.secret = secret

    def get_secret(self) -> str | None:
        return self.secret

    def set_secret(self, secret: str) -> None:
        self.secret = secret

    def remove_secret(self) -> None:
        self.secret = None


def default_settings_path(environ: Mapping[str, str] | None = None) -> Path:
    values = os.environ if environ is None else environ
    override = values.get("STAGEPILOT_SETTINGS_PATH")
    if override:
        return Path(override).expanduser()
    if sys.platform == "win32":
        app_data = values.get("APPDATA")
        base = Path(app_data) if app_data else Path.home() / "AppData" / "Roaming"
    else:
        base = Path(values.get("XDG_CONFIG_HOME", Path.home() / ".config"))
    return base / "StagePilot" / "settings.json"


def _deep_merge(base: dict[str, object], updates: Mapping[str, object]) -> dict[str, object]:
    merged = dict(base)
    for key, value in updates.items():
        existing = merged.get(key)
        if isinstance(existing, dict) and isinstance(value, Mapping):
            merged[key] = _deep_merge(existing, value)
        else:
            merged[key] = value
    return merged


def _optional(values: Mapping[str, str], name: str) -> str | None:
    value = values.get(name)
    if value is None:
        return None
    return value.strip() or None


def _boolean(value: str) -> bool:
    return value.casefold() in {"1", "true", "yes", "on"}


def _environment_overrides(values: Mapping[str, str]) -> dict[str, object]:
    overrides: dict[str, object] = {}

    def assign(section: str, field: str, value: object) -> None:
        target = overrides.setdefault(section, {})
        assert isinstance(target, dict)
        target[field] = value

    scalar_fields: tuple[tuple[str, str, object], ...] = (
        ("STAGEPILOT_HOST", "bind_host", str),
        ("STAGEPILOT_PORT", "bind_port", int),
        ("STAGEPILOT_LOG_LEVEL", "log_level", lambda value: value.upper()),
        ("STAGEPILOT_TIMEZONE", "timezone", str),
    )
    for environment_name, field, converter in scalar_fields:
        if environment_name in values:
            assert callable(converter)
            overrides[field] = converter(values[environment_name])

    planning_fields: tuple[tuple[str, str, object], ...] = (
        ("STAGEPILOT_PCO_APP_ID", "app_id", _optional),
        ("STAGEPILOT_PCO_SECRET", "secret", _optional),
        ("STAGEPILOT_PCO_SERVICE_TYPE_ID", "service_type_id", _optional),
        ("STAGEPILOT_PCO_PLAN_TITLE", "plan_title_preference", _optional),
        ("STAGEPILOT_PCO_SERVICE_TIME", "preferred_service_time", _optional),
        ("STAGEPILOT_PCO_LOOKAHEAD_DAYS", "upcoming_lookahead_days", int),
        ("STAGEPILOT_PCO_TIMEOUT_SECONDS", "request_timeout_seconds", float),
        ("STAGEPILOT_PCO_USER_AGENT", "user_agent", str),
    )
    for environment_name, field, converter in planning_fields:
        if environment_name not in values:
            continue
        value = (
            converter(values, environment_name)
            if converter is _optional
            else converter(values[environment_name])  # type: ignore[operator]
        )
        assign("planning_center", field, value)

    midi_fields: tuple[tuple[str, str, object], ...] = (
        ("STAGEPILOT_MIDI_INPUT_NAME", "input_name", _optional),
        ("STAGEPILOT_MIDI_CHANNEL", "channel", int),
        ("STAGEPILOT_MIDI_NOTE", "note", int),
        ("STAGEPILOT_MIDI_DEBOUNCE_MS", "debounce_ms", int),
    )
    for environment_name, field, converter in midi_fields:
        if environment_name not in values:
            continue
        value = (
            converter(values, environment_name)
            if converter is _optional
            else converter(values[environment_name])  # type: ignore[operator]
        )
        assign("midi", field, value)

    lights_fields: tuple[tuple[str, str, object], ...] = (
        ("STAGEPILOT_LIGHTS_OUTPUT_NAME", "output_name", _optional),
        ("STAGEPILOT_LIGHTS_CHANNEL", "channel", int),
        ("STAGEPILOT_LIGHTS_PULSE_MS", "pulse_ms", int),
        ("STAGEPILOT_LIGHTS_METHOD", "method", lambda value: value.lower()),
        ("STAGEPILOT_LIGHTS_OSC_HOST", "osc_host", str),
        ("STAGEPILOT_LIGHTS_OSC_PORT", "osc_port", int),
    )
    for environment_name, field, converter in lights_fields:
        if environment_name not in values:
            continue
        value = (
            converter(values, environment_name)
            if converter is _optional
            else converter(values[environment_name])  # type: ignore[operator]
        )
        assign("lights", field, value)
    if "STAGEPILOT_LIGHTS_ENABLED" in values:
        assign("lights", "enabled", _boolean(values["STAGEPILOT_LIGHTS_ENABLED"]))

    mapping_names = {
        "STAGEPILOT_MIDI_START_NEXT_VELOCITY": "start_next",
        "STAGEPILOT_MIDI_RESTART_CURRENT_VELOCITY": "restart_current",
        "STAGEPILOT_MIDI_PREVIOUS_VELOCITY": "previous",
        "STAGEPILOT_MIDI_NEXT_VELOCITY": "next",
        "STAGEPILOT_MIDI_RELOAD_PLAN_VELOCITY": "reload_plan",
        "STAGEPILOT_MIDI_STOP_TIMER_VELOCITY": "stop_timer",
    }
    for environment_name, field in mapping_names.items():
        if environment_name in values:
            midi_overrides = overrides.setdefault("midi", {})
            assert isinstance(midi_overrides, dict)
            mappings = midi_overrides.setdefault("mappings", {})
            assert isinstance(mappings, dict)
            mappings[field] = int(values[environment_name])

    propresenter_fields: tuple[tuple[str, str, object], ...] = (
        ("STAGEPILOT_PROPRESENTER_HOST", "host", str),
        ("STAGEPILOT_PROPRESENTER_PORT", "port", int),
        ("STAGEPILOT_PROPRESENTER_TIMER_NAME", "timer_name", str),
        ("STAGEPILOT_PROPRESENTER_LOOK_ID", "look_id", str),
        ("STAGEPILOT_PROPRESENTER_TIMEOUT_SECONDS", "request_timeout_seconds", float),
        ("STAGEPILOT_PROPRESENTER_RECONNECT_INITIAL_SECONDS", "reconnect_initial_seconds", float),
        ("STAGEPILOT_PROPRESENTER_RECONNECT_MAX_SECONDS", "reconnect_max_seconds", float),
        ("STAGEPILOT_PROPRESENTER_HEALTH_CHECK_SECONDS", "health_check_interval_seconds", float),
    )
    for environment_name, field, converter in propresenter_fields:
        if environment_name in values:
            assign("propresenter", field, converter(values[environment_name]))  # type: ignore[operator]

    modes: dict[str, object] = {}
    if "STAGEPILOT_DEMO_MODE" in values:
        modes["service_source"] = (
            ServiceSource.DEMO
            if _boolean(values["STAGEPILOT_DEMO_MODE"])
            else ServiceSource.PLANNING_CENTER
        )
    if "STAGEPILOT_DEMO_SIMULATE_MIDI" in values:
        modes["midi_source"] = (
            MidiSource.SIMULATED
            if _boolean(values["STAGEPILOT_DEMO_SIMULATE_MIDI"])
            else MidiSource.REAL
        )
    if "STAGEPILOT_DEMO_SIMULATE_PROPRESENTER" in values:
        modes["timer_output"] = (
            TimerOutput.SIMULATED
            if _boolean(values["STAGEPILOT_DEMO_SIMULATE_PROPRESENTER"])
            else TimerOutput.PROPRESENTER
        )
    if "STAGEPILOT_MIDI_ENABLED" in values:
        enabled = _boolean(values["STAGEPILOT_MIDI_ENABLED"])
        assign("midi", "enabled", enabled)
        modes.setdefault("midi_source", MidiSource.REAL if enabled else MidiSource.SIMULATED)
    if "STAGEPILOT_PROPRESENTER_ENABLED" in values:
        enabled = _boolean(values["STAGEPILOT_PROPRESENTER_ENABLED"])
        assign("propresenter", "enabled", enabled)
        modes.setdefault(
            "timer_output",
            TimerOutput.PROPRESENTER if enabled else TimerOutput.SIMULATED,
        )
    if "STAGEPILOT_SERVICE_SOURCE" in values:
        modes["service_source"] = values["STAGEPILOT_SERVICE_SOURCE"]
    if "STAGEPILOT_MIDI_SOURCE" in values:
        modes["midi_source"] = values["STAGEPILOT_MIDI_SOURCE"]
        assign("midi", "enabled", values["STAGEPILOT_MIDI_SOURCE"] == MidiSource.REAL)
    if "STAGEPILOT_TIMER_OUTPUT" in values:
        modes["timer_output"] = values["STAGEPILOT_TIMER_OUTPUT"]
        assign(
            "propresenter",
            "enabled",
            values["STAGEPILOT_TIMER_OUTPUT"] == TimerOutput.PROPRESENTER,
        )
    if modes:
        overrides["integration_modes"] = modes
    return overrides


class SettingsService:
    """Resolve settings precedence and coordinate safe persistence operations."""

    def __init__(
        self,
        store: SettingsStore,
        credentials: CredentialStore,
        *,
        environ: Mapping[str, str] | None = None,
        access_token_provider: Callable[[], str | None] | None = None,
    ) -> None:
        self._store = store
        self._credentials = credentials
        self._environ = dict(os.environ if environ is None else environ)
        # Supplies the current OAuth access token when
        # `planning_center.connection_method == "oauth"`. Kept as a callable
        # so the token blob stays owned by the OAuth service (and its
        # keyring account) instead of being copied into this service.
        self._access_token_provider = access_token_provider
        self._persistent = PersistentSettings()
        self._runtime: Settings | None = None
        self._secret: str | None = None
        self.warning: str | None = None
        self.credential_saved = False

    def service_source_pin(self) -> str | None:
        """Name of the environment variable pinning the Services source, if any."""

        for name in ("STAGEPILOT_SERVICE_SOURCE", "STAGEPILOT_DEMO_MODE"):
            if name in self._environ:
                return name
        return None

    def midi_pin(self, *, source: bool, enabled: bool) -> str | None:
        """Name of the environment variable pinning the MIDI source or availability, if any."""

        names = ["STAGEPILOT_MIDI_SOURCE", "STAGEPILOT_DEMO_SIMULATE_MIDI"]
        if enabled:
            names.append("STAGEPILOT_MIDI_ENABLED")
        for name in names:
            if name in self._environ:
                return name
        return None

    def set_access_token_provider(self, provider: Callable[[], str | None] | None) -> None:
        self._access_token_provider = provider
        self._runtime = self._resolve(self._persistent, self._secret)

    def refresh_access_token(self) -> None:
        """Re-resolve the cached runtime settings from the access token provider.

        `effective_runtime_settings()` returns a cached `Settings` snapshot, so
        an OAuth access token refreshed in the background (or just before an
        on-demand request) is not picked up automatically. Callers that just
        forced a token refresh must call this before reading runtime settings
        again, otherwise they observe the stale cached (possibly expired)
        access token.
        """

        self._runtime = self._resolve(self._persistent, self._secret)

    def _access_token(self, settings: PersistentSettings | None = None) -> str | None:
        current = settings or self._persistent
        if current.planning_center.connection_method != "oauth":
            return None
        if self._access_token_provider is None:
            return None
        try:
            return self._access_token_provider()
        except CredentialStoreError:
            return None

    @classmethod
    def default(cls) -> SettingsService:
        return cls(
            SettingsFileStore(default_settings_path()),
            KeyringCredentialStore(),
        )

    @classmethod
    def ephemeral(cls, settings: Settings) -> SettingsService:
        persistent = PersistentSettings.from_runtime(settings)
        secret = settings.planning_center.secret
        service = cls(
            MemorySettingsStore(persistent),
            MemoryCredentialStore(secret.get_secret_value() if secret is not None else None),
            environ={},
        )
        service._persistent = persistent
        service._runtime = settings
        service._secret = secret.get_secret_value() if secret is not None else None
        service.credential_saved = secret is not None
        return service

    def load(self, *, session_overrides: Mapping[str, object] | None = None) -> Settings:
        warnings: list[str] = []
        try:
            saved = self._store.load()
        except SettingsFileError as exc:
            saved = None
            warnings.append(str(exc))
        self._persistent = saved or PersistentSettings()

        secret: str | None = None
        planning_center = self._persistent.planning_center
        # No Planning Center App ID means nothing usable is configured: do not touch the
        # Keychain at all (every Keychain read can prompt for the login password).
        if planning_center.app_id is not None or planning_center.connection_method == "oauth":
            try:
                secret = self._credentials.get_secret()
            except CredentialStoreError as exc:
                warnings.append(str(exc))
        self._secret = secret
        self.credential_saved = secret is not None

        self._runtime = self._resolve(self._persistent, secret, session_overrides)
        self.warning = " ".join(warnings) or None
        return self._runtime

    def _resolve(
        self,
        settings: PersistentSettings,
        secret: str | None,
        session_overrides: Mapping[str, object] | None = None,
    ) -> Settings:
        base = settings.to_runtime(secret, self._access_token(settings)).model_dump(mode="python")
        merged = _deep_merge(base, _environment_overrides(self._environ))
        if session_overrides:
            merged = _deep_merge(merged, session_overrides)
        return Settings.model_validate(merged)

    def snapshot(self) -> PersistentSettings:
        return self._persistent.model_copy(deep=True)

    def effective_snapshot(self) -> PersistentSettings:
        if self._runtime is None:
            return self.snapshot()
        resolved = PersistentSettings.from_runtime(self._runtime)
        return resolved.model_copy(
            update={
                "planning_center_length_undo": self._persistent.planning_center_length_undo,
                "onboarding": self._persistent.onboarding,
                "web_dashboard_pin_enabled": self._persistent.web_dashboard_pin_enabled,
                "web_dashboard_pin_hash": self._persistent.web_dashboard_pin_hash,
                "midi": resolved.midi.model_copy(update={"enabled": self._persistent.midi.enabled}),
            },
            deep=True,
        )

    def effective_runtime_settings(self) -> Settings:
        """Return an internal runtime copy, including masked secret values."""

        if self._runtime is None:
            self._runtime = self._resolve(self._persistent, self._secret)
        return self._runtime.model_copy(deep=True)

    def save(self, settings: PersistentSettings) -> None:
        if settings.web_dashboard_pin_hash is None:
            settings = settings.model_copy(
                update={"web_dashboard_pin_hash": self._persistent.web_dashboard_pin_hash}
            )
        if self._persistent.onboarding.general_completed:
            settings = settings.model_copy(
                update={"onboarding": self._persistent.onboarding},
                deep=True,
            )
        self._store.save(settings)
        self._persistent = settings.model_copy(deep=True)
        self._runtime = self._resolve(self._persistent, self._secret)

    def update_dashboard_access(self, *, enabled: bool, pin_hash: str | None = None) -> None:
        updated = self._persistent.model_copy(
            update={
                "web_dashboard_pin_enabled": enabled,
                "web_dashboard_pin_hash": (
                    pin_hash if pin_hash is not None else self._persistent.web_dashboard_pin_hash
                ),
            }
        )
        self.save(updated)

    def update_planning_center(
        self,
        settings: PersistentPlanningCenterSettings,
        *,
        secret: SecretStr | None = None,
        remove_secret: bool = False,
    ) -> None:
        updated = self._persistent.model_copy(
            update={"planning_center": settings},
            deep=True,
        )
        self._store.save(updated)
        try:
            if remove_secret:
                self._credentials.remove_secret()
                self._secret = None
                self.credential_saved = False
            elif secret is not None:
                replacement = secret.get_secret_value()
                self._credentials.set_secret(replacement)
                self._secret = replacement
                self.credential_saved = True
        except CredentialStoreError:
            self._store.save(self._persistent)
            raise
        self._persistent = updated
        self._runtime = self._resolve(updated, self._secret)

    def update_connection_method(self, method: Literal["oauth", "manual"]) -> None:
        """Switch between the manual PAT and OAuth paths.

        Additive and non-destructive: any existing PAT secret is left in
        place, so switching back to "manual" restores the previous
        connection without re-entering credentials.
        """

        planning_center = self._persistent.planning_center.model_copy(
            update={"connection_method": method}
        )
        self.save(
            self._persistent.model_copy(update={"planning_center": planning_center}, deep=True)
        )

    def persist_midi_input(self, input_name: str | None) -> None:
        midi = self._persistent.midi.model_copy(update={"input_name": input_name})
        self.save(self._persistent.model_copy(update={"midi": midi}, deep=True))

    def persist_propresenter(self, settings: ProPresenterSettings) -> None:
        self.save(
            self._persistent.model_copy(
                update={"propresenter": settings},
                deep=True,
            )
        )

    def persist_lights(self, settings: LightsSettings) -> None:
        self.save(
            self._persistent.model_copy(
                update={"lights": settings},
                deep=True,
            )
        )


def load_runtime_settings(
    *,
    path: Path | None = None,
    credential_store: CredentialStore | None = None,
    environ: Mapping[str, str] | None = None,
    session_overrides: Mapping[str, object] | None = None,
) -> Settings:
    """Resolve the full v0.5 precedence chain into runtime settings."""

    values = os.environ if environ is None else environ
    service = SettingsService(
        SettingsFileStore(path or default_settings_path(values)),
        credential_store or KeyringCredentialStore(),
        environ=values,
    )
    return service.load(session_overrides=session_overrides)
