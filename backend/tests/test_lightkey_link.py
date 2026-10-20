"""Lightkey link: connect, record against a song, save, replay, and stand back for the operator.

A fake ``lightkey-api`` (httpx MockTransport) stands in for the Lightkey Mac. It reports cue
changes with a timestamp on ITS clock, which runs a fixed offset ahead of ours, so the clock-offset
conversion is exercised too.
"""

from __future__ import annotations

import asyncio
import time
from datetime import UTC, datetime, timedelta
from typing import Any

import httpx
import pytest

from stagepilot.core.config import LightkeySettings, OscArgument
from stagepilot.core.event_bus import EventBus
from stagepilot.core.events import EventType, SongPayload, TimerPayload, new_event
from stagepilot.core.lightkey import LightkeyEdge, LightkeyTake
from stagepilot.models.state import Song
from stagepilot.plugins.lights.lightkey import FileTakeStore, LightkeyLink, MemoryTakeStore

OFFSET_MS = 5_000.0  # the Lightkey Mac's clock runs this far ahead of ours


class FakeLightkeyApi:
    def __init__(self, active: list[str]) -> None:
        self.online = True
        self.active = sorted(active)
        self.seq = 1
        self.events: list[dict[str, Any]] = []
        self.down = False
        self.status_code = 200

    def now_ms(self) -> float:
        return time.time() * 1000 + OFFSET_MS

    def change(self, added: list[str] = (), removed: list[str] = ()) -> None:  # type: ignore[assignment]
        self.active = sorted((set(self.active) - set(removed)) | set(added))
        self.seq += 1
        self.events.append(
            {
                "seq": self.seq,
                "online": True,
                "added": sorted(added),
                "removed": sorted(removed),
                "effective_ms": self.now_ms(),
                "effective_src": "dmx",
            }
        )

    def handler(self, request: httpx.Request) -> httpx.Response:
        if self.down:
            raise httpx.ConnectError("down")
        if self.status_code != 200:
            return httpx.Response(self.status_code, json={"error": "no"})
        if request.url.path == "/time":
            return httpx.Response(200, json={"t": self.now_ms()})
        since = int(request.url.params.get("since", 0))
        state = {
            "online": self.online,
            "active": self.active if self.online else None,
            "seq": self.seq,
            "server_time": self.now_ms(),
        }
        events = [e for e in self.events if e["seq"] > since]
        return httpx.Response(200, json={"events": events, "state": state})


class Rig:
    def __init__(self, api: FakeLightkeyApi, **settings: Any) -> None:
        self.api = api
        self.bus = EventBus()
        self.sent: list[tuple[float, str]] = []
        self.settings: tuple[bool, bool, str, LightkeySettings] = (
            True,
            True,
            "10.30.0.211",
            LightkeySettings(**settings),
        )
        self.store = MemoryTakeStore()
        self.t0 = time.monotonic()

        async def send(address: str, _args: list[OscArgument]) -> None:
            self.sent.append((time.monotonic() - self.t0, address))

        self.link = LightkeyLink(
            self.bus,
            lambda: self.settings,
            send,
            self.store,
            client_factory=lambda: httpx.AsyncClient(
                transport=httpx.MockTransport(api.handler), timeout=2
            ),
        )

    async def connect(self) -> None:
        await self.link.start()
        await wait_for(lambda: self.link.status().link == "connected")

    async def song_start(self, song: Song, *, at: datetime | None = None) -> None:
        await self.bus.publish(
            new_event(EventType.SONG_STARTED, source="t", payload=SongPayload(song=song, index=0))
        )
        await self.bus.publish(
            new_event(
                EventType.TIMER_STARTED,
                source="t",
                payload=TimerPayload(
                    duration_seconds=int(song.duration_seconds or 0),
                    song=song,
                    started_at=at or datetime.now(UTC),
                ),
            )
        )

    async def song_stop(self, song: Song, position: float) -> None:
        await self.bus.publish(
            new_event(
                EventType.TIMER_STOPPED,
                source="t",
                payload=TimerPayload(
                    duration_seconds=0, song=song, position_seconds=position, remaining_seconds=0
                ),
            )
        )


