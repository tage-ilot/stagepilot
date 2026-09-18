from __future__ import annotations

import json
from pathlib import Path

import pytest
from fastapi.testclient import TestClient
from keyring.errors import KeyringError
from pydantic import SecretStr, ValidationError

from stagepilot.core.config import (
    IntegrationModes,
    MidiSource,
    ServiceSource,
    TimerOutput,
)
from stagepilot.core.settings import (
    CredentialStoreError,
    KeyringCredentialStore,
    PersistentPlanningCenterSettings,
    PersistentSettings,
    SettingsFileError,
    SettingsFileStore,
    SettingsService,
    default_settings_path,
)
from stagepilot.main import create_app
from stagepilot.services.dashboard_auth import hash_dashboard_pin, verify_dashboard_pin


class RecordingCredentialStore:
    def __init__(self, secret: str | None = None) -> None:
        self.secret = secret
        self.saved: list[str] = []
        self.removals = 0

    def get_secret(self) -> str | None:
        return self.secret

    def set_secret(self, secret: str) -> None:
        self.secret = secret
        self.saved.append(secret)

    def remove_secret(self) -> None:
        self.secret = None
        self.removals += 1


class RejectingCredentialStore(RecordingCredentialStore):
    def set_secret(self, secret: str) -> None:
        raise CredentialStoreError("The secure credential could not be saved.")


def settings_service(
    path: Path,
    credentials: RecordingCredentialStore | None = None,
    *,
    environ: dict[str, str] | None = None,
) -> SettingsService:
    return SettingsService(
        SettingsFileStore(path),
        credentials or RecordingCredentialStore(),
        environ=environ or {},
    )


