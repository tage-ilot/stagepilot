"""Playback pause/resume/seek/reconnect mapping, end to end through A's fake server.

Fake Playback server -> real PlaybackClient -> PlaybackMapper -> real StateService.
"""

from __future__ import annotations

import asyncio
from collections.abc import AsyncIterator, Callable
from dataclasses import dataclass, field
from datetime import UTC, datetime
from unittest.mock import AsyncMock

import pytest_asyncio

from playback_fake import FakePlayback, ScriptedFinder
from stagepilot.core.event_bus import EventBus
from stagepilot.core.events import ActionName, EventType, StagePilotEvent
from stagepilot.core.state import StateStore
from stagepilot.models.state import TimerState, TimerStatus
from stagepilot.plugins.playback_api.arbiter import ArbitratedDispatcher, PlaybackSourceArbiter
from stagepilot.plugins.playback_api.client import PlaybackClient, PlaybackStatus
from stagepilot.plugins.playback_api.mapping import PlaybackMapper
from stagepilot.plugins.playback_api.normalizer import PlaybackEvent
from stagepilot.services.state_service import StateService
from test_state_service import load, make_plan, song, start_timer

ORDER = (101, 102, 103)


@dataclass
class Rig:
    pb: FakePlayback
    bus: EventBus
    store: StateStore
    service: StateService
    mapper: PlaybackMapper
    items: list[tuple[PlaybackStatus, tuple[PlaybackEvent, ...]]] = field(default_factory=list)
    resumed: list[StagePilotEvent] = field(default_factory=list)
    _done: int = 0
    _last: object = None

    def __call__(self, status: PlaybackStatus, events: tuple[PlaybackEvent, ...]) -> None:
        self.items.append((status, events))

    async def pump(self) -> None:
        """Wait for the client to go quiet, then feed everything to the mapper like the plugin."""
        quiet = 0
        count = -1
        for _ in range(100):  # bounded: a retrying client never goes fully quiet
            if quiet >= 8:
                break
            await asyncio.sleep(0.02)
            quiet = quiet + 1 if len(self.items) == count else 0
            count = len(self.items)
        for status, events in self.items[self._done :]:
            heartbeat = status.heartbeat
            fresh = heartbeat is not None and heartbeat is not self._last
            if fresh:
                self._last = heartbeat
            if not status.connected:
                self._last = None
                self.mapper.discard_pending()
            observed = self.mapper.observe(
                heartbeat if fresh else None,
                events,
                status.setlist_id if status.connected else None,
            )
            for item in observed:
                await self.mapper.dispatch(item)
        self._done = len(self.items)

    async def timer(self) -> TimerState:
        return (await self.store.snapshot()).timer

    async def begin_song_one(self) -> None:
        """Select the first song and start its timer, as a normal 'song started' would."""
        await self.service.dispatch(ActionName.START_NEXT)
        await start_timer(self.bus, 240, datetime.now(UTC))


@pytest_asyncio.fixture
async def rig() -> AsyncIterator[Rig]:
    bus = EventBus()
    store = StateStore()
    service = StateService(bus, store)
    await service.start()
    await load(bus, make_plan(song("a", "A", 1, 240), song("b", "B", 2, 240), song("c", "C", 3)))
    mapper = PlaybackMapper(service, store)
    mapper.install_order(ORDER, 1)
    with FakePlayback(song=101) as pb:
        r = Rig(pb, bus, store, service, mapper)
        await bus.subscribe(EventType.TIMER_RESUMED, r.resumed.append)
        client = PlaybackClient(
            finder=ScriptedFinder(pb), observer=r, reconnect_delays=(0.01,), heartbeat_timeout=0.3
        )
        await client.start()
        assert await asyncio.to_thread(pb.wait_connected, 1)
        await r.pump()
        try:
            yield r
        finally:
            await client.stop()
            await service.stop()


async def eventually(check: Callable[[], bool], limit: float = 3.0) -> None:
    async def poll() -> None:
        while not check():  # noqa: ASYNC110
            await asyncio.sleep(0.01)

    await asyncio.wait_for(poll(), limit)


