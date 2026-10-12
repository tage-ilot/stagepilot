"""Shared contracts for dispatching StagePilot domain actions."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Protocol

from stagepilot.core.events import ActionName


@dataclass(frozen=True, slots=True)
class ActionOutcome:
    """Result returned after the state service handles one domain action."""

    accepted: bool
    message: str


class ActionDispatcher(Protocol):
    """Narrow action boundary used by input integrations such as MIDI."""

    async def dispatch(
        self,
        action: ActionName,
        source: str = "api",
    ) -> ActionOutcome: ...

    async def dispatch_song_position(
        self,
        position: int,
        source: str = "api",
    ) -> ActionOutcome:
        """Start the exact one-based service-plan song requested by an input."""
        ...


class TimerPositionDispatcher(Protocol):
    """Optional position-aware timer actions (Playback pause/resume/seek)."""

    async def resume_timer_at(self, position_seconds: float, source: str = "api") -> ActionOutcome:
        """Continue the current song's countdown from a Playback position."""
        ...

    async def seek_timer(self, position_seconds: float, source: str = "api") -> ActionOutcome:
        """Re-apply the remaining time from a new position, keeping run state."""
        ...

    async def stop_timer_at(self, position_seconds: float, source: str = "api") -> ActionOutcome:
        """Stop, freezing the remaining time at an explicit position."""
        ...
