"""Resilient Playback MIDI input plugin and cue-to-action translation."""

from __future__ import annotations

import asyncio
import hashlib
import time
from collections import Counter, deque
from collections.abc import Callable, Coroutine
from concurrent.futures import ThreadPoolExecutor
from contextlib import suppress
from dataclasses import dataclass
from datetime import UTC, datetime
from functools import partial
from typing import Any
from uuid import UUID, uuid4

from stagepilot.core.actions import ActionDispatcher, ActionOutcome
from stagepilot.core.config import MidiSettings
from stagepilot.core.event_bus import EventBus
from stagepilot.core.events import (
    ActionName,
    ConnectionPayload,
    EventType,
    MidiNotePayload,
    new_event,
)
from stagepilot.core.logging import get_logger
from stagepilot.core.midi import (
    MAX_SONG_POSITION_VELOCITY,
    MIN_SONG_POSITION_VELOCITY,
    MidiController,
    MidiCueName,
    MidiInputInfo,
    MidiInputSnapshot,
    MidiMessageDisposition,
    MidiMonitorMessage,
)
from stagepilot.core.plugin import Plugin
from stagepilot.core.state import StateStore
from stagepilot.models.state import ConnectionStatus, PluginHealth, PluginStatus
from stagepilot.plugins.midi_playback.client import (
    MidiBackendContract,
    MidiBackendFactory,
    MidiInputPortContract,
    MidoMidiBackend,
)
from stagepilot.plugins.midi_playback.models import MidiMessage

QUEUE_CAPACITY = 64
DEFAULT_RECONNECT_DELAYS = (1.0, 2.0, 4.0, 8.0, 15.0)
DEFAULT_MONITOR_INTERVAL = 1.0
MIDI_MONITOR_CAPACITY = 50
_QUEUE_STOP = object()
type MonotonicClock = Callable[[], float]


@dataclass(slots=True)
class _QueuedMidiMessage:
    message: MidiMessage
    connection_id: UUID | None
    simulated: bool
    release_after: bool = False
    completion: asyncio.Future[ActionOutcome] | None = None
    ownership_revision: int | None = None


