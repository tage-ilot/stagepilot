from __future__ import annotations

from dataclasses import dataclass, field

import pytest

from stagepilot.core.config import DemoSettings, ProPresenterSettings, Settings
from stagepilot.core.events import ActionName
from stagepilot.main import create_app
from stagepilot.models.state import ConnectionStatus, TimerStatus
from stagepilot.plugins.propresenter import (
    ProPresenterCountdown,
    ProPresenterIdentifier,
    ProPresenterLook,
    ProPresenterTimer,
)


@dataclass
class FakeProPresenterClient:
    timer: ProPresenterTimer = field(
        default_factory=lambda: ProPresenterTimer(
            id=ProPresenterIdentifier(
                uuid="timer-uuid",
                name="Song Countdown",
                index=0,
            ),
            allows_overrun=False,
            countdown=ProPresenterCountdown(duration=60),
        )
    )
    calls: list[tuple[str, object | None]] = field(default_factory=list)
    closed: bool = False

    async def close(self) -> None:
        self.closed = True

    async def list_timers(self) -> list[ProPresenterTimer]:
        self.calls.append(("list", None))
        return [self.timer]

    async def find_timer(self, name: str) -> ProPresenterTimer:
        self.calls.append(("find", name))
        return self.timer

    async def list_looks(self) -> list[ProPresenterLook]:
        return [
            ProPresenterLook(
                id=ProPresenterIdentifier(uuid="look-default", name="Default", index=0)
            )
        ]

    async def current_look(self) -> ProPresenterLook:
        return (await self.list_looks())[0]

    async def trigger_look(self, _look_id: str) -> None:
        return None

    async def stop_timer(self, timer_id: str) -> None:
        self.calls.append(("stop", timer_id))

    async def set_timer_duration(
        self,
        timer: ProPresenterTimer,
        duration_seconds: int,
    ) -> ProPresenterTimer:
        self.calls.append(("set", duration_seconds))
        self.calls.append(("reset", timer.id.uuid))
        self.timer = timer.model_copy(
            update={"countdown": ProPresenterCountdown(duration=duration_seconds)}
        )
        return self.timer

    async def reset_timer(self, timer_id: str) -> None:
        self.calls.append(("reset", timer_id))

    async def start_timer(self, timer_id: str) -> None:
        self.calls.append(("start", timer_id))


@pytest.mark.asyncio
async def test_demo_plan_can_drive_real_propresenter_plugin() -> None:
    fake = FakeProPresenterClient()
    settings = Settings(
        demo_mode=True,
        demo=DemoSettings(simulate_propresenter=False),
        propresenter=ProPresenterSettings(
            enabled=True,
            health_check_interval_seconds=300,
        ),
    )
    app = create_app(
        settings,
        propresenter_client_factory=lambda _settings: fake,
    )

    async with app.router.lifespan_context(app):
        runtime = app.state.runtime
        initial = await runtime.state_store.snapshot()
        assert initial.plan is not None
        assert initial.plan.songs[0].title == "Battle Belongs"
        assert initial.propresenter_status is ConnectionStatus.CONNECTED

        outcome = await runtime.state_service.dispatch(ActionName.START_NEXT, source="test")
        assert outcome.accepted
        running = await runtime.state_store.snapshot()
        assert running.current_song is not None
        assert running.current_song.title == "Battle Belongs"
        assert running.timer.status is TimerStatus.RUNNING
        assert running.timer.duration_seconds == 281

        assert fake.calls == [
            ("list", None),
            ("stop", "timer-uuid"),
            ("set", 281),
            ("reset", "timer-uuid"),
            ("start", "timer-uuid"),
        ]

        restart = await runtime.state_service.dispatch(ActionName.RESTART_CURRENT, source="test")
        assert restart.accepted
        assert fake.calls[-4:] == [
            ("stop", "timer-uuid"),
            ("set", 281),
            ("reset", "timer-uuid"),
            ("start", "timer-uuid"),
        ]

        stopped = await runtime.state_service.dispatch(ActionName.STOP_TIMER, source="test")
        assert stopped.accepted
        state = await runtime.state_store.snapshot()
        assert state.timer.status is TimerStatus.STOPPED
        assert fake.calls[-1] == ("stop", "timer-uuid")

        reset = await runtime.state_service.dispatch(ActionName.RESET_POSITION, source="test")
        assert reset.accepted
        reset_state = await runtime.state_store.snapshot()
        assert reset_state.current_song is None
        assert reset_state.timer.status is TimerStatus.STOPPED
        assert reset_state.timer.duration_seconds is None
        assert fake.calls[-4:] == [
            ("stop", "timer-uuid"),
            ("set", 0),
            ("reset", "timer-uuid"),
            ("reset", "timer-uuid"),
        ]

    assert fake.closed


