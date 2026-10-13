from __future__ import annotations

from collections.abc import AsyncIterator
from datetime import UTC, date, datetime, timedelta

import pytest
import pytest_asyncio

from stagepilot.core.event_bus import EventBus
from stagepilot.core.events import (
    ActionName,
    ConnectionPayload,
    EventType,
    ServiceLoadPayload,
    ServicePayload,
    StagePilotEvent,
    TimerPayload,
    new_event,
)
from stagepilot.core.state import StateStore
from stagepilot.models.state import (
    ConnectionStatus,
    ServiceLoadStatus,
    ServicePlan,
    Song,
    TimerStatus,
)
from stagepilot.services.state_service import StateService


def make_plan(
    *songs: Song,
    plan_id: str = "plan-1",
) -> ServicePlan:
    return ServicePlan(
        id=plan_id,
        title="Test Service",
        date=date(2026, 7, 12),
        service_type="Weekend",
        service_times=["09:00"],
        songs=list(songs),
    )


def song(
    identifier: str,
    title: str,
    order: int,
    duration_seconds: int | None = 240,
) -> Song:
    return Song(
        id=identifier,
        title=title,
        order=order,
        duration_seconds=duration_seconds,
    )


@pytest_asyncio.fixture
async def service() -> AsyncIterator[tuple[EventBus, StateStore, StateService]]:
    bus = EventBus()
    store = StateStore()
    state_service = StateService(bus, store)
    await state_service.start()
    try:
        yield bus, store, state_service
    finally:
        await state_service.stop()


async def load(bus: EventBus, plan: ServicePlan) -> None:
    await bus.publish(
        new_event(
            EventType.SERVICE_LOADED,
            source="test",
            payload=ServicePayload(plan=plan),
        )
    )