async def wait_for(check: Any, timeout: float = 3.0) -> None:  # noqa: ASYNC109
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        if check():
            return
        await asyncio.sleep(0.02)
    raise AssertionError("condition not reached in time")


def song(duration: int = 10) -> Song:
    return Song(
        id="item-1",
        source_song_id="song-1",
        title="Holy Forever",
        duration_seconds=duration,
        order=1,
    )


@pytest.mark.asyncio
async def test_connects_and_reports_status() -> None:
    rig = Rig(FakeLightkeyApi(["A", "B"]))
    await rig.connect()
    status = rig.link.status()
    assert status.link == "connected"
    assert status.active_count == 2
    assert status.api_url == "http://10.30.0.211:8770"
    assert status.replay is True
    await rig.link.stop()


@pytest.mark.asyncio
async def test_unreachable_then_recovers() -> None:
    api = FakeLightkeyApi(["A"])
    api.down = True
    rig = Rig(api)
    await rig.link.start()
    await wait_for(lambda: rig.link.status().link == "unreachable")
    assert rig.link.status().detail == "Cannot reach the Lightkey API."
    api.down = False
    rig.link.settings_changed()
    await wait_for(lambda: rig.link.status().link == "connected", timeout=5)
    await rig.link.stop()


@pytest.mark.asyncio
async def test_refused_address_says_how_to_fix_it() -> None:
    api = FakeLightkeyApi(["A"])
    api.status_code = 403
    rig = Rig(api)
    await rig.link.start()
    await wait_for(lambda: rig.link.status().link == "unreachable")
    assert "refused" in (rig.link.status().detail or "")
    await rig.link.stop()


@pytest.mark.asyncio
async def test_lightkey_closed_is_its_own_state() -> None:
    api = FakeLightkeyApi(["A"])
    api.online = False
    rig = Rig(api)
    await rig.link.start()
    await wait_for(lambda: rig.link.status().link == "lightkey_closed")
    await rig.link.stop()


@pytest.mark.asyncio
async def test_cannot_record_when_not_connected_or_not_osc() -> None:
    api = FakeLightkeyApi(["A"])
    api.down = True
    rig = Rig(api)
    await rig.link.start()
    await wait_for(lambda: rig.link.status().link == "unreachable")
    assert "Connect to Lightkey first" in await rig.link.set_recording(True)
    assert rig.link.status().recording is False
    api.down = False
    rig.link.settings_changed()
    await wait_for(lambda: rig.link.status().link == "connected", timeout=5)
    rig.settings = (True, False, "x", LightkeySettings())
    assert "OSC" in await rig.link.set_recording(True)
    assert rig.link.status().recording is False
    await rig.link.stop()


@pytest.mark.asyncio
async def test_records_changes_with_song_times_and_saves_them() -> None:
    api = FakeLightkeyApi(["Warm"])
    rig = Rig(api)
    await rig.connect()
    await rig.link.set_recording(True)
    assert rig.link.status().recording is True
    s = song(10)
    await rig.song_start(s)
    await asyncio.sleep(0.5)
    api.change(added=["Stars"])
    await asyncio.sleep(0.6)
    api.change(added=["Strobe"], removed=["Stars"])
    await asyncio.sleep(0.5)
    await rig.song_stop(s, position=10.0)  # played to the end
    await wait_for(lambda: rig.link.has_take("song-1"))
    take = rig.store.takes["song-1"]
    assert take.initial == ["Warm"]
    assert [(e.added, e.removed) for e in take.edges] == [(["Stars"], []), (["Strobe"], ["Stars"])]
    # Times are song time on OUR clock: the 5 s Lightkey-clock offset must have been removed.
    assert take.edges[0].at_seconds == pytest.approx(0.5, abs=0.25)
    assert take.edges[1].at_seconds == pytest.approx(1.1, abs=0.25)
    assert "Saved" in (rig.link.status().message or "")
    await rig.link.stop()


