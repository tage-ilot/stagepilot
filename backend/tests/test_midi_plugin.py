from __future__ import annotations

import asyncio
import hashlib
from collections.abc import Callable
from dataclasses import dataclass
from typing import Literal

import pytest

from stagepilot.core.actions import ActionOutcome
from stagepilot.core.config import MidiNoteMappings, MidiSettings
from stagepilot.core.event_bus import EventBus, Subscription
from stagepilot.core.events import (
    ActionName,
    ConnectionPayload,
    EventType,
    MidiNotePayload,
    StagePilotEvent,
)
from stagepilot.core.midi import MidiCueName, MidiMessageDisposition
from stagepilot.core.state import StateStore
from stagepilot.models.state import ConnectionStatus, PluginStatus
from stagepilot.plugins.midi_playback.client import (
    MidiBackendContract,
    MidiInputPortContract,
    MidiMessageCallback,
)
from stagepilot.plugins.midi_playback.models import MidiMessage
from stagepilot.plugins.midi_playback.plugin import MidiPlaybackPlugin

WaitPredicate = Callable[[], bool]


class FakeMidiPort:
    def __init__(self, name: str, callback: MidiMessageCallback) -> None:
        self.name = name
        self.callback = callback
        self.closed = False
        self.close_calls = 0

    def close(self) -> None:
        self.close_calls += 1
        self.closed = True

    def emit(
        self,
        message_type: Literal["note_on", "note_off"] = "note_on",
        *,
        channel: int = 1,
        note: int = 112,
        velocity: int = 100,
    ) -> None:
        self.callback(
            MidiMessage(
                type=message_type,
                channel=channel,
                note=note,
                velocity=velocity,
            )
        )


class FakeMidiBackend:
    def __init__(self, input_names: list[str]) -> None:
        self.input_names = input_names
        self.list_calls = 0
        self.open_calls: list[str] = []
        self.ports: list[FakeMidiPort] = []

    def list_input_names(self) -> list[str]:
        self.list_calls += 1
        return list(self.input_names)

    def open_input(
        self,
        name: str,
        callback: MidiMessageCallback,
    ) -> MidiInputPortContract:
        if self.input_names.count(name) != 1:
            raise ValueError("MIDI input must have one exact match.")
        self.open_calls.append(name)
        port = FakeMidiPort(name, callback)
        self.ports.append(port)
        return port


class FakeBackendFactory:
    def __init__(self, backend: MidiBackendContract) -> None:
        self.backend = backend
        self.calls = 0

    def __call__(self) -> MidiBackendContract:
        self.calls += 1
        return self.backend


@dataclass(frozen=True, slots=True)
class DispatchCall:
    action: ActionName
    source: str


class RecordingDispatcher:
    def __init__(self) -> None:
        self.calls: list[DispatchCall] = []
        self.song_positions: list[tuple[int, str]] = []

    async def dispatch(
        self,
        action: ActionName,
        source: str = "api",
    ) -> ActionOutcome:
        self.calls.append(DispatchCall(action, source))
        return ActionOutcome(True, f"Accepted {action.value}.")

    async def dispatch_song_position(
        self,
        position: int,
        source: str = "api",
    ) -> ActionOutcome:
        self.song_positions.append((position, source))
        return ActionOutcome(True, f"Accepted song {position}.")


class MutableClock:
    def __init__(self, value: float = 10.0) -> None:
        self.value = value

    def __call__(self) -> float:
        return self.value

    def advance(self, seconds: float) -> None:
        self.value += seconds


@dataclass(slots=True)
class PluginHarness:
    plugin: MidiPlaybackPlugin
    backend: FakeMidiBackend
    factory: FakeBackendFactory
    dispatcher: RecordingDispatcher
    clock: MutableClock
    events: list[StagePilotEvent]
    event_bus: EventBus
    subscription: Subscription

    async def close(self) -> None:
        await self.plugin.stop()
        await self.event_bus.unsubscribe(self.subscription)


