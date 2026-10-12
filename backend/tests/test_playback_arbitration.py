import asyncio
from dataclasses import dataclass
from unittest.mock import AsyncMock

import pytest

from stagepilot.core.actions import ActionOutcome
from stagepilot.core.events import ActionName
from stagepilot.core.state import StateStore
from stagepilot.plugins.midi_playback.models import MidiMessage
from stagepilot.plugins.playback_api.arbiter import ArbitratedDispatcher, PlaybackSourceArbiter
from stagepilot.plugins.playback_api.normalizer import PlaybackEvent
from test_midi_config_api import FakeMidiBackend
from test_playback_integration import FakePlayback, application, controller, wait_for


@dataclass
class Clock:
    now: float = 0

    def __call__(self) -> float:
        return self.now


def stable_api(arbiter: PlaybackSourceArbiter, clock: Clock) -> None:
    for _ in range(4):
        arbiter.observe_api(True, True)
        clock.now += 1


@pytest.mark.parametrize(
    "api,midi,owner",
    [
        (False, False, "none"),
        (True, False, "playback_api"),
        (False, True, "midi"),
        (True, True, "playback_api"),
    ],
)
def test_single_owner_and_connection(api: bool, midi: bool, owner: str) -> None:
    clock = Clock()
    arbiter = PlaybackSourceArbiter(clock=clock)
    arbiter.midi_connected = midi
    if api:
        stable_api(arbiter, clock)
    result = arbiter.snapshot()
    assert result.active_source == owner
    assert result.connected == (api or midi)
    assert result.sources["playback_api"].connected is api
    assert result.sources["midi"].connected is midi


def test_immediate_failover_stable_failback_and_flapping() -> None:
    clock = Clock()
    arbiter = PlaybackSourceArbiter(clock=clock)
    arbiter.midi_connected = True
    stable_api(arbiter, clock)
    assert arbiter.allows("playback_api")
    arbiter.observe_api(False, False)
    assert arbiter.allows("midi")
    arbiter.observe_api(True, True)
    assert arbiter.allows("midi")
    clock.now += 1
    arbiter.observe_api(False, False)
    arbiter.observe_api(True, True)
    assert arbiter.allows("midi")
    stable_api(arbiter, clock)
    assert arbiter.allows("playback_api")
    clock.now += 5
    assert arbiter.allows("midi")
    assert not arbiter.snapshot().sources["playback_api"].connected


def test_single_heartbeat_is_not_stable_failback_and_midi_loss_does_not_wait() -> None:
    clock = Clock()
    arbiter = PlaybackSourceArbiter(clock=clock)
    arbiter.midi_connected = True
    arbiter.observe_api(True, True)
    clock.now = 3
    assert arbiter.allows("midi")
    arbiter.midi_connected = False
    assert arbiter.allows("playback_api")


@pytest.mark.parametrize("action", ["position", "restart", "stop", "start_next"])
async def test_cross_source_dedupe_and_expiry(action: str) -> None:
    clock = Clock()
    arbiter = PlaybackSourceArbiter(clock=clock)
    arbiter.observe_api(True, True)
    dispatcher = AsyncMock()
    dispatcher.dispatch.return_value = ActionOutcome(True, "accepted")
    dispatcher.dispatch_song_position.return_value = ActionOutcome(True, "accepted")
    proxy = ArbitratedDispatcher(arbiter, dispatcher, StateStore())
    if action in ("position", "start_next"):
        assert (await proxy.dispatch_song_position(1, source="playback_api")).accepted
    else:
        name = ActionName.STOP_TIMER if action == "stop" else ActionName.RESTART_CURRENT
        assert (await proxy.dispatch(name, source="playback_api")).accepted
    arbiter.midi_connected = True
    arbiter.observe_api(False, False)
    if action == "position":
        result = await proxy.dispatch_song_position(1, source="midi_playback")
    else:
        name = {
            "restart": ActionName.RESTART_CURRENT,
            "stop": ActionName.STOP_TIMER,
            "start_next": ActionName.START_NEXT,
        }[action]
        result = await proxy.dispatch(name, source="midi_playback")
    assert not result.accepted and "duplicate cross-source" in result.message
    clock.now += 2
    assert (await proxy.dispatch_song_position(1, source="midi_playback")).accepted


async def test_rejected_action_does_not_poison_failover() -> None:
    clock = Clock()
    arbiter = PlaybackSourceArbiter(clock=clock)
    arbiter.observe_api(True, True)
    dispatcher = AsyncMock()
    dispatcher.dispatch_song_position.return_value = ActionOutcome(False, "no plan")
    proxy = ArbitratedDispatcher(arbiter, dispatcher, StateStore())
    await proxy.dispatch_song_position(1, source="playback_api")
    arbiter.midi_connected = True
    arbiter.observe_api(False, False)
    dispatcher.dispatch_song_position.return_value = ActionOutcome(True, "accepted")
    assert (await proxy.dispatch_song_position(1, source="midi_playback")).accepted