@pytest.mark.asyncio
async def test_short_recording_does_not_overwrite_a_good_take() -> None:
    rig = Rig(FakeLightkeyApi(["A"]))
    good = LightkeyTake(
        song_key="song-1",
        song_title="Holy Forever",
        recorded_at=datetime.now(UTC),
        duration_seconds=10,
        initial=["A"],
        edges=[LightkeyEdge(at_seconds=2, added=["B"])],
    )
    rig.store.takes = {"song-1": good}
    await rig.connect()
    await rig.link.set_recording(True)
    s = song(10)
    await rig.song_start(s)
    await rig.song_stop(s, position=1.0)  # 10% of the song
    await asyncio.sleep(0.1)
    assert rig.store.takes["song-1"] == good
    assert "Not saved" in (rig.link.status().message or "") or rig.link.status().recording
    await rig.link.stop()


@pytest.mark.asyncio
async def test_seek_while_recording_discards_the_take() -> None:
    rig = Rig(FakeLightkeyApi(["A"]))
    await rig.connect()
    await rig.link.set_recording(True)
    s = song(10)
    await rig.song_start(s)
    await rig.bus.publish(
        new_event(
            EventType.TIMER_SEEKED,
            source="t",
            payload=TimerPayload(
                duration_seconds=3,
                song=s,
                position_seconds=7.0,
                remaining_seconds=3,
                running=True,
                started_at=datetime.now(UTC),
            ),
        )
    )
    await asyncio.sleep(0.1)
    assert rig.link.status().recording_song is None
    assert "moved" in (rig.link.status().message or "")
    assert not rig.store.takes
    await rig.link.stop()


@pytest.mark.asyncio
async def test_recording_turned_off_mid_song_saves_if_enough_was_captured() -> None:
    rig = Rig(FakeLightkeyApi(["A"]))
    await rig.connect()
    await rig.link.set_recording(True)
    s = song(2)
    await rig.song_start(s, at=datetime.now(UTC) - timedelta(seconds=1.5))
    await asyncio.sleep(0.05)
    await rig.link.set_recording(False)
    assert rig.link.has_take("song-1")
    await rig.link.stop()


def stored_take() -> LightkeyTake:
    return LightkeyTake(
        song_key="song-1",
        song_title="Holy Forever",
        recorded_at=datetime.now(UTC),
        duration_seconds=10,
        initial=["Warm"],
        edges=[
            LightkeyEdge(at_seconds=0.5, added=["Stars"]),
            LightkeyEdge(at_seconds=1.0, added=["Strobe"], removed=["Stars"]),
        ],
    )


@pytest.mark.asyncio
async def test_replay_syncs_then_fires_edges_slightly_early() -> None:
    rig = Rig(FakeLightkeyApi(["Warm"]))
    rig.store.takes = {"song-1": stored_take()}
    await rig.connect()
    rig.t0 = time.monotonic()
    await rig.song_start(song(10))
    await wait_for(lambda: len(rig.sent) >= 6, timeout=4)
    assert [address for _t, address in rig.sent] == [
        "/live/*/cue/Stars/deactivate",
        "/live/*/cue/Strobe/deactivate",
        "/live/*/cue/Warm/activate",
        "/live/*/cue/Stars/activate",
        "/live/*/cue/Stars/deactivate",
        "/live/*/cue/Strobe/activate",
    ]
    assert rig.sent[3][0] == pytest.approx(0.46, abs=0.15)  # 0.5 s edge, 40 ms lead
    assert rig.sent[4][0] == pytest.approx(0.96, abs=0.15)
    assert rig.link.status().replaying_song == "Holy Forever"
    await rig.link.stop()


