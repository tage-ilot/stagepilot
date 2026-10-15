from __future__ import annotations

import asyncio
import socket
from collections.abc import Iterator, Sequence
from contextlib import closing, contextmanager
from dataclasses import dataclass, field
from pathlib import Path

import pytest
from pydantic import ValidationError
from pythonosc.osc_message import OscMessage

from stagepilot.core.config import (
    LightingCue,
    LightingMethod,
    LightsSettings,
    OscArgument,
    OscLightingCue,
    OscPreset,
    SongLightingCueMap,
)
from stagepilot.core.event_bus import EventBus
from stagepilot.core.events import EventType, SongPayload, TimerPayload, new_event
from stagepilot.core.settings import (
    MemoryCredentialStore,
    PersistentSettings,
    SettingsFileStore,
    SettingsService,
)
from stagepilot.core.state import StateStore
from stagepilot.models.state import ConnectionStatus, Song
from stagepilot.plugins.lights import LightsPlugin, UdpOscOutputBackend


@contextmanager
def udp_receiver() -> Iterator[socket.socket]:
    with closing(socket.socket(socket.AF_INET, socket.SOCK_DGRAM)) as sock:
        sock.bind(("127.0.0.1", 0))
        sock.settimeout(2.0)
        yield sock


def receive(sock: socket.socket) -> tuple[str, list[object]]:
    data, _ = sock.recvfrom(4096)
    message = OscMessage(data)
    return message.address, list(message.params)


def arg(kind: str, value: object) -> OscArgument:
    return OscArgument.model_validate({"type": kind, "value": value})


# ---- OSC output over a real localhost UDP socket ----


def test_osc_output_sends_exact_address_and_typed_args() -> None:
    with udp_receiver() as sock:
        port = sock.getsockname()[1]
        output = UdpOscOutputBackend().open_output("127.0.0.1", port)
        output.send_message(
            "/cue/1/start",
            [arg("int", 7), arg("float", 0.5), arg("string", "go"), arg("bool", True)],
        )
        output.send_message("/blackout", [])
        output.close()
        assert receive(sock) == ("/cue/1/start", [7, 0.5, "go", True])
        assert receive(sock) == ("/blackout", [])
    assert output.closed


def test_osc_output_float_stays_float_and_int_stays_int() -> None:
    with udp_receiver() as sock:
        output = UdpOscOutputBackend().open_output("127.0.0.1", sock.getsockname()[1])
        output.send_message("/x", [arg("float", 1), arg("int", 1)])
        output.close()
        _, params = receive(sock)
    assert params == [1.0, 1]
    assert isinstance(params[0], float)
    assert isinstance(params[1], int)


def test_osc_output_unknown_host_is_a_clear_error() -> None:
    with pytest.raises(ValueError, match="could not be found"):
        UdpOscOutputBackend().open_output("no-such-host.invalid", 21600)


def test_osc_output_send_after_close_raises() -> None:
    with udp_receiver() as sock:
        output = UdpOscOutputBackend().open_output("127.0.0.1", sock.getsockname()[1])
    output.close()
    with pytest.raises(RuntimeError):
        output.send_message("/x", [])


# ---- validation ----


@pytest.mark.parametrize("address", ["cue/1", "/cue 1", "/cue\t1", "", "/" + "a" * 256])
def test_osc_cue_rejects_bad_addresses(address: str) -> None:
    with pytest.raises(ValidationError):
        OscLightingCue(at_seconds=0, address=address)


def test_osc_cue_accepts_and_trims_valid_address() -> None:
    assert OscLightingCue(at_seconds=3, address="  /lightkey/cue/1  ").address == "/lightkey/cue/1"


@pytest.mark.parametrize(
    ("kind", "value"),
    [
        ("int", 1.5),
        ("int", True),
        ("int", "3"),
        ("int", 2**31),
        ("float", "x"),
        ("float", True),
        ("bool", 1),
        ("string", 4),
        ("bool", "true"),
    ],
)
def test_osc_argument_rejects_mismatched_values(kind: str, value: object) -> None:
    with pytest.raises(ValidationError):
        arg(kind, value)


def test_osc_host_must_not_contain_spaces() -> None:
    with pytest.raises(ValidationError):
        LightsSettings(osc_host="my host")


# ---- settings defaults and migration ----


