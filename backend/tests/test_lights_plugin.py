from __future__ import annotations

import asyncio
from dataclasses import dataclass, field

import pytest

from stagepilot.core.config import LightingCue, LightsSettings, SongLightingCueMap
from stagepilot.core.event_bus import EventBus
from stagepilot.core.events import EventType, SongPayload, TimerPayload, new_event
from stagepilot.core.state import StateStore
from stagepilot.models.state import ConnectionStatus, Song
from stagepilot.plugins.lights import LightsPlugin


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
    names: list[str] = field(default_factory=lambda: ["StagePilot to Lightkey"])
    ports: list[FakeOutputPort] = field(default_factory=list)

    def list_output_names(self) -> list[str]:
        return list(self.names)

    def open_output(self, name: str) -> FakeOutputPort:
        if self.names.count(name) != 1:
            raise ValueError("Output unavailable.")
        port = FakeOutputPort()
        self.ports.append(port)
        return port


def worship_song() -> Song:
    return Song(
        id="plan-item-1",
        source_song_id="song-1",
        title="Holy Forever",
        duration_seconds=300,
        order=1,
    )


def lights_settings(*cues: LightingCue) -> LightsSettings:
    return LightsSettings(
        enabled=True,
        output_name="StagePilot to Lightkey",
        channel=3,
        pulse_ms=10,
        cue_maps={
            "song-1": SongLightingCueMap(
                song_key="song-1",
                song_title="Holy Forever",
                cues=list(cues),
            )
        },
    )


async def publish_song_start(bus: EventBus, *, duration: int = 300) -> None:
    song = worship_song()
    await bus.publish(
        new_event(
            EventType.SONG_STARTED,
            source="test",
            payload=SongPayload(song=song, index=0),
        )
    )
    await bus.publish(
        new_event(
            EventType.TIMER_STARTED,
            source="propresenter",
            payload=TimerPayload(duration_seconds=duration, song=song),
        )
    )


@pytest.mark.asyncio
async def test_timer_start_sends_elapsed_cue_as_note_on_off_pulse() -> None:
    bus = EventBus()
    backend = FakeOutputBackend()
    plugin = LightsPlugin(
        bus,
        StateStore(),
        lights_settings(LightingCue(at_seconds=0, note=72, velocity=110, label="Intro")),
        backend_factory=lambda: backend,
    )

    await plugin.start()
    await publish_song_start(bus)
    await asyncio.sleep(0.05)
    snapshot = await plugin.snapshot()
    await plugin.stop()

    assert backend.ports[0].messages == [
        ("note_on", 3, 72, 110),
        ("note_off", 3, 72, 0),
    ]
    assert snapshot.connection_status is ConnectionStatus.CONNECTED
    assert snapshot.last_cue is not None
    assert snapshot.last_cue.label == "Intro"


@pytest.mark.asyncio
async def test_output_monitor_reports_disconnect_and_reconnects() -> None:
    bus = EventBus()
    backend = FakeOutputBackend()
    plugin = LightsPlugin(
        bus,
        StateStore(),
        lights_settings(),
        backend_factory=lambda: backend,
        monitor_interval=0.01,
    )

    await plugin.start()
    first_port = backend.ports[0]
    backend.names.clear()

    deadline = asyncio.get_running_loop().time() + 1
    while asyncio.get_running_loop().time() < deadline:
        snapshot = await plugin.snapshot()
        if snapshot.connection_status is ConnectionStatus.DISCONNECTED:
            break
        await asyncio.sleep(0.01)
    else:
        raise AssertionError("Lighting output loss was not detected.")

    assert first_port.closed is True
    backend.names.append("StagePilot to Lightkey")

    deadline = asyncio.get_running_loop().time() + 1
    while asyncio.get_running_loop().time() < deadline:
        snapshot = await plugin.snapshot()
        if snapshot.connection_status is ConnectionStatus.CONNECTED:
            break
        await asyncio.sleep(0.01)
    else:
        raise AssertionError("Lighting output recovery was not detected.")

    assert len(backend.ports) == 2
    await plugin.stop()


@pytest.mark.asyncio
async def test_unavailable_configured_output_starts_disconnected() -> None:
    bus = EventBus()
    backend = FakeOutputBackend(names=[])
    plugin = LightsPlugin(
        bus,
        StateStore(),
        lights_settings(),
        backend_factory=lambda: backend,
        monitor_interval=0.01,
    )

    await plugin.start()
    snapshot = await plugin.snapshot()
    await plugin.stop()

    assert snapshot.connection_status is ConnectionStatus.DISCONNECTED
    assert snapshot.detail is not None
    assert "unavailable" in snapshot.detail


