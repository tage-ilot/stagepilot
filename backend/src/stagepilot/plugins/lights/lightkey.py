"""Lightkey link: connect to ``lightkey-api``, record live cue changes against a song, replay them.

Owned by ``LightsPlugin`` (started and stopped with it) so it adds no new plugin or setting page.
It polls ``GET {api}/log`` on the Lightkey Mac, which reports every change of the active-cue set
with a timestamp taken from Lightkey's own DMX output. Those timestamps are converted to this
machine's clock with a measured offset and then to song time, so a stored edge is as exact as the
countdown is.

The pure rules (what an edge is, what a replay sends, who wins) live in ``core/lightkey.py`` and
are the part to port to Rust verbatim; this file is the thin async shell around them.
"""

from __future__ import annotations

import asyncio
import json
import os
import time
from collections.abc import Awaitable, Callable
from contextlib import suppress
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Protocol
from uuid import uuid4

import httpx
from pydantic import BaseModel, Field, ValidationError

from stagepilot.core.config import LightkeySettings, OscArgument
from stagepilot.core.event_bus import EventBus, Subscription
from stagepilot.core.events import EventType, SongPayload, StagePilotEvent, TimerPayload
from stagepilot.core.lightkey import (
    REPLAY_LEAD_SECONDS,
    LightkeyStatus,
    LightkeyTake,
    LightkeyTakeSummary,
    OperatorHold,
    ReplayCommand,
    build_take_edges,
    diff,
    effective_api_url,
    osc_address,
    replay_plan,
    summarize,
    take_is_worth_keeping,
)
from stagepilot.core.logging import get_logger
from stagepilot.models.state import Song

POLL_SECONDS = 0.25
RETRY_SECONDS = 2.0
OFFSET_SAMPLES = 7
OFFSET_REFRESH_SECONDS = 120.0
HTTP_TIMEOUT_SECONDS = 2.0
# Seconds either side of a resume that still counts as "the same place in the song".
RESUME_TOLERANCE_SECONDS = 2.0

type SendOsc = Callable[[str, list[OscArgument]], Awaitable[None]]
type SettingsProvider = Callable[[], tuple[bool, bool, str, LightkeySettings]]


class TakeStore(Protocol):
    def load(self) -> dict[str, LightkeyTake]: ...

    def save(self, takes: dict[str, LightkeyTake]) -> None: ...


class MemoryTakeStore:
    def __init__(self) -> None:
        self.takes: dict[str, LightkeyTake] = {}

    def load(self) -> dict[str, LightkeyTake]:
        return {key: take.model_copy(deep=True) for key, take in self.takes.items()}

    def save(self, takes: dict[str, LightkeyTake]) -> None:
        self.takes = {key: take.model_copy(deep=True) for key, take in takes.items()}


class _TakeFile(BaseModel):
    version: int = 1
    takes: list[LightkeyTake] = Field(default_factory=list)


class FileTakeStore:
    """All captures in one atomically written JSON file next to the settings file."""

    def __init__(self, path: Path) -> None:
        self.path = path

    def load(self) -> dict[str, LightkeyTake]:
        if not self.path.exists():
            return {}
        try:
            data = _TakeFile.model_validate(json.loads(self.path.read_text(encoding="utf-8")))
        except (OSError, ValueError, ValidationError):
            # Never block startup on a bad capture file: keep it for inspection, start empty.
            with suppress(OSError):
                self.path.replace(self.path.with_name(self.path.name + ".bad"))
            return {}
        return {take.song_key: take for take in data.takes}

    def save(self, takes: dict[str, LightkeyTake]) -> None:
        payload = _TakeFile(takes=sorted(takes.values(), key=lambda take: take.song_key))
        temporary = self.path.with_name(f".{self.path.name}.{uuid4().hex}.tmp")
        try:
            self.path.parent.mkdir(parents=True, exist_ok=True)
            with temporary.open("x", encoding="utf-8", newline="\n") as handle:
                handle.write(payload.model_dump_json(indent=1))
                handle.flush()
                os.fsync(handle.fileno())
            os.replace(temporary, self.path)
        finally:
            with suppress(OSError):
                temporary.unlink(missing_ok=True)


