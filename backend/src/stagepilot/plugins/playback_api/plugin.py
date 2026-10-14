"""Selected input lifecycle, bounded observations and explicit order discovery."""

from __future__ import annotations

import asyncio
import json
import threading
import time
from collections import deque
from collections.abc import Callable
from contextlib import suppress
from dataclasses import dataclass, field
from datetime import UTC, datetime
from typing import Literal

from stagepilot.core.actions import ActionDispatcher
from stagepilot.core.config import MidiSource, PlaybackApiSettings, Settings
from stagepilot.core.event_bus import EventBus
from stagepilot.core.events import ConnectionPayload, EventType, new_event
from stagepilot.core.logging import _redact, get_logger
from stagepilot.core.plugin import Plugin
from stagepilot.core.settings import PersistentSettings, SettingsFileError, SettingsService
from stagepilot.core.state import StateStore
from stagepilot.models.state import ConnectionStatus, PluginHealth, PluginStatus, ServicePlan
from stagepilot.plugins.midi_playback import MidiPlaybackPlugin
from stagepilot.plugins.playback_api.arbiter import (
    ArbitratedDispatcher,
    PlaybackConnection,
    PlaybackSourceArbiter,
)
from stagepilot.plugins.playback_api.client import (
    ConnectionOptions,
    Observer,
    PlaybackClient,
    PlaybackStatus,
)
from stagepilot.plugins.playback_api.find import (
    SCAN_BLOCK_DETAIL,
    ScanCandidate,
    ScanReport,
    scan_network,
)
from stagepilot.plugins.playback_api.mapping import Observation, PlaybackMapper
from stagepilot.plugins.playback_api.normalizer import Heartbeat, PlaybackEvent
from stagepilot.services.planning_center_lengths import PlanningCenterLengthService, plan_key

ClientFactory = Callable[[ConnectionOptions, Observer], PlaybackClient]
MidiFactory = Callable[[Settings], MidiPlaybackPlugin]
Scanner = Callable[..., list[ScanCandidate] | ScanReport]

NOT_FOUND_REASON = (
    "Couldn't find Playback. Check that Playback is open, Remote Connections is on, "
    "and both computers are on the same network."
)
UNREACHABLE_REASON = "Looking for Playback… If it stays offline, turn on Remote Connections."


def default_scanner(**kwargs: object) -> list[ScanCandidate] | ScanReport:
    return scan_network(**kwargs)  # type: ignore[arg-type]


# Plain-language message and ONE concrete next step per failure class.
SCAN_MESSAGES: dict[str, str] = {
    "permission_denied": (
        f"{SCAN_BLOCK_DETAIL}. Quit StagePilot, open System Settings > Privacy & Security > "
        "Local Network, turn StagePilot off and on again, then reopen StagePilot and scan again."
    ),
    "no_route": (
        "This computer has no route to that address. Check that it is on the same Wi-Fi or "
        "network as Playback."
    ),
    "refused": (
        "That computer answered, but Playback isn't accepting connections. In Playback, "
        "turn on Remote Connections."
    ),
    "timed_out": (
        "Nothing answered in time. Check the address and that Playback is open on that computer."
    ),
    "not_playback": (
        "Something answered, but it didn't behave like Playback. Check the address, and that "
        "Remote Connections is on in Playback."
    ),
    "invalid_address": (
        "That isn't a valid address. Enter a computer name or an address like 192.0.2.10."
    ),
    "unexpected_error": (
        "Something unexpected went wrong while scanning. Copy the diagnostic details."
    ),
}

# Opens the macOS Local Network privacy pane (verified present in the Privacy & Security
# extension on macOS 15; older systems fall back to the Privacy pane).
LOCAL_NETWORK_SETTINGS_URL = (
    "x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork"
)

_LOG = get_logger("playback")
RECENT_LOG: deque[str] = deque(maxlen=200)


def playback_log(event: str, **fields: object) -> None:
    """INFO log + bounded in-memory copy for the diagnostics bundle. Secrets are redacted."""
    safe = _redact({"event": event, **fields})
    assert isinstance(safe, dict)
    RECENT_LOG.append(json.dumps(safe, default=str, sort_keys=True))
    _LOG.info(event, **{k: v for k, v in safe.items() if k != "event"})