@pytest.mark.asyncio
async def test_replay_switch_off_sends_nothing() -> None:
    rig = Rig(FakeLightkeyApi(["Warm"]), replay=False)
    rig.store.takes = {"song-1": stored_take()}
    await rig.connect()
    await rig.song_start(song(10))
    await asyncio.sleep(0.8)
    assert rig.sent == []
    assert rig.link.status().replay_blocked == "Replay is switched off."
    await rig.link.stop()


@pytest.mark.asyncio
async def test_replay_does_not_run_while_recording_armed() -> None:
    rig = Rig(FakeLightkeyApi(["Warm"]))
    rig.store.takes = {"song-1": stored_take()}
    await rig.connect()
    await rig.link.set_recording(True)
    await rig.song_start(song(10))
    await asyncio.sleep(0.8)
    assert rig.sent == []
    assert rig.link.status().replay_blocked == "Paused while recording."
    await rig.link.stop()


@pytest.mark.asyncio
async def test_replay_stands_back_from_a_cue_the_operator_just_changed() -> None:
    api = FakeLightkeyApi(["Warm"])
    rig = Rig(api)
    rig.store.takes = {"song-1": stored_take()}
    await rig.connect()
    # The operator grabs "Strobe" shortly before replay would turn it on.
    api.change(added=["Strobe"])
    await asyncio.sleep(0.6)
    await rig.song_start(song(10))
    await asyncio.sleep(1.4)
    addresses = [a for _t, a in rig.sent]
    assert not any("Strobe" in a for a in addresses)  # held, so skipped entirely
    assert "/live/*/cue/Stars/activate" in addresses  # untouched cues still follow the take
    await rig.link.stop()


@pytest.mark.asyncio
async def test_stop_cancels_remaining_replay() -> None:
    rig = Rig(FakeLightkeyApi(["Warm"]))
    rig.store.takes = {"song-1": stored_take()}
    await rig.connect()
    s = song(10)
    await rig.song_start(s)
    await asyncio.sleep(0.2)
    await rig.song_stop(s, position=0.2)
    count = len(rig.sent)
    await asyncio.sleep(1.0)
    assert len(rig.sent) == count
    assert rig.link.status().replaying_song is None
    await rig.link.stop()


@pytest.mark.asyncio
async def test_seek_replays_from_the_new_position() -> None:
    rig = Rig(FakeLightkeyApi(["Warm"]))
    rig.store.takes = {"song-1": stored_take()}
    await rig.connect()
    s = song(10)
    await rig.bus.publish(
        new_event(
            EventType.TIMER_SEEKED,
            source="t",
            payload=TimerPayload(
                duration_seconds=5,
                song=s,
                position_seconds=5.0,
                remaining_seconds=5,
                running=True,
                started_at=datetime.now(UTC),
            ),
        )
    )
    await wait_for(lambda: len(rig.sent) >= 3)
    # At 5 s the take says: Warm and Strobe on, Stars off.
    assert sorted(rig.sent[i][1] for i in range(3)) == [
        "/live/*/cue/Stars/deactivate",
        "/live/*/cue/Strobe/activate",
        "/live/*/cue/Warm/activate",
    ]
    await asyncio.sleep(0.3)
    assert len(rig.sent) == 3  # nothing left to schedule after 5 s
    await rig.link.stop()


@pytest.mark.asyncio
async def test_agent_restart_resets_the_sequence_without_a_false_change() -> None:
    api = FakeLightkeyApi(["A"])
    api.seq = 40
    rig = Rig(api)
    await rig.connect()
    api.seq = 1  # agent restarted
    api.active = ["A", "B"]
    await asyncio.sleep(0.8)
    assert rig.link.status().active_count == 2
    assert rig.link.status().link == "connected"
    await rig.link.stop()


def test_file_store_round_trip_and_bad_file(tmp_path: Any) -> None:
    store = FileTakeStore(tmp_path / "t.json")
    assert store.load() == {}
    store.save({"song-1": stored_take()})
    assert store.load()["song-1"].edges[1].removed == ["Stars"]
    (tmp_path / "t.json").write_text("{not json")
    assert store.load() == {}
    assert (tmp_path / "t.json.bad").exists()
