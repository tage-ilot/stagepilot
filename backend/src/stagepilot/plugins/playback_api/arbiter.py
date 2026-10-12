"""Connection-only ownership and bounded cross-source action deduplication."""

import time
from collections.abc import Callable
from typing import Literal, cast

from pydantic import BaseModel

from stagepilot.core.actions import ActionDispatcher, ActionOutcome, TimerPositionDispatcher
from stagepilot.core.events import ActionName
from stagepilot.core.state import StateStore

Source = Literal["playback_api", "midi", "none"]


class SourceStatus(BaseModel):
    connected: bool
    reason: str


class PlaybackConnection(BaseModel):
    active_source: Source
    sources: dict[str, SourceStatus]
    connected: bool
    reason: str


class PlaybackSourceArbiter:
    def __init__(self, *, clock: Callable[[], float] = time.monotonic) -> None:
        self.clock = clock
        self.websocket = False
        self.heartbeat_at: float | None = None
        self.stable_since: float | None = None
        self.midi_connected = False
        # MIDI may own only when the user chose MIDI; API mode never falls back to it.
        self.midi_allowed = True
        self.owner: Source = "none"
        self.revision = 0
        self._recent: dict[tuple[str, int | None], tuple[Source, float]] = {}

    def observe_api(self, connected: bool, fresh: bool) -> None:
        now = self.clock()
        if not connected or (self.heartbeat_at is not None and now - self.heartbeat_at >= 5):
            self.stable_since = None
            self.heartbeat_at = None
        self.websocket = connected
        if connected and fresh:
            if self.stable_since is None or (
                self.heartbeat_at is not None and now - self.heartbeat_at > 1.5
            ):
                self.stable_since = now
            self.heartbeat_at = now
        self.snapshot()

    def snapshot(self) -> PlaybackConnection:
        now = self.clock()
        api = self.websocket and self.heartbeat_at is not None and now - self.heartbeat_at < 5
        if not api:
            self.stable_since = None
        owner: Source = "midi" if self.midi_connected and self.midi_allowed else "none"
        if api and (
            self.owner == "playback_api"
            or not self.midi_connected
            or (
                self.stable_since is not None
                and now - self.stable_since >= 3
                and self.heartbeat_at is not None
                and now - self.heartbeat_at <= 1.5
            )
        ):
            owner = "playback_api"
        if owner != self.owner:
            self.owner = owner
            self.revision += 1
        reason = {
            "playback_api": "Connected to Playback.",
            "midi": "Connected through MIDI.",
            "none": "Not connected.",
        }[owner]
        return PlaybackConnection(
            active_source=owner,
            connected=owner != "none",
            reason=reason,
            sources={
                "playback_api": SourceStatus(
                    connected=api,
                    reason="Playback is connected." if api else "Playback is not connected.",
                ),
                "midi": SourceStatus(
                    connected=self.midi_connected,
                    reason="MIDI is connected."
                    if self.midi_connected
                    else "MIDI is not connected.",
                ),
            },
        )

    def allows(self, source: Source) -> bool:
        return self.snapshot().active_source == source

    def duplicate(self, source: Source, key: tuple[str, int | None]) -> bool:
        now = self.clock()
        self._recent = {k: v for k, v in self._recent.items() if now - v[1] < 2}
        previous = self._recent.get(key)
        return previous is not None and previous[0] != source

    def remember(self, source: Source, key: tuple[str, int | None]) -> None:
        self._recent[key] = (source, self.clock())