def default_client(options: ConnectionOptions, observer: Observer) -> PlaybackClient:
    return PlaybackClient(options, observer=observer)


class DiscoveryConflict(ValueError):
    """Operator must resolve a live-state conflict before discovery."""


@dataclass(frozen=True)
class MonitorEntry:
    event: PlaybackEvent
    discovery: bool
    at: datetime = field(default_factory=lambda: datetime.now(UTC))


class PlaybackInputPlugin(Plugin):
    version = "0.1.0"

    def __init__(
        self,
        event_bus: EventBus,
        state_store: StateStore,
        settings: Settings,
        dispatcher: ActionDispatcher,
        settings_service: SettingsService,
        midi_factory: MidiFactory,
        *,
        client_factory: ClientFactory = default_client,
        scanner: Scanner = default_scanner,
        step_timeout: float = 5.0,
        max_steps: int = 200,
    ) -> None:
        super().__init__(event_bus, state_store)
        if step_timeout <= 0 or max_steps < 1:
            raise ValueError("Discovery limits must be positive")
        self.name = (
            "midi_playback"
            if settings.integration_modes.midi_source is MidiSource.REAL
            else "playback_api"
        )
        self.settings = settings.model_copy(deep=True)
        self._service = settings_service
        self.planning_center_lengths = PlanningCenterLengthService(
            settings_service,
            state_store,
            dispatcher,
            is_playing=lambda: bool(self.status.heartbeat and self.status.heartbeat.playing),
        )
        self.planning_center_lengths.plan_current = self.scan_plan_is_current
        self.planning_center_lengths.on_applied = self.mark_lengths_applied
        self._midi_factory = midi_factory
        self._client_factory = client_factory
        self._scanner = scanner
        self.scan_state: Literal["idle", "scanning", "found", "not_found"] = "idle"
        self.scan_candidates: list[ScanCandidate] = []
        self.scan_reason: str | None = None
        self.scan_current = 0
        self.scan_total = 0
        self.scan_phase: str | None = None
        self.scan_error_class: str | None = None
        self.scan_typed = False
        self.scan_started_at: float | None = None
        self.scan_elapsed = 0.0
        self.last_scan: dict[str, object] | None = None
        self._scan_task: asyncio.Task[None] | None = None
        self._scan_cancel = threading.Event()
        self.midi: MidiPlaybackPlugin | None = None
        if settings.integration_modes.midi_source is MidiSource.REAL and settings.midi.enabled:
            self.midi = midi_factory(settings)
        self.arbiter = PlaybackSourceArbiter()
        self.arbiter.midi_allowed = not self.selected
        self._dispatcher = ArbitratedDispatcher(self.arbiter, dispatcher, state_store)
        self.mapper = PlaybackMapper(self._dispatcher, state_store)
        self._dispatcher.suppressed = lambda: self.mapper.discovery
        self._dispatcher.api_mapping_revision = lambda: self.mapper.revision
        self._ticker: asyncio.Task[None] | None = None
        self._published: tuple[bool, str] | None = None
        self._ownership_revision = 0
        self._wire_midi()
        saved = settings.playback_api
        if saved.song_order and saved.captured_version is not None:
            self.mapper.install_order(
                tuple(saved.song_order), saved.captured_version, saved.setlist_id
            )
            self.mapper.set_skips(tuple(saved.skipped_song_ids), tuple(saved.skipped_plan_song_ids))
        self.client: PlaybackClient | None = None
        self._queue: asyncio.Queue[tuple[int, Observation | ConnectionPayload]] = asyncio.Queue(64)
        self._history: deque[MonitorEntry] = deque(maxlen=100)
        self._consumer: asyncio.Task[None] | None = None
        self._generation = 0
        self._running = False
        self._active = False
        self._lifecycle = asyncio.Lock()
        self._dispatch_lock = asyncio.Lock()
        self._connection_lock = asyncio.Lock()
        self._discovery_task: asyncio.Task[object] | None = None
        self.discovery: Literal["idle", "running", "failed", "done"] = "idle"
        self.progress = 0
        self.discovery_song = 0
        self.discovery_total = 0
        self._navigation_steps = 0
        self.discovery_error: str | None = None
        self._changed = asyncio.Event()
        self._heartbeat: Heartbeat | None = None
        self._serial = 0
        self._fresh_at = 0.0
        self._connection = False
        self._step_timeout = step_timeout
        self._max_steps = max_steps

    @property
    def selected(self) -> bool:
        return self.settings.integration_modes.midi_source is MidiSource.PLAYBACK_API

    @property
    def events(self) -> tuple[MonitorEntry, ...]:
        return tuple(self._history)

    @property
    def status(self) -> PlaybackStatus:
        return (
            self.client.status
            if self.client is not None
            else PlaybackStatus(port=self.settings.playback_api.port)
        )

    def _options(self) -> ConnectionOptions:
        config = self.settings.playback_api
        return ConnectionOptions(
            config.enabled and self.selected,
            config.host,
            config.port,
            config.auto_scan,
            config.fast_transport,
        )

    def _wire_midi(self) -> None:
        if self.midi is not None:
            self.midi._action_dispatcher = self._dispatcher
            self.midi.action_guard = self._midi_guard
            self.midi.ownership_revision = self._revision
            self.midi.connection_observer = self._midi_connection

    def _revision(self) -> int:
        self.arbiter.snapshot()
        return self.arbiter.revision

    def _midi_guard(self) -> str | None:
        if self.discovery == "running":
            return "ignored: song order discovery active"
        if self.selected:
            return "ignored: MIDI is not the selected connection method"
        if self.arbiter.snapshot().active_source == "playback_api":
            return "ignored: Playback API active"
        return None

    async def _midi_connection(self, status: ConnectionStatus) -> None:
        self.arbiter.midi_connected = status is ConnectionStatus.CONNECTED
        await self._publish_connection()

    @property
    def connection(self) -> PlaybackConnection:
        result = self.arbiter.snapshot()
        # One plain sentence, owned here; the frontend never composes its own.
        if self.scan_state == "scanning":
            result.reason = "Looking for Playback…"
        elif result.active_source == "playback_api":
            host = self.status.host
            name = "this computer" if host == "127.0.0.1" else host
            result.reason = (
                f"Connected to Playback on {name}." if name else "Connected to Playback."
            )
        elif result.active_source == "none":
            if self.scan_state == "not_found":
                result.reason = NOT_FOUND_REASON
            elif self.selected and self.settings.playback_api.enabled:
                result.reason = (
                    f"Not connected. {SCAN_BLOCK_DETAIL}. Open Local Network settings to fix it."
                    if self.status.last_error == SCAN_BLOCK_DETAIL
                    else f"Not connected. {UNREACHABLE_REASON}"
                    if self.status.last_error
                    else "Not connected. Looking for Playback…"
                )
            else:
                result.reason = "Not connected. Choose Scan Network to find Playback."
        return result

    async def _publish_connection(self) -> None:
        async with self._connection_lock:
            connection = self.connection
            if self._ownership_revision != self.arbiter.revision:
                self.mapper.discard_pending()
                self._ownership_revision = self.arbiter.revision
            key = (connection.connected, connection.reason)
            if key == self._published:
                return
            await self.event_bus.publish(
                new_event(
                    EventType.CONNECTION_CHANGED,
                    source="playback_connection",
                    payload=ConnectionPayload(
                        integration="midi",
                        status=ConnectionStatus.CONNECTED
                        if connection.connected
                        else ConnectionStatus.DISCONNECTED,
                        detail=connection.reason,
                    ),
                )
            )
            self._published = key

    async def _tick(self) -> None:
        while True:
            await self._publish_connection()
            await asyncio.sleep(0.05)

    async def start(self) -> None:
        async with self._lifecycle:
            if self._running:
                return
            self._running = True
            self._consumer = asyncio.create_task(self._consume(), name="playback-input-consumer")
            self._ticker = asyncio.create_task(self._tick(), name="playback-source-arbiter")
            await self._start_selected()

    async def _start_selected(self) -> None:
        self.arbiter.midi_allowed = not self.selected
        if (
            self.settings.integration_modes.midi_source is MidiSource.REAL
            and self.settings.midi.enabled
        ):
            if self.midi is None:
                self.midi = self._midi_factory(self.settings)
                self._wire_midi()
            try:
                await self.midi.start()
            except RuntimeError:
                # MIDI is the selected method here; report its failure.
                raise
        if self.selected and self.settings.playback_api.enabled:
            self._active = True
            generation = self._generation
            self.client = self._client_factory(
                self._options(), lambda status, events: self._observe(generation, status, events)
            )
            await self.client.start()

    async def _stop_selected(self, *, keep_midi: bool = False) -> None:
        self._active = False
        self._generation += 1
        self.mapper.discard_pending()
        self.arbiter.observe_api(False, False)
        await self._publish_connection()
        if self.client is not None:
            await self.client.stop()
        if self.midi is not None and not keep_midi:
            await self.midi.stop()
        self._heartbeat = None
        self._connection = False
        if not keep_midi:
            self.arbiter.midi_connected = False
        self._changed.set()
        while not self._queue.empty():
            self._queue.get_nowait()
        await self._publish_connection()

    async def stop(self) -> None:
        await self._cancel_scan()
        task = self._discovery_task
        if task is not None and task is not asyncio.current_task():
            task.cancel()
            with suppress(asyncio.CancelledError):
                await task
        async with self._lifecycle:
            self._running = False
            if self._ticker is not None:
                self._ticker.cancel()
                with suppress(asyncio.CancelledError):
                    await self._ticker
                self._ticker = None
            # Finish any action already in flight before releasing input resources.
            async with self._dispatch_lock:
                await self._stop_selected()
            if self._consumer is not None:
                self._consumer.cancel()
                with suppress(asyncio.CancelledError):
                    await self._consumer
                self._consumer = None

    async def save_settings(self, settings: PersistentSettings) -> None:
        async with self._lifecycle:
            if (
                self.discovery == "running"
                or self.planning_center_lengths.result.status == "running"
            ):
                raise DiscoveryConflict(
                    "Wait for song discovery or Planning Center changes to finish."
                )
            saved = self._service.snapshot().playback_api
            if (
                settings.playback_api.song_order,
                settings.playback_api.captured_version,
                settings.playback_api.captured_at,
                settings.playback_api.setlist_id,
                settings.playback_api.song_lengths,
                settings.playback_api.lengths_measured_at,
                settings.playback_api.discovery_duration_seconds,
            ) != (
                saved.song_order,
                saved.captured_version,
                saved.captured_at,
                saved.setlist_id,
                saved.song_lengths,
                saved.lengths_measured_at,
                saved.discovery_duration_seconds,
            ):
                raise ValueError("Song order can only be changed by Discover Song Order.")
            settings = settings.model_copy(
                update={
                    "planning_center_length_undo": (
                        self._service.snapshot().planning_center_length_undo
                    ),
                    "playback_api": settings.playback_api.model_copy(
                        update={
                            "planning_center_update_service_type_id": (
                                saved.planning_center_update_service_type_id
                            ),
                            "scan_plan_key": saved.scan_plan_key,
                            "lengths_applied_key": saved.lengths_applied_key,
                        }
                    ),
                }
            )
            config = settings.playback_api
            ConnectionOptions(config.enabled, config.host, config.port, config.auto_scan)
            self._service.save(settings)
            await self._reconfigure(self._service.effective_runtime_settings())

    async def save_arrangement(
        self, skipped_song_ids: list[int], skipped_plan_song_ids: list[str]
    ) -> None:
        """Save which songs are skipped. The order of either list is never touched."""
        async with self._lifecycle:
            if (
                self.discovery == "running"
                or self.planning_center_lengths.result.status == "running"
            ):
                raise DiscoveryConflict(
                    "Wait for song discovery or Planning Center changes to finish."
                )
            current = self._service.snapshot()
            plan = (await self.state_store.snapshot()).plan
            if not current.playback_api.song_order:
                raise ValueError("Scan songs first, then choose which songs to skip.")
            if plan is None and skipped_plan_song_ids:
                raise ValueError("Load a service plan before skipping one of its songs.")
            if plan is not None and not set(skipped_plan_song_ids) <= {s.id for s in plan.songs}:
                raise ValueError("A skipped song is not in the loaded service plan.")
            updated = PlaybackApiSettings.model_validate(
                {
                    **current.playback_api.model_dump(),
                    "skipped_song_ids": skipped_song_ids,
                    "skipped_plan_song_ids": skipped_plan_song_ids,
                }
            )
            # Skips change which plan song each measured length belongs to, so a previous
            # Planning Center write no longer describes this arrangement.
            updated = updated.model_copy(update={"lengths_applied_key": None})
            self._service.save(current.model_copy(update={"playback_api": updated}, deep=True))
            self.settings.playback_api = updated
            self.mapper.set_skips(tuple(skipped_song_ids), tuple(skipped_plan_song_ids))

    async def reconfigure(self, settings: Settings) -> None:
        async with self._lifecycle:
            if self.discovery == "running":
                raise DiscoveryConflict("Wait for Discover Song Order before changing inputs.")
            await self._reconfigure(settings)

    async def _reconfigure(self, settings: Settings) -> None:
        same_input = (
            self.settings.integration_modes.midi_source == settings.integration_modes.midi_source
            and self.settings.midi == settings.midi
            and self.settings.playback_api == settings.playback_api
        )
        if same_input:
            return
        if (
            self.midi is not None
            and self.settings.integration_modes.midi_source is MidiSource.REAL
            and settings.integration_modes.midi_source is MidiSource.REAL
            and settings.midi.enabled
            and self.settings.midi.enabled
            and self._running
        ):
            await self.midi.reconfigure(settings.midi)
            self.settings = settings.model_copy(deep=True)
            return
        async with self._dispatch_lock:
            await self._stop_selected()
            self.settings = settings.model_copy(deep=True)
            self.midi = None
            self.client = None
            if self._running:
                await self._start_selected()

    def _scan_key(self, plan: ServicePlan | None) -> str | None:
        if plan is None:
            return None
        config = self.settings.playback_api
        category = (
            config.planning_center_update_service_type_id
            or self.settings.planning_center.service_type_id
        )
        return json.dumps([category, plan_key(plan)])

    async def plan_changed(self) -> bool:
        """True when the loaded plan or update category differs from what was scanned."""
        saved = self.settings.playback_api.scan_plan_key
        current = self._scan_key((await self.state_store.snapshot()).plan)
        return saved is not None and current is not None and saved != current  # unknown != changed

    def _applied_key(self, plan_id: str) -> str | None:
        captured = self.settings.playback_api.captured_at
        return json.dumps([captured.isoformat(), plan_id]) if captured else None

    def mark_lengths_applied(self, plan_id: str) -> None:
        """Remember (persistently) that this scan's lengths were fully written to this plan."""
        key = self._applied_key(plan_id)
        if key is None:
            return
        config = self.settings.playback_api.model_copy(update={"lengths_applied_key": key})
        self._service.save(
            self._service.snapshot().model_copy(update={"playback_api": config}, deep=True)
        )
        self.settings.playback_api = config

    async def lengths_applied(self) -> bool:
        plan = (await self.state_store.snapshot()).plan
        saved = self.settings.playback_api.lengths_applied_key
        return bool(plan and saved and saved == self._applied_key(plan.id))

    async def scan_plan_is_current(self) -> bool:
        return not await self.plan_changed()

    def _observe(
        self, generation: int, status: PlaybackStatus, events: tuple[PlaybackEvent, ...]
    ) -> None:
        if not self._active or generation != self._generation:
            return
        heartbeat = status.heartbeat
        fresh = heartbeat is not None and heartbeat is not self._heartbeat
        if fresh:
            self._heartbeat = heartbeat
            self._serial += 1
            self._fresh_at = time.monotonic()
        if not status.connected:
            self._heartbeat = None
            self.mapper.discard_pending()
        if fresh or not status.connected:
            self._changed.set()
        self.arbiter.observe_api(status.connected, fresh)
        if self._ownership_revision != self.arbiter.revision:
            self.mapper.discard_pending()
            self._ownership_revision = self.arbiter.revision
        observations = self.mapper.observe(
            heartbeat if fresh else None, events, status.setlist_id if status.connected else None
        )
        if (
            self.mapper.stale
            and self.discovery != "running"
            and self.settings.playback_api.song_lengths
            and status.connected
        ):
            config = self.settings.playback_api.model_copy(
                update={
                    "song_lengths": [],
                    "lengths_measured_at": None,
                }
            )
            self.settings.playback_api = config
            self._service.save(
                self._service.snapshot().model_copy(update={"playback_api": config}, deep=True)
            )
        self._history.extend(MonitorEntry(item.event, item.discovery) for item in observations)
        if status.connected != self._connection:
            self._connection = status.connected
            self._put(
                (
                    generation,
                    ConnectionPayload(
                        integration="midi",
                        status=ConnectionStatus.CONNECTED
                        if status.connected
                        else ConnectionStatus.DISCONNECTED,
                        detail=status.last_error
                        or (
                            f"Playback connected at {status.host}:{status.port}"
                            if status.connected
                            else None
                        ),
                    ),
                )
            )
        for item in observations:
            if self.arbiter.allows("playback_api"):
                self._put((generation, item))

    def _put(self, item: tuple[int, Observation | ConnectionPayload]) -> None:
        try:
            self._queue.put_nowait(item)
        except asyncio.QueueFull:
            # Never replay overflow later. Monitor remains independently bounded.
            self.discovery_error = "Playback event queue full; an event was dropped."

    async def _consume(self) -> None:
        while True:
            item = await self._queue.get()
            try:
                async with self._dispatch_lock:
                    if not self._active or item[0] != self._generation:
                        continue
                    payload = item[1]
                    if isinstance(payload, ConnectionPayload):
                        await self._publish_connection()
                    elif self.arbiter.allows("playback_api"):
                        await self._publish_connection()
                        await self.mapper.dispatch(payload)
            except Exception:
                self.discovery_error = "Playback action processing failed."

    async def health(self) -> PluginHealth:
        if self.midi is not None and not self.selected:
            return await self.midi.health()
        # Health describes the lifecycle supervisor, not external connectivity.
        status = PluginStatus.RUNNING if self._running else PluginStatus.STOPPED
        if self._running and any(
            task is not None and task.done() for task in (self._consumer, self._ticker)
        ):
            status = PluginStatus.ERROR
        return PluginHealth(
            name=self.name,
            version=self.version,
            status=status,
            last_error=None
            if self.connection.connected
            else self.discovery_error or self.status.last_error,
        )

    async def find(self, host: str | None = None) -> PlaybackStatus:
        """Start a scan. Turns Playback on and selects it first if needed."""
        if host is not None:
            host = host.strip() or None
            ConnectionOptions(True, host, self.settings.playback_api.port)
        if self.discovery == "running":
            raise DiscoveryConflict("Wait for Discover Song Order before scanning.")
        # Cancel before taking the lifecycle lock: the old scan may be waiting on it.
        await self._cancel_scan()
        async with self._lifecycle:
            self._scan_cancel = threading.Event()
            self.scan_state = "scanning"
            self.scan_candidates = []
            self.scan_reason = None
            self.scan_current = self.scan_total = 0
            self.scan_error_class = None
            self.scan_typed = host is not None
            self.scan_phase = f"Connecting to {host}…" if host else "This computer"
            self.scan_started_at = time.monotonic()
            self.scan_elapsed = 0.0
            self._scan_task = asyncio.create_task(self._scan(host), name="playback-scan")
        return self.status

    async def cancel_scan(self) -> None:
        await self._cancel_scan()
        if self.scan_state == "scanning":
            self.scan_state = "idle"
            self.scan_reason = "Scan cancelled."

    async def _cancel_scan(self) -> None:
        task = self._scan_task
        self._scan_cancel.set()
        if task is not None and task is not asyncio.current_task():
            with suppress(asyncio.CancelledError, Exception):
                await task
        self._scan_task = None

    async def _adopt(self, host: str | None) -> None:
        """Select Playback, turn it on, and remember the address (one save)."""
        saved = self._service.snapshot()
        playback = saved.playback_api.model_copy(update={"enabled": True, "host": host})
        modes = saved.integration_modes.model_copy(update={"midi_source": MidiSource.PLAYBACK_API})
        # The caller holds the lifecycle lock, so use the lock-free internals.
        self._service.save(
            saved.model_copy(
                update={"playback_api": playback, "integration_modes": modes}, deep=True
            )
        )
        await self._reconfigure(self._service.effective_runtime_settings())

    def _finish_scan(
        self, state: Literal["idle", "found", "not_found"], reason: str | None
    ) -> None:
        if self.scan_started_at is not None:
            self.scan_elapsed = time.monotonic() - self.scan_started_at
        self.scan_state = state
        self.scan_reason = reason

    async def _scan(self, host: str | None) -> None:
        cancel = self._scan_cancel
        config = self.settings.playback_api
        started = time.monotonic()
        playback_log(
            "playback_scan_start", trigger="typed_host" if host else "scan_button", typed_host=host
        )

        def progress(current: int, total: int) -> None:
            self.scan_current, self.scan_total = current, total
            if self.scan_started_at is not None:
                self.scan_elapsed = time.monotonic() - self.scan_started_at

        def on_phase(name: str, current: int, total: int) -> None:
            self.scan_phase = f"Connecting to {host}…" if host else name
            self.scan_current, self.scan_total = current, total

        try:
            result = await asyncio.to_thread(
                self._scanner,
                host_override=host,
                port=config.port,
                auto_scan=True,
                cancel=cancel,
                progress=progress,
                on_phase=on_phase,
            )
            if cancel.is_set():
                return
            report = result if isinstance(result, ScanReport) else None
            found = list(result.candidates) if isinstance(result, ScanReport) else list(result)
            if report is not None:
                self.last_scan = report.summary()
                playback_log(
                    "playback_scan_result",
                    outcome=report.outcome,
                    error_class=report.error_class,
                    networks=report.networks,
                    interfaces=report.interfaces,
                    hosts_probed=report.hosts_probed,
                    hosts_total=report.hosts_total,
                    errors=report.error_counts,
                    sample_errors=report.sample_errors,
                    loopback_ok=report.loopback_ok,
                    lan_check=report.lan_check,
                    classification=report.classification,
                    classification_inputs=report.classification_inputs,
                    found=len(found),
                    duration_s=round(time.monotonic() - started, 2),
                )
            self.scan_candidates = found
            if report is not None and report.outcome == "error" and not found:
                self.scan_error_class = report.error_class
                self._finish_scan(
                    "not_found",
                    SCAN_MESSAGES.get(report.error_class or "", SCAN_MESSAGES["unexpected_error"]),
                )
                return
            if not found:
                self._finish_scan("not_found", NOT_FOUND_REASON)
                return
            if len(found) == 1:
                async with self._lifecycle:
                    if cancel.is_set() or self.discovery == "running":
                        return
                    await self._adopt(found[0].host)
            self._finish_scan("found", None)
        except SettingsFileError as exc:
            playback_log("playback_scan_save_failed", error_type=type(exc).__name__)
            self._finish_scan("not_found", "Found Playback but could not save the address.")
        except Exception as exc:
            self.scan_error_class = "unexpected_error"
            self.last_scan = {
                "outcome": "error",
                "error_class": "unexpected_error",
                "sample_errors": [f"{type(exc).__name__}: {exc}"],
                "typed_host": host,
                "duration_s": round(time.monotonic() - started, 2),
            }
            playback_log("playback_scan_exception", error_type=type(exc).__name__, error=str(exc))
            self._finish_scan("not_found", SCAN_MESSAGES["unexpected_error"])

    def _check_discovery(self, version: int | None = None) -> Heartbeat:
        heartbeat = self._heartbeat
        if (
            not self._active
            or not self.arbiter.snapshot().sources["playback_api"].connected
            or heartbeat is None
        ):
            raise DiscoveryConflict(
                "Playback is disconnected. Enable remote connections and connect first."
            )
        if heartbeat.playing:
            raise DiscoveryConflict("Stop Playback before discovering song order.")
        if heartbeat.setlist_version is None:
            raise DiscoveryConflict("Playback has not supplied a valid setlist version.")
        if version is not None and heartbeat.setlist_version != version:
            raise DiscoveryConflict("Playback setlist changed during discovery; try again.")
        return heartbeat

    async def _step(self, direction: Literal["previous", "next"], version: int) -> int:
        before = self._check_discovery(version).song_id
        if self._navigation_steps >= self._max_steps:
            raise DiscoveryConflict("Song order discovery exceeded the 200-step safety limit.")
        client = self.client
        assert client is not None
        serial = self._serial
        await client._discovery_step(direction)
        self.progress += 1
        self._navigation_steps += 1
        deadline = time.monotonic() + self._step_timeout
        while True:
            current = self._check_discovery(version)
            if current.song_id != before:
                return current.song_id
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                # At a nonwrapping end, Playback continues sending fresh unchanged
                # heartbeats. Silence is NOT evidence of an end.
                if (
                    self._serial >= serial + 2
                    and time.monotonic() - self._fresh_at < self._step_timeout / 2
                ):
                    return before
                raise DiscoveryConflict("Playback heartbeat timed out during song order discovery.")
            self._changed.clear()
            with suppress(TimeoutError):
                await asyncio.wait_for(self._changed.wait(), remaining)

    async def discover_song_order(self) -> None:
        async with self._lifecycle:
            if (
                self.discovery == "running"
                or self.planning_center_lengths.result.status == "running"
            ):
                raise DiscoveryConflict(
                    "Song order discovery or Planning Center update is already running."
                )
            heartbeat = self._check_discovery()
            async with self._dispatch_lock:
                self.mapper.set_discovery(True)
                self.discovery = "running"
                self.progress = 0
                self._navigation_steps = 0
                self.discovery_song = 0
                self.discovery_total = len(self.mapper.song_order) if not self.mapper.stale else 0
                self.discovery_error = None
                self._discovery_task = asyncio.current_task()
        scan_started = time.monotonic()
        original = heartbeat.song_id
        version = heartbeat.setlist_version
        assert version is not None
        order: list[int] = []
        try:
            current = original
            visited = {current}
            while True:
                previous = await self._step("previous", version)
                if previous == current:
                    break
                if previous in visited:
                    raise DiscoveryConflict("Playback navigation wrapped; order discovery aborted.")
                visited.add(previous)
                current = previous
            order = [current]
            lengths: list[float | None] = []
            while True:
                self.discovery_song = len(order)
                client = self.client
                assert client is not None
                try:
                    length = await client.measure_song_length(
                        settle=self._step_timeout, on_command=self._measurement_progress
                    )
                except Exception:
                    # Length reading is best effort; navigation remains authoritative.
                    length = None
                lengths.append(length)
                self._check_discovery(version)
                following = await self._step("next", version)
                if following == current:
                    break
                if following in order:
                    raise DiscoveryConflict("Playback navigation wrapped; order discovery aborted.")
                order.append(following)
                current = following
            if original not in order:
                raise DiscoveryConflict("Original Playback selection was lost during discovery.")
            self.discovery_total = len(order)
            for expected in reversed(order[order.index(original) : -1]):
                if await self._step("previous", version) != expected:
                    raise DiscoveryConflict("Could not restore the original Playback selection.")
            self._check_discovery(version)
            plan = (await self.state_store.snapshot()).plan
            saved = self.settings.playback_api.model_copy(
                update={
                    "scan_plan_key": self._scan_key(plan),
                    "song_order": order,
                    "captured_version": version,
                    "setlist_id": self.status.setlist_id,
                    "captured_at": datetime.now(UTC),
                    "song_lengths": lengths,
                    "discovery_duration_seconds": time.monotonic() - scan_started,
                    "lengths_measured_at": datetime.now(UTC),
                }
            )
            self._service.save(
                self._service.snapshot().model_copy(update={"playback_api": saved}, deep=True)
            )
            self.settings.playback_api = saved
            self.mapper.install_order(tuple(order), version, saved.setlist_id)
            self.mapper.set_skips((), tuple(saved.skipped_plan_song_ids))
            self.mapper.observe(self._heartbeat, ())
            self.discovery = "done"
        except asyncio.CancelledError:
            # Best effort, bounded Previous/Next restoration while suppression
            # is still active. Never select/play to recover a cancelled scan.
            with suppress(Exception):
                selected = self._check_discovery(version).song_id
                direction: Literal["previous", "next"] = (
                    "previous"
                    if original in order
                    and selected in order
                    and order.index(selected) > order.index(original)
                    else "next"
                )
                for _ in range(self._max_steps):
                    if self._check_discovery(version).song_id == original:
                        break
                    before = self._check_discovery(version).song_id
                    if await self._step(direction, version) == before:
                        break
            self.discovery = "failed"
            self.discovery_error = (
                "Song order discovery cancelled; no result saved. Check Playback selection."
            )
            raise
        except Exception as exc:
            self.discovery = "failed"
            self.discovery_error = f"{exc} No result saved; check Playback selection."
        finally:
            self.mapper.set_discovery(False)
            self._discovery_task = None

    def _measurement_progress(self) -> None:
        self.progress += 1