async def back_online(rig: Rig, beats: int = 1) -> None:
    """Wait until the client is connected again, then send heartbeats."""
    await eventually(lambda: rig.pb.connections >= 2)
    for _ in range(300):
        await rig.pump()
        if rig.items and rig.items[-1][0].connected:
            break
        await asyncio.sleep(0.02)
    rig.pb.tick(beats)
    await rig.pump()


async def play_ten_seconds(rig: Rig) -> None:
    await rig.begin_song_one()
    rig.pb.play()
    rig.pb.tick(10)
    await rig.pump()


async def test_play_pause_play(rig: Rig) -> None:
    await play_ten_seconds(rig)
    assert (await rig.timer()).status is TimerStatus.RUNNING
    rig.pb.pause()
    await rig.pump()
    stopped = await rig.timer()
    assert stopped.status is TimerStatus.STOPPED
    assert stopped.remaining_seconds == 230
    assert stopped.position_seconds == 10
    rig.pb.play()
    await rig.pump()
    running = await rig.timer()
    assert running.status is TimerStatus.RUNNING
    assert running.duration_seconds == 230
    assert running.position_seconds == 10
    assert len(rig.resumed) == 1


async def test_pause_seek_forward_play(rig: Rig) -> None:
    await play_ten_seconds(rig)
    rig.pb.pause()
    rig.pb.seek(100)
    await rig.pump()
    held = await rig.timer()
    assert held.status is TimerStatus.STOPPED  # seek while stopped never runs
    assert held.remaining_seconds == 140
    assert held.position_seconds == 100
    rig.pb.play()
    await rig.pump()
    running = await rig.timer()
    assert running.status is TimerStatus.RUNNING
    assert running.duration_seconds == 140


async def test_seek_while_playing_keeps_running(rig: Rig) -> None:
    await play_ten_seconds(rig)
    rig.pb.seek(200)
    await rig.pump()
    running = await rig.timer()
    assert running.status is TimerStatus.RUNNING
    assert running.duration_seconds == 40
    assert running.position_seconds == 200


async def test_seek_past_end_stops_with_zero_remaining(rig: Rig) -> None:
    await play_ten_seconds(rig)
    rig.pb.seek(500)
    await rig.pump()
    done = await rig.timer()
    assert done.status is TimerStatus.STOPPED
    assert done.remaining_seconds == 0


async def test_pause_seek_zero_play_is_the_existing_restart(rig: Rig) -> None:
    await play_ten_seconds(rig)
    rig.pb.pause()
    rig.pb.seek(0.0)
    await rig.pump()
    assert (await rig.timer()).remaining_seconds == 240
    rig.pb.play()
    await rig.pump()
    assert (await rig.store.snapshot()).last_action is ActionName.RESTART_CURRENT
    assert rig.resumed == []  # a restart, not a resume


async def test_drop_and_return_same_song_aligns_timer(rig: Rig) -> None:
    await play_ten_seconds(rig)
    rig.pb.drop()
    rig.pb.position = 15.0  # Playback kept playing while StagePilot was disconnected
    await back_online(rig, 1)
    running = await rig.timer()
    assert running.status is TimerStatus.RUNNING
    assert running.position_seconds == 15  # aligned at the first heartbeat back
    assert running.duration_seconds == 225
    assert not rig.mapper.stale  # reconnect alone never invalidates the order


async def test_return_while_stopped_on_same_song_resumes_when_playing(rig: Rig) -> None:
    await play_ten_seconds(rig)
    rig.pb.pause()
    await rig.pump()
    rig.pb.go_offline()
    await rig.pump()
    rig.pb.play()  # pressed on Playback while StagePilot could not hear it
    rig.pb.come_back()
    await back_online(rig, 1)
    assert (await rig.timer()).status is TimerStatus.RUNNING


async def test_return_on_a_different_song_is_ignored(rig: Rig) -> None:
    await play_ten_seconds(rig)
    before = await rig.timer()
    rig.pb.go_offline()
    await rig.pump()
    rig.pb.select_song(102, 30.0)
    rig.pb.playing = True
    rig.pb.come_back()
    await back_online(rig, 1)
    assert await rig.timer() == before
    assert rig.resumed == []