class _Session:
    """One song being recorded."""

    def __init__(self, song: Song, started_at: float, baseline: set[str]) -> None:
        self.song = song
        self.key = song.source_song_id or song.id
        self.started_at = started_at  # wall-clock seconds when song time 0 happened
        self.baseline = set(baseline)
        self.active = set(baseline)
        self.observed: list[tuple[float, list[str], list[str]]] = []
        self.last_position = 0.0  # furthest song position reached
        self.running = True


class LightkeyLink:
    def __init__(
        self,
        event_bus: EventBus,
        settings_provider: SettingsProvider,
        send_osc: SendOsc,
        store: TakeStore,
        *,
        client_factory: Callable[[], httpx.AsyncClient] | None = None,
    ) -> None:
        self._bus = event_bus
        self._settings = settings_provider
        self._send_osc = send_osc
        self._store = store
        self._client_factory = client_factory or (
            lambda: httpx.AsyncClient(timeout=HTTP_TIMEOUT_SECONDS)
        )
        self._logger = get_logger("lightkey")
        self._takes: dict[str, LightkeyTake] = {}
        self._subscriptions: list[Subscription] = []
        self._poll_task: asyncio.Task[None] | None = None
        self._replay_task: asyncio.Task[None] | None = None
        self._stop = asyncio.Event()
        self._wake = asyncio.Event()
        # link state
        self._link: str = "off"
        self._detail: str | None = None
        self._active: set[str] | None = None
        self._seq = 0
        self._offset = 0.0  # lightkey_clock - local_clock, seconds
        self._offset_at = 0.0
        # recording state
        self._armed = False
        self._session: _Session | None = None
        self._message: str | None = None
        # replay state
        self._replaying: str | None = None
        self._hold = OperatorHold()
        self._pending_song: Song | None = None
        self._clock: tuple[float, float] | None = None  # (song position at, wall time at)

    # ------------------------------------------------------------ lifecycle
    async def start(self) -> None:
        self._stop.clear()
        try:
            self._takes = await asyncio.to_thread(self._store.load)
        except OSError:
            self._takes = {}
        bus = self._bus
        for event_type, handler in (
            (EventType.SONG_STARTED, self._on_song),
            (EventType.SONG_RESTARTED, self._on_song),
            (EventType.TIMER_STARTED, self._on_started),
            (EventType.TIMER_RESUMED, self._on_repositioned),
            (EventType.TIMER_SEEKED, self._on_repositioned),
            (EventType.TIMER_STOP_REQUESTED, self._on_stopped),
            (EventType.TIMER_STOPPED, self._on_stopped),
            (EventType.TIMER_FAILED, self._on_stopped),
            (EventType.TIMER_RESET_REQUESTED, self._on_reset),
        ):
            self._subscriptions.append(await bus.subscribe(event_type, handler))
        self._poll_task = asyncio.create_task(self._poll_loop(), name="stagepilot-lightkey-poll")

    async def stop(self) -> None:
        self._stop.set()
        self._wake.set()
        await self._cancel_replay()
        task, self._poll_task = self._poll_task, None
        if task is not None:
            task.cancel()
            with suppress(asyncio.CancelledError):
                await task
        for subscription in self._subscriptions:
            await self._bus.unsubscribe(subscription)
        self._subscriptions.clear()
        self._session = None
        self._armed = False

    def settings_changed(self) -> None:
        """Called after the saved Lightkey (or Lighting) settings change."""

        self._link = "off"
        self._active = None
        self._seq = 0
        self._offset_at = 0.0
        self._wake.set()

    # ------------------------------------------------------------ status
    def status(self) -> LightkeyStatus:
        enabled, _osc, osc_host, settings = self._settings()
        takes = sorted(
            (summarize(take) for take in self._takes.values()),
            key=lambda item: item.song_title.casefold(),
        )
        return LightkeyStatus(
            api_url=effective_api_url(settings, osc_host),
            link="off" if not enabled else self._link,
            detail=self._detail,
            active_count=len(self._active) if self._active is not None else None,
            recording=self._armed,
            recording_song=self._session.song.title if self._session else None,
            replay=settings.replay,
            replay_blocked=self._replay_blocked_reason(),
            replaying_song=self._replaying,
            message=self._message,
            takes=takes,
        )

    def take_summaries(self) -> list[LightkeyTakeSummary]:
        return [summarize(take) for take in self._takes.values()]

    def _replay_blocked_reason(self) -> str | None:
        enabled, osc, _host, settings = self._settings()
        if not enabled:
            return "Lighting is off."
        if not osc:
            return "Replay needs Lighting set to OSC."
        if self._armed:
            return "Paused while recording."
        if not settings.replay:
            return "Replay is switched off."
        return None

    # ------------------------------------------------------------ actions
    async def set_recording(self, on: bool) -> str:
        if on == self._armed:
            return self._message or ""
        if on:
            enabled, osc, _host, _settings = self._settings()
            if not enabled:
                return self._say("Turn Lighting on first.")
            if not osc:
                return self._say("Recording needs Lighting set to OSC.")
            if self._link != "connected":
                return self._say("Connect to Lightkey first.")
            self._armed = True
            await self._cancel_replay()
            return self._say("Recording starts with the next song.")
        self._armed = False
        session, self._session = self._session, None
        if session is not None:
            return await self._finish(session, reason="Recording turned off.")
        return self._say("Recording is off.")

    async def clear_take(self, song_key: str) -> bool:
        if song_key not in self._takes:
            return False
        takes = dict(self._takes)
        takes.pop(song_key)
        await asyncio.to_thread(self._store.save, takes)
        self._takes = takes
        self._say("Capture cleared.")
        return True

    def has_take(self, song_key: str) -> bool:
        return song_key in self._takes

    def _say(self, text: str) -> str:
        self._message = text
        return text

    # ------------------------------------------------------------ polling
    def _set_link(self, link: str, detail: str | None) -> None:
        self._link = link
        self._detail = detail

    async def _poll_loop(self) -> None:
        async with self._client_factory() as client:
            while not self._stop.is_set():
                enabled, _osc, _host, _settings = self._settings()
                delay = POLL_SECONDS
                if not enabled:
                    self._set_link("off", None)
                    delay = RETRY_SECONDS
                else:
                    try:
                        await self._poll_once(client)
                    except asyncio.CancelledError:
                        raise
                    except Exception as exc:  # the link must survive anything the network does
                        self._on_unreachable(exc)
                        delay = RETRY_SECONDS
                with suppress(TimeoutError):
                    await asyncio.wait_for(self._wake.wait(), timeout=delay)
                self._wake.clear()

    def _on_unreachable(self, exc: Exception) -> None:
        if isinstance(exc, httpx.HTTPStatusError) and exc.response.status_code == 403:
            detail = "Lightkey API refused this address. Allow the wired interface in its config."
        elif isinstance(exc, httpx.TimeoutException):
            detail = "Lightkey API did not answer."
        else:
            detail = "Cannot reach the Lightkey API."
        if self._link == "connected":
            self._logger.warning("lightkey_link_lost", reason=type(exc).__name__)
        self._active = None
        self._set_link("unreachable", detail)

    async def _poll_once(self, client: httpx.AsyncClient) -> None:
        _enabled, _osc, osc_host, settings = self._settings()
        base = effective_api_url(settings, osc_host)
        if time.monotonic() - self._offset_at > OFFSET_REFRESH_SECONDS or self._link != "connected":
            await self._measure_offset(client, base)
        response = await client.get(f"{base}/log", params={"since": self._seq})
        response.raise_for_status()
        body: dict[str, Any] = response.json()
        state: dict[str, Any] = body.get("state", {})
        server_seq = int(state.get("seq", 0))
        if server_seq < self._seq:  # the agent restarted: its numbering began again
            self._seq = 0
            self._active = None
            self._wake.set()
            return
        online = bool(state.get("online"))
        fresh = self._link != "connected" or self._active is None
        if not online:
            self._active = None
            self._seq = server_seq
            self._set_link("lightkey_closed", "Lightkey is not running.")
            return
        active_now = {str(name) for name in state.get("active") or []}
        if fresh:
            self._active = active_now
            self._seq = server_seq
            self._set_link("connected", None)
            return
        for event in body.get("events", []):
            self._seq = max(self._seq, int(event.get("seq", 0)))
            if "added" not in event:
                continue  # an online/offline event; the state above covers it
            await self._apply_change(
                [str(name) for name in event.get("added", [])],
                [str(name) for name in event.get("removed", [])],
                float(event.get("effective_ms", state.get("server_time", 0))) / 1000.0,
            )
        self._active = active_now
        self._set_link("connected", None)

    async def _measure_offset(self, client: httpx.AsyncClient, base: str) -> None:
        """Clock offset from the lowest-round-trip of several samples (NTP style)."""

        best: tuple[float, float] | None = None
        for _ in range(OFFSET_SAMPLES):
            sent = time.time()
            response = await client.get(f"{base}/time")
            received = time.time()
            response.raise_for_status()
            remote = float(response.json()["t"]) / 1000.0
            rtt = received - sent
            if best is None or rtt < best[0]:
                best = (rtt, remote - (sent + received) / 2.0)
        if best is not None:
            self._offset = best[1]
            self._offset_at = time.monotonic()

    async def _apply_change(
        self, added: list[str], removed: list[str], lightkey_time: float
    ) -> None:
        local_time = lightkey_time - self._offset
        now = time.monotonic()
        for cue in (*added, *removed):
            if self._hold.note_change(cue, now):
                self._logger.info("lightkey_operator_change", cue=cue)
        session = self._session
        if session is not None and session.running and not self._stop.is_set():
            position = local_time - session.started_at
            session.observed.append((position, added, removed))
            session.active -= set(removed)
            session.active |= set(added)

    # ------------------------------------------------------------ timer events
    @staticmethod
    def _song_key(song: Song) -> str:
        return song.source_song_id or song.id

    async def _on_song(self, event: StagePilotEvent) -> None:
        if isinstance(event.payload, SongPayload):
            self._pending_song = event.payload.song

    def _clock_from(self, payload: TimerPayload, position: float) -> float:
        """Wall-clock second at which song time 0 happened for this timer event."""

        started = payload.started_at or datetime.now(UTC)
        return started.timestamp() - position

    async def _on_started(self, event: StagePilotEvent) -> None:
        payload = event.payload
        if not isinstance(payload, TimerPayload):
            return
        song = payload.song or self._pending_song
        if song is None:
            return
        await self._cancel_replay()
        previous, self._session = self._session, None
        if previous is not None:
            await self._finish(previous, reason="A new song started.")
        started_at = self._clock_from(payload, 0.0)
        if self._armed:
            if self._active is None or self._link != "connected":
                self._say("Not recording this song: Lightkey is not connected.")
                return
            self._session = _Session(song, started_at, self._active)
            self._say(f'Recording "{song.title}".')
            return
        await self._start_replay(song, payload, 0.0)

    async def _on_repositioned(self, event: StagePilotEvent) -> None:
        payload = event.payload
        if not isinstance(payload, TimerPayload):
            return
        song = payload.song or self._pending_song
        position = max(0.0, payload.position_seconds or 0.0)
        session = self._session
        if session is not None:
            same_place = abs(position - session.last_position) <= RESUME_TOLERANCE_SECONDS
            if event.type is EventType.TIMER_RESUMED and payload.running and same_place:
                # Paused and resumed in place: carry on, and catch up on anything changed meanwhile.
                session.started_at = self._clock_from(payload, position)
                session.running = True
                if self._active is not None:
                    added, removed = diff(session.active, self._active)
                    if added or removed:
                        session.observed.append((position, added, removed))
                        session.active = set(self._active)
                return
            self._session = None
            self._say("Recording discarded: the song was moved.")
            return
        await self._cancel_replay()
        if not payload.running or song is None:
            return
        await self._start_replay(song, payload, position)

    async def _on_stopped(self, event: StagePilotEvent) -> None:
        payload = event.payload
        await self._cancel_replay()
        session = self._session
        if session is None:
            return
        position = (
            payload.position_seconds
            if isinstance(payload, TimerPayload) and payload.position_seconds is not None
            else time.time() - session.started_at
        )
        session.last_position = max(session.last_position, position)
        session.running = False
        duration = float(session.song.duration_seconds or 0)
        if take_is_worth_keeping(session.last_position, duration):
            self._session = None
            await self._finish(session, reason="Song finished.")
        else:
            self._say(f'Paused "{session.song.title}". Recording resumes if the song does.')

    async def _on_reset(self, _event: StagePilotEvent) -> None:
        await self._cancel_replay()
        if self._session is not None:
            self._session = None
            self._say("Recording discarded: the countdown was reset.")

    async def _finish(self, session: _Session, *, reason: str) -> str:
        duration = float(session.song.duration_seconds or 0)
        if session.running:
            session.last_position = max(session.last_position, time.time() - session.started_at)
        covered = min(session.last_position, duration) if duration else session.last_position
        if not take_is_worth_keeping(covered, duration):
            return self._say(
                f"Not saved: only {covered:.0f} of {duration:.0f} seconds of "
                f'"{session.song.title}" were recorded.'
            )
        initial, edges = build_take_edges(session.baseline, session.observed, duration)
        take = LightkeyTake(
            song_key=session.key,
            song_title=session.song.title,
            recorded_at=datetime.now(UTC),
            duration_seconds=duration,
            initial=initial,
            edges=edges,
        )
        takes = dict(self._takes)
        takes[session.key] = take
        try:
            await asyncio.to_thread(self._store.save, takes)
        except OSError:
            self._logger.error("lightkey_take_save_failed", song=session.key)
            return self._say("Could not save the capture to disk.")
        self._takes = takes
        return self._say(f'Saved "{session.song.title}": {len(edges)} changes. {reason}')

    # ------------------------------------------------------------ replay
    async def _start_replay(self, song: Song, payload: TimerPayload, position: float) -> None:
        if self._replay_blocked_reason() is not None:
            return
        take = self._takes.get(self._song_key(song))
        if take is None:
            return
        sync_now, later = replay_plan(take, position)
        wall_started = payload.started_at.timestamp() if payload.started_at else time.time()
        elapsed = max(0.0, time.time() - wall_started)
        clock_zero = asyncio.get_running_loop().time() - elapsed - position
        self._replaying = song.title
        self._replay_task = asyncio.create_task(
            self._run_replay(sync_now, later, clock_zero, take.duration_seconds),
            name="stagepilot-lightkey-replay",
        )

    async def _run_replay(
        self,
        sync_now: list[ReplayCommand],
        later: list[ReplayCommand],
        clock_zero: float,
        song_seconds: float,
    ) -> None:
        loop = asyncio.get_running_loop()
        try:
            for command in sync_now:
                await self._send(command)
            for command in later:
                delay = clock_zero + command.at_seconds - REPLAY_LEAD_SECONDS - loop.time()
                if delay > 0:
                    await asyncio.sleep(delay)
                await self._send(command)
            # Stay "replaying" until the song's end (or a stop), so the strip does not go quiet
            # while the song is still playing.
            await asyncio.sleep(max(0.0, clock_zero + song_seconds - loop.time()))
        except asyncio.CancelledError:
            raise
        except Exception as exc:
            self._logger.error("lightkey_replay_failed", error=str(exc))
            self._say("A replay command could not be sent.")
        finally:
            if self._replay_task is asyncio.current_task():
                self._replaying = None

    async def _send(self, command: ReplayCommand) -> None:
        now = time.monotonic()
        if self._hold.is_held(command.cue, now):
            return  # the operator touched this cue a moment ago: leave it with them
        self._hold.note_sent(command.cue, now)
        await self._send_osc(osc_address(command.cue, command.verb), [])

    async def _cancel_replay(self) -> None:
        task, self._replay_task = self._replay_task, None
        self._replaying = None
        if task is not None:
            task.cancel()
            with suppress(asyncio.CancelledError):
                await task
