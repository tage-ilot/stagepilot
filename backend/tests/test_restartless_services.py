"""Demo <-> Planning Center switching and credential changes through the real HTTP routes."""

from __future__ import annotations

from fastapi.testclient import TestClient

from stagepilot.core.config import (
    IntegrationModes,
    PlanningCenterSettings,
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
from test_planning_center_settings_live_apply import LiveClientFactory

PC_SETTINGS = {"app_id": "app-1", "service_type_id": "svc-1", "secret": "s3cret"}


def _demo_app(factory: LiveClientFactory):  # type: ignore[no-untyped-def]
    return create_app(
        Settings(integration_modes=IntegrationModes(service_source=ServiceSource.DEMO)),
        planning_center_client_factory=factory,
    )


def _switch(client: TestClient, source: str):  # type: ignore[no-untyped-def]
    current = client.get("/api/v1/settings").json()["settings"]
    current["integration_modes"]["service_source"] = source
    return client.put("/api/v1/settings", json=current)


def _run_timer(client: TestClient, app):  # type: ignore[no-untyped-def]
    """Start the timer the way a timer output does: through a TIMER_STARTED event."""

    assert client.post("/api/v1/actions/start_next").status_code == 200
    runtime = app.state.runtime
    portal = client.portal
    assert portal is not None
    portal.call(
        runtime.event_bus.publish,
        new_event(
            EventType.TIMER_STARTED,
            source="test",
            payload=TimerPayload(duration_seconds=120),
        ),
    )
    assert client.get("/api/v1/state").json()["timer"]["status"] == "running"


def test_demo_to_planning_center_and_back_without_restart() -> None:
    factory = LiveClientFactory()
    app = _demo_app(factory)
    runtime = app.state.runtime
    with TestClient(app) as client:
        state = client.get("/api/v1/state").json()
        assert state["plan"]["id"] != "plan-svc-1"
        assert "demo" in state["plugins"] and runtime.planning_center is None

        saved = client.post("/api/v1/planning-center/settings", json=PC_SETTINGS)
        assert saved.status_code == 200
        # Saving credentials alone must not switch the source.
        assert runtime.planning_center is None

        response = _switch(client, "planning_center")
        assert response.status_code == 200
        assert response.json()["restart_required"] is False
        assert runtime.planning_center is not None
        state = client.get("/api/v1/state").json()
        assert state["plan"]["id"] == "plan-svc-1"
        assert "demo" not in state["plugins"]
        assert state["plugins"]["planning_center"]["status"] == "running"
        assert state["planning_center_status"] == "connected"
        first_plugin = runtime.planning_center

        back = _switch(client, "demo")
        assert back.status_code == 200
        assert runtime.planning_center is None
        state = client.get("/api/v1/state").json()
        assert state["plan"]["id"] != "plan-svc-1"
        assert "planning_center" not in state["plugins"]
        assert "demo" in state["plugins"]
        assert state["planning_center_status"] == "disconnected"
        assert factory.built_clients[0].closed

        _switch(client, "planning_center")
        assert runtime.planning_center is not None and runtime.planning_center is not first_plugin


def test_demo_plan_is_gone_when_planning_center_has_none() -> None:
    factory = LiveClientFactory()
    app = _demo_app(factory)
    with TestClient(app) as client:
        # No credentials yet: the switch starts the plugin, which reports a real error.
        response = _switch(client, "planning_center")
        assert response.status_code == 200
        assert response.json()["warning"]
        state = client.get("/api/v1/state").json()
        assert state["plan"] is None
        assert state["plugins"]["planning_center"]["status"] == "error" or (
            "planning_center" not in state["plugins"]
        )


def test_switch_refused_while_timer_running() -> None:
    factory = LiveClientFactory()
    app = _demo_app(factory)
    with TestClient(app) as client:
        client.post("/api/v1/planning-center/settings", json=PC_SETTINGS)
        _run_timer(client, app)
        assert client.get("/api/v1/state").json()["timer"]["status"] == "running"
        response = _switch(client, "planning_center")
        assert response.status_code == 409
        assert response.json()["detail"] == (
            "Stop the timer before switching between Demo and Planning Center."
        )
        assert app.state.runtime.planning_center is None
        assert "demo" in client.get("/api/v1/state").json()["plugins"]


def test_credential_change_rebuilds_client_without_restart() -> None:
    factory = LiveClientFactory()
    settings = Settings(
        integration_modes=IntegrationModes(service_source=ServiceSource.PLANNING_CENTER),
        planning_center=PlanningCenterSettings(app_id="app-1", secret="old", service_type_id="a"),
    )
    app = create_app(settings, planning_center_client_factory=factory)
    with TestClient(app) as client:
        built = len(factory.built_clients)
        response = client.post(
            "/api/v1/planning-center/settings",
            json={"app_id": "app-2", "service_type_id": "a", "secret": "new"},
        )
        assert response.status_code == 200
        assert response.json()["restart_required"] is False
        assert len(factory.built_clients) > built
        assert factory.built_settings[-1].app_id is not None
        assert factory.built_settings[-1].app_id.get_secret_value() == "app-2"
        assert factory.built_settings[-1].secret is not None
        assert factory.built_settings[-1].secret.get_secret_value() == "new"
        assert factory.built_clients[0].closed

        removed = client.post(
            "/api/v1/planning-center/settings",
            json={"app_id": "app-2", "service_type_id": "a", "remove_secret": True},
        )
        assert removed.status_code == 200
        assert removed.json()["restart_required"] is False


def test_credential_change_refused_while_timer_running() -> None:
    factory = LiveClientFactory()
    settings = Settings(
        integration_modes=IntegrationModes(service_source=ServiceSource.PLANNING_CENTER),
        planning_center=PlanningCenterSettings(app_id="app-1", secret="old", service_type_id="a"),
    )
    app = create_app(settings, planning_center_client_factory=factory)
    with TestClient(app) as client:
        _run_timer(client, app)
        response = client.post(
            "/api/v1/planning-center/settings",
            json={"app_id": "app-2", "service_type_id": "a", "secret": "new"},
        )
        assert response.status_code == 409
        assert "Stop the timer" in response.json()["detail"]


def test_environment_pin_blocks_source_switch_with_plain_message() -> None:
    factory = LiveClientFactory()
    service = SettingsService(
        MemorySettingsStore(PersistentSettings()),
        MemoryCredentialStore(None),
        environ={"STAGEPILOT_SERVICE_SOURCE": "demo"},
    )
    app = create_app(
        settings_service=service,
        planning_center_client_factory=factory,
        dashboard_auth_enforced=False,
        oauth_credential_store=MemoryCredentialStore(None),
    )
    with TestClient(app) as client:
        response = _switch(client, "planning_center")
        assert response.status_code == 409
        assert response.json()["detail"] == (
            "This is set by the STAGEPILOT_SERVICE_SOURCE environment variable "
            "and cannot be changed here."
        )
        assert app.state.runtime.planning_center is None


def test_old_settings_file_without_new_fields_still_loads() -> None:
    old = PersistentSettings.model_validate({"planning_center": {"app_id": "x"}})
    assert old.integration_modes.service_source in ServiceSource
