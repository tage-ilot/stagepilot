"""Concurrent connection changes end consistent: live state always equals the final saved state."""

from __future__ import annotations

import threading
from collections.abc import Callable
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
    input_id,
)
from test_planning_center_settings_live_apply import LiveClientFactory
from test_propresenter_api import FakeClient
from test_restartless_presentation import _run_timer as _run_timer_presentation
from test_restartless_services import PC_SETTINGS, _run_timer

ROUNDS = 30


class EndlessFactory:
    """ProPresenter client factory that can be asked for any number of clients."""

    def __call__(self, settings: Any) -> FakeClient:
        return FakeClient()


def _put(client: TestClient, mutate: Callable[[dict[str, Any]], None]) -> Any:
    current = client.get("/api/v1/settings").json()["settings"]
    mutate(current)
    return client.put("/api/v1/settings", json=current)


def _storm(jobs: list[Callable[[], int]]) -> list[int]:
    """Run every job at the same instant from its own thread; return the status codes."""

    barrier = threading.Barrier(len(jobs))
    codes: list[int] = [0] * len(jobs)

    def run(index: int) -> None:
        barrier.wait()
        codes[index] = jobs[index]()

    threads = [threading.Thread(target=run, args=(i,)) for i in range(len(jobs))]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    return codes


def _services_app() -> FastAPI:
    return create_app(
        Settings(integration_modes=IntegrationModes(service_source=ServiceSource.DEMO)),
        planning_center_client_factory=LiveClientFactory(),
    )


def _presentation_app() -> FastAPI:
    return create_app(
        Settings(integration_modes=IntegrationModes(service_source=ServiceSource.DEMO)),
        propresenter_client_factory=EndlessFactory(),
    )


def _services_consistent(client: TestClient) -> None:
    plugins = set(client.get("/api/v1/state").json()["plugins"])
    assert len(plugins & {"demo", "planning_center"}) == 1, plugins
    saved = client.get("/api/v1/settings").json()["settings"]["integration_modes"]["service_source"]
    assert saved == ("planning_center" if "planning_center" in plugins else "demo"), (
        saved,
        plugins,
    )


def _presentation_consistent(client: TestClient) -> None:
    plugins = set(client.get("/api/v1/state").json()["plugins"])
    settings = client.get("/api/v1/settings").json()["settings"]
    wanted = (
        settings["propresenter"]["enabled"]
        and settings["integration_modes"]["timer_output"] == "propresenter"
    )
    assert ("propresenter" in plugins) == wanted, (wanted, plugins)


def _service_job(client: TestClient, target: str) -> Callable[[], int]:
    return lambda: (
        _put(
            client, lambda s: s["integration_modes"].__setitem__("service_source", target)
        ).status_code
    )


def _presentation_job(client: TestClient, on: bool) -> Callable[[], int]:
    def mutate(s: dict[str, Any]) -> None:
        s["integration_modes"]["timer_output"] = "propresenter" if on else "simulated"
        s["propresenter"]["enabled"] = on
        s["propresenter"]["health_check_interval_seconds"] = 300

    return lambda: _put(client, mutate).status_code


def _midi_settings_job(client: TestClient, name: str) -> Callable[[], int]:
    return lambda: _put(client, lambda s: s["midi"].__setitem__("input_name", name)).status_code


def _midi_select_job(client: TestClient, name: str) -> Callable[[], int]:
    return lambda: (
        client.post("/api/v1/midi/input-selection", json={"input_id": input_id(name)}).status_code
    )


def test_concurrent_service_source_saves_stay_consistent() -> None:
    with TestClient(_services_app()) as client:
        client.post("/api/v1/planning-center/settings", json=PC_SETTINGS)
        for _ in range(ROUNDS):
            codes = _storm(
                [
                    _service_job(client, "planning_center" if i % 2 == 0 else "demo")
                    for i in range(6)
                ]
            )
            assert all(code == 200 for code in codes), codes
            _services_consistent(client)


def test_concurrent_presentation_saves_stay_consistent() -> None:
    with TestClient(_presentation_app()) as client:
        for _ in range(ROUNDS):
            codes = _storm([_presentation_job(client, i % 2 == 0) for i in range(6)])
            assert all(code == 200 for code in codes), codes
            _presentation_consistent(client)


def test_mixed_storm_ends_with_live_equal_saved() -> None:
    backend = FakeMidiBackend([CONFIGURED_INPUT, BACKUP_INPUT])
    settings = Settings(
        integration_modes=IntegrationModes(
            service_source=ServiceSource.DEMO, midi_source=MidiSource.REAL
        ),
        midi=MidiSettings(enabled=True, input_name=CONFIGURED_INPUT, debounce_ms=0),
    )
    app = create_app(
        settings_service=SettingsService(
            MemorySettingsStore(PersistentSettings.from_runtime(settings)),
            MemoryCredentialStore(None),
            environ={},
        ),
        planning_center_client_factory=LiveClientFactory(),
        propresenter_client_factory=EndlessFactory(),
        midi_backend_factory=FakeMidiBackendFactory(backend),
        dashboard_auth_enforced=False,
        oauth_credential_store=MemoryCredentialStore(None),
    )
    with TestClient(app) as client:
        client.post("/api/v1/planning-center/settings", json=PC_SETTINGS)
        for _ in range(ROUNDS):
            jobs: list[Callable[[], int]] = []
            for i in range(4):
                jobs.append(_service_job(client, "planning_center" if i % 2 == 0 else "demo"))
                jobs.append(_presentation_job(client, i % 2 == 0))
                name = BACKUP_INPUT if i % 2 == 0 else CONFIGURED_INPUT
                jobs.append(_midi_settings_job(client, name))
                jobs.append(_midi_select_job(client, name))
            codes = _storm(jobs)
            assert all(code in (200, 409) for code in codes), codes
            _services_consistent(client)
            _presentation_consistent(client)
        assert app.state.runtime.plugin_manager.has("demo") != app.state.runtime.plugin_manager.has(
            "planning_center"
        )


def test_timer_running_still_refuses_with_409_under_concurrency() -> None:
    app = _services_app()
    with TestClient(app) as client:
        client.post("/api/v1/planning-center/settings", json=PC_SETTINGS)
        _run_timer(client, app)
        codes = _storm([_service_job(client, "planning_center") for _ in range(4)])
        assert codes == [409] * 4, codes
        assert app.state.runtime.plugin_manager.has("demo")
        _services_consistent(client)


def test_timer_running_still_refuses_presentation_switch() -> None:
    app = _presentation_app()
    with TestClient(app) as client:
        _run_timer_presentation(client, app)
        codes = _storm([_presentation_job(client, True) for _ in range(4)])
        assert codes == [409] * 4, codes
        assert not app.state.runtime.plugin_manager.has("propresenter")
