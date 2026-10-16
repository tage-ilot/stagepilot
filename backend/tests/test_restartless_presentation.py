"""Presentation (ProPresenter) connect / switch through the real HTTP routes, no app restart."""

from __future__ import annotations

from typing import Any

from fastapi import FastAPI
from fastapi.testclient import TestClient

from stagepilot.core.config import IntegrationModes, ProPresenterSettings, ServiceSource, Settings
from stagepilot.core.events import EventType, TimerPayload, new_event
from stagepilot.core.settings import (
    MemoryCredentialStore,
    MemorySettingsStore,
    PersistentSettings,
    SettingsService,
)
from stagepilot.main import create_app
from test_propresenter_api import FakeClient, RecordingFactory


def _app(factory: RecordingFactory, **kwargs):  # type: ignore[no-untyped-def]
    return create_app(
        Settings(integration_modes=IntegrationModes(service_source=ServiceSource.DEMO)),
        propresenter_client_factory=factory,
        **kwargs,
    )


def _put_output(client: TestClient, output: str, *, enabled: bool = True):  # type: ignore[no-untyped-def]
    current = client.get("/api/v1/settings").json()["settings"]
    current["integration_modes"]["timer_output"] = output
    current["propresenter"]["enabled"] = enabled
    current["propresenter"]["health_check_interval_seconds"] = 300
    return client.put("/api/v1/settings", json=current)


def _run_timer(client: TestClient, app) -> None:  # type: ignore[no-untyped-def]
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


def test_simulated_to_propresenter_and_back_without_restart() -> None:
    factory = RecordingFactory([FakeClient(), FakeClient()])
    app = _app(factory)
    runtime = app.state.runtime
    with TestClient(app) as client:
        assert runtime.propresenter_controller is None
        assert not app.state.runtime.plugin_manager.has("propresenter")
        # The simulation owns the timer: it published a stopped timer at startup.
        before = client.get("/api/v1/propresenter").json()
        assert before["enabled"] is False

        response = _put_output(client, "propresenter")
        assert response.status_code == 200
        assert response.json()["restart_required"] is False
        assert runtime.propresenter_controller is not None
        assert runtime.plugin_manager.has("propresenter")
        state = client.get("/api/v1/state").json()
        assert state["plugins"]["propresenter"]["status"] == "running"
        assert state["propresenter_status"] == "connected"
        assert state["timer"]["status"] == "idle"
        assert client.get("/api/v1/propresenter").json()["enabled"] is True

        # Saving settings now reconfigures in place and says nothing about restarting.
        saved = client.post(
            "/api/v1/propresenter/settings",
            json={
                "host": "10.0.0.5",
                "port": 1025,
                "timer_name": "Song Countdown",
                "look_id": None,
                "request_timeout_seconds": 3,
            },
        )
        assert saved.status_code == 200
        assert "restart" not in saved.json()["message"].lower()

        back = _put_output(client, "simulated", enabled=False)
        assert back.status_code == 200
        assert back.json()["restart_required"] is False
        assert runtime.propresenter_controller is None
        assert not runtime.plugin_manager.has("propresenter")
        state = client.get("/api/v1/state").json()
        assert "propresenter" not in state["plugins"]
        assert "demo" in state["plugins"]
        assert client.get("/api/v1/propresenter").json()["enabled"] is False


def test_simulated_path_is_restored_after_switching_back() -> None:
    factory = RecordingFactory([FakeClient()])
    app = _app(factory)
    with TestClient(app) as client:
        _put_output(client, "propresenter")
        # Real output: starting a song does not start the simulated timer by itself.
        _put_output(client, "simulated", enabled=False)
        client.post("/api/v1/actions/start_next")
        assert client.get("/api/v1/state").json()["timer"]["status"] == "running"