async def plugin_harness(
    *,
    input_names: list[str] | None = None,
    input_name: str | None = "Playback",
    settings: MidiSettings | None = None,
    clock: MutableClock | None = None,
) -> PluginHarness:
    event_bus = EventBus()
    state_store = StateStore()
    backend = FakeMidiBackend(input_names if input_names is not None else ["Playback"])
    factory = FakeBackendFactory(backend)
    dispatcher = RecordingDispatcher()
    resolved_clock = clock or MutableClock()
    events: list[StagePilotEvent] = []

    async def capture(event: StagePilotEvent) -> None:
        events.append(event)

    subscription = await event_bus.subscribe(None, capture)
    plugin = MidiPlaybackPlugin(
        event_bus,
        state_store,
        settings
        or MidiSettings(
            enabled=True,
            input_name=input_name,
            channel=1,
            debounce_ms=250,
        ),
        dispatcher,
        backend_factory=factory,
        monotonic_clock=resolved_clock,
        reconnect_delays=(0.005,),
        monitor_interval=0.005,
    )
    return PluginHarness(
        plugin=plugin,
        backend=backend,
        factory=factory,
        dispatcher=dispatcher,
        clock=resolved_clock,
        events=events,
        event_bus=event_bus,
        subscription=subscription,
    )


async def wait_until(predicate: WaitPredicate) -> None:
    try:
        async with asyncio.timeout(10):
            while not predicate():  # noqa: ASYNC110 - deterministic test polling
                await asyncio.sleep(0.002)
    except TimeoutError as exc:
        raise AssertionError("Timed out waiting for asynchronous MIDI work.") from exc


async def wait_for_health(
    plugin: MidiPlaybackPlugin,
    status: PluginStatus,
) -> None:
    try:
        async with asyncio.timeout(10):
            while (await plugin.health()).status is not status:  # noqa: ASYNC110
                await asyncio.sleep(0.002)
    except TimeoutError as exc:
        raise AssertionError(f"Timed out waiting for MIDI health {status.value}.") from exc


def connection_events(harness: PluginHarness) -> list[ConnectionPayload]:
    return [
        event.payload
        for event in harness.events
        if event.type is EventType.CONNECTION_CHANGED
        and isinstance(event.payload, ConnectionPayload)
        and event.payload.integration == "midi"
    ]


def note_events(harness: PluginHarness) -> list[MidiNotePayload]:
    return [
        event.payload
        for event in harness.events
        if event.type is EventType.MIDI_NOTE_RECEIVED and isinstance(event.payload, MidiNotePayload)
    ]


@pytest.mark.asyncio
async def test_start_connects_exact_input_reports_health_and_safe_discovery() -> None:
    harness = await plugin_harness(input_names=["Keyboard", "Playback"])
    try:
        await harness.plugin.start()
        await wait_until(lambda: len(harness.backend.ports) == 1)
        await wait_for_health(harness.plugin, PluginStatus.RUNNING)

        snapshot = await harness.plugin.input_snapshot(refresh=True)
        playback = next(value for value in snapshot.inputs if value.name == "Playback")

        assert harness.factory.calls == 1
        assert harness.backend.open_calls == ["Playback"]
        assert snapshot.enabled is True
        assert snapshot.channel == 1
        assert snapshot.note == 112
        assert snapshot.configured_input_name == "Playback"
        assert playback.id == hashlib.sha256(b"Playback").hexdigest()
        assert playback.selected is True
        assert playback.connected is True
        assert playback.ambiguous is False
        assert [value.name for value in snapshot.inputs] == ["Keyboard", "Playback"]
        assert [event.status for event in connection_events(harness)][-2:] == [
            ConnectionStatus.CONNECTING,
            ConnectionStatus.CONNECTED,
        ]

        await harness.plugin.start()
        assert harness.factory.calls == 1
        assert harness.backend.open_calls == ["Playback"]
    finally:
        await harness.close()


@pytest.mark.asyncio
async def test_dispatches_all_six_configured_notes_in_fifo_order() -> None:
    harness = await plugin_harness()
    try:
        await harness.plugin.start()
        await wait_until(lambda: len(harness.backend.ports) == 1)
        # A note sent before the connection is active is (correctly) dropped as stale.
        await wait_for_health(harness.plugin, PluginStatus.RUNNING)
        port = harness.backend.ports[0]

        for velocity in range(100, 106):
            port.emit(note=112, velocity=velocity)

        await wait_until(lambda: len(harness.dispatcher.calls) == 6)
        assert harness.dispatcher.calls == [
            DispatchCall(ActionName.START_NEXT, "midi_playback"),
            DispatchCall(ActionName.RESTART_CURRENT, "midi_playback"),
            DispatchCall(ActionName.PREVIOUS, "midi_playback"),
            DispatchCall(ActionName.NEXT, "midi_playback"),
            DispatchCall(ActionName.RELOAD_PLAN, "midi_playback"),
            DispatchCall(ActionName.STOP_TIMER, "midi_playback"),
        ]
        assert [event.action for event in note_events(harness)] == [
            call.action for call in harness.dispatcher.calls
        ]
        assert all(event.simulated is False for event in note_events(harness))
        assert all(event.connection_id is not None for event in note_events(harness))
    finally:
        await harness.close()