async def _switch(client, source: str) -> None:  # type: ignore[no-untyped-def]
    saved = (await client.get("/api/v1/settings")).json()["settings"]
    saved["integration_modes"]["midi_source"] = source
    assert (await client.put("/api/v1/settings", json=saved)).status_code == 200


async def test_api_mode_stale_order_still_stops_and_midi_is_never_started() -> None:
    fake = FakePlayback(index=0)
    midi = FakeMidiBackend(["Playback MIDI"])
    async with application(fake, midi=midi) as (app, client):
        inputs = controller(app)
        await wait_for(lambda: inputs.connection.active_source == "playback_api")
        dispatcher = AsyncMock()
        dispatcher.dispatch.return_value = ActionOutcome(True, "accepted")
        dispatcher.dispatch_song_position.return_value = ActionOutcome(True, "accepted")
        inputs._dispatcher.dispatcher = dispatcher
        assert inputs.mapper.stale
        inputs._observe(
            inputs._generation,
            inputs.status,
            (PlaybackEvent("song.started", 1, 900), PlaybackEvent("song.paused", 1, 900)),
        )
        await wait_for(lambda: dispatcher.dispatch.await_count == 1)
        dispatcher.dispatch.assert_awaited_once_with(ActionName.STOP_TIMER, source="playback_api")
        dispatcher.dispatch_song_position.assert_not_awaited()
        assert inputs.midi is None and not midi.ports and not inputs.arbiter.midi_connected
        status = (await client.get("/api/v1/playback-api/status")).json()
        assert status["connected"] and status["active_source"] == "playback_api"
        assert status["stale"] and status["reason"].startswith("Connected to Playback")


async def test_api_mode_playback_down_stays_not_connected_without_midi() -> None:
    fake = FakePlayback(index=0)
    midi = FakeMidiBackend(["Playback MIDI"])
    async with application(fake, midi=midi) as (app, client):
        inputs = controller(app)
        assert inputs.client is not None
        await inputs.client.stop()
        await wait_for(lambda: inputs.connection.active_source == "none")
        await asyncio.sleep(0.2)
        status = (await client.get("/api/v1/playback-api/status")).json()
        assert not status["connected"] and status["active_source"] == "none"
        assert "MIDI" not in status["reason"] and "backup" not in status["reason"]
        assert status["reason"].startswith("Not connected")
        assert not midi.ports and inputs.midi is None
        assert (await client.get("/api/v1/state")).json()["midi_status"] != "connected"
        assert inputs._midi_guard() == "ignored: MIDI is not the selected connection method"


async def test_api_mode_midi_events_are_never_accepted() -> None:
    from stagepilot.plugins.playback_api.arbiter import ArbitratedDispatcher

    fake = FakePlayback(index=0)
    async with application(fake, midi=FakeMidiBackend(["Playback MIDI"])) as (app, _):
        inputs = controller(app)
        assert inputs.client is not None
        await inputs.client.stop()
        await wait_for(lambda: inputs.connection.active_source == "none")
        inputs.arbiter.midi_connected = True  # even a forced MIDI signal cannot own in API mode
        assert inputs.connection.active_source == "none"
        dispatcher = AsyncMock()
        proxy = ArbitratedDispatcher(inputs.arbiter, dispatcher, app.state.runtime.state_store)
        result = await proxy.dispatch_song_position(1, source="midi_playback")
        assert not result.accepted and "not the selected" in result.message
        dispatcher.dispatch_song_position.assert_not_awaited()


async def test_midi_mode_midi_works_and_api_is_ignored() -> None:
    fake = FakePlayback(index=0)
    midi = FakeMidiBackend(["Playback MIDI"])
    async with application(fake, midi=midi) as (app, client):
        inputs = controller(app)
        await _switch(client, "real")
        await wait_for(lambda: bool(midi.ports))
        inputs = controller(app)
        await wait_for(lambda: inputs.arbiter.midi_connected)
        assert inputs.connection.active_source == "midi"
        midi.ports[-1].callback(MidiMessage(type="note_on", channel=1, note=112, velocity=1))
        await wait_for(lambda: bool(inputs.midi and inputs.midi._recent_messages))
        messages = (await client.get("/api/v1/midi/messages")).json()["messages"]
        assert "ignored" not in messages[0]["detail"]
        assert (await client.get("/api/v1/state")).json()["midi_status"] == "connected"


async def test_runtime_switch_api_to_midi_to_api_starts_and_stops_midi() -> None:
    fake = FakePlayback(index=0)
    midi = FakeMidiBackend(["Playback MIDI"])
    async with application(fake, midi=midi) as (app, client):
        assert not midi.ports
        await _switch(client, "real")
        await wait_for(lambda: bool(midi.ports))
        await _switch(client, "playback_api")
        inputs = controller(app)
        await wait_for(lambda: inputs.connection.active_source == "playback_api")
        assert midi.ports[-1].closed and not inputs.arbiter.midi_connected
        assert (await client.get("/api/v1/playback-api/status")).json()["active_source"] == (
            "playback_api"
        )