@pytest.mark.asyncio
async def test_demo_events_cannot_impersonate_real_integration_connections(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, _state_service = service

    for integration in ("planning_center", "midi", "propresenter"):
        await bus.publish(
            new_event(
                EventType.CONNECTION_CHANGED,
                source="demo",
                payload=ConnectionPayload(
                    integration=integration,
                    status=ConnectionStatus.CONNECTED,
                    detail="simulated",
                ),
            )
        )

    snapshot = await store.snapshot()
    assert snapshot.planning_center_status is ConnectionStatus.DISCONNECTED
    assert snapshot.midi_status is ConnectionStatus.DISCONNECTED
    assert snapshot.propresenter_status is ConnectionStatus.DISCONNECTED


@pytest.mark.asyncio
async def test_start_next_song_selects_first_then_advances(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, state_service = service
    await load(bus, make_plan(song("a", "Alpha", 1), song("b", "Beta", 2)))
    started: list[StagePilotEvent] = []
    await bus.subscribe(EventType.SONG_STARTED, started.append)

    first = await state_service.dispatch(ActionName.START_NEXT)
    first_state = await store.snapshot()
    second = await state_service.dispatch(ActionName.START_NEXT)
    second_state = await store.snapshot()

    assert first.accepted is True
    assert first_state.current_song and first_state.current_song.title == "Alpha"
    assert first_state.next_song and first_state.next_song.title == "Beta"
    assert second.accepted is True
    assert second_state.current_song and second_state.current_song.title == "Beta"
    assert second_state.next_song is None
    assert len(started) == 2


@pytest.mark.asyncio
async def test_restart_current_song_does_not_advance(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, state_service = service
    await load(bus, make_plan(song("a", "Alpha", 1), song("b", "Beta", 2)))
    await state_service.dispatch(ActionName.START_NEXT)
    restarted: list[StagePilotEvent] = []
    await bus.subscribe(EventType.SONG_RESTARTED, restarted.append)

    outcome = await state_service.dispatch(ActionName.RESTART_CURRENT)
    state = await store.snapshot()

    assert outcome.accepted is True
    assert state.current_song_index == 0
    assert state.current_song and state.current_song.title == "Alpha"
    assert len(restarted) == 1


@pytest.mark.asyncio
async def test_song_position_dispatch_starts_the_exact_requested_song(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, state_service = service
    await load(
        bus,
        make_plan(
            song("a", "Alpha", 1),
            song("b", "Beta", 2),
            song("c", "Gamma", 3),
        ),
    )
    started: list[StagePilotEvent] = []
    await bus.subscribe(EventType.SONG_STARTED, started.append)

    outcome = await state_service.dispatch_song_position(3, source="midi_playback")
    state = await store.snapshot()

    assert outcome.accepted is True
    assert outcome.message == 'Started song 3, "Gamma".'
    assert state.current_song_index == 2
    assert state.current_song and state.current_song.title == "Gamma"
    assert state.next_song is None
    assert len(started) == 1
    assert started[0].source == "midi_playback"


@pytest.mark.asyncio
async def test_song_position_dispatch_rejects_a_velocity_outside_the_loaded_plan(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, state_service = service
    await load(bus, make_plan(song("a", "Alpha", 1), song("b", "Beta", 2)))

    outcome = await state_service.dispatch_song_position(3, source="midi_playback")
    state = await store.snapshot()

    assert outcome.accepted is False
    assert outcome.message == "Song velocity 3 does not match a loaded song (1-2)."
    assert state.current_song is None
    assert state.current_song_index is None


@pytest.mark.asyncio
async def test_previous_selects_song_without_starting_it(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, state_service = service
    await load(bus, make_plan(song("a", "Alpha", 1), song("b", "Beta", 2)))
    await state_service.dispatch(ActionName.START_NEXT)
    await state_service.dispatch(ActionName.START_NEXT)
    started: list[StagePilotEvent] = []
    await bus.subscribe(EventType.SONG_STARTED, started.append)

    outcome = await state_service.dispatch(ActionName.PREVIOUS)
    state = await store.snapshot()

    assert outcome.accepted is True
    assert state.current_song_index == 0
    assert state.current_song and state.current_song.title == "Alpha"
    assert started == []


@pytest.mark.asyncio
async def test_next_selects_then_start_next_starts_that_selection(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, state_service = service
    await load(bus, make_plan(song("a", "Alpha", 1), song("b", "Beta", 2)))

    selected = await state_service.dispatch(ActionName.NEXT)
    selected_state = await store.snapshot()
    started = await state_service.dispatch(ActionName.START_NEXT)

    assert selected.accepted is True
    assert selected_state.current_song_index == 0
    assert started.accepted is True
    assert (await store.snapshot()).current_song_index == 0


@pytest.mark.asyncio
async def test_first_and_last_song_bounds_are_safe(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, state_service = service
    await load(bus, make_plan(song("a", "Only", 1)))

    before_first = await state_service.dispatch(ActionName.PREVIOUS)
    await state_service.dispatch(ActionName.START_NEXT)
    after_last_next = await state_service.dispatch(ActionName.NEXT)
    after_last_start = await state_service.dispatch(ActionName.START_NEXT)

    assert before_first.accepted is False
    assert after_last_next.accepted is False
    assert after_last_start.accepted is False
    assert (await store.snapshot()).current_song_index == 0


@pytest.mark.asyncio
async def test_empty_plan_actions_are_rejected_without_mutating_position(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, state_service = service
    await load(bus, make_plan())

    for action in (
        ActionName.START_NEXT,
        ActionName.RESTART_CURRENT,
        ActionName.PREVIOUS,
        ActionName.NEXT,
    ):
        assert (await state_service.dispatch(action)).accepted is False

    state = await store.snapshot()
    assert state.current_song is None
    assert state.next_song is None


@pytest.mark.asyncio
async def test_plan_reload_preserves_current_song_by_stable_id(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, state_service = service
    await load(bus, make_plan(song("a", "Alpha", 1), song("b", "Beta", 2)))
    await state_service.dispatch(ActionName.START_NEXT)

    await load(
        bus,
        make_plan(
            song("b", "Beta", 1),
            song("a", "Alpha", 2),
            plan_id="plan-reloaded",
        ),
    )
    state = await store.snapshot()

    assert state.current_song and state.current_song.id == "a"
    assert state.current_song_index == 1
    assert state.last_successful_plan_reload_at is not None


@pytest.mark.asyncio
async def test_plan_reload_resets_ambiguous_duplicate_song_names(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, state_service = service
    await load(bus, make_plan(song("old-1", "Repeat", 1), song("old-2", "Repeat", 2)))
    await state_service.dispatch(ActionName.START_NEXT)

    await load(
        bus,
        make_plan(
            song("new-1", "Repeat", 1),
            song("new-2", "Repeat", 2),
            plan_id="plan-reloaded",
        ),
    )
    state = await store.snapshot()

    assert state.current_song is None
    assert state.current_song_index is None
    assert state.next_song and state.next_song.id == "new-1"


@pytest.mark.asyncio
@pytest.mark.parametrize("duration_seconds", [None, 0])
async def test_missing_or_zero_duration_never_starts_timer(
    service: tuple[EventBus, StateStore, StateService],
    duration_seconds: int | None,
) -> None:
    bus, store, state_service = service
    await load(bus, make_plan(song("a", "No Duration", 1, duration_seconds)))
    started: list[StagePilotEvent] = []
    await bus.subscribe(EventType.SONG_STARTED, started.append)

    outcome = await state_service.dispatch(ActionName.START_NEXT)
    state = await store.snapshot()

    assert outcome.accepted is False
    assert state.current_song and state.current_song.title == "No Duration"
    assert state.timer.status is TimerStatus.ERROR
    assert state.recent_errors
    assert started == []


@pytest.mark.asyncio
async def test_reset_position_returns_to_pre_service_state(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, state_service = service
    reset_requests: list[StagePilotEvent] = []
    await bus.subscribe(EventType.TIMER_RESET_REQUESTED, reset_requests.append)
    await load(bus, make_plan(song("a", "Alpha", 1), song("b", "Beta", 2)))
    await state_service.dispatch(ActionName.START_NEXT)

    outcome = await state_service.dispatch(ActionName.RESET_POSITION)
    state = await store.snapshot()

    assert outcome.accepted is True
    assert state.current_song is None
    assert state.current_song_index is None
    assert state.next_song and state.next_song.id == "a"
    assert state.timer.status is TimerStatus.STOPPED
    assert len(reset_requests) == 1
    assert outcome.message == "Service position and timer reset."


@pytest.mark.asyncio
async def test_date_rollover_clears_plan_and_resets_running_timer(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, _state_service = service
    stop_requests: list[StagePilotEvent] = []
    await bus.subscribe(EventType.TIMER_STOP_REQUESTED, stop_requests.append)
    await load(bus, make_plan(song("a", "Alpha", 1)))
    await bus.publish(
        new_event(
            EventType.TIMER_STARTED,
            source="test",
            payload=TimerPayload(duration_seconds=240),
        )
    )

    await bus.publish(
        new_event(
            EventType.SERVICE_LOAD_CHANGED,
            source="test",
            payload=ServiceLoadPayload(
                status=ServiceLoadStatus.ERROR,
                target_date=date(2026, 7, 13),
                message="The new service could not be loaded.",
            ),
        )
    )
    state = await store.snapshot()

    assert state.plan is None
    assert state.current_song is None
    assert state.current_song_index is None
    assert state.next_song is None
    assert state.timer.status is TimerStatus.STOPPED
    assert state.timer.duration_seconds is None
    assert state.timer.started_at is None
    assert len(stop_requests) == 1


@pytest.mark.asyncio
async def test_same_day_stale_load_keeps_plan_and_running_timer(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, _state_service = service
    plan = make_plan(song("a", "Alpha", 1))
    await load(bus, plan)
    await bus.publish(
        new_event(
            EventType.TIMER_STARTED,
            source="test",
            payload=TimerPayload(duration_seconds=240),
        )
    )

    await bus.publish(
        new_event(
            EventType.SERVICE_LOAD_CHANGED,
            source="test",
            payload=ServiceLoadPayload(
                status=ServiceLoadStatus.ERROR,
                target_date=plan.date,
                message="The refresh failed.",
                is_stale=True,
            ),
        )
    )
    state = await store.snapshot()

    assert state.plan == plan
    assert state.service_load.is_stale is True
    assert state.timer.status is TimerStatus.RUNNING
    assert state.timer.duration_seconds == 240


async def start_timer(bus: EventBus, seconds: int, started_at: datetime | None = None) -> None:
    await bus.publish(
        new_event(
            EventType.TIMER_STARTED,
            source="test",
            payload=TimerPayload(duration_seconds=seconds, started_at=started_at),
        )
    )


async def stop_event(bus: EventBus, timestamp: datetime | None = None) -> None:
    event = new_event(
        EventType.TIMER_STOPPED, source="test", payload=TimerPayload(duration_seconds=0)
    )
    if timestamp is not None:
        event = event.model_copy(update={"timestamp": timestamp})
    await bus.publish(event)


@pytest.mark.asyncio
async def test_stop_keeps_remaining_and_position_then_resume_continues(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, state_service = service
    await load(bus, make_plan(song("a", "Alpha", 1, 240)))
    await state_service.dispatch(ActionName.START_NEXT)
    t0 = datetime(2026, 7, 12, 9, 0, tzinfo=UTC)
    await start_timer(bus, 240, t0)
    await stop_event(bus, t0 + timedelta(seconds=60))

    stopped = (await store.snapshot()).timer
    assert stopped.status is TimerStatus.STOPPED
    assert stopped.remaining_seconds == 180
    assert stopped.position_seconds == 60

    resumed: list[StagePilotEvent] = []
    await bus.subscribe(EventType.TIMER_RESUMED, resumed.append)
    outcome = await state_service.resume_timer_at(60.4)
    running = (await store.snapshot()).timer
    assert outcome.accepted is True
    assert running.status is TimerStatus.RUNNING
    assert running.duration_seconds == 180
    assert running.position_seconds == pytest.approx(60.4)
    assert running.remaining_seconds is None
    payload = resumed[0].payload
    assert isinstance(payload, TimerPayload)
    assert payload.remaining_seconds == 180
    assert payload.duration_seconds == 180
    assert payload.position_seconds == pytest.approx(60.4)
    assert payload.running is True
    assert payload.song is not None and payload.song.title == "Alpha"


@pytest.mark.asyncio
async def test_seek_while_running_reapplies_and_keeps_running(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, state_service = service
    await load(bus, make_plan(song("a", "Alpha", 1, 240)))
    await state_service.dispatch(ActionName.START_NEXT)
    await start_timer(bus, 240)
    seeked: list[StagePilotEvent] = []
    await bus.subscribe(EventType.TIMER_SEEKED, seeked.append)

    outcome = await state_service.seek_timer(100)

    timer = (await store.snapshot()).timer
    assert outcome.accepted is True
    assert timer.status is TimerStatus.RUNNING
    assert timer.duration_seconds == 140
    assert timer.position_seconds == 100
    payload = seeked[0].payload
    assert isinstance(payload, TimerPayload)
    assert (payload.remaining_seconds, payload.position_seconds, payload.running) == (
        140,
        100,
        True,
    )


@pytest.mark.asyncio
async def test_seek_while_stopped_updates_frozen_remaining_without_running(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, state_service = service
    await load(bus, make_plan(song("a", "Alpha", 1, 240)))
    await state_service.dispatch(ActionName.START_NEXT)
    await start_timer(bus, 240)
    await stop_event(bus)
    seeked: list[StagePilotEvent] = []
    await bus.subscribe(EventType.TIMER_SEEKED, seeked.append)

    await state_service.seek_timer(200)

    timer = (await store.snapshot()).timer
    assert timer.status is TimerStatus.STOPPED
    assert timer.remaining_seconds == 40
    assert timer.position_seconds == 200
    payload = seeked[0].payload
    assert isinstance(payload, TimerPayload)
    assert payload.running is False and payload.remaining_seconds == 40


@pytest.mark.asyncio
async def test_position_past_the_end_clamps_remaining_at_zero(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, state_service = service
    await load(bus, make_plan(song("a", "Alpha", 1, 240)))
    await state_service.dispatch(ActionName.START_NEXT)
    await start_timer(bus, 240)

    await state_service.seek_timer(999)
    timer = (await store.snapshot()).timer
    assert timer.status is TimerStatus.STOPPED
    assert timer.remaining_seconds == 0

    stops: list[StagePilotEvent] = []
    await bus.subscribe(EventType.TIMER_STOP_REQUESTED, stops.append)
    await state_service.resume_timer_at(999)
    timer = (await store.snapshot()).timer
    assert timer.status is TimerStatus.STOPPED
    assert timer.remaining_seconds == 0
    assert len(stops) == 1


@pytest.mark.asyncio
async def test_seek_to_start_then_resume_runs_the_full_duration(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, state_service = service
    await load(bus, make_plan(song("a", "Alpha", 1, 240)))
    await state_service.dispatch(ActionName.START_NEXT)
    await start_timer(bus, 240)
    await stop_event(bus)
    await state_service.seek_timer(0)
    assert (await store.snapshot()).timer.remaining_seconds == 240
    await state_service.resume_timer_at(0)
    timer = (await store.snapshot()).timer
    assert timer.status is TimerStatus.RUNNING and timer.duration_seconds == 240
    # The "song started" restart path is untouched.
    restarted: list[StagePilotEvent] = []
    await bus.subscribe(EventType.SONG_RESTARTED, restarted.append)
    outcome = await state_service.dispatch(ActionName.RESTART_CURRENT)
    assert outcome.accepted is True and len(restarted) == 1


@pytest.mark.asyncio
async def test_resume_and_seek_are_rejected_safely_without_song_or_duration(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, state_service = service
    assert (await state_service.resume_timer_at(5)).accepted is False
    assert (await state_service.seek_timer(5)).accepted is False
    await load(bus, make_plan(song("a", "Alpha", 1, None)))
    await state_service.dispatch(ActionName.NEXT)
    assert (await state_service.resume_timer_at(5)).accepted is False
    assert (await state_service.seek_timer(float("nan"))).accepted is False
    assert (await state_service.seek_timer(float("inf"))).accepted is False
    assert (await store.snapshot()).timer.status is TimerStatus.IDLE


@pytest.mark.asyncio
async def test_stop_timer_at_freezes_from_playback_position(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, state_service = service
    await load(bus, make_plan(song("a", "Alpha", 1, 240)))
    await state_service.dispatch(ActionName.START_NEXT)
    await start_timer(bus, 240)
    stops: list[StagePilotEvent] = []
    await bus.subscribe(EventType.TIMER_STOP_REQUESTED, stops.append)

    outcome = await state_service.stop_timer_at(90)
    await stop_event(bus)  # a plugin confirming the stop must not blank the value

    timer = (await store.snapshot()).timer
    assert outcome.accepted is True
    assert (timer.status, timer.remaining_seconds, timer.position_seconds) == (
        TimerStatus.STOPPED,
        150,
        90,
    )
    payload = stops[0].payload
    assert isinstance(payload, TimerPayload)
    assert (payload.remaining_seconds, payload.position_seconds) == (150, 90)


@pytest.mark.asyncio
async def test_reset_position_clears_frozen_remaining_and_new_song_drops_it(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, state_service = service
    await load(bus, make_plan(song("a", "Alpha", 1, 240), song("b", "Beta", 2, 200)))
    await state_service.dispatch(ActionName.START_NEXT)
    await start_timer(bus, 240)
    await stop_event(bus)
    await state_service.seek_timer(60)
    assert (await store.snapshot()).timer.remaining_seconds == 180

    await state_service.dispatch(ActionName.NEXT)  # new song while stopped
    timer = (await store.snapshot()).timer
    assert timer.status is TimerStatus.STOPPED and timer.remaining_seconds is None

    await state_service.dispatch(ActionName.PREVIOUS)
    await state_service.seek_timer(60)
    await state_service.dispatch(ActionName.RESET_POSITION)
    timer = (await store.snapshot()).timer
    assert timer.remaining_seconds is None and timer.position_seconds is None


@pytest.mark.asyncio
async def test_manual_stop_keeps_remaining_time(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, state_service = service
    await load(bus, make_plan(song("a", "Alpha", 1, 240)))
    await state_service.dispatch(ActionName.START_NEXT)
    t0 = datetime(2026, 7, 12, 9, 0, tzinfo=UTC)
    await start_timer(bus, 240, t0)
    await state_service.dispatch(ActionName.STOP_TIMER)
    await stop_event(bus, t0 + timedelta(seconds=30))
    timer = (await store.snapshot()).timer
    assert (timer.status, timer.remaining_seconds) == (TimerStatus.STOPPED, 210)


# --- on-screen labels (owner 2026-10-13): "playing" and "up next" describe the show ---


async def labels(store: StateStore) -> tuple[str | None, str | None, str | None]:
    state = await store.snapshot()
    return (
        state.playing_song.title if state.playing_song else None,
        state.up_next_song.title if state.up_next_song else None,
        state.current_song.title if state.current_song else None,
    )


def three() -> ServicePlan:
    return make_plan(song("a", "A", 1), song("b", "B", 2), song("c", "C", 3))


@pytest.mark.asyncio
async def test_labels_before_anything_starts(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, _ = service
    await load(bus, three())
    assert await labels(store) == (None, "A", None)


@pytest.mark.asyncio
async def test_labels_follow_start_next(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, svc = service
    await load(bus, three())
    await svc.dispatch(ActionName.START_NEXT)
    assert await labels(store) == ("A", "B", "A")
    await svc.dispatch(ActionName.START_NEXT)
    assert await labels(store) == ("B", "C", "B")
    await svc.dispatch(ActionName.START_NEXT)
    assert await labels(store) == ("C", None, "C")


@pytest.mark.asyncio
async def test_previous_keeps_the_playing_song_and_makes_the_previous_one_up_next(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, svc = service
    await load(bus, three())
    await svc.dispatch(ActionName.START_NEXT)
    await svc.dispatch(ActionName.START_NEXT)  # B plays
    started: list[StagePilotEvent] = []
    await bus.subscribe(EventType.SONG_STARTED, started.append)

    outcome = await svc.dispatch(ActionName.PREVIOUS)

    assert outcome.accepted is True
    # B is still what is playing; A is what the next Start next will play. The cursor moved to A.
    assert await labels(store) == ("B", "A", "A")
    assert started == []
    state = await store.snapshot()
    assert state.playing_song_index == 1
    assert state.current_song_index == 0  # Playback / MIDI mapping still use the cursor


@pytest.mark.asyncio
async def test_start_next_after_previous_plays_that_song_and_labels_follow(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, svc = service
    await load(bus, three())
    await svc.dispatch(ActionName.START_NEXT)
    await svc.dispatch(ActionName.START_NEXT)
    await svc.dispatch(ActionName.PREVIOUS)
    await svc.dispatch(ActionName.START_NEXT)
    assert await labels(store) == ("A", "B", "A")


@pytest.mark.asyncio
async def test_previous_twice_walks_back_one_song_at_a_time(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, svc = service
    await load(bus, three())
    for _ in range(3):
        await svc.dispatch(ActionName.START_NEXT)  # C plays
    await svc.dispatch(ActionName.PREVIOUS)
    assert await labels(store) == ("C", "B", "B")
    await svc.dispatch(ActionName.PREVIOUS)
    assert await labels(store) == ("C", "A", "A")
    assert (await svc.dispatch(ActionName.PREVIOUS)).accepted is False  # already first
    assert await labels(store) == ("C", "A", "A")


@pytest.mark.asyncio
async def test_restart_current_plays_the_cursor_song(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, svc = service
    await load(bus, three())
    await svc.dispatch(ActionName.START_NEXT)
    await svc.dispatch(ActionName.START_NEXT)  # B plays
    await svc.dispatch(ActionName.PREVIOUS)  # cursor A, B still playing
    await svc.dispatch(ActionName.RESTART_CURRENT)  # restarts the cursor song: A
    assert await labels(store) == ("A", "A", "A")


@pytest.mark.asyncio
async def test_next_only_moves_up_next(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, svc = service
    await load(bus, three())
    await svc.dispatch(ActionName.START_NEXT)  # A plays
    await svc.dispatch(ActionName.NEXT)  # cursor B
    assert await labels(store) == ("A", "B", "B")


@pytest.mark.asyncio
async def test_reset_position_clears_playing_and_points_up_next_at_the_first_song(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, svc = service
    await load(bus, three())
    await svc.dispatch(ActionName.START_NEXT)
    await svc.dispatch(ActionName.START_NEXT)
    await svc.dispatch(ActionName.RESET_POSITION)
    assert await labels(store) == (None, "A", None)


@pytest.mark.asyncio
async def test_playback_position_start_sets_the_labels(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, svc = service
    await load(bus, three())
    await svc.dispatch_song_position(2, source="midi_playback")
    assert await labels(store) == ("B", "C", "B")


@pytest.mark.asyncio
async def test_a_song_without_a_duration_is_not_reported_as_playing(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, svc = service
    await load(bus, make_plan(song("a", "A", 1, duration_seconds=None), song("b", "B", 2)))
    outcome = await svc.dispatch(ActionName.START_NEXT)
    assert outcome.accepted is False
    assert await labels(store) == (None, "A", "A")  # nothing started; Start next would retry A


@pytest.mark.asyncio
async def test_reloading_the_plan_keeps_the_playing_song_by_id(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, svc = service
    await load(bus, three())
    await svc.dispatch(ActionName.START_NEXT)
    await svc.dispatch(ActionName.START_NEXT)  # B plays
    await svc.dispatch(ActionName.PREVIOUS)  # cursor A, so the plan reload keeps the cursor on A
    four = make_plan(song("a", "A", 1), song("b", "B", 2), song("c", "C", 3), song("d", "D", 4))
    await load(bus, four)
    # B is found by id in the new plan, so it is still the playing song; Start next still plays A.
    assert await labels(store) == ("B", "A", "A")


@pytest.mark.asyncio
async def test_reloading_a_plan_where_the_playing_song_is_gone_clears_playing_but_not_up_next(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, svc = service
    await load(bus, three())
    await svc.dispatch(ActionName.START_NEXT)
    await svc.dispatch(ActionName.START_NEXT)  # B plays
    await svc.dispatch(ActionName.PREVIOUS)  # cursor A
    await load(bus, make_plan(song("a", "A", 1), song("c", "C", 2)))
    playing, up_next, _ = await labels(store)
    assert playing is None
    assert up_next == "A"


@pytest.mark.asyncio
async def test_a_new_plan_with_no_match_clears_playing(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, svc = service
    await load(bus, three())
    await svc.dispatch(ActionName.START_NEXT)
    await load(bus, make_plan(song("x", "X", 1), song("y", "Y", 2), plan_id="plan-2"))
    assert await labels(store) == (None, "X", None)


@pytest.mark.asyncio
async def test_a_plan_reload_after_previous_does_not_change_what_start_next_plays(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    """Regression: the pending target used to be recomputed as "after the cursor", so a reload
    after Previous silently moved Start next from A to B."""
    bus, store, svc = service
    await load(bus, three())
    await svc.dispatch(ActionName.START_NEXT)
    await svc.dispatch(ActionName.START_NEXT)  # B plays
    await svc.dispatch(ActionName.PREVIOUS)  # Start next will play A
    four = make_plan(song("a", "A", 1), song("b", "B", 2), song("c", "C", 3), song("d", "D", 4))
    await load(bus, four)
    assert (await labels(store))[1] == "A"
    outcome = await svc.dispatch(ActionName.START_NEXT)
    assert outcome.message == 'Started "A".'


@pytest.mark.asyncio
async def test_a_plan_reload_after_the_final_song_keeps_end_of_service(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, store, svc = service
    await load(bus, three())
    for _ in range(3):
        await svc.dispatch(ActionName.START_NEXT)  # C plays, nothing left
    await load(bus, make_plan(song("a", "A", 1), song("b", "B", 2), song("c", "C", 3)))
    assert await labels(store) == ("C", None, "C")
    assert (await svc.dispatch(ActionName.START_NEXT)).accepted is False


@pytest.mark.asyncio
async def test_previous_and_next_say_they_only_queued_the_song(
    service: tuple[EventBus, StateStore, StateService],
) -> None:
    bus, _, svc = service
    await load(bus, three())
    await svc.dispatch(ActionName.START_NEXT)
    await svc.dispatch(ActionName.START_NEXT)
    previous = await svc.dispatch(ActionName.PREVIOUS)
    assert previous.message == '"A" is up next. Press Start next to play it.'
    following = await svc.dispatch(ActionName.NEXT)
    assert following.message == '"B" is up next. Press Start next to play it.'