async def test_return_with_stale_order_is_ignored(rig: Rig) -> None:
    await play_ten_seconds(rig)
    before = await rig.timer()
    rig.pb.go_offline()
    await rig.pump()
    rig.pb.version = 2  # setlist edited while away: saved order no longer valid
    rig.pb.seek(80)
    rig.pb.come_back()
    await back_online(rig, 1)
    assert rig.mapper.stale
    assert await rig.timer() == before
    # Pause still requests a stop even though the order is stale.
    rig.pb.pause()
    await rig.pump()
    assert (await rig.store.snapshot()).last_action is ActionName.STOP_TIMER


async def test_stale_order_blocks_resume_and_seek(rig: Rig) -> None:
    await play_ten_seconds(rig)
    rig.pb.pause()
    await rig.pump()
    held = await rig.timer()
    rig.pb.version = 2
    rig.pb.beat()
    await rig.pump()  # Playback reports the edited setlist: order is stale
    rig.pb.seek(100)
    rig.pb.play()
    await rig.pump()
    assert rig.mapper.stale
    assert await rig.timer() == held
    assert rig.resumed == []


async def test_stall_leaves_timer_alone_then_realigns(rig: Rig) -> None:
    await play_ten_seconds(rig)
    before = await rig.timer()
    rig.pb.stall()
    await eventually(lambda: not rig.items[-1][0].connected)
    await rig.pump()
    assert await rig.timer() == before  # a stall alone never changes the countdown
    rig.pb.position = 40.0
    rig.pb.stall(False)
    await back_online(rig, 1)
    running = await rig.timer()
    assert running.status is TimerStatus.RUNNING
    assert running.position_seconds == 40


async def test_missing_song_id_does_not_crash_or_map(rig: Rig) -> None:
    await play_ten_seconds(rig)
    rig.pb.send_raw(
        '{"heartbeat":{"stateData":{"sequenceTime":50,"sequencerPlayState":{"playing":{}},'
        '"padPlayerPlayState":{"stopped":{}},"setlistCloudVersion":1}}}'
    )
    rig.pb.tick(1)  # keep the link healthy; the song-less heartbeat is dropped
    # Wait for that tick's heartbeat (position 11) to arrive instead of guessing how long the
    # client stays quiet: on a slow runner pump() could return before or after it was mapped.
    await eventually(
        lambda: any(s.heartbeat is not None and s.heartbeat.position == 11 for s, _ in rig.items)
    )
    await rig.pump()
    timer = await rig.timer()
    assert timer.status is TimerStatus.RUNNING
    # The invariant: the song-less frame's position 50 is never mapped. The following healthy
    # heartbeat (position 11) may or may not move the timer, so do not pin an exact value.
    assert (timer.position_seconds or 0) < 50
    assert not rig.mapper.stale


async def test_connect_while_not_playing_does_nothing(rig: Rig) -> None:
    await rig.begin_song_one()
    await start_timer(rig.bus, 240, datetime.now(UTC))
    before = await rig.timer()
    rig.pb.pause()
    rig.pb.drop()
    await back_online(rig, 1)
    assert (await rig.timer()).status in (TimerStatus.RUNNING, TimerStatus.STOPPED)
    assert before.status is TimerStatus.RUNNING
    assert rig.resumed == []


async def test_arbiter_forwards_position_calls_and_dedupes_against_midi() -> None:
    inner = AsyncMock()
    arbiter = PlaybackSourceArbiter()
    gate = ArbitratedDispatcher(arbiter, inner, StateStore())
    arbiter.observe_api(False, False)
    arbiter.midi_connected = True  # MIDI owns the show: API calls are ignored
    outcome = await gate.stop_timer_at(12.0, source="playback_api")
    assert outcome.accepted is False
    inner.stop_timer_at.assert_not_awaited()
    # With MIDI absent and API unowned nothing leaks either.
    assert (await gate.seek_timer(1.0, source="playback_api")).accepted is False
    assert (await gate.resume_timer_at(1.0, source="playback_api")).accepted is False
    # Suppression (song-order discovery) still wins.
    gate.suppressed = lambda: True
    assert (await gate.seek_timer(1.0, source="playback_api")).message.startswith("ignored")