def test_windows_settings_path_uses_roaming_appdata(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr("stagepilot.core.settings.sys.platform", "win32")
    path = default_settings_path({"APPDATA": str(tmp_path)})

    assert path == tmp_path / "StagePilot" / "settings.json"


def test_linux_settings_path_uses_xdg_config_home(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr("stagepilot.core.settings.sys.platform", "linux")
    xdg_config = tmp_path / "xdg"

    path = default_settings_path(
        {"APPDATA": str(tmp_path / "ignored-appdata"), "XDG_CONFIG_HOME": str(xdg_config)}
    )

    assert path == xdg_config / "StagePilot" / "settings.json"


def test_built_in_defaults_load_without_a_saved_file(tmp_path: Path) -> None:
    service = settings_service(tmp_path / "settings.json")

    settings = service.load()

    assert settings.timezone == "America/Los_Angeles"
    assert settings.bind_port == 8765
    assert settings.integration_modes == IntegrationModes()
    assert service.snapshot().onboarding.general_completed is False
    assert settings.midi.note == 112
    assert service.snapshot().web_dashboard_pin_enabled is True
    assert service.snapshot().beta_updates_enabled is False
    assert verify_dashboard_pin("1234", service.snapshot().web_dashboard_pin_hash)
    assert set(dict(settings.midi.mappings.configured()).values()) == {
        100,
        101,
        102,
        103,
        104,
        105,
    }


def test_beta_updates_toggle_survives_a_new_service_instance(tmp_path: Path) -> None:
    path = tmp_path / "settings.json"
    first = settings_service(path)
    first.load()
    assert first.snapshot().beta_updates_enabled is False

    first.save(first.snapshot().model_copy(update={"beta_updates_enabled": True}))

    second = settings_service(path)
    second.load()
    assert second.snapshot().beta_updates_enabled is True
    assert second.effective_snapshot().beta_updates_enabled is True


def test_dashboard_pin_is_hashed_and_survives_restart(tmp_path: Path) -> None:
    path = tmp_path / "settings.json"
    first = settings_service(path)
    first.load()
    first.update_dashboard_access(enabled=True, pin_hash=hash_dashboard_pin("8642"))

    saved_text = path.read_text(encoding="utf-8")
    assert "8642" not in saved_text

    second = settings_service(path)
    second.load()
    saved = second.snapshot()
    assert saved.web_dashboard_pin_enabled is True
    assert verify_dashboard_pin("8642", saved.web_dashboard_pin_hash)
    assert not verify_dashboard_pin("1234", saved.web_dashboard_pin_hash)


def test_saved_settings_survive_a_new_service_instance(tmp_path: Path) -> None:
    path = tmp_path / "settings.json"
    credentials = RecordingCredentialStore("saved-secret")
    first = settings_service(path, credentials)
    saved = PersistentSettings(
        onboarding={"general_completed": True},
        integration_modes=IntegrationModes(
            service_source=ServiceSource.PLANNING_CENTER,
            midi_source=MidiSource.REAL,
            timer_output=TimerOutput.PROPRESENTER,
        ),
        timezone="America/New_York",
        server_port=9876,
        planning_center=PersistentPlanningCenterSettings(
            app_id="saved-app-id",
            service_type_id="42",
            plan_title_preference="Sunday Morning",
            preferred_service_time="09:00",
        ),
        midi={"channel": 3, "input_name": "Playback iPad"},
        propresenter={"host": "192.168.1.50", "port": 1026, "timer_name": "Worship"},
    )
    first.save(saved)

    restarted_service = settings_service(path, credentials)
    restarted = restarted_service.load()

    assert restarted.timezone == "America/New_York"
    assert restarted.bind_port == 9876
    assert restarted.integration_modes == saved.integration_modes
    assert restarted.midi.enabled is True
    assert restarted.midi.input_name == "Playback iPad"
    assert restarted.midi.channel == 3
    assert restarted.propresenter.enabled is True
    assert restarted.propresenter.host == "192.168.1.50"
    assert restarted.planning_center.credentials() == ("saved-app-id", "saved-secret")
    assert restarted.planning_center.plan_title_preference == "Sunday Morning"
    assert restarted.planning_center.preferred_service_time == "09:00"
    assert restarted_service.snapshot().onboarding.general_completed is True


def test_environment_and_session_values_override_saved_settings(tmp_path: Path) -> None:
    path = tmp_path / "settings.json"
    SettingsFileStore(path).save(
        PersistentSettings(
            timezone="America/New_York",
            server_port=9000,
            midi={"channel": 2},
            planning_center=PersistentPlanningCenterSettings(app_id="saved-app-id"),
        )
    )
    service = settings_service(
        path,
        RecordingCredentialStore("saved-secret"),
        environ={
            "STAGEPILOT_TIMEZONE": "America/Chicago",
            "STAGEPILOT_PORT": "9100",
            "STAGEPILOT_MIDI_CHANNEL": "4",
            "STAGEPILOT_PCO_SECRET": "environment-secret",
        },
    )

    settings = service.load(
        session_overrides={"timezone": "America/Los_Angeles", "midi": {"channel": 6}}
    )

    assert settings.timezone == "America/Los_Angeles"
    assert settings.bind_port == 9100
    assert settings.midi.channel == 6
    assert settings.planning_center.credentials() == (
        "saved-app-id",
        "environment-secret",
    )


def test_invalid_and_corrupt_files_fail_safely_without_being_overwritten(
    tmp_path: Path,
) -> None:
    path = tmp_path / "settings.json"
    corrupt = "{ definitely-not-json"
    path.write_text(corrupt, encoding="utf-8")
    service = settings_service(path)

    settings = service.load()

    assert settings.timezone == "America/Los_Angeles"
    assert service.warning == (
        "Saved settings are corrupt or invalid; built-in defaults were used."
    )
    assert path.read_text(encoding="utf-8") == corrupt
    with pytest.raises(ValidationError):
        PersistentSettings(server_port=70000)


def test_atomic_write_failure_preserves_the_previous_file(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    path = tmp_path / "settings.json"
    store = SettingsFileStore(path)
    store.save(PersistentSettings(timezone="America/New_York"))
    original = path.read_text(encoding="utf-8")

    def fail_replace(_source: Path, _destination: Path) -> None:
        raise OSError("simulated replace failure")

    monkeypatch.setattr("stagepilot.core.settings.os.replace", fail_replace)

    with pytest.raises(SettingsFileError, match="atomically"):
        store.save(PersistentSettings(timezone="America/Chicago"))

    assert path.read_text(encoding="utf-8") == original
    assert list(tmp_path.glob("*.tmp")) == []


def test_planning_center_secret_is_replaced_removed_and_never_written_to_json(
    tmp_path: Path,
) -> None:
    path = tmp_path / "settings.json"
    credentials = RecordingCredentialStore()
    service = settings_service(path, credentials)
    service.load()
    public = PersistentPlanningCenterSettings(
        app_id="visible-app-id",
        service_type_id="42",
    )

    service.update_planning_center(public, secret=SecretStr("first-private-secret"))
    service.update_planning_center(public, secret=SecretStr("replacement-secret"))

    file_text = path.read_text(encoding="utf-8")
    assert "visible-app-id" in file_text
    assert "first-private-secret" not in file_text
    assert "replacement-secret" not in file_text
    assert credentials.secret == "replacement-secret"
    assert credentials.saved == ["first-private-secret", "replacement-secret"]
    assert service.credential_saved is True

    service.update_planning_center(public, remove_secret=True)

    assert credentials.secret is None
    assert credentials.removals == 1
    assert service.credential_saved is False


def test_credential_backend_errors_do_not_expose_the_secret(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    private_secret = "never-include-this-value"

    def reject_secret(_service: str, _account: str, secret: str) -> None:
        raise KeyringError(f"backend rejected {secret}")

    monkeypatch.setattr("stagepilot.core.settings.keyring.set_password", reject_secret)

    with pytest.raises(CredentialStoreError) as captured:
        KeyringCredentialStore().set_secret(private_secret)

    assert private_secret not in str(captured.value)
    assert captured.value.__cause__ is None


def test_credential_failure_restores_the_previous_public_settings(tmp_path: Path) -> None:
    path = tmp_path / "settings.json"
    credentials = RejectingCredentialStore("working-secret")
    service = settings_service(path, credentials)
    original = PersistentSettings(
        planning_center=PersistentPlanningCenterSettings(app_id="working-app-id")
    )
    service.save(original)
    service.load()

    with pytest.raises(CredentialStoreError):
        service.update_planning_center(
            PersistentPlanningCenterSettings(app_id="replacement-app-id"),
            secret=SecretStr("rejected-secret"),
        )

    assert service.snapshot().planning_center.app_id == "working-app-id"
    saved = PersistentSettings.model_validate_json(path.read_text(encoding="utf-8"))
    assert saved.planning_center.app_id == "working-app-id"


def test_settings_api_never_returns_the_planning_center_secret(tmp_path: Path) -> None:
    path = tmp_path / "settings.json"
    credentials = RecordingCredentialStore()
    service = settings_service(path, credentials)
    runtime_settings = service.load()
    app = create_app(runtime_settings, settings_service=service)

    with TestClient(app) as client:
        saved = client.post(
            "/api/v1/planning-center/settings",
            json={
                "app_id": "visible-app-id",
                "service_type_id": "42",
                "secret": "api-private-secret",
            },
        )
        fetched = client.get("/api/v1/settings")
        status = client.get("/api/v1/planning-center/status")
        removed = client.post(
            "/api/v1/planning-center/settings",
            json={
                "app_id": "visible-app-id",
                "service_type_id": "42",
                "remove_secret": True,
            },
        )

    assert [saved.status_code, fetched.status_code, status.status_code] == [200, 200, 200]
    public_text = "\n".join((saved.text, fetched.text, status.text))
    assert "api-private-secret" not in public_text
    assert 'secret"' not in public_text
    assert saved.json()["planning_center_secret_saved"] is True
    assert fetched.json()["settings"]["planning_center"]["app_id"] == "visible-app-id"
    assert status.json()["planning_center_secret_saved"] is True
    assert removed.status_code == 200
    assert removed.json()["planning_center_secret_saved"] is False
    assert credentials.secret is None
    assert json.loads(path.read_text(encoding="utf-8"))["planning_center"] == {
        "app_id": "visible-app-id",
        "service_type_id": "42",
        "plan_title_preference": None,
        "preferred_service_time": None,
        "upcoming_lookahead_days": 30,
        "request_timeout_seconds": 10.0,
    }


def test_settings_put_is_validated_and_survives_subsequent_get(tmp_path: Path) -> None:
    path = tmp_path / "settings.json"
    service = settings_service(path)
    runtime_settings = service.load()
    app = create_app(runtime_settings, settings_service=service)
    updated = PersistentSettings(
        onboarding={"general_completed": True},
        timezone="America/Denver",
        server_port=9001,
    )

    with TestClient(app) as client:
        saved = client.put("/api/v1/settings", json=updated.model_dump(mode="json"))
        fetched = client.get("/api/v1/settings")
        invalid = client.put(
            "/api/v1/settings",
            json={**updated.model_dump(mode="json"), "server_port": 70000},
        )

    assert saved.status_code == 200
    assert saved.json()["restart_required"] is True
    assert fetched.status_code == 200
    assert fetched.json()["settings"]["timezone"] == "America/Denver"
    assert fetched.json()["settings"]["server_port"] == 9001
    assert fetched.json()["settings"]["onboarding"]["general_completed"] is True
    assert invalid.status_code == 422


def test_stale_settings_put_cannot_clear_completed_general_onboarding(tmp_path: Path) -> None:
    path = tmp_path / "settings.json"
    service = settings_service(path)
    runtime_settings = service.load()
    app = create_app(runtime_settings, settings_service=service)
    completed = PersistentSettings(onboarding={"general_completed": True})
    stale = PersistentSettings(
        onboarding={"general_completed": False},
        timezone="America/Denver",
    )

    with TestClient(app) as client:
        first = client.put("/api/v1/settings", json=completed.model_dump(mode="json"))
        second = client.put("/api/v1/settings", json=stale.model_dump(mode="json"))
        fetched = client.get("/api/v1/settings")

    assert first.status_code == 200
    assert second.status_code == 200
    assert fetched.json()["settings"]["timezone"] == "America/Denver"
    assert fetched.json()["settings"]["onboarding"]["general_completed"] is True
    assert json.loads(path.read_text(encoding="utf-8"))["onboarding"] == {"general_completed": True}