@pytest.mark.asyncio
async def test_timer_payload_carries_song_without_relying_on_subscriber_order() -> None:
    bus = EventBus()
    backend = FakeOutputBackend()
    plugin = LightsPlugin(
        bus,
        StateStore(),
        lights_settings(LightingCue(at_seconds=0, note=71, velocity=120)),
        backend_factory=lambda: backend,
    )

    await plugin.start()
    await bus.publish(
        new_event(
            EventType.TIMER_STARTED,
            source="propresenter",
            payload=TimerPayload(duration_seconds=300, song=worship_song()),
        )
    )
    await asyncio.sleep(0.05)
    await plugin.stop()

    assert backend.ports[0].messages == [
        ("note_on", 3, 71, 120),
        ("note_off", 3, 71, 0),
    ]


@pytest.mark.asyncio
@pytest.mark.parametrize(
    "event_type",
    [EventType.TIMER_STOP_REQUESTED, EventType.TIMER_RESET_REQUESTED],
)
async def test_stop_or_reset_request_cancels_future_lighting_cues(
    event_type: EventType,
) -> None:
    bus = EventBus()
    backend = FakeOutputBackend()
    plugin = LightsPlugin(
        bus,
        StateStore(),
        lights_settings(LightingCue(at_seconds=1, note=73, velocity=127)),
        backend_factory=lambda: backend,
    )

    await plugin.start()
    await publish_song_start(bus)
    await bus.publish(new_event(event_type, source="test"))
    await asyncio.sleep(0.05)
    await plugin.stop()

    assert backend.ports[0].messages == []


@pytest.mark.asyncio
async def test_restart_cancels_old_timeline_and_starts_exactly_one_new_timeline() -> None:
    bus = EventBus()
    backend = FakeOutputBackend()
    cue = LightingCue(at_seconds=0, note=74, velocity=100)
    plugin = LightsPlugin(
        bus,
        StateStore(),
        lights_settings(cue),
        backend_factory=lambda: backend,
    )

    await plugin.start()
    await publish_song_start(bus)
    await asyncio.sleep(0.03)
    await bus.publish(
        new_event(
            EventType.SONG_RESTARTED,
            source="test",
            payload=SongPayload(song=worship_song(), index=0),
        )
    )
    await bus.publish(
        new_event(
            EventType.TIMER_STARTED,
            source="propresenter",
            payload=TimerPayload(duration_seconds=300),
        )
    )
    await asyncio.sleep(0.03)
    await plugin.stop()

    assert [message[0] for message in backend.ports[0].messages] == [
        "note_on",
        "note_off",
        "note_on",
        "note_off",
    ]


@pytest.mark.asyncio
async def test_cues_after_confirmed_timer_duration_are_not_scheduled() -> None:
    bus = EventBus()
    backend = FakeOutputBackend()
    plugin = LightsPlugin(
        bus,
        StateStore(),
        lights_settings(LightingCue(at_seconds=5, note=75, velocity=127)),
        backend_factory=lambda: backend,
    )

    await plugin.start()
    await publish_song_start(bus, duration=4)
    await asyncio.sleep(0.02)
    await plugin.stop()

    assert backend.ports[0].messages == []


def _notes(backend: FakeOutputBackend) -> list[int]:
    return [m[2] for p in backend.ports for m in p.messages if m[0] == "note_on"]


async def _reposition(
    bus: EventBus,
    kind: EventType,
    position: float,
    *,
    running: bool = True,
    duration: int = 300,
) -> None:
    remaining = max(0, round(duration - position))
    await bus.publish(
        new_event(
            kind,
            source="test",
            payload=TimerPayload(
                duration_seconds=remaining,
                song=worship_song(),
                position_seconds=position,
                remaining_seconds=remaining,
                running=running,
            ),
        )
    )


def _plugin(bus: EventBus, backend: FakeOutputBackend, *cues: LightingCue) -> LightsPlugin:
    return LightsPlugin(bus, StateStore(), lights_settings(*cues), backend_factory=lambda: backend)


def _cue(at: float, note: int) -> LightingCue:
    return LightingCue(at_seconds=at, note=note, velocity=100, label=f"c{note}")