def test_fresh_install_defaults_to_osc_lightkey() -> None:
    lights = LightsSettings()
    assert lights.method is LightingMethod.OSC
    assert lights.osc_preset is OscPreset.LIGHTKEY
    assert (lights.osc_host, lights.osc_port) == ("127.0.0.1", 21600)
    assert PersistentSettings().lights.method is LightingMethod.OSC


def test_existing_saved_lighting_settings_stay_midi_and_cues_survive(tmp_path: Path) -> None:
    legacy_cue = {
        "id": "11111111-1111-4111-8111-111111111111",
        "at_seconds": 65,
        "note": 72,
        "velocity": 110,
        "label": "First chorus",
    }
    legacy = {
        "enabled": True,
        "output_name": "StagePilot to Lightkey",
        "channel": 4,
        "pulse_ms": 125,
        "cue_maps": {"song-1": {"song_key": "song-1", "song_title": "Holy", "cues": [legacy_cue]}},
    }
    lights = LightsSettings.model_validate(legacy)
    assert lights.method is LightingMethod.MIDI
    cue_map = lights.cue_maps["song-1"]
    # Times now load as tenths (65 -> 65.0) and a cue gains an unset colour; nothing else changes.
    assert [cue.model_dump(mode="json") for cue in cue_map.cues] == [
        {**legacy_cue, "at_seconds": 65.0, "color": None}
    ]
    assert cue_map.osc_cues == []

    # Through the real settings file: old file without "method" loads as MIDI, then round-trips.
    store = SettingsFileStore(tmp_path / "settings.json")
    store.save(PersistentSettings(lights=lights))
    raw = (tmp_path / "settings.json").read_text()
    import json

    data = json.loads(raw)
    del data["lights"]["method"]
    for key in ("osc_preset", "osc_host", "osc_port"):
        data["lights"].pop(key, None)
    for cue_map_data in data["lights"]["cue_maps"].values():
        cue_map_data.pop("osc_cues", None)
    (tmp_path / "settings.json").write_text(json.dumps(data))
    runtime = SettingsService(store, MemoryCredentialStore(), environ={}).load()
    assert runtime.lights.method is LightingMethod.MIDI
    assert runtime.lights.cue_maps["song-1"].cues[0].note == 72


def test_new_method_and_osc_settings_round_trip(tmp_path: Path) -> None:
    store = SettingsFileStore(tmp_path / "settings.json")
    lights = LightsSettings(
        enabled=True,
        method=LightingMethod.OSC,
        osc_preset=OscPreset.QLC_PLUS,
        osc_host="192.168.4.50",
        osc_port=7700,
        cue_maps={
            "s": SongLightingCueMap(
                song_key="s",
                song_title="S",
                osc_cues=[
                    OscLightingCue(at_seconds=1, address="/a", args=[arg("int", 1)], label="A")
                ],
                cues=[LightingCue(at_seconds=2, note=60)],
            )
        },
    )
    store.save(PersistentSettings(lights=lights))
    loaded = SettingsService(store, MemoryCredentialStore(), environ={}).load().lights
    assert loaded.method is LightingMethod.OSC
    assert (loaded.osc_preset, loaded.osc_host, loaded.osc_port) == (
        OscPreset.QLC_PLUS,
        "192.168.4.50",
        7700,
    )
    assert loaded.cue_maps["s"].osc_cues[0].args[0].value == 1
    assert loaded.cue_maps["s"].cues[0].note == 60


# ---- plugin: only the saved method fires ----


@dataclass
class FakeMidiPort:
    closed: bool = False
    messages: list[tuple[str, int, int, int]] = field(default_factory=list)

    def send_note_on(self, channel: int, note: int, velocity: int) -> None:
        self.messages.append(("note_on", channel, note, velocity))

    def send_note_off(self, channel: int, note: int) -> None:
        self.messages.append(("note_off", channel, note, 0))

    def close(self) -> None:
        self.closed = True


@dataclass
class FakeMidiBackend:
    ports: list[FakeMidiPort] = field(default_factory=list)

    def list_output_names(self) -> list[str]:
        return ["Out"]

    def open_output(self, name: str) -> FakeMidiPort:
        port = FakeMidiPort()
        self.ports.append(port)
        return port


@dataclass
class FakeOscOutput:
    closed: bool = False
    sent: list[tuple[str, list[object]]] = field(default_factory=list)

    def send_message(self, address: str, args: Sequence[OscArgument]) -> None:
        self.sent.append((address, [a.value for a in args]))

    def close(self) -> None:
        self.closed = True


