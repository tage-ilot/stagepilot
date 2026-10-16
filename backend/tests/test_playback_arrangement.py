"""Skipping songs on either side: the order never changes, only who pairs with whom."""

from datetime import date
from unittest.mock import AsyncMock

import pytest
from pydantic import ValidationError

from stagepilot.core.config import PlaybackApiSettings
from stagepilot.core.state import StateStore
from stagepilot.models.state import ApplicationState, ServicePlan, Song
from stagepilot.plugins.playback_api.arrangement import align_lengths, pair_songs, plan_index_for
from stagepilot.plugins.playback_api.mapping import PlaybackMapper
from stagepilot.plugins.playback_api.normalizer import Heartbeat, PlaybackEvent

# The owner's example: five plan songs, five Playback songs; plan song 4 is acoustic (no track)
# and Playback song 5 is background music that is not in the service.
PLAN = ["a", "b", "c", "d", "e"]
ORDER = [900, 17, 42, 55, 70]
SKIP_PLAN = {"d"}
SKIP_PB = {70}


def test_without_skips_nothing_changes() -> None:
    assert pair_songs(PLAN, ORDER) == [(0, 0), (1, 1), (2, 2), (3, 3), (4, 4)]
    assert [plan_index_for(s, PLAN, ORDER) for s in ORDER] == [0, 1, 2, 3, 4]


def test_the_owners_example_pairs_around_both_skips() -> None:
    assert pair_songs(PLAN, ORDER, SKIP_PLAN, SKIP_PB) == [(0, 0), (1, 1), (2, 2), (4, 3)]
    assert plan_index_for(55, PLAN, ORDER, SKIP_PLAN, SKIP_PB) == 4  # 4th track drives plan song 5
    assert plan_index_for(70, PLAN, ORDER, SKIP_PLAN, SKIP_PB) is None  # background music


def test_pairs_never_cross_or_reorder() -> None:
    for plan_skips in [set(), {"a"}, {"b", "d"}, {"c"}]:
        for pb_skips in [set(), {900}, {17, 55}, {70}]:
            pairs = pair_songs(PLAN, ORDER, plan_skips, pb_skips)
            assert [p for p, _ in pairs] == sorted(p for p, _ in pairs)
            assert [b for _, b in pairs] == sorted(b for _, b in pairs)
            assert len({p for p, _ in pairs}) == len(pairs) == len({b for _, b in pairs})


def test_a_song_with_no_partner_drives_nothing() -> None:
    # Skip a plan song but nothing in Playback: the last Playback song has no plan song left.
    assert plan_index_for(70, PLAN, ORDER, {"a"}, set()) is None
    assert plan_index_for(900, PLAN, ORDER, {"a"}, set()) == 1


def test_unknown_song_and_missing_plan_drive_nothing() -> None:
    assert plan_index_for(1, PLAN, ORDER, SKIP_PLAN, SKIP_PB) is None
    assert plan_index_for(900, None, ORDER, SKIP_PLAN, SKIP_PB) is None


def test_a_plan_that_lost_a_skipped_song_is_not_trusted() -> None:
    assert plan_index_for(55, ["a", "b", "c", "e"], ORDER, SKIP_PLAN, SKIP_PB) is None


def test_lengths_are_laid_out_by_plan_song() -> None:
    lengths = [418.0, 312.0, 535.0, 298.0, 606.0]
    assert align_lengths(PLAN, ORDER, lengths, SKIP_PLAN, SKIP_PB) == [
        418.0,
        312.0,
        535.0,
        None,
        298.0,
    ]
    assert align_lengths(PLAN, ORDER, lengths) == lengths


def test_settings_accept_skips_and_reject_nonsense() -> None:
    ok = PlaybackApiSettings(song_order=ORDER, skipped_song_ids=[70], skipped_plan_song_ids=["d"])
    assert ok.skipped_song_ids == [70]
    with pytest.raises(ValidationError):
        PlaybackApiSettings(song_order=ORDER, skipped_song_ids=[1])  # not in the order
    with pytest.raises(ValidationError):
        PlaybackApiSettings(song_order=ORDER, skipped_song_ids=[70, 70])
    with pytest.raises(ValidationError):
        PlaybackApiSettings(skipped_plan_song_ids=["d", "d"])


def test_a_new_scan_clears_playback_skips_but_keeps_plan_skips() -> None:
    saved = PlaybackApiSettings(
        song_order=ORDER, skipped_song_ids=[70], skipped_plan_song_ids=["d"]
    )
    scanned = saved.model_copy(update={"song_order": [1, 2, 3]})
    assert scanned.skipped_song_ids == []
    assert scanned.skipped_plan_song_ids == ["d"]


def plan() -> ServicePlan:
    return ServicePlan(
        id="p",
        title="Sunday",
        date=date(2026, 10, 18),
        service_type="Sunday",
        songs=[
            Song(id=song_id, title=f"Song {song_id}", order=i + 1) for i, song_id in enumerate(PLAN)
        ],
    )


def mapper_with_plan(
    skip_pb: set[int], skip_plan: set[str], loaded: ServicePlan | None
) -> tuple[PlaybackMapper, AsyncMock, StateStore]:
    dispatcher = AsyncMock()
    store = StateStore(ApplicationState(plan=loaded))
    mapper = PlaybackMapper(dispatcher, store)
    mapper.install_order(tuple(ORDER), 8)
    mapper.set_skips(tuple(skip_pb), tuple(skip_plan))
    return mapper, dispatcher, store


async def start(mapper: PlaybackMapper, song: int) -> None:
    events = mapper.observe(
        Heartbeat(song, 0, True, False, 8), (PlaybackEvent("song.started", 1, song),)
    )
    for event in events:
        await mapper.dispatch(event)