@pytest.mark.asyncio
async def test_resume_skips_past_cues_and_times_upcoming_from_position() -> None:
    bus, backend = EventBus(), FakeOutputBackend()
    plugin = _plugin(bus, backend, _cue(10, 1), _cue(100, 2), _cue(200, 3))
    await plugin.start()
    await _reposition(bus, EventType.TIMER_RESUMED, 99.95)
    await asyncio.sleep(0.01)
    assert _notes(backend) == []  # nothing burst, upcoming cue not yet due
    await asyncio.sleep(0.2)
    await plugin.stop()
    assert _notes(backend) == [2]  # past cue 1 skipped; cue 3 is 100 s away


@pytest.mark.asyncio
async def test_seek_forward_skips_cues_between() -> None:
    bus, backend = EventBus(), FakeOutputBackend()
    plugin = _plugin(bus, backend, _cue(5, 1), _cue(60, 2), _cue(150, 3))
    await plugin.start()
    await _reposition(bus, EventType.TIMER_SEEKED, 149.95)
    await asyncio.sleep(0.25)
    await plugin.stop()
    assert _notes(backend) == [3]


@pytest.mark.asyncio
async def test_seek_back_replays_only_upcoming_cues_from_new_offset() -> None:
    bus, backend = EventBus(), FakeOutputBackend()
    plugin = _plugin(bus, backend, _cue(10, 1), _cue(50, 2), _cue(200, 3))
    await plugin.start()
    await _reposition(bus, EventType.TIMER_SEEKED, 9.95)
    await asyncio.sleep(0.25)
    await plugin.stop()
    assert _notes(backend) == [1]


@pytest.mark.asyncio
async def test_seek_while_stopped_sends_nothing_and_cancels_pending() -> None:
    bus, backend = EventBus(), FakeOutputBackend()
    plugin = _plugin(bus, backend, _cue(1, 1), _cue(20, 2))
    await plugin.start()
    await publish_song_start(bus)  # full timeline pending
    await bus.publish(
        new_event(
            EventType.TIMER_STOP_REQUESTED, source="t", payload=TimerPayload(duration_seconds=0)
        )
    )
    await _reposition(bus, EventType.TIMER_SEEKED, 19.95, running=False)
    await asyncio.sleep(1.3)
    await plugin.stop()
    assert _notes(backend) == []


@pytest.mark.asyncio
async def test_seek_to_zero_while_running_replays_full_timeline() -> None:
    bus, backend = EventBus(), FakeOutputBackend()
    plugin = _plugin(bus, backend, _cue(0, 1), _cue(1, 2))
    await plugin.start()
    await _reposition(bus, EventType.TIMER_SEEKED, 0.0)
    await asyncio.sleep(1.3)
    await plugin.stop()
    assert _notes(backend) == [1, 2]


@pytest.mark.asyncio
async def test_cue_exactly_at_position_boundary_is_sent() -> None:
    bus, backend = EventBus(), FakeOutputBackend()
    plugin = _plugin(bus, backend, _cue(99, 1), _cue(100, 2))
    await plugin.start()
    await _reposition(bus, EventType.TIMER_RESUMED, 100.0)
    await asyncio.sleep(0.2)
    await plugin.stop()
    assert _notes(backend) == [2]


@pytest.mark.asyncio
async def test_position_past_end_is_clamped_and_sends_nothing() -> None:
    bus, backend = EventBus(), FakeOutputBackend()
    plugin = _plugin(bus, backend, _cue(10, 1), _cue(299, 2))
    await plugin.start()
    await _reposition(bus, EventType.TIMER_SEEKED, 400.0)  # remaining clamps to 0
    await asyncio.sleep(0.1)
    await plugin.stop()
    assert _notes(backend) == []


@pytest.mark.asyncio
async def test_stop_from_stopped_state_is_harmless() -> None:
    bus, backend = EventBus(), FakeOutputBackend()
    plugin = _plugin(bus, backend, _cue(1, 1))
    await plugin.start()
    await _reposition(bus, EventType.TIMER_RESUMED, 0.0)
    for _ in range(2):
        await bus.publish(
            new_event(
                EventType.TIMER_STOP_REQUESTED, source="t", payload=TimerPayload(duration_seconds=0)
            )
        )
    await asyncio.sleep(1.3)
    await plugin.stop()
    assert _notes(backend) == []