@dataclass
class FakeOscBackend:
    outputs: list[FakeOscOutput] = field(default_factory=list)
    opened: list[tuple[str, int]] = field(default_factory=list)

    def open_output(self, host: str, port: int) -> FakeOscOutput:
        self.opened.append((host, port))
        output = FakeOscOutput()
        self.outputs.append(output)
        return output

    @property
    def sent(self) -> list[tuple[str, list[object]]]:
        return [message for output in self.outputs for message in output.sent]


def song() -> Song:
    return Song(
        id="song-1",
        source_song_id="song-1",
        order=1,
        title="Holy Forever",
        duration_seconds=300,
    )


def settings(method: LightingMethod) -> LightsSettings:
    return LightsSettings(
        enabled=True,
        method=method,
        output_name="Out",
        pulse_ms=10,
        osc_port=9000,
        cue_maps={
            "song-1": SongLightingCueMap(
                song_key="song-1",
                song_title="Holy Forever",
                cues=[LightingCue(at_seconds=0, note=72, velocity=100, label="midi")],
                osc_cues=[
                    OscLightingCue(
                        at_seconds=0,
                        address="/cue/1",
                        args=[arg("float", 1.0)],
                        label="osc",
                    )
                ],
            )
        },
    )


def plugin(
    bus: EventBus, midi: FakeMidiBackend, osc: FakeOscBackend, method: LightingMethod
) -> LightsPlugin:
    return LightsPlugin(
        bus,
        StateStore(),
        settings(method),
        backend_factory=lambda: midi,
        osc_backend_factory=lambda: osc,
    )


async def start_song(bus: EventBus) -> None:
    await bus.publish(
        new_event(EventType.SONG_STARTED, source="t", payload=SongPayload(song=song(), index=0))
    )
    await bus.publish(
        new_event(
            EventType.TIMER_STARTED,
            source="t",
            payload=TimerPayload(duration_seconds=300, song=song()),
        )
    )
    await asyncio.sleep(0.05)


@pytest.mark.asyncio
async def test_osc_method_sends_one_shot_osc_and_no_midi() -> None:
    bus, midi, osc = EventBus(), FakeMidiBackend(), FakeOscBackend()
    lights = plugin(bus, midi, osc, LightingMethod.OSC)
    await lights.start()
    await start_song(bus)
    snapshot = await lights.snapshot()
    await lights.stop()

    assert osc.opened == [("127.0.0.1", 9000)]
    assert osc.sent == [("/cue/1", [1.0])]
    assert midi.ports == []  # MIDI cues stay saved but never fire
    assert snapshot.method is LightingMethod.OSC
    assert snapshot.last_osc_cue is not None
    assert snapshot.last_osc_cue.label == "osc"
    assert snapshot.outputs == []


@pytest.mark.asyncio
async def test_midi_method_sends_midi_and_never_osc() -> None:
    bus, midi, osc = EventBus(), FakeMidiBackend(), FakeOscBackend()
    lights = plugin(bus, midi, osc, LightingMethod.MIDI)
    await lights.start()
    await start_song(bus)
    await lights.stop()

    assert midi.ports[0].messages == [("note_on", 1, 72, 100), ("note_off", 1, 72, 0)]
    assert osc.outputs == []


@pytest.mark.asyncio
async def test_switching_method_cancels_timeline_and_new_method_waits_for_next_song() -> None:
    bus, midi, osc = EventBus(), FakeMidiBackend(), FakeOscBackend()
    base = settings(LightingMethod.MIDI)
    late = base.cue_maps["song-1"].model_copy(
        update={
            "cues": [LightingCue(at_seconds=1, note=60)],
            "osc_cues": [OscLightingCue(at_seconds=1, address="/late")],
        }
    )
    lights = LightsPlugin(
        bus,
        StateStore(),
        base.model_copy(update={"cue_maps": {"song-1": late}}),
        backend_factory=lambda: midi,
        osc_backend_factory=lambda: osc,
    )
    await lights.start()
    await start_song(bus)
    assert lights._timeline_task is not None
    running = lights._timeline_task

    outcome = await lights.reconfigure(
        lights._settings.model_copy(update={"method": LightingMethod.OSC})
    )
    assert outcome.accepted
    assert running.cancelled() or running.done()
    assert lights._timeline_task is None
    await asyncio.sleep(1.2)  # the old MIDI cue and the new OSC cue would both be due now
    await lights.stop()

    assert osc.sent == []
    assert [m for p in midi.ports for m in p.messages if m[0] == "note_on"] == []


