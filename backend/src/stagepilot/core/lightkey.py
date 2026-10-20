"""Lightkey live-state recording and replay: models and pure logic (no I/O, no clock, no network).

Lightkey sends nothing back over OSC or MIDI, so StagePilot reads which cues are active from the
separate ``lightkey-api`` agent running on the Lightkey Mac (https://github.com/huntrw6/lightkey-api).
While a song's countdown runs and recording is armed, every change in the active-cue set is
stored as an *edge* at a song time. On a later run of the same song the edges are replayed as OSC
``activate`` / ``deactivate`` commands, so only the cues that were captured are ever driven; every
other cue stays under the live operator's hands, and an operator who touches a replayed cue wins for
a short hold.

Everything here is a pure function of its arguments so it ports to Rust unchanged. The same cases
are pinned in ``backend/tests/lightkey_golden.json``; a Rust port must pass that file as-is.
"""

from __future__ import annotations

from collections.abc import Iterable, Sequence
from datetime import datetime
from typing import Literal

from pydantic import BaseModel, Field, field_validator, model_validator

from stagepilot.core.config import DEFAULT_LIGHTKEY_API_PORT, LightkeySettings
from stagepilot.models.state import ConnectionStatus

# A take is kept only if the recording covered at least this share of the song, so a stray
# start/stop never overwrites a good capture.
MIN_TAKE_FRACTION = 0.5
# Seconds before a recorded edge that the replay command is sent, to absorb OSC + fade latency
# (measured on the rig: command to first changed DMX frame was 26-47 ms with a 0 s fade).
REPLAY_LEAD_SECONDS = 0.04
# How long an operator's own change to a replayed cue is respected before replay may touch it again.
OPERATOR_HOLD_SECONDS = 3.0
# A cue change seen this soon after replay sent a command for that cue is replay's own echo.
ECHO_WINDOW_SECONDS = 2.5

type Verb = Literal["activate", "deactivate"]


class LightkeyEdge(BaseModel):
    """One change of the active-cue set, at ``at_seconds`` into the song."""

    at_seconds: float = Field(ge=0, le=86_399)
    added: list[str] = Field(default_factory=list, max_length=512)
    removed: list[str] = Field(default_factory=list, max_length=512)

    @field_validator("at_seconds", mode="after")
    @classmethod
    def round_ms(cls, value: float) -> float:
        return round(value, 3)

    @model_validator(mode="after")
    def not_empty_and_disjoint(self) -> LightkeyEdge:
        if not self.added and not self.removed:
            raise ValueError("An edge must add or remove at least one cue.")
        if set(self.added) & set(self.removed):
            raise ValueError("A cue cannot be added and removed by the same edge.")
        return self


class LightkeyTake(BaseModel):
    """Everything captured for one song: the starting set plus the edges after it.

    ``initial`` is the active-cue set at song time 0 (edges at 0.0 are folded into it by the
    recorder). Replaying the take drives only cues named somewhere in it (``managed_cues``).
    """

    song_key: str = Field(min_length=1, max_length=256)
    song_title: str = Field(min_length=1, max_length=500)
    recorded_at: datetime
    duration_seconds: float = Field(ge=0, le=86_399)
    initial: list[str] = Field(default_factory=list, max_length=512)
    edges: list[LightkeyEdge] = Field(default_factory=list, max_length=20_000)

    @model_validator(mode="after")
    def edges_are_sorted(self) -> LightkeyTake:
        self.edges.sort(key=lambda edge: edge.at_seconds)
        return self


class LightkeyTakeSummary(BaseModel):
    song_key: str
    song_title: str
    recorded_at: datetime
    duration_seconds: float
    edge_count: int
    cue_count: int


def effective_api_url(settings: LightkeySettings, osc_host: str) -> str:
    """The URL to poll: the saved address, else the OSC host on the default API port."""

    if settings.api_url:
        text = settings.api_url
        host_part = text.split("://", 1)[1]
        return text if ":" in host_part else f"{text}:{DEFAULT_LIGHTKEY_API_PORT}"
    return f"http://{osc_host}:{DEFAULT_LIGHTKEY_API_PORT}"


class LightkeyStatus(BaseModel):
    """What the StagePilot Lightkey strip shows."""

    api_url: str
    # off: Lighting disabled; connected: API answered and Lightkey is running;
    # lightkey_closed: API answered, Lightkey is not running; unreachable: no answer.
    link: Literal["off", "connected", "lightkey_closed", "unreachable"]
    detail: str | None = None
    active_count: int | None = None
    recording: bool = False
    recording_song: str | None = None
    replay: bool = True
    replay_blocked: str | None = None
    replaying_song: str | None = None
    message: str | None = None
    takes: list[LightkeyTakeSummary] = Field(default_factory=list)

    def as_connection(self) -> ConnectionStatus:
        connected = self.link == "connected"
        return ConnectionStatus.CONNECTED if connected else ConnectionStatus.DISCONNECTED


# ------------------------------------------------------------------ pure logic


def diff(previous: Iterable[str], current: Iterable[str]) -> tuple[list[str], list[str]]:
    """``(added, removed)`` going from ``previous`` to ``current``, each sorted."""

    before, now = set(previous), set(current)
    return sorted(now - before), sorted(before - now)