async def _pp_runtime_events(fake: FakeProPresenterClient):  # type: ignore[no-untyped-def]
    from stagepilot.core.events import EventType, TimerPayload, new_event
    from stagepilot.models.state import Song

    settings = Settings(
        demo_mode=True,
        demo=DemoSettings(simulate_propresenter=False),
        propresenter=ProPresenterSettings(enabled=True, health_check_interval_seconds=300),
    )
    app = create_app(settings, propresenter_client_factory=lambda _s: fake)
    song = Song(id="s", title="T", duration_seconds=281, order=1)

    def ev(kind, remaining, running):  # type: ignore[no-untyped-def]
        return new_event(
            kind,
            source="test",
            payload=TimerPayload(
                duration_seconds=remaining,
                song=song,
                position_seconds=281 - remaining,
                remaining_seconds=remaining,
                running=running,
            ),
        )

    return app, EventType, ev


@pytest.mark.asyncio
async def test_stop_holds_then_resume_and_seek_apply_remaining() -> None:
    fake = FakeProPresenterClient()
    app, ET, ev = await _pp_runtime_events(fake)
    async with app.router.lifespan_context(app):
        bus = app.state.runtime.event_bus
        fake.calls.clear()
        await bus.publish(ev(ET.TIMER_STOP_REQUESTED, 100, False))
        assert fake.calls == [("stop", "timer-uuid")]  # hold: no reset, no duration change
        fake.calls.clear()
        await bus.publish(ev(ET.TIMER_RESUMED, 100, True))
        assert fake.calls == [
            ("stop", "timer-uuid"),
            ("set", 100),
            ("reset", "timer-uuid"),
            ("start", "timer-uuid"),
        ]
        fake.calls.clear()
        await bus.publish(ev(ET.TIMER_SEEKED, 40, True))
        assert ("set", 40) in fake.calls and fake.calls[-1] == ("start", "timer-uuid")
        fake.calls.clear()
        await bus.publish(ev(ET.TIMER_SEEKED, 0, False))  # seek while stopped / clamp 0
        assert ("set", 0) in fake.calls and not any(c[0] == "start" for c in fake.calls)


@pytest.mark.asyncio
async def test_seek_while_stopped_updates_held_value_without_running() -> None:
    fake = FakeProPresenterClient()
    app, ET, ev = await _pp_runtime_events(fake)
    async with app.router.lifespan_context(app):
        fake.calls.clear()
        await app.state.runtime.event_bus.publish(ev(ET.TIMER_SEEKED, 55, False))
        assert ("set", 55) in fake.calls
        assert not any(c[0] == "start" for c in fake.calls)


@pytest.mark.asyncio
async def test_reposition_failure_is_logged_not_raised() -> None:
    fake = FakeProPresenterClient()

    async def boom(*_a, **_k):  # type: ignore[no-untyped-def]
        raise RuntimeError("down")

    app, ET, ev = await _pp_runtime_events(fake)
    async with app.router.lifespan_context(app):
        fake.set_timer_duration = boom  # type: ignore[method-assign]
        await app.state.runtime.event_bus.publish(ev(ET.TIMER_RESUMED, 10, True))
        assert app.state.runtime is not None