async def test_starting_the_fourth_track_starts_the_fifth_plan_song() -> None:
    mapper, dispatcher, _ = mapper_with_plan(SKIP_PB, SKIP_PLAN, plan())
    await start(mapper, 55)
    dispatcher.dispatch_song_position.assert_awaited_once_with(5, source="playback_api")


async def test_background_music_leaves_the_timer_alone() -> None:
    mapper, dispatcher, _ = mapper_with_plan(SKIP_PB, SKIP_PLAN, plan())
    await start(mapper, 70)
    dispatcher.dispatch_song_position.assert_not_awaited()
    dispatcher.dispatch.assert_not_awaited()


async def test_a_plan_that_no_longer_fits_holds_cues_back() -> None:
    changed = plan().model_copy(update={"songs": plan().songs[:3]})  # "d" is gone
    mapper, dispatcher, _ = mapper_with_plan(SKIP_PB, SKIP_PLAN, changed)
    await start(mapper, 17)
    dispatcher.dispatch_song_position.assert_not_awaited()


async def test_no_skips_keeps_the_original_behaviour_even_without_a_plan() -> None:
    mapper, dispatcher, _ = mapper_with_plan(set(), set(), None)
    await start(mapper, 17)
    dispatcher.dispatch_song_position.assert_awaited_once_with(2, source="playback_api")


async def test_changing_the_arrangement_drops_queued_cues() -> None:
    mapper, dispatcher, _ = mapper_with_plan(set(), set(), plan())
    queued = mapper.observe(
        Heartbeat(55, 0, True, False, 8), (PlaybackEvent("song.started", 1, 55),)
    )
    mapper.set_skips((70,), ("d",))
    for event in queued:
        await mapper.dispatch(event)
    dispatcher.dispatch_song_position.assert_not_awaited()


# --- through the real API, with the fake Playback harness ---------------------------------------
from test_playback_integration import FakePlayback, application, controller  # noqa: E402
from test_playback_lengths import _connect_pc, _scanned  # noqa: E402

BASE = "/api/v1/playback-api"


async def test_status_reports_plan_song_ids_and_no_skips_by_default() -> None:
    async with application(FakePlayback(index=0)) as (app, client):
        await _scanned(client, app)
        data = (await client.get(f"{BASE}/status")).json()
        assert data["skipped_song_ids"] == [] and data["skipped_plan_song_ids"] == []
        assert data["arrangement_current"] is True
        assert all(song["id"] for song in data["plan_songs"])


async def test_saving_skips_persists_reports_and_applies_them() -> None:
    async with application(FakePlayback(index=0)) as (app, client):
        await _scanned(client, app)
        inputs = controller(app)
        order = list(inputs.mapper.song_order)
        plan_ids = [s["id"] for s in (await client.get(f"{BASE}/status")).json()["plan_songs"]]
        reply = await client.put(
            f"{BASE}/arrangement",
            json={"skipped_song_ids": [order[-1]], "skipped_plan_song_ids": [plan_ids[0]]},
        )
        assert reply.status_code == 200, reply.text
        data = reply.json()
        assert data["skipped_song_ids"] == [order[-1]]
        assert data["skipped_plan_song_ids"] == [plan_ids[0]]
        assert inputs.mapper.skipped_song_ids == {order[-1]}
        assert inputs.settings.playback_api.skipped_plan_song_ids == [plan_ids[0]]
        # Order, version and lengths are untouched by choosing skips.
        assert data["song_order"] == order


async def test_an_unknown_song_is_refused_and_nothing_is_saved() -> None:
    async with application(FakePlayback(index=0)) as (app, client):
        await _scanned(client, app)
        bad = await client.put(
            f"{BASE}/arrangement",
            json={"skipped_song_ids": [123456789], "skipped_plan_song_ids": []},
        )
        assert bad.status_code == 422
        worse = await client.put(
            f"{BASE}/arrangement", json={"skipped_song_ids": [], "skipped_plan_song_ids": ["nope"]}
        )
        assert worse.status_code == 409
        dup = await client.put(
            f"{BASE}/arrangement",
            json={"skipped_song_ids": [], "skipped_plan_song_ids": ["x", "x"]},
        )
        assert dup.status_code == 422
        assert (await client.get(f"{BASE}/status")).json()["skipped_song_ids"] == []


async def test_skips_need_a_scan_first() -> None:
    async with application(FakePlayback(index=0)) as (app, client):
        await _connect_pc(app)
        reply = await client.put(
            f"{BASE}/arrangement", json={"skipped_song_ids": [], "skipped_plan_song_ids": []}
        )
        assert reply.status_code == 409


async def test_the_arrangement_endpoint_accepts_nothing_else() -> None:
    async with application(FakePlayback(index=0)) as (app, client):
        await _scanned(client, app)
        reply = await client.put(f"{BASE}/arrangement", json={"song_order": [1, 2, 3]})
        assert reply.status_code == 422


async def test_a_plan_that_lost_a_skipped_song_is_flagged_not_silently_applied() -> None:
    async with application(FakePlayback(index=0)) as (app, client):
        await _scanned(client, app)
        status = (await client.get(f"{BASE}/status")).json()
        gone = status["plan_songs"][0]["id"]
        await client.put(
            f"{BASE}/arrangement", json={"skipped_song_ids": [], "skipped_plan_song_ids": [gone]}
        )
        state = app.state.runtime.state_store
        await state.mutate(
            lambda s: setattr(s, "plan", s.plan.model_copy(update={"songs": s.plan.songs[1:]}))
        )
        data = (await client.get(f"{BASE}/status")).json()
        assert data["arrangement_current"] is False