@pytest.mark.asyncio
async def test_switching_back_restores_kept_cues() -> None:
    bus, midi, osc = EventBus(), FakeMidiBackend(), FakeOscBackend()
    lights = plugin(bus, midi, osc, LightingMethod.OSC)
    await lights.start()
    await lights.reconfigure(lights._settings.model_copy(update={"method": LightingMethod.MIDI}))
    await lights.reconfigure(lights._settings.model_copy(update={"method": LightingMethod.OSC}))
    cue_map = lights._settings.cue_maps["song-1"]
    assert len(cue_map.cues) == 1
    assert len(cue_map.osc_cues) == 1
    await start_song(bus)
    await lights.stop()
    assert osc.sent == [("/cue/1", [1.0])]


@pytest.mark.asyncio
async def test_osc_test_cue_sends_and_reports_sent_not_connected() -> None:
    bus, midi, osc = EventBus(), FakeMidiBackend(), FakeOscBackend()
    lights = plugin(bus, midi, osc, LightingMethod.OSC)
    await lights.start()
    outcome = await lights.test_osc_cue("/test", [arg("int", 5)])
    await lights.stop()
    assert outcome.accepted
    assert "Sent /test" in outcome.message
    assert "Connected" not in outcome.message
    assert osc.sent == [("/test", [5])]


@pytest.mark.asyncio
async def test_osc_test_cue_refused_while_midi_is_saved_and_midi_test_refused_for_osc() -> None:
    bus = EventBus()
    midi_plugin = plugin(bus, FakeMidiBackend(), FakeOscBackend(), LightingMethod.MIDI)
    osc_plugin = plugin(bus, FakeMidiBackend(), FakeOscBackend(), LightingMethod.OSC)
    assert not (await midi_plugin.test_osc_cue("/x", [])).accepted
    assert not (await osc_plugin.test_cue(60, 100)).accepted
    assert osc_plugin._osc_output is None


@pytest.mark.asyncio
async def test_osc_test_cue_rejects_bad_address() -> None:
    bus, osc = EventBus(), FakeOscBackend()
    lights = plugin(bus, FakeMidiBackend(), osc, LightingMethod.OSC)
    await lights.start()
    outcome = await lights.test_osc_cue("no-slash", [])
    await lights.stop()
    assert not outcome.accepted
    assert osc.sent == []


@pytest.mark.asyncio
async def test_osc_send_error_is_surfaced_as_error_status() -> None:
    class FailingOutput(FakeOscOutput):
        def send_message(self, address: str, args: Sequence[OscArgument]) -> None:
            raise RuntimeError("OSC send to 127.0.0.1:9000 failed: Network is unreachable.")

    class FailingBackend(FakeOscBackend):
        def open_output(self, host: str, port: int) -> FakeOscOutput:
            return FailingOutput()

    bus = EventBus()
    lights = LightsPlugin(
        bus,
        StateStore(),
        settings(LightingMethod.OSC),
        backend_factory=FakeMidiBackend,
        osc_backend_factory=FailingBackend,
    )
    await lights.start()
    outcome = await lights.test_osc_cue("/x", [])
    snapshot = await lights.snapshot()
    await lights.stop()
    assert not outcome.accepted
    assert "Network is unreachable" in outcome.message
    assert snapshot.connection_status is ConnectionStatus.ERROR


@pytest.mark.asyncio
async def test_bad_osc_host_reports_error_on_connect() -> None:
    bus = EventBus()
    bad = settings(LightingMethod.OSC).model_copy(update={"osc_host": "no-such-host.invalid"})
    lights = LightsPlugin(bus, StateStore(), bad, backend_factory=FakeMidiBackend)
    await lights.start()
    snapshot = await lights.snapshot()
    await lights.stop()
    assert snapshot.connection_status is ConnectionStatus.ERROR
    assert "could not be found" in (snapshot.detail or "")


@pytest.mark.asyncio
async def test_osc_over_real_udp_from_timeline() -> None:
    with udp_receiver() as sock:
        bus = EventBus()
        lights = LightsPlugin(
            bus,
            StateStore(),
            settings(LightingMethod.OSC).model_copy(update={"osc_port": sock.getsockname()[1]}),
            backend_factory=FakeMidiBackend,
        )
        await lights.start()
        await start_song(bus)
        await lights.stop()
        assert receive(sock) == ("/cue/1", [1.0])
