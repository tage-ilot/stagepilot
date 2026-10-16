"""MIDI input discovery and connection through the real HTTP routes, with no app restart."""

from __future__ import annotations

from typing import Any

from fastapi import FastAPI
from fastapi.testclient import TestClient

from stagepilot.core.config import (
    IntegrationModes,
    MidiSettings,
    MidiSource,
    ServiceSource,
    Settings,
)
from stagepilot.core.events import EventType, TimerPayload, new_event
from stagepilot.core.settings import (
    MemoryCredentialStore,
    MemorySettingsStore,
    PersistentSettings,
    SettingsService,
)
from stagepilot.main import create_app
from test_midi_selection_api import (
    BACKUP_INPUT,
    CONFIGURED_INPUT,
    FakeMidiBackend,
    FakeMidiBackendFactory,
    get_inputs,
    input_id,
    wait_for_snapshot,
    wait_until,
)


def _app(
    backend: FakeMidiBackend,
    *,
    input_name: str | None = None,
) -> FastAPI:
    settings = Settings(
        integration_modes=IntegrationModes(
            service_source=ServiceSource.DEMO, midi_source=MidiSource.REAL
        ),
        midi=MidiSettings(enabled=True, input_name=input_name, debounce_ms=0),
    )
    service = SettingsService(
        MemorySettingsStore(PersistentSettings.from_runtime(settings)),
        MemoryCredentialStore(None),
        environ=environ or {},
    )
    return create_app(
        settings_service=service,
        midi_backend_factory=FakeMidiBackendFactory(backend),
        dashboard_auth_enforced=False,
        oauth_credential_store=MemoryCredentialStore(None),
    )


def _put(client: TestClient, mutate: Any) -> Any:
    current = client.get("/api/v1/settings").json()["settings"]
    mutate(current)
    return client.put("/api/v1/settings", json=current)


def _run_timer(client: TestClient, app: Any) -> None:
    assert client.post("/api/v1/actions/start_next").status_code == 200
    portal = client.portal
    assert portal is not None
    portal.call(
        app.state.runtime.event_bus.publish,
        new_event(
            EventType.TIMER_STARTED, source="test", payload=TimerPayload(duration_seconds=120)
        ),
    )
    assert client.get("/api/v1/state").json()["timer"]["status"] == "running"


def test_choose_input_in_settings_connects_then_switches_without_restart() -> None:
    backend = FakeMidiBackend([CONFIGURED_INPUT, BACKUP_INPUT])
    app = _app(backend)
    with TestClient(app) as client:
        assert backend.opened_names == []
        response = _put(client, lambda s: s["midi"].update(input_name=CONFIGURED_INPUT))
        assert response.status_code == 200
        assert response.json()["restart_required"] is False
        wait_for_snapshot(client, selected_name=CONFIGURED_INPUT, connected_name=CONFIGURED_INPUT)

        response = _put(client, lambda s: s["midi"].update(input_name=BACKUP_INPUT))
        assert response.status_code == 200
        assert response.json()["restart_required"] is False
        wait_for_snapshot(client, selected_name=BACKUP_INPUT, connected_name=BACKUP_INPUT)
        assert backend.opened_names == [CONFIGURED_INPUT, BACKUP_INPUT]
        assert backend.ports[0].closed


def test_refresh_picks_up_device_added_after_startup_and_removal_disconnects() -> None:
    backend = FakeMidiBackend([])
    app = _app(backend, input_name=BACKUP_INPUT)
    with TestClient(app) as client:
        assert [v.name for v in get_inputs(client).inputs] == []
        backend.set_names([BACKUP_INPUT])
        refreshed = client.post("/api/v1/midi/inputs/refresh").json()
        assert [v["name"] for v in refreshed["inputs"]] == [BACKUP_INPUT]
        # The saved input appears later and is connected with no restart.
        wait_for_snapshot(client, selected_name=BACKUP_INPUT, connected_name=BACKUP_INPUT)
        backend.set_names([])
        wait_until(lambda: not any(v.connected for v in get_inputs(client).inputs))
        wait_until(lambda: client.get("/api/v1/state").json()["midi_status"] == "disconnected")
        # It reconnects on its own when the device returns (existing supervisor behaviour).
        backend.set_names([BACKUP_INPUT])
        wait_for_snapshot(client, selected_name=BACKUP_INPUT, connected_name=BACKUP_INPUT)


def test_availability_change_is_live() -> None:
    backend = FakeMidiBackend([CONFIGURED_INPUT])
    app = _app(backend, input_name=CONFIGURED_INPUT)
    with TestClient(app) as client:
        wait_for_snapshot(client, selected_name=CONFIGURED_INPUT, connected_name=CONFIGURED_INPUT)
        response = _put(client, lambda s: s["integration_modes"].update(midi_source="simulated"))
        assert response.status_code == 200
        assert response.json()["restart_required"] is False
        wait_until(lambda: backend.ports[0].closed)
        response = _put(client, lambda s: s["integration_modes"].update(midi_source="real"))
        assert response.status_code == 200
        assert response.json()["restart_required"] is False
        wait_until(lambda: len(backend.ports) == 2)
        wait_for_snapshot(client, selected_name=CONFIGURED_INPUT, connected_name=CONFIGURED_INPUT)


def test_timer_running_refuses_input_change_but_allows_pure_tweak() -> None:
    backend = FakeMidiBackend([CONFIGURED_INPUT, BACKUP_INPUT])
    app = _app(backend, input_name=CONFIGURED_INPUT)
    with TestClient(app) as client:
        wait_for_snapshot(client, selected_name=CONFIGURED_INPUT, connected_name=CONFIGURED_INPUT)
        _run_timer(client, app)
        response = _put(client, lambda s: s["midi"].update(input_name=BACKUP_INPUT))
        assert response.status_code == 409
        assert response.json()["detail"] == "Stop the timer before changing the MIDI input."
        selection = client.post(
            "/api/v1/midi/input-selection", json={"input_id": input_id(BACKUP_INPUT)}
        )
        assert selection.status_code == 409
        assert selection.json()["detail"] == "Stop the timer before changing the MIDI input."
        assert not backend.ports[0].closed
        tweak = _put(client, lambda s: s["midi"].update(note=60))
        assert tweak.status_code == 200
        assert tweak.json()["restart_required"] is False


def test_environment_pin_reports_variable_name() -> None:
    backend = FakeMidiBackend([CONFIGURED_INPUT])
    app = _app(backend, input_name=CONFIGURED_INPUT, environ={"STAGEPILOT_MIDI_SOURCE": "real"})
    with TestClient(app) as client:
        response = _put(client, lambda s: s["integration_modes"].update(midi_source="simulated"))
        assert response.status_code == 409
        assert response.json()["detail"] == (
            "This is set by the STAGEPILOT_MIDI_SOURCE environment variable "
            "and cannot be changed here."
        )


def test_old_settings_file_still_loads() -> None:
    old = PersistentSettings.model_validate({"midi": {"input_name": "Old Device"}})
    assert old.midi.input_name == "Old Device"