@pytest.mark.asyncio
async def test_velocities_one_through_99_dispatch_exact_song_positions() -> None:
    harness = await plugin_harness()
    try:
        await harness.plugin.start()
        await wait_until(lambda: len(harness.backend.ports) == 1)
        # A note sent before the connection is active is (correctly) dropped as stale.
        await wait_for_health(harness.plugin, PluginStatus.RUNNING)
        port = harness.backend.ports[0]

        port.emit(note=112, velocity=1)
        port.emit(note=112, velocity=7)
        port.emit(note=112, velocity=99)

        await wait_until(lambda: len(harness.dispatcher.song_positions) == 3)

        assert harness.dispatcher.song_positions == [
            (1, "midi_playback"),
            (7, "midi_playback"),
            (99, "midi_playback"),
        ]
        assert harness.dispatcher.calls == []
        assert [event.song_position for event in note_events(harness)] == [1, 7, 99]
    finally:
        await harness.close()


@pytest.mark.asyncio
async def test_filters_other_channels_and_unmapped_notes_before_dispatch() -> None:
    harness = await plugin_harness()
    try:
        await harness.plugin.start()
        await wait_until(lambda: len(harness.backend.ports) == 1)
        # A note sent before the connection is active is (correctly) dropped as stale.
        await wait_for_health(harness.plugin, PluginStatus.RUNNING)
        port = harness.backend.ports[0]

        port.emit(channel=2, note=112, velocity=100)
        port.emit(channel=1, note=100, velocity=127)
        port.emit(channel=1, note=112, velocity=103)

        await wait_until(lambda: len(harness.dispatcher.calls) == 1)
        assert harness.dispatcher.calls == [DispatchCall(ActionName.NEXT, "midi_playback")]
        assert [(event.channel, event.note, event.velocity) for event in note_events(harness)] == [
            (1, 112, 103)
        ]
    finally:
        await harness.close()


@pytest.mark.asyncio
async def test_reconfigure_changes_the_live_note_and_channel_filter() -> None:
    harness = await plugin_harness()
    try:
        await harness.plugin.start()
        await wait_until(lambda: len(harness.backend.ports) == 1)
        port = harness.backend.ports[0]

        outcome = await harness.plugin.reconfigure(
            MidiSettings(
                enabled=True,
                input_name="Playback",
                channel=4,
                note=124,
                debounce_ms=0,
            )
        )
        snapshot = await harness.plugin.input_snapshot()

        port.emit(channel=4, note=112, velocity=100)
        port.emit(channel=1, note=124, velocity=100)
        port.emit(channel=4, note=124, velocity=100)

        await wait_until(lambda: len(harness.dispatcher.calls) == 1)
        async with asyncio.timeout(2):
            while len(await harness.plugin.recent_messages()) != 3:  # noqa: ASYNC110
                await asyncio.sleep(0.002)
        messages = await harness.plugin.recent_messages()

        assert outcome.accepted is True
        assert "channel 4, note E8 (124)" in outcome.message
        assert snapshot.channel == 4
        assert snapshot.note == 124
        assert harness.dispatcher.calls == [DispatchCall(ActionName.START_NEXT, "midi_playback")]
        assert [message.disposition for message in messages] == [
            MidiMessageDisposition.DISPATCHED,
            MidiMessageDisposition.WRONG_CHANNEL,
            MidiMessageDisposition.UNMAPPED,
        ]
    finally:
        await harness.close()