class MidiPlaybackPlugin(Plugin, MidiController):
    """Listen to one MIDI input and dispatch configured StagePilot actions."""

    name = "midi_playback"
    version = "0.1.0"

    def __init__(
        self,
        event_bus: EventBus,
        state_store: StateStore,
        settings: MidiSettings,
        action_dispatcher: ActionDispatcher,
        *,
        backend_factory: MidiBackendFactory | None = None,
        monotonic_clock: MonotonicClock | None = None,
        reconnect_delays: tuple[float, ...] = DEFAULT_RECONNECT_DELAYS,
        monitor_interval: float = DEFAULT_MONITOR_INTERVAL,
        queue_capacity: int = QUEUE_CAPACITY,
    ) -> None:
        super().__init__(event_bus, state_store)
        if not reconnect_delays or any(delay < 0 for delay in reconnect_delays):
            raise ValueError("MIDI reconnect delays must be non-negative.")
        if monitor_interval <= 0:
            raise ValueError("MIDI monitor interval must be positive.")
        if queue_capacity < 1:
            raise ValueError("MIDI queue capacity must be positive.")
        self._settings = settings
        self._action_dispatcher = action_dispatcher
        self._backend_factory = backend_factory or MidoMidiBackend
        self._clock = monotonic_clock or time.monotonic
        self._reconnect_delays = reconnect_delays
        self._monitor_interval = monitor_interval
        self._queue_capacity = queue_capacity
        self._backend: MidiBackendContract | None = None
        self._executor: ThreadPoolExecutor | None = None
        self._loop: asyncio.AbstractEventLoop | None = None
        self._queue: asyncio.Queue[_QueuedMidiMessage | object] | None = None
        self._consumer_task: asyncio.Task[None] | None = None
        self._supervisor_task: asyncio.Task[None] | None = None
        self._side_tasks: set[asyncio.Task[Any]] = set()
        self._wake = asyncio.Event()
        self._selection_lock = asyncio.Lock()
        self._port: MidiInputPortContract | None = None
        self._active_connection_id: UUID | None = None
        self._selected_input_name = settings.input_name
        self._connected_input_name: str | None = None
        self._available_input_names: list[str] = []
        self._recent_messages: deque[MidiMonitorMessage] = deque(maxlen=MIDI_MONITOR_CAPACITY)
        self._held_notes: set[tuple[int, int, int]] = set()
        self._last_triggered_at: dict[tuple[int, int, int], float] = {}
        self._status = PluginStatus.STOPPED
        self._connection_status = ConnectionStatus.DISCONNECTED
        self._connection_detail: str | None = None
        self._last_error: str | None = None
        self._last_activity_at: datetime | None = None
        self._stopping = False
        self._overflow_reported = False
        self._logger = get_logger(self.name)
        self.action_guard: Callable[[], str | None] | None = None
        self.ownership_revision: Callable[[], int] | None = None
        self.connection_observer: Callable[[ConnectionStatus], Coroutine[Any, Any, None]] | None = (
            None
        )

    async def start(self) -> None:
        if self._status is not PluginStatus.STOPPED:
            return
        self._status = PluginStatus.STARTING
        self._stopping = False
        self._loop = asyncio.get_running_loop()
        self._wake = asyncio.Event()
        self._selection_lock = asyncio.Lock()
        self._selected_input_name = self._settings.input_name
        self._recent_messages.clear()
        self._queue = asyncio.Queue(maxsize=self._queue_capacity)
        self._executor = ThreadPoolExecutor(max_workers=1, thread_name_prefix="stagepilot-midi")
        try:
            self._backend = await self._run_backend(self._backend_factory)
        except Exception:
            await self._cleanup_failed_start()
            raise RuntimeError("The MIDI backend could not be initialized.") from None

        self._consumer_task = asyncio.create_task(
            self._consume_messages(),
            name="stagepilot-midi-consumer",
        )
        self._supervisor_task = asyncio.create_task(
            self._supervise_connection(),
            name="stagepilot-midi-supervisor",
        )

    async def stop(self) -> None:
        if self._status is PluginStatus.STOPPED and self._backend is None:
            return
        self._status = PluginStatus.STOPPING
        self._stopping = True
        self._active_connection_id = None
        self._wake.set()

        supervisor = self._supervisor_task
        if supervisor is not None:
            with suppress(asyncio.CancelledError):
                await supervisor
        self._supervisor_task = None

        queue = self._queue
        if queue is not None:
            while True:
                try:
                    queued = queue.get_nowait()
                except asyncio.QueueEmpty:
                    break
                if isinstance(queued, _QueuedMidiMessage):
                    self._complete(
                        queued,
                        ActionOutcome(False, "MIDI input stopped before the cue was processed."),
                    )
            queue.put_nowait(_QUEUE_STOP)

        consumer = self._consumer_task
        if consumer is not None:
            with suppress(asyncio.CancelledError):
                await consumer
        self._consumer_task = None

        if self._side_tasks:
            await asyncio.gather(*tuple(self._side_tasks), return_exceptions=True)
        self._side_tasks.clear()
        await self._set_connection(
            ConnectionStatus.DISCONNECTED,
            "MIDI Playback input stopped.",
            plugin_status=PluginStatus.STOPPING,
        )

        executor = self._executor
        self._executor = None
        if executor is not None:
            await asyncio.to_thread(executor.shutdown, wait=True, cancel_futures=True)
        self._backend = None
        self._queue = None
        self._loop = None
        self._held_notes.clear()
        self._last_triggered_at.clear()
        self._selected_input_name = self._settings.input_name
        self._connected_input_name = None
        self._status = PluginStatus.STOPPED
        self._last_activity_at = datetime.now(UTC)

    async def health(self) -> PluginHealth:
        owned_tasks = (self._consumer_task, self._supervisor_task)
        if (
            not self._stopping
            and self._status not in {PluginStatus.STOPPED, PluginStatus.STOPPING}
            and any(task is not None and task.done() for task in owned_tasks)
        ):
            self._status = PluginStatus.ERROR
            self._last_error = "A MIDI background task stopped unexpectedly."
        return PluginHealth(
            name=self.name,
            version=self.version,
            status=self._status,
            last_error=self._last_error,
            last_activity_at=self._last_activity_at,
        )

    async def reconfigure(self, settings: MidiSettings) -> ActionOutcome:
        """Apply cue-filter settings without reopening the selected MIDI input."""

        if self._stopping or self._status in {PluginStatus.STOPPED, PluginStatus.STOPPING}:
            return ActionOutcome(False, "The MIDI Playback plugin is not running.")
        if not settings.enabled:
            return ActionOutcome(False, "MIDI is turned off in the saved settings.")

        self._settings = settings.model_copy(deep=True)
        self._held_notes.clear()
        self._last_triggered_at.clear()
        if settings.input_name != self._selected_input_name:
            # A different input was saved: connect to it live (the supervisor retries
            # while the device is absent and picks it up when it appears).
            async with self._selection_lock:
                self._selected_input_name = settings.input_name
                self._active_connection_id = None
                self._wake.set()
                if settings.input_name is None:
                    await self._set_connection(
                        ConnectionStatus.DISCONNECTED,
                        "No MIDI input is selected; cue simulation remains available.",
                        plugin_status=PluginStatus.STARTING,
                    )
                else:
                    await self._set_connection(
                        ConnectionStatus.CONNECTING,
                        f'Connecting to "{settings.input_name}".',
                        plugin_status=PluginStatus.STARTING,
                    )
        self._logger.info(
            "midi_settings_reconfigured",
            channel=settings.channel,
            note=settings.note,
            debounce_ms=settings.debounce_ms,
        )
        return ActionOutcome(
            True,
            (
                "MIDI cue settings applied: channel "
                f"{settings.channel}, note {self._note_name(settings.note)} ({settings.note})."
            ),
        )

    async def input_snapshot(self, *, refresh: bool = False) -> MidiInputSnapshot:
        if refresh and self._backend is not None and self._executor is not None:
            try:
                await self._refresh_input_names()
                self._wake.set()
            except Exception:
                await self._set_connection(
                    ConnectionStatus.ERROR,
                    "Could not enumerate MIDI input ports.",
                    plugin_status=PluginStatus.ERROR,
                )
        counts = Counter(self._available_input_names)
        names = sorted(counts, key=str.casefold)
        inputs = tuple(
            MidiInputInfo(
                id=self._input_id(name),
                name=name,
                ambiguous=counts[name] > 1,
                selected=name == self._selected_input_name,
                connected=name == self._connected_input_name,
            )
            for name in names
        )
        return MidiInputSnapshot(
            enabled=self._settings.enabled,
            channel=self._settings.channel,
            note=self._settings.note,
            configured_input_name=self._settings.input_name,
            selected_input_name=self._selected_input_name,
            inputs=inputs,
            mappings=self._settings.mappings.configured(),
        )

    async def select_input(self, input_id: str | None) -> ActionOutcome:
        """Select one currently discovered input for this backend session."""

        if self._stopping or self._backend is None or self._executor is None:
            return ActionOutcome(False, "The MIDI Playback plugin is not running.")

        async with self._selection_lock:
            selected_name: str | None = None
            if input_id is not None:
                try:
                    names = await self._refresh_input_names()
                except Exception:
                    await self._set_connection(
                        ConnectionStatus.ERROR,
                        "Could not enumerate MIDI input ports.",
                        plugin_status=PluginStatus.ERROR,
                    )
                    return ActionOutcome(False, "MIDI inputs could not be refreshed.")
                matching_names = {name for name in names if self._input_id(name) == input_id}
                if len(matching_names) != 1:
                    return ActionOutcome(False, "The selected MIDI input is no longer available.")
                selected_name = next(iter(matching_names))
                if names.count(selected_name) != 1:
                    return ActionOutcome(False, "The selected MIDI input name is ambiguous.")

            if selected_name == self._selected_input_name:
                if selected_name is None:
                    return ActionOutcome(True, "MIDI input is already disconnected.")
                return ActionOutcome(True, f'"{selected_name}" is already selected.')

            self._selected_input_name = selected_name
            self._active_connection_id = None
            self._held_notes.clear()
            self._last_triggered_at.clear()
            self._wake.set()
            if selected_name is None:
                await self._set_connection(
                    ConnectionStatus.DISCONNECTED,
                    "No MIDI input is selected; cue simulation remains available.",
                    plugin_status=PluginStatus.STARTING,
                )
                return ActionOutcome(True, "MIDI input disconnected for this session.")

            await self._set_connection(
                ConnectionStatus.CONNECTING,
                f'Connecting to "{selected_name}".',
                plugin_status=PluginStatus.STARTING,
            )
            return ActionOutcome(True, f'Selected "{selected_name}" for this session.')

    async def simulate_cue(self, cue: MidiCueName) -> ActionOutcome:
        if self._stopping or self._queue is None or self._status is PluginStatus.STOPPED:
            return ActionOutcome(False, "The MIDI Playback plugin is not running.")
        velocity = self._settings.mappings.velocity_for(cue)
        if velocity is None:
            return ActionOutcome(False, f'The MIDI cue "{cue.value}" is not mapped.')
        loop = asyncio.get_running_loop()
        completion: asyncio.Future[ActionOutcome] = loop.create_future()
        queued = _QueuedMidiMessage(
            message=MidiMessage(
                type="note_on",
                channel=self._settings.channel,
                note=self._settings.note,
                velocity=velocity,
            ),
            connection_id=None,
            simulated=True,
            release_after=True,
            completion=completion,
        )
        if not self._offer_message(queued):
            return ActionOutcome(
                False, "The MIDI input queue is full; the simulated cue was dropped."
            )
        return await completion

    async def recent_messages(self) -> tuple[MidiMonitorMessage, ...]:
        return tuple(reversed(self._recent_messages))

    async def _supervise_connection(self) -> None:
        retry_index = 0
        try:
            while not self._stopping:
                configured_name = self._selected_input_name
                if configured_name is None:
                    await self._set_connection(
                        ConnectionStatus.DISCONNECTED,
                        "No MIDI input is selected; cue simulation remains available.",
                        plugin_status=PluginStatus.STARTING,
                    )
                    await self._wait_before_retry(self._monitor_interval)
                    retry_index = 0
                    continue

                try:
                    await self._refresh_input_names()
                except Exception:
                    await self._set_connection(
                        ConnectionStatus.ERROR,
                        "Could not enumerate MIDI input ports.",
                        plugin_status=PluginStatus.ERROR,
                    )
                    await self._wait_before_retry(self._reconnect_delays[retry_index])
                    retry_index = min(retry_index + 1, len(self._reconnect_delays) - 1)
                    continue

                if configured_name != self._selected_input_name:
                    retry_index = 0
                    continue

                matches = self._available_input_names.count(configured_name)
                if matches == 0:
                    await self._set_connection(
                        ConnectionStatus.DISCONNECTED,
                        "The selected MIDI input is unavailable; retrying.",
                        plugin_status=PluginStatus.STARTING,
                    )
                    await self._wait_before_retry(self._reconnect_delays[retry_index])
                    retry_index = min(retry_index + 1, len(self._reconnect_delays) - 1)
                    continue
                if matches > 1:
                    await self._set_connection(
                        ConnectionStatus.ERROR,
                        "Multiple MIDI inputs have the selected name; select a unique input.",
                        plugin_status=PluginStatus.ERROR,
                    )
                    await self._wait_before_retry(self._reconnect_delays[retry_index])
                    retry_index = min(retry_index + 1, len(self._reconnect_delays) - 1)
                    continue

                await self._set_connection(
                    ConnectionStatus.CONNECTING,
                    "Opening the selected MIDI input.",
                    plugin_status=PluginStatus.STARTING,
                )
                connection_id = uuid4()
                try:
                    port = await self._run_backend(
                        partial(self._open_input, configured_name, connection_id)
                    )
                except Exception:
                    await self._set_connection(
                        ConnectionStatus.ERROR,
                        "Could not open the selected MIDI input; retrying.",
                        plugin_status=PluginStatus.ERROR,
                    )
                    await self._wait_before_retry(self._reconnect_delays[retry_index])
                    retry_index = min(retry_index + 1, len(self._reconnect_delays) - 1)
                    continue

                if self._stopping or configured_name != self._selected_input_name:
                    await self._close_port(port)
                    if self._stopping:
                        return
                    retry_index = 0
                    continue
                self._port = port
                self._active_connection_id = connection_id
                self._connected_input_name = configured_name
                self._held_notes.clear()
                self._last_triggered_at.clear()
                self._overflow_reported = False
                retry_index = 0
                await self._set_connection(
                    ConnectionStatus.CONNECTED,
                    "Playback MIDI input is connected.",
                    plugin_status=PluginStatus.RUNNING,
                )

                disconnect_detail = "The selected MIDI input disconnected; retrying."
                while not self._stopping:
                    await self._wait_before_retry(self._monitor_interval)
                    if self._stopping:
                        break
                    try:
                        closed = await self._run_backend(partial(self._port_is_closed, port))
                        await self._refresh_input_names()
                    except Exception:
                        disconnect_detail = "The MIDI input failed during monitoring; retrying."
                        break
                    if configured_name != self._selected_input_name:
                        disconnect_detail = "The MIDI input selection changed; reconnecting."
                        break
                    if closed or self._available_input_names.count(configured_name) != 1:
                        break
                    await self._set_connection(
                        ConnectionStatus.CONNECTED,
                        "Playback MIDI input is connected.",
                        plugin_status=PluginStatus.RUNNING,
                    )

                self._active_connection_id = None
                self._connected_input_name = None
                self._held_notes.clear()
                await self._close_port(port)
                self._port = None
                if not self._stopping:
                    await self._set_connection(
                        ConnectionStatus.DISCONNECTED,
                        disconnect_detail,
                        plugin_status=PluginStatus.STARTING,
                    )
        finally:
            self._active_connection_id = None
            self._connected_input_name = None
            final_port = self._port
            self._port = None
            if final_port is not None:
                await self._close_port(final_port)

    async def _consume_messages(self) -> None:
        queue = self._require_queue()
        while True:
            queued = await queue.get()
            if queued is _QUEUE_STOP:
                return
            if not isinstance(queued, _QueuedMidiMessage):
                continue
            if self._stopping:
                self._complete(
                    queued,
                    ActionOutcome(False, "MIDI input stopped before the cue was processed."),
                )
                continue
            try:
                outcome = await self._process_message(queued)
            except Exception:
                self._logger.error("midi_message_processing_failed")
                outcome = ActionOutcome(False, "The MIDI cue could not be processed.")
                self._record_message(
                    queued,
                    MidiMessageDisposition.ERROR,
                    outcome.message,
                )
            finally:
                if queued.release_after:
                    self._release_held_note(
                        queued.message.channel,
                        queued.message.note,
                    )
            self._complete(queued, outcome)

    async def _process_message(self, queued: _QueuedMidiMessage) -> ActionOutcome:
        message = queued.message
        if queued.connection_id is not None and queued.connection_id != self._active_connection_id:
            return ActionOutcome(False, "A stale MIDI message was ignored.")
        if (
            message.type != "note_off"
            and message.velocity != 0
            and self.ownership_revision is not None
            and queued.ownership_revision != self.ownership_revision()
        ):
            detail = self.action_guard() if self.action_guard is not None else None
            detail = detail or "ignored: Playback source ownership changed"
            self._record_message(queued, MidiMessageDisposition.ACTION_REJECTED, detail)
            return ActionOutcome(False, detail)

        if message.type == "note_off" or message.velocity == 0:
            self._release_held_note(message.channel, message.note)
            outcome = ActionOutcome(False, "MIDI note release received.")
            self._record_message(
                queued,
                MidiMessageDisposition.NOTE_RELEASE,
                outcome.message,
            )
            return outcome

        if message.channel != self._settings.channel:
            outcome = ActionOutcome(
                False,
                f"Ignored: StagePilot listens on channel {self._settings.channel}.",
            )
            self._record_message(
                queued,
                MidiMessageDisposition.WRONG_CHANNEL,
                outcome.message,
            )
            return outcome

        if message.note != self._settings.note:
            outcome = ActionOutcome(
                False,
                (
                    "Ignored: StagePilot listens for MIDI note "
                    f"{self._note_name(self._settings.note)} ({self._settings.note})."
                ),
            )
            self._record_message(
                queued,
                MidiMessageDisposition.UNMAPPED,
                outcome.message,
            )
            return outcome

        song_position = (
            message.velocity
            if MIN_SONG_POSITION_VELOCITY <= message.velocity <= MAX_SONG_POSITION_VELOCITY
            else None
        )
        cue = (
            None if song_position is not None else self._settings.mappings.cue_for(message.velocity)
        )
        if cue is None:
            if song_position is not None:
                action = ActionName.START_NEXT
            else:
                outcome = ActionOutcome(
                    False,
                    (f"Ignored: velocity {message.velocity} is not mapped to a StagePilot cue."),
                )
                self._record_message(
                    queued,
                    MidiMessageDisposition.UNMAPPED,
                    outcome.message,
                )
                return outcome
        else:
            action = cue.action

        self._logger.info(
            "midi_cue_recognized",
            channel=message.channel,
            note=message.note,
            velocity=message.velocity,
            action=action.value,
            song_position=song_position,
            simulated=queued.simulated,
        )
        await self.event_bus.publish(
            new_event(
                EventType.MIDI_NOTE_RECEIVED,
                source="midi.simulation" if queued.simulated else self.name,
                payload=MidiNotePayload(
                    channel=message.channel,
                    note=message.note,
                    velocity=message.velocity,
                    action=action,
                    song_position=song_position,
                    connection_id=queued.connection_id,
                    simulated=queued.simulated,
                ),
            )
        )

        trigger_key = (message.channel, message.note, message.velocity)
        now = self._clock()
        previous = self._last_triggered_at.get(trigger_key)
        debounce_seconds = self._settings.debounce_ms / 1000
        if trigger_key in self._held_notes or (
            previous is not None and now - previous < debounce_seconds
        ):
            self._logger.info(
                "midi_duplicate_ignored",
                channel=message.channel,
                note=message.note,
                velocity=message.velocity,
            )
            outcome = ActionOutcome(False, "Duplicate MIDI cue ignored.")
            self._record_message(
                queued,
                MidiMessageDisposition.DUPLICATE,
                outcome.message,
                action=action,
            )
            return outcome

        self._held_notes.add(trigger_key)
        self._last_triggered_at[trigger_key] = now
        ignored = self.action_guard() if self.action_guard is not None else None
        if ignored is not None:
            outcome = ActionOutcome(False, ignored)
            self._record_message(
                queued, MidiMessageDisposition.ACTION_REJECTED, ignored, action=action
            )
            return outcome
        source = "midi.simulation" if queued.simulated else self.name
        if song_position is not None:
            outcome = await self._action_dispatcher.dispatch_song_position(
                song_position,
                source=source,
            )
        else:
            outcome = await self._action_dispatcher.dispatch(action, source=source)
        self._record_message(
            queued,
            (
                MidiMessageDisposition.DISPATCHED
                if outcome.accepted
                else MidiMessageDisposition.ACTION_REJECTED
            ),
            outcome.message,
            action=action,
        )
        self._last_activity_at = datetime.now(UTC)
        return outcome

    def _release_held_note(self, channel: int, note: int) -> None:
        """Release every velocity latch associated with one MIDI note."""

        matching = {
            trigger for trigger in self._held_notes if trigger[0] == channel and trigger[1] == note
        }
        self._held_notes.difference_update(matching)

    def _receive_from_backend(self, connection_id: UUID, message: MidiMessage) -> None:
        loop = self._loop
        if loop is None or self._stopping:
            return
        queued = _QueuedMidiMessage(
            message=message,
            connection_id=connection_id,
            simulated=False,
        )
        try:
            loop.call_soon_threadsafe(self._offer_message, queued)
        except RuntimeError:
            return

    def _offer_message(self, queued: _QueuedMidiMessage) -> bool:
        queue = self._queue
        if queue is None or self._stopping:
            self._complete(queued, ActionOutcome(False, "The MIDI Playback plugin is stopping."))
            return False
        if queued.connection_id is not None and queued.connection_id != self._active_connection_id:
            self._complete(queued, ActionOutcome(False, "A stale MIDI message was ignored."))
            return False
        if queue.full():
            if not self._overflow_reported:
                self._overflow_reported = True
                self._spawn_side_task(self._report_queue_overflow())
            self._complete(
                queued,
                ActionOutcome(False, "The MIDI input queue is full; the cue was dropped."),
            )
            self._record_message(
                queued,
                MidiMessageDisposition.QUEUE_FULL,
                "The MIDI input queue was full; this message was dropped.",
            )
            return False
        if self.ownership_revision is not None:
            queued.ownership_revision = self.ownership_revision()
        queue.put_nowait(queued)
        return True

    async def _report_queue_overflow(self) -> None:
        self._logger.error("midi_input_queue_overflow", capacity=self._queue_capacity)
        await self._set_connection(
            ConnectionStatus.ERROR,
            "The MIDI input queue overflowed and a cue was dropped.",
            plugin_status=PluginStatus.ERROR,
        )

    async def _refresh_input_names(self) -> list[str]:
        names = await self._run_backend(self._require_backend().list_input_names)
        self._available_input_names = list(names)
        return self._available_input_names

    def _open_input(self, name: str, connection_id: UUID) -> MidiInputPortContract:
        return self._require_backend().open_input(
            name,
            partial(self._receive_from_backend, connection_id),
        )

    @staticmethod
    def _port_is_closed(port: MidiInputPortContract) -> bool:
        return port.closed

    async def _close_port(self, port: MidiInputPortContract) -> None:
        try:
            await self._run_backend(port.close)
        except Exception:
            self._logger.warning("midi_input_close_failed")

    async def _run_backend[ResultT](self, operation: Callable[[], ResultT]) -> ResultT:
        loop = asyncio.get_running_loop()
        executor = self._executor
        if executor is None:
            raise RuntimeError("MIDI backend executor is unavailable.")
        return await loop.run_in_executor(executor, operation)

    async def _wait_before_retry(self, delay: float) -> None:
        if self._stopping:
            return
        try:
            await asyncio.wait_for(self._wake.wait(), timeout=delay)
        except TimeoutError:
            return
        finally:
            self._wake.clear()

    async def _set_connection(
        self,
        status: ConnectionStatus,
        detail: str,
        *,
        plugin_status: PluginStatus,
    ) -> None:
        self._status = plugin_status
        self._last_error = detail if plugin_status is PluginStatus.ERROR else None
        self._last_activity_at = datetime.now(UTC)
        if status is self._connection_status and detail == self._connection_detail:
            return
        self._connection_status = status
        self._connection_detail = detail
        if self.connection_observer is not None:
            await self.connection_observer(status)
            return
        await self.event_bus.publish(
            new_event(
                EventType.CONNECTION_CHANGED,
                source=self.name,
                payload=ConnectionPayload(
                    integration="midi",
                    status=status,
                    detail=detail,
                ),
            )
        )

    async def _cleanup_failed_start(self) -> None:
        executor = self._executor
        self._executor = None
        if executor is not None:
            await asyncio.to_thread(executor.shutdown, wait=True, cancel_futures=True)
        self._backend = None
        self._queue = None
        self._loop = None
        self._status = PluginStatus.ERROR

    def _spawn_side_task(self, coroutine: Coroutine[Any, Any, None]) -> None:
        if self._stopping:
            coroutine.close()
            return
        task = asyncio.create_task(coroutine)
        self._side_tasks.add(task)
        task.add_done_callback(self._side_tasks.discard)

    def _require_backend(self) -> MidiBackendContract:
        if self._backend is None:
            raise RuntimeError("MIDI backend is unavailable.")
        return self._backend

    def _require_queue(self) -> asyncio.Queue[_QueuedMidiMessage | object]:
        if self._queue is None:
            raise RuntimeError("MIDI input queue is unavailable.")
        return self._queue

    @staticmethod
    def _complete(queued: _QueuedMidiMessage, outcome: ActionOutcome) -> None:
        completion = queued.completion
        if completion is not None and not completion.done():
            completion.set_result(outcome)

    @staticmethod
    def _input_id(name: str) -> str:
        return hashlib.sha256(name.encode("utf-8")).hexdigest()

    def _record_message(
        self,
        queued: _QueuedMidiMessage,
        disposition: MidiMessageDisposition,
        detail: str,
        *,
        action: ActionName | None = None,
    ) -> None:
        message = queued.message
        self._recent_messages.append(
            MidiMonitorMessage(
                timestamp=datetime.now(UTC),
                input_name=("Cue simulation" if queued.simulated else self._connected_input_name),
                message_type=message.type,
                channel=message.channel,
                note=message.note,
                note_name=self._note_name(message.note),
                velocity=message.velocity,
                disposition=disposition,
                detail=detail,
                action=action,
                simulated=queued.simulated,
            )
        )

    @staticmethod
    def _note_name(note: int) -> str:
        names = ("C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B")
        # Playback labels MIDI note 0 as C-2. MIDI octave labels are not
        # standardized, so the monitor deliberately follows Playback's UI.
        return f"{names[note % 12]}{note // 12 - 2}"
