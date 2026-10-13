"""The fake Playback server itself: scripted behaviour is observable through the real client.

These prove the harness, not production resume/reconnect behaviour.
"""

from __future__ import annotations

import asyncio
from collections.abc import Callable

from playback_fake import FakeClock, FakePlayback, ScriptedFinder
from stagepilot.plugins.playback_api.client import PlaybackClient, PlaybackStatus
from stagepilot.plugins.playback_api.normalizer import PlaybackEvent


class Recorder:
    def __init__(self) -> None:
        self.statuses: list[PlaybackStatus] = []
        self.events: list[PlaybackEvent] = []

    def __call__(self, status: PlaybackStatus, events: tuple[PlaybackEvent, ...]) -> None:
        self.statuses.append(status)
        self.events.extend(events)

    @property
    def types(self) -> list[str]:
        return [e.type for e in self.events]

    @property
    def last(self) -> PlaybackStatus:
        return self.statuses[-1]


async def until(cond: Callable[[], bool], limit: float = 3.0) -> None:
    async def poll() -> None:
        while not cond():  # noqa: ASYNC110 - polling a thread-fed recorder
            await asyncio.sleep(0.005)

    await asyncio.wait_for(poll(), limit)


def make_client(
    finder: ScriptedFinder,
    rec: Recorder,
    *,
    delays: tuple[float, ...] = (0.01,),
    timeout: float = 5.0,
) -> PlaybackClient:
    return PlaybackClient(
        finder=finder, observer=rec, reconnect_delays=delays, heartbeat_timeout=timeout
    )


def test_fake_clock_advances() -> None:
    clock = FakeClock(10)
    assert clock() == 10
    assert clock.advance(2.5) == 12.5
    assert clock() == 12.5


async def test_heartbeats_carry_position_and_playing() -> None:
    rec = Recorder()
    with FakePlayback(song=7, position=3.0) as pb:
        client = make_client(ScriptedFinder(pb), rec)
        await client.start()
        await until(lambda: bool(rec.statuses) and rec.last.connected)
        pb.play()
        pb.tick(2)
        await until(lambda: rec.last.heartbeat is not None and rec.last.heartbeat.position == 5.0)
        hb = rec.last.heartbeat
        assert hb is not None
        assert (hb.song_id, hb.playing) == (7, True)
        await client.stop()


async def test_pause_resume_and_seek_while_playing_and_paused() -> None:
    rec = Recorder()
    with FakePlayback(position=10.0, playing=True) as pb:
        client = make_client(ScriptedFinder(pb), rec)
        await client.start()
        await until(lambda: bool(rec.statuses) and rec.last.connected)
        pb.pause()
        await until(lambda: rec.last.heartbeat is not None and not rec.last.heartbeat.playing)
        pb.tick(3)  # paused: position does not move
        pb.seek(40.0)  # seek while paused
        await until(lambda: rec.last.heartbeat is not None and rec.last.heartbeat.position == 40.0)
        pb.resume()
        pb.tick(1)
        pb.seek(5.0)  # seek while playing
        await until(lambda: rec.last.heartbeat is not None and rec.last.heartbeat.position == 5.0)
        assert rec.last.heartbeat is not None and rec.last.heartbeat.playing
        assert "seek" in rec.types
        assert {"song.paused", "song.resumed"} <= set(rec.types)
        await client.stop()


async def test_seek_to_zero_then_play_is_a_restart_signal() -> None:
    rec = Recorder()
    with FakePlayback(position=60.0) as pb:
        client = make_client(ScriptedFinder(pb), rec)
        await client.start()
        await until(lambda: bool(rec.statuses) and rec.last.connected)
        pb.seek_to_start_and_play()
        await until(lambda: "song.started" in rec.types)
        started = next(e for e in rec.events if e.type == "song.started")
        assert started.position is not None and started.position < 2.0
        await client.stop()


async def test_drop_and_return_reconnects_with_fast_delays() -> None:
    rec = Recorder()
    finder = ScriptedFinder()
    with FakePlayback(song=1) as pb:
        finder.point_to(pb)
        client = make_client(finder, rec)
        await client.start()
        # The server counts the socket before the client publishes its first status; wait for the
        # status itself (a slow Windows runner exposed the empty-list race).
        await until(lambda: bool(rec.statuses) and rec.last.connected)
        pb.go_offline()  # Playback quits / Remote Connections off
        await until(lambda: bool(rec.statuses) and not rec.last.connected)
        probes = finder.calls
        await asyncio.sleep(0.1)
        assert finder.calls > probes  # client keeps probing while it is away
        pb.come_back()  # ...and returns
        await until(lambda: bool(rec.statuses) and rec.last.connected)
        assert pb.connections >= 1
        assert rec.types.count("state.snapshot") >= 2  # every connect is a fresh snapshot
        await client.stop()


async def test_host_change_follows_the_finder() -> None:
    rec = Recorder()
    finder = ScriptedFinder()
    with FakePlayback(song=1) as a, FakePlayback(song=2) as b:
        finder.point_to(a)
        client = make_client(finder, rec)
        await client.start()
        await until(lambda: bool(rec.statuses) and rec.last.connected and rec.last.port == a.port)
        finder.point_to(b)
        a.drop()
        await until(lambda: bool(rec.statuses) and rec.last.connected and rec.last.port == b.port)
        assert rec.last.heartbeat is not None and rec.last.heartbeat.song_id == 2
        await client.stop()


async def test_setlist_id_absent_until_announced() -> None:
    rec = Recorder()
    with FakePlayback() as pb:
        client = make_client(ScriptedFinder(pb), rec)
        await client.start()
        await until(lambda: bool(rec.statuses) and rec.last.connected)
        pb.tick(2)
        await until(lambda: len(rec.statuses) >= 3)
        assert rec.last.setlist_id is None
        pb.load_setlist("abc-123")
        pb.tick(1)
        await until(lambda: rec.last.setlist_id == "abc-123")
        await client.stop()


async def test_stall_keeps_socket_open_until_heartbeat_timeout() -> None:
    rec = Recorder()
    with FakePlayback() as pb:
        client = make_client(ScriptedFinder(pb), rec, delays=(30,), timeout=0.3)
        await client.start()
        await until(lambda: bool(rec.statuses) and rec.last.connected)
        pb.stall()
        pb.tick(5)  # nothing is sent
        assert pb.connections == 1
        await until(
            lambda: bool(rec.statuses) and not rec.last.connected
        )  # 0.3 s heartbeat timeout fires
        await client.stop()
