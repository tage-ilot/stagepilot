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


def _osc_pad(label: str = "Chorus wash", **extra: object) -> dict[str, object]:
    return {
        "id": "5b1f9a04-3a39-4c6c-9d1c-6a4f2c6a9e01",
        "label": label,
        "color": "teal",
        "method": "osc",
        "address": "/cue/7/go",
        "args": [{"type": "int", "value": 1}],
        **extra,
    }


def test_pads_persist_and_survive_settings_and_cue_map_saves() -> None:
    app = create_app(Settings(lights=LightsSettings(method=LightingMethod.OSC)))
    midi_pad = {
        "id": "9f0a5c5e-0f55-4a87-a0a1-2f0d3f9bb111",
        "label": "Blackout",
        "method": "midi",
        "note": 12,
        "velocity": 100,
    }
    with TestClient(app) as client:
        saved = client.put("/api/v1/lights/pads", json={"pads": [_osc_pad(), midi_pad]})
        client.post(
            "/api/v1/lights/settings",
            json={"enabled": True, "method": "osc", "osc_host": "127.0.0.1", "osc_port": 21600},
        )
        client.put(
            "/api/v1/lights/cue-map",
            json={"song_key": "s", "song_title": "S", "method": "osc", "osc_cues": []},
        )
        pads = client.get("/api/v1/settings").json()["settings"]["lights"]["pads"]
        duplicate = client.put("/api/v1/lights/pads", json={"pads": [_osc_pad(), _osc_pad("B")]})
        bad = client.put("/api/v1/lights/pads", json={"pads": [_osc_pad(address="nope")]})
        missing_note = client.put(
            "/api/v1/lights/pads", json={"pads": [{"label": "x", "method": "midi"}]}
        )
        cleared = client.put("/api/v1/lights/pads", json={"pads": []})
        after = client.get("/api/v1/settings").json()["settings"]["lights"]["pads"]

    assert saved.status_code == 200
    assert [pad["label"] for pad in pads] == ["Chorus wash", "Blackout"]  # order kept
    assert pads[1]["note"] == 12 and pads[1]["address"] is None
    assert duplicate.status_code == 422
    assert bad.status_code == 422
    assert missing_note.status_code == 422
    assert cleared.status_code == 200 and after == []


def test_capture_fires_the_pad_and_adds_a_cue_at_the_rounded_time_and_undoes() -> None:
    app = create_app(Settings(lights=LightsSettings(method=LightingMethod.OSC)))
    with TestClient(app) as client:
        client.post(
            "/api/v1/lights/settings",
            json={"enabled": True, "method": "osc", "osc_host": "127.0.0.1", "osc_port": 9},
        )
        client.put("/api/v1/lights/pads", json={"pads": [_osc_pad()]})
        body = {"pad_id": _osc_pad()["id"], "song_key": "s", "song_title": "S"}
        first = client.post("/api/v1/lights/capture", json={**body, "at_seconds": 72.04})
        second = client.post("/api/v1/lights/capture", json={**body, "at_seconds": 12.3})
        preview = client.post("/api/v1/lights/capture", json={"pad_id": body["pad_id"]})
        partial = client.post("/api/v1/lights/capture", json={**body})
        unknown = client.post(
            "/api/v1/lights/capture", json={"pad_id": "00000000-0000-4000-8000-000000000000"}
        )
        cues = client.get("/api/v1/settings").json()["settings"]["lights"]["cue_maps"]["s"][
            "osc_cues"
        ]
        undo = client.post(
            "/api/v1/lights/capture/undo",
            json={"song_key": "s", "cue_id": second.json()["cue_id"]},
        )
        undo_again = client.post(
            "/api/v1/lights/capture/undo",
            json={"song_key": "s", "cue_id": second.json()["cue_id"]},
        )
        remaining = client.get("/api/v1/settings").json()["settings"]["lights"]["cue_maps"]["s"][
            "osc_cues"
        ]

    assert first.json()["at_seconds"] == 72.0
    assert [cue["at_seconds"] for cue in cues] == [12.3, 72.0]  # sorted
    assert cues[0]["label"] == "Chorus wash" and cues[0]["color"] == "teal"
    assert cues[0]["address"] == "/cue/7/go"
    assert preview.status_code == 200 and preview.json()["cue_id"] is None
    assert len(cues) == 2  # preview added nothing
    assert partial.status_code == 422
    assert unknown.status_code == 404
    assert undo.json()["accepted"] is True
    assert undo_again.json()["accepted"] is False
    assert [cue["at_seconds"] for cue in remaining] == [72.0]


def test_capture_refuses_a_pad_for_the_inactive_method_and_keeps_the_song() -> None:
    app = create_app(Settings(lights=LightsSettings(method=LightingMethod.MIDI)))
    with TestClient(app) as client:
        client.put("/api/v1/lights/pads", json={"pads": [_osc_pad()]})
        response = client.post(
            "/api/v1/lights/capture",
            json={
                "pad_id": _osc_pad()["id"],
                "song_key": "s",
                "song_title": "S",
                "at_seconds": 5,
            },
        )
        cue_maps = client.get("/api/v1/settings").json()["settings"]["lights"]["cue_maps"]

    assert response.status_code == 409
    assert cue_maps == {}


def test_fractional_cue_times_round_trip_and_old_integer_times_load() -> None:
    app = create_app(Settings(lights=LightsSettings(method=LightingMethod.OSC)))
    with TestClient(app) as client:
        client.put(
            "/api/v1/lights/cue-map",
            json={
                "song_key": "s",
                "song_title": "S",
                "method": "osc",
                "osc_cues": [
                    {"at_seconds": 3.26, "address": "/a"},
                    {"at_seconds": 4, "address": "/b"},
                ],
            },
        )
        cues = client.get("/api/v1/settings").json()["settings"]["lights"]["cue_maps"]["s"][
            "osc_cues"
        ]
    assert [cue["at_seconds"] for cue in cues] == [3.3, 4.0]