class ArbitratedDispatcher:
    def __init__(
        self, arbiter: PlaybackSourceArbiter, dispatcher: ActionDispatcher, state: StateStore
    ) -> None:
        self.arbiter = arbiter
        self.dispatcher = dispatcher
        self.state = state
        self.suppressed: Callable[[], bool] = lambda: False
        self.api_mapping_revision: Callable[[], int] = lambda: 0

    async def dispatch(self, action: ActionName, source: str = "api") -> ActionOutcome:
        self.arbiter.snapshot()
        revision = self.arbiter.revision
        mapping_revision = self.api_mapping_revision()
        state = await self.state.snapshot()
        self.arbiter.snapshot()
        if revision != self.arbiter.revision:
            return ActionOutcome(False, "ignored: Playback source ownership changed")
        if (
            source == "playback_api"
            and action in (ActionName.START_NEXT, ActionName.RESTART_CURRENT)
            and mapping_revision != self.api_mapping_revision()
        ):
            return ActionOutcome(False, "ignored: Playback song order changed")
        position = state.current_song_index + 1 if state.current_song_index is not None else None
        if action is ActionName.START_NEXT:
            position = 1 if state.current_song_index is None else state.current_song_index + 2
        key = (
            ("start", position)
            if action in (ActionName.START_NEXT, ActionName.RESTART_CURRENT)
            else (action.value, None)
        )
        return await self._dispatch(source, key, action=action)

    async def dispatch_song_position(self, position: int, source: str = "api") -> ActionOutcome:
        return await self._dispatch(source, ("start", position), position=position)

    async def _timer(self, call: tuple[str, float], source: str) -> ActionOutcome:
        target = cast(TimerPositionDispatcher, self.dispatcher)
        method = getattr(target, call[0])
        outcome: ActionOutcome = await method(call[1], source=source)
        return outcome

    async def resume_timer_at(self, position_seconds: float, source: str = "api") -> ActionOutcome:
        return await self._dispatch(
            source, ("resume", None), timer_call=("resume_timer_at", position_seconds)
        )

    async def seek_timer(self, position_seconds: float, source: str = "api") -> ActionOutcome:
        return await self._dispatch(
            source, ("seek", None), timer_call=("seek_timer", position_seconds)
        )

    async def stop_timer_at(self, position_seconds: float, source: str = "api") -> ActionOutcome:
        return await self._dispatch(
            source,
            (ActionName.STOP_TIMER.value, None),
            timer_call=("stop_timer_at", position_seconds),
        )

    async def _dispatch(
        self,
        source: str,
        key: tuple[str, int | None],
        *,
        action: ActionName | None = None,
        position: int | None = None,
        timer_call: tuple[str, float] | None = None,
    ) -> ActionOutcome:
        if self.suppressed():
            return ActionOutcome(False, "ignored: song order discovery active")
        if source == "midi.simulation" and not self.arbiter.allows("playback_api"):
            if timer_call is not None:
                return await self._timer(timer_call, source)
            if position is not None:
                return await self.dispatcher.dispatch_song_position(position, source=source)
            assert action is not None
            return await self.dispatcher.dispatch(action, source=source)
        origin: Source = "playback_api" if source == "playback_api" else "midi"
        if origin == "midi" and not self.arbiter.midi_allowed:
            return ActionOutcome(False, "ignored: MIDI is not the selected connection method")
        if not self.arbiter.allows(origin):
            return ActionOutcome(
                False,
                "ignored: Playback API active" if origin == "midi" else "ignored: MIDI active",
            )
        if self.arbiter.duplicate(origin, key):
            return ActionOutcome(False, "ignored: duplicate cross-source Playback action")
        if action is ActionName.START_NEXT and any(
            previous_source != origin and previous_key[0] == "start"
            for previous_key, (previous_source, _) in self.arbiter._recent.items()
        ):
            return ActionOutcome(False, "ignored: duplicate cross-source Playback action")
        # Reserve before awaiting: simultaneous queues cannot dispatch the same action.
        self.arbiter.remember(origin, key)
        if timer_call is not None:
            outcome = await self._timer(timer_call, source)
        elif position is not None:
            outcome = await self.dispatcher.dispatch_song_position(position, source=source)
        else:
            assert action is not None
            outcome = await self.dispatcher.dispatch(action, source=source)
        if not outcome.accepted:
            self.arbiter._recent.pop(key, None)
        return outcome
