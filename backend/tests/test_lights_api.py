from __future__ import annotations

from dataclasses import dataclass, field

from fastapi.testclient import TestClient

from stagepilot.core.config import LightingMethod, LightsSettings, Settings
from stagepilot.main import create_app


@dataclass
class FakeOutputPort:
    closed: bool = False
    messages: list[tuple[str, int, int, int]] = field(default_factory=list)

    def send_note_on(self, channel: int, note: int, velocity: int) -> None:
        self.messages.append(("note_on", channel, note, velocity))

    def send_note_off(self, channel: int, note: int) -> None:
        self.messages.append(("note_off", channel, note, 0))

    def close(self) -> None:
        self.closed = True


@dataclass
class FakeOutputBackend:
    ports: list[FakeOutputPort] = field(default_factory=list)

    def list_output_names(self) -> list[str]:
        return ["StagePilot to Lightkey"]

    def open_output(self, name: str) -> FakeOutputPort:
        assert name == "StagePilot to Lightkey"
        port = FakeOutputPort()
        self.ports.append(port)
        return port


def test_lights_settings_test_pulse_and_cue_map_persist_through_api() -> None:
    backend = FakeOutputBackend()
    app = create_app(
        Settings(lights=LightsSettings(method=LightingMethod.MIDI)),
        lights_backend_factory=lambda: backend,
    )

    with TestClient(app) as client:
        initial = client.get("/api/v1/lights")
        configured = client.post(
            "/api/v1/lights/settings",
            json={
                "enabled": True,
                "output_name": "StagePilot to Lightkey",
                "channel": 4,
                "pulse_ms": 10,
            },
        )
        tested = client.post("/api/v1/lights/test", json={"note": 76, "velocity": 115})
        saved = client.put(
            "/api/v1/lights/cue-map",
            json={
                "song_key": "song-1",
                "song_title": "Holy Forever",
                "cues": [
                    {
                        "id": "c17d19ab-1447-4e73-898e-468b2dfa87c7",
                        "at_seconds": 65,
                        "note": 77,
                        "velocity": 120,
                        "label": "First chorus",
                    }
                ],
            },
        )
        settings = client.get("/api/v1/settings")

    assert initial.status_code == 200
    assert initial.json()["outputs"][0]["name"] == "StagePilot to Lightkey"
    assert configured.status_code == 200
    assert configured.json()["accepted"] is True
    assert configured.json()["lights"]["connection_status"] == "connected"
    assert tested.status_code == 200
    assert tested.json()["accepted"] is True
    assert backend.ports[0].messages == [
        ("note_on", 4, 76, 115),
        ("note_off", 4, 76, 0),
    ]
    assert saved.status_code == 200
    cue_map = settings.json()["settings"]["lights"]["cue_maps"]["song-1"]
    assert cue_map["song_title"] == "Holy Forever"
    assert cue_map["cues"][0]["at_seconds"] == 65


def test_osc_cue_map_and_test_cue_through_api_keep_midi_cues() -> None:
    app = create_app(Settings(lights=LightsSettings(method=LightingMethod.MIDI)))
    midi_cue = {"at_seconds": 5, "note": 60, "velocity": 100, "label": "m"}
    osc_cue = {
        "at_seconds": 9,
        "address": "/cue/2",
        "args": [{"type": "int", "value": 3}],
        "label": "o",
    }
    with TestClient(app) as client:
        client.put(
            "/api/v1/lights/cue-map",
            json={"song_key": "s", "song_title": "S", "method": "midi", "cues": [midi_cue]},
        )
        saved = client.put(
            "/api/v1/lights/cue-map",
            json={"song_key": "s", "song_title": "S", "method": "osc", "osc_cues": [osc_cue]},
        )
        bad = client.put(
            "/api/v1/lights/cue-map",
            json={
                "song_key": "s",
                "song_title": "S",
                "method": "osc",
                "osc_cues": [{**osc_cue, "address": "bad address"}],
            },
        )
        stored = client.get("/api/v1/settings").json()["settings"]["lights"]["cue_maps"]["s"]
        switched = client.post(
            "/api/v1/lights/settings",
            json={"enabled": True, "method": "osc", "osc_host": "127.0.0.1", "osc_port": 21600},
        )
        bad_test = client.post("/api/v1/lights/test-osc", json={"address": "nope"})
        legacy = client.post("/api/v1/lights/settings", json={"enabled": False, "output_name": "x"})

    assert saved.status_code == 200
    assert bad.status_code == 422
    assert len(stored["cues"]) == 1  # MIDI cue kept
    assert stored["osc_cues"][0]["address"] == "/cue/2"
    assert switched.json()["lights"]["method"] == "osc"
    assert bad_test.status_code == 422
    assert legacy.json()["lights"]["method"] == "midi"  # a pre-OSC client body means MIDI