def test_failed_start_leaves_nothing_registered_and_reports_error() -> None:
    class Boom(Exception):
        pass

    def broken(_settings: ProPresenterSettings):  # type: ignore[no-untyped-def]
        raise Boom("host unreachable")

    app = create_app(
        Settings(integration_modes=IntegrationModes(service_source=ServiceSource.DEMO)),
        propresenter_client_factory=broken,
    )
    runtime = app.state.runtime
    with TestClient(app) as client:
        response = _put_output(client, "propresenter")
        assert response.status_code == 200
        assert "host unreachable" in (response.json()["warning"] or "")
        assert runtime.propresenter_controller is None
        assert not runtime.plugin_manager.has("propresenter")
        state = client.get("/api/v1/state").json()
        assert "propresenter" not in state["plugins"]


def test_switch_refused_while_timer_running() -> None:
    factory = RecordingFactory([FakeClient()])
    app = _app(factory)
    runtime = app.state.runtime
    with TestClient(app) as client:
        client.post("/api/v1/actions/start_next")
        assert client.get("/api/v1/state").json()["timer"]["status"] == "running"
        response = _put_output(client, "propresenter")
        assert response.status_code == 409
        assert response.json()["detail"] == (
            "Stop the timer before switching Presentation between simulated and ProPresenter."
        )
        assert runtime.propresenter_controller is None
        assert not runtime.plugin_manager.has("propresenter")


def test_switch_back_refused_while_timer_running() -> None:
    factory = RecordingFactory([FakeClient()])
    app = _app(factory)
    runtime = app.state.runtime
    with TestClient(app) as client:
        assert _put_output(client, "propresenter").status_code == 200
        _run_timer(client, app)
        response = _put_output(client, "simulated", enabled=False)
        assert response.status_code == 409
        assert runtime.propresenter_controller is not None


def test_idle_edit_on_running_plugin_is_allowed_while_timer_runs() -> None:
    factory = RecordingFactory([FakeClient(), FakeClient()])
    app = _app(factory)
    with TestClient(app) as client:
        assert _put_output(client, "propresenter").status_code == 200
        _run_timer(client, app)
        saved = client.post(
            "/api/v1/propresenter/settings",
            json={
                "host": "127.0.0.1",
                "port": 1025,
                "timer_name": "Song Countdown",
                "look_id": None,
                "request_timeout_seconds": 4,
            },
        )
        assert saved.status_code == 200


def test_environment_pin_blocks_switch_with_plain_message() -> None:
    service = SettingsService(
        MemorySettingsStore(PersistentSettings()),
        MemoryCredentialStore(None),
        environ={"STAGEPILOT_TIMER_OUTPUT": "simulated"},
    )
    app = create_app(
        settings_service=service,
        propresenter_client_factory=RecordingFactory([FakeClient()]),
        dashboard_auth_enforced=False,
        oauth_credential_store=MemoryCredentialStore(None),
    )
    with TestClient(app) as client:
        response = _put_output(client, "propresenter")
        assert response.status_code == 409
        assert response.json()["detail"] == (
            "This is set by the STAGEPILOT_TIMER_OUTPUT environment variable "
            "and cannot be changed here."
        )
        assert app.state.runtime.propresenter_controller is None


def test_startup_activation_holds_no_stale_reference() -> None:
    factory = RecordingFactory([FakeClient(), FakeClient()])
    app = _app(factory)
    runtime = app.state.runtime
    with TestClient(app) as client:
        activation = _activation(app)
        assert activation._propresenter is None
        _put_output(client, "propresenter")
        assert activation._propresenter is runtime.propresenter_controller
        assert activation._propresenter is not None
        _put_output(client, "simulated", enabled=False)
        assert activation._propresenter is None


def _activation(app: FastAPI) -> Any:
    # The service is captured by the publish callback closure.
    callback = app.state.runtime.presentation._on_changed
    for cell in callback.__closure__ or ():
        if type(cell.cell_contents).__name__ == "StartupActivationService":
            return cell.cell_contents
    raise AssertionError("startup activation not found")


def test_old_settings_file_still_loads() -> None:
    old = PersistentSettings.model_validate({"propresenter": {"host": "1.2.3.4"}})
    assert old.propresenter.host == "1.2.3.4"