@pytest.mark.asyncio
async def test_monitor_records_received_notes_and_why_they_were_ignored() -> None:
    harness = await plugin_harness()
    try:
        await harness.plugin.start()
        await wait_until(lambda: len(harness.backend.ports) == 1)
        # A note sent before the connection is active is (correctly) dropped as stale.
        await wait_for_health(harness.plugin, PluginStatus.RUNNING)
        port = harness.backend.ports[0]

        port.emit(channel=2, note=112, velocity=90)
        port.emit(channel=1, note=16, velocity=100)
        port.emit(channel=1, note=112, velocity=100)
        port.emit("note_off", channel=1, note=112, velocity=0)

        await wait_until(lambda: len(harness.dispatcher.calls) == 1)
        async with asyncio.timeout(2):
            while len(await harness.plugin.recent_messages()) != 4:  # noqa: ASYNC110
                await asyncio.sleep(0.002)
        messages = await harness.plugin.recent_messages()

        assert [message.disposition for message in messages] == [
            MidiMessageDisposition.NOTE_RELEASE,
            MidiMessageDisposition.DISPATCHED,
            MidiMessageDisposition.UNMAPPED,
            MidiMessageDisposition.WRONG_CHANNEL,
        ]
        assert [(message.note, message.note_name) for message in messages] == [
            (112, "E7"),
            (112, "E7"),
            (16, "E-1"),
            (112, "E7"),
        ]
        assert messages[1].action is ActionName.START_NEXT
        assert messages[2].detail == "Ignored: StagePilot listens for MIDI note E7 (112)."
        assert messages[3].channel == 2
        assert messages[3].velocity == 90
        assert messages[3].input_name == "Playback"
    finally:
        await harness.close()


@pytest.mark.parametrize(
    ("note", "playback_name"),
    [
        (0, "C-2"),
        (16, "E-1"),
        (100, "E6"),
        (112, "E7"),
        (124, "E8"),
        (127, "G8"),
    ],
)
def test_note_names_follow_playback_octave_numbering(
    note: int,
    playback_name: str,
) -> None:
    assert MidiPlaybackPlugin._note_name(note) == playback_name


@pytest.mark.asyncio
async def test_note_release_latching_and_monotonic_debounce_prevent_duplicates() -> None:
    clock = MutableClock()
    harness = await plugin_harness(clock=clock)
    try:
        await harness.plugin.start()
        await wait_until(lambda: len(harness.backend.ports) == 1)
        # A note sent before the connection is active is (correctly) dropped as stale.
        await wait_for_health(harness.plugin, PluginStatus.RUNNING)
        port = harness.backend.ports[0]

        port.emit(note=112)
        port.emit(note=112)
        port.emit("note_off", note=112, velocity=0)
        port.emit(note=112)
        port.emit(note=112, velocity=102)

        await wait_until(lambda: len(harness.dispatcher.calls) == 2)
        assert harness.dispatcher.calls == [
            DispatchCall(ActionName.START_NEXT, "midi_playback"),
            DispatchCall(ActionName.PREVIOUS, "midi_playback"),
        ]

        clock.advance(0.251)
        port.emit(note=112)
        await wait_until(lambda: len(harness.dispatcher.calls) == 3)
        assert harness.dispatcher.calls[-1] == DispatchCall(
            ActionName.START_NEXT,
            "midi_playback",
        )
    finally:
        await harness.close()


@pytest.mark.asyncio
async def test_simulation_uses_the_mapping_event_dispatch_and_duplicate_pipeline() -> None:
    clock = MutableClock()
    harness = await plugin_harness(input_names=[], input_name=None, clock=clock)
    try:
        await harness.plugin.start()

        first = await harness.plugin.simulate_cue(MidiCueName.RELOAD_PLAN)
        duplicate = await harness.plugin.simulate_cue(MidiCueName.RELOAD_PLAN)
        clock.advance(0.251)
        after_debounce = await harness.plugin.simulate_cue(MidiCueName.RELOAD_PLAN)

        assert first.accepted is True
        assert duplicate == ActionOutcome(False, "Duplicate MIDI cue ignored.")
        assert after_debounce.accepted is True
        assert harness.dispatcher.calls == [
            DispatchCall(ActionName.RELOAD_PLAN, "midi.simulation"),
            DispatchCall(ActionName.RELOAD_PLAN, "midi.simulation"),
        ]
        assert len(note_events(harness)) == 3
        assert all(event.simulated is True for event in note_events(harness))
        assert all(event.connection_id is None for event in note_events(harness))
        assert all(event.note == 112 for event in note_events(harness))
        assert all(event.velocity == 104 for event in note_events(harness))
    finally:
        await harness.close()


@pytest.mark.asyncio
async def test_simulation_rejects_an_unmapped_cue_without_publishing_or_dispatching() -> None:
    settings = MidiSettings(
        enabled=True,
        input_name=None,
        mappings=MidiNoteMappings(stop_timer=None),
    )
    harness = await plugin_harness(input_names=[], settings=settings)
    try:
        await harness.plugin.start()

        outcome = await harness.plugin.simulate_cue(MidiCueName.STOP_TIMER)

        assert outcome == ActionOutcome(False, 'The MIDI cue "stop_timer" is not mapped.')
        assert harness.dispatcher.calls == []
        assert note_events(harness) == []
    finally:
        await harness.close()