def state_at(take: LightkeyTake, position: float) -> set[str]:
    """The captured active set at ``position`` seconds (an edge at ``position`` is applied)."""

    active = set(take.initial)
    for edge in take.edges:
        if edge.at_seconds > position:
            break
        active -= set(edge.removed)
        active |= set(edge.added)
    return active


def managed_cues(take: LightkeyTake) -> set[str]:
    """Every cue the take ever mentions. Replay never touches a cue outside this set."""

    cues = set(take.initial)
    for edge in take.edges:
        cues.update(edge.added)
        cues.update(edge.removed)
    return cues


def osc_address(cue: str, verb: Verb) -> str:
    """Lightkey's idempotent cue address. ``*`` matches any control panel."""

    return f"/live/*/cue/{cue}/{verb}"


def is_addressable(cue: str) -> bool:
    """A cue name containing ``/`` or a control character cannot be put in an OSC address."""

    return bool(cue) and "/" not in cue and not any(ord(character) < 32 for character in cue)


class ReplayCommand(BaseModel):
    at_seconds: float
    cue: str
    verb: Verb


def replay_plan(
    take: LightkeyTake, position: float
) -> tuple[list[ReplayCommand], list[ReplayCommand]]:
    """``(sync_now, later)`` for starting or repositioning playback at ``position`` seconds.

    ``sync_now`` forces every managed cue to its captured state at ``position`` (``activate`` for
    on, ``deactivate`` for off) and is sent immediately. ``later`` holds one command per cue change
    after ``position`` at its edge time, in time order. Unaddressable names are dropped.
    """

    managed = sorted(cue for cue in managed_cues(take) if is_addressable(cue))
    on = state_at(take, position)
    sync_now = [
        ReplayCommand(at_seconds=position, cue=cue, verb="activate" if cue in on else "deactivate")
        for cue in managed
    ]
    later: list[ReplayCommand] = []
    for edge in take.edges:
        if edge.at_seconds <= position:
            continue
        for cue in edge.removed:
            if is_addressable(cue):
                later.append(ReplayCommand(at_seconds=edge.at_seconds, cue=cue, verb="deactivate"))
        for cue in edge.added:
            if is_addressable(cue):
                later.append(ReplayCommand(at_seconds=edge.at_seconds, cue=cue, verb="activate"))
    return sync_now, later


def build_take_edges(
    baseline: Iterable[str],
    observed: Sequence[tuple[float, Sequence[str], Sequence[str]]],
    duration: float,
) -> tuple[list[str], list[LightkeyEdge]]:
    """Fold timed observations into ``(initial, edges)``.

    ``observed`` is ``(song_seconds, added, removed)`` in arrival order. Times are clamped into
    ``[0, duration]``; an observation at 0 becomes part of ``initial``; adding a cue that is
    already on, or removing one that is already off, is dropped so the stored set never lies.
    """

    active = set(baseline)
    initial = set(baseline)
    edges: list[LightkeyEdge] = []
    for raw_time, added, removed in sorted(observed, key=lambda item: item[0]):
        time = min(max(raw_time, 0.0), duration)
        really_removed = sorted(set(removed) & active)
        really_added = sorted(set(added) - active)
        if not really_added and not really_removed:
            continue
        active -= set(really_removed)
        active |= set(really_added)
        if time <= 0.0:
            initial = set(active)
            continue
        edges.append(LightkeyEdge(at_seconds=time, added=really_added, removed=really_removed))
    return sorted(initial), edges


def take_is_worth_keeping(covered_seconds: float, duration_seconds: float) -> bool:
    """True when the recording covered enough of the song to replace an earlier take."""

    return duration_seconds > 0 and covered_seconds >= duration_seconds * MIN_TAKE_FRACTION


def summarize(take: LightkeyTake) -> LightkeyTakeSummary:
    return LightkeyTakeSummary(
        song_key=take.song_key,
        song_title=take.song_title,
        recorded_at=take.recorded_at,
        duration_seconds=take.duration_seconds,
        edge_count=len(take.edges),
        cue_count=len(managed_cues(take)),
    )


class OperatorHold:
    """Tracks which replayed cues the live operator has touched, so replay stands back.

    Time is passed in (monotonic seconds) so this stays pure. ``note_sent`` records that replay
    just drove a cue; ``note_change`` is called for every change the Lightkey API reports. A change
    to a cue soon after replay sent it is replay's own echo; any other change is the operator's, and
    that cue is skipped by replay until ``OPERATOR_HOLD_SECONDS`` later.
    """

    def __init__(self) -> None:
        self._sent: dict[str, float] = {}
        self._held_until: dict[str, float] = {}

    def note_sent(self, cue: str, now: float) -> None:
        self._sent[cue] = now

    def note_change(self, cue: str, now: float) -> bool:
        """Returns True when this change was the operator's (and starts or extends a hold)."""

        sent_at = self._sent.get(cue)
        if sent_at is not None and now - sent_at <= ECHO_WINDOW_SECONDS:
            return False
        self._held_until[cue] = now + OPERATOR_HOLD_SECONDS
        return True

    def is_held(self, cue: str, now: float) -> bool:
        return self._held_until.get(cue, float("-inf")) > now

    def held_cues(self, now: float) -> list[str]:
        return sorted(cue for cue, until in self._held_until.items() if until > now)
