"""HTTP surface of the Lightkey link and the saved settings it adds to Lighting."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

import httpx
from fastapi.testclient import TestClient

from stagepilot.core.config import LightingMethod, LightkeySettings, LightsSettings, Settings
from stagepilot.main import create_app


@dataclass
class _Osc:
    closed: bool = False
    sent: list[str] = field(default_factory=list)

    def send_message(self, address: str, _args: Any) -> None:
        self.sent.append(address)

    def close(self) -> None:
        self.closed = True


class _OscBackend:
    def __init__(self) -> None:
        self.output = _Osc()

    def open_output(self, _host: str, _port: int) -> _Osc:
        return self.output


def _fake_agent(request: httpx.Request) -> httpx.Response:
    if request.url.path == "/time":
        return httpx.Response(200, json={"t": 1_000.0})
    return httpx.Response(
        200,
        json={"events": [], "state": {"online": True, "active": ["A", "B"], "seq": 3}},
    )


def _app(lights: LightsSettings | None = None) -> Any:
    from stagepilot.plugins.lights import LightsPlugin  # noqa: F401  (import check)

    backend = _OscBackend()
    app = create_app(
        Settings(lights=lights or LightsSettings(enabled=True, method=LightingMethod.OSC)),
        lights_osc_backend_factory=lambda: backend,
    )
    app.state.test_osc = backend
    return app


def test_defaults_replay_on_and_url_derived_from_the_osc_host() -> None:
    with TestClient(_app(LightsSettings(enabled=True, osc_host="10.30.0.211"))) as client:
        status = client.get("/api/v1/lights/lightkey").json()
    assert status["replay"] is True
    assert status["api_url"] == "http://10.30.0.211:8770"
    assert status["recording"] is False
    assert status["takes"] == []


def test_settings_save_only_what_changed_and_persist() -> None:
    with TestClient(_app()) as client:
        a = client.put("/api/v1/lights/lightkey/settings", json={"replay": False})
        b = client.put("/api/v1/lights/lightkey/settings", json={"api_url": "10.30.0.211:9000"})
        saved = client.get("/api/v1/settings").json()["settings"]["lights"]["lightkey"]
        status = client.get("/api/v1/lights/lightkey").json()
    assert a.status_code == 200 and a.json()["lightkey"]["replay"] is False
    assert b.status_code == 200
    assert saved == {"api_url": "http://10.30.0.211:9000", "replay": False}  # replay kept
    assert status["api_url"] == "http://10.30.0.211:9000"


def test_bad_address_is_rejected_with_422() -> None:
    with TestClient(_app()) as client:
        response = client.put("/api/v1/lights/lightkey/settings", json={"api_url": "http://a/b c"})
    assert response.status_code == 422


def test_recording_refused_until_connected() -> None:
    with TestClient(_app()) as client:
        response = client.post("/api/v1/lights/lightkey/recording", json={"recording": True})
    body = response.json()
    assert response.status_code == 200
    assert body["accepted"] is False
    assert body["lightkey"]["recording"] is False


def test_clearing_a_missing_take_is_reported_not_an_error() -> None:
    with TestClient(_app()) as client:
        response = client.delete("/api/v1/lights/lightkey/takes/nope")
    assert response.status_code == 200
    assert response.json()["accepted"] is False


def test_saving_lighting_settings_keeps_the_lightkey_block() -> None:
    lights = LightsSettings(
        enabled=True, lightkey=LightkeySettings(api_url="10.0.0.5:8770", replay=False)
    )
    with TestClient(_app(lights)) as client:
        client.post(
            "/api/v1/lights/settings",
            json={"enabled": True, "method": "osc", "osc_host": "10.0.0.9", "osc_port": 21600},
        )
        saved = client.get("/api/v1/settings").json()["settings"]["lights"]
    assert saved["lightkey"] == {"api_url": "http://10.0.0.5:8770", "replay": False}
    assert saved["osc_host"] == "10.0.0.9"


def test_old_settings_without_a_lightkey_block_load_with_replay_on() -> None:
    old = {"enabled": True, "method": "osc", "osc_host": "1.2.3.4", "cue_maps": {}}
    loaded = LightsSettings.model_validate(old)
    assert loaded.lightkey == LightkeySettings(api_url="", replay=True)