@pytest.mark.asyncio
async def test_stale_callback_is_rejected_after_reconnect() -> None:
    harness = await plugin_harness()
    try:
        await harness.plugin.start()
        await wait_until(lambda: len(harness.backend.ports) == 1)
        old_port = harness.backend.ports[0]
        old_port.closed = True

        await wait_until(lambda: len(harness.backend.ports) == 2)
        await wait_for_health(harness.plugin, PluginStatus.RUNNING)
        new_port = harness.backend.ports[1]
        old_port.emit(note=112)
        new_port.emit(note=112, velocity=101)
        new_port.emit(note=112, velocity=102)

        await wait_until(lambda: len(harness.dispatcher.calls) == 2)
        assert harness.dispatcher.calls == [
            DispatchCall(ActionName.RESTART_CURRENT, "midi_playback"),
            DispatchCall(ActionName.PREVIOUS, "midi_playback"),
        ]
        assert [event.note for event in note_events(harness)] == [112, 112]
        assert [event.velocity for event in note_events(harness)] == [101, 102]
        assert old_port.close_calls == 1
    finally:
        await harness.close()


@pytest.mark.asyncio
async def test_missing_input_degrades_health_then_reconnects_when_it_appears() -> None:
    harness = await plugin_harness(input_names=[])
    try:
        await harness.plugin.start()
        await wait_until(
            lambda: any(
                event.status is ConnectionStatus.DISCONNECTED
                for event in connection_events(harness)
            )
        )

        assert (await harness.plugin.health()).status is PluginStatus.STARTING
        assert harness.backend.ports == []

        harness.backend.input_names.append("Playback")
        await wait_until(lambda: len(harness.backend.ports) == 1)
        await wait_for_health(harness.plugin, PluginStatus.RUNNING)

        harness.backend.ports[0].emit(note=112, velocity=105)
        await wait_until(lambda: len(harness.dispatcher.calls) == 1)
        assert harness.dispatcher.calls == [DispatchCall(ActionName.STOP_TIMER, "midi_playback")]
        assert [event.status for event in connection_events(harness)][-2:] == [
            ConnectionStatus.CONNECTING,
            ConnectionStatus.CONNECTED,
        ]
    finally:
        await harness.close()


@pytest.mark.asyncio
async def test_ambiguous_discovery_is_safe_and_reports_error_health() -> None:
    harness = await plugin_harness(
        input_names=["Zulu", "Playback", "alpha", "Playback"],
    )
    try:
        await harness.plugin.start()
        await wait_for_health(harness.plugin, PluginStatus.ERROR)

        snapshot = await harness.plugin.input_snapshot(refresh=True)
        assert [value.name for value in snapshot.inputs] == ["alpha", "Playback", "Zulu"]
        assert snapshot.inputs[1].ambiguous is True
        assert snapshot.inputs[1].selected is True
        assert snapshot.inputs[1].connected is False
        assert harness.backend.open_calls == []
        assert connection_events(harness)[-1].status is ConnectionStatus.ERROR
        assert "Multiple MIDI inputs" in (connection_events(harness)[-1].detail or "")
    finally:
        await harness.close()


@pytest.mark.asyncio
async def test_shutdown_is_idempotent_closes_port_and_rejects_late_work() -> None:
    harness = await plugin_harness()
    await harness.plugin.start()
    await wait_until(lambda: len(harness.backend.ports) == 1)
    # A note sent before the connection is active is (correctly) dropped as stale.
    await wait_for_health(harness.plugin, PluginStatus.RUNNING)
    port = harness.backend.ports[0]

    await harness.plugin.stop()
    await harness.plugin.stop()
    port.emit(note=112)
    outcome = await harness.plugin.simulate_cue(MidiCueName.START_NEXT)

    assert port.close_calls == 1
    assert harness.dispatcher.calls == []
    assert outcome == ActionOutcome(False, "The MIDI Playback plugin is not running.")
    assert (await harness.plugin.health()).status is PluginStatus.STOPPED
    assert connection_events(harness)[-1].status is ConnectionStatus.DISCONNECTED
    assert connection_events(harness)[-1].detail == "MIDI Playback input stopped."
    await harness.event_bus.unsubscribe(harness.subscription)
