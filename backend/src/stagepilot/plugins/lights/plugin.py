"""Event-driven OSC or MIDI output timeline for lighting applications such as Lightkey."""

from __future__ import annotations

import asyncio
from collections.abc import Sequence
from contextlib import suppress
from datetime import UTC, datetime

from stagepilot.core.actions import ActionOutcome
from stagepilot.core.config import (
    LightingCue,
    LightingMethod,
    LightsSettings,
    OscArgument,
    OscLightingCue,
    SongLightingCueMap,
    validate_osc_address,
)
from stagepilot.core.event_bus import EventBus, Subscription
from stagepilot.core.events import (
    ConnectionPayload,
    EventType,
    SongPayload,
    StagePilotEvent,
    TimerPayload,
    new_event,
)
from stagepilot.core.lights import LightingOutputSummary, LightsSnapshot
from stagepilot.core.logging import get_logger
from stagepilot.core.plugin import Plugin
from stagepilot.core.state import StateStore
from stagepilot.models.state import ConnectionStatus, PluginHealth, PluginStatus, Song
from stagepilot.plugins.lights.client import (
    MidiOutputBackendContract,
    MidiOutputBackendFactory,
    MidiOutputPortContract,
    MidoMidiOutputBackend,
)
from stagepilot.plugins.lights.osc import (
    OscOutputBackendFactory,
    OscOutputContract,
    UdpOscOutputBackend,
)

type AnyLightingCue = LightingCue | OscLightingCue


class LightsPlugin(Plugin):
    """Send each song's elapsed-time cue map through the saved method only.

    OSC cues are one-shot UDP messages. MIDI cues are short note pulses. The inactive
    method's cues stay saved but are never scheduled, and the two are never sent together.
    """

    name = "lights"
    version = "0.1.0"

    def __init__(
        self,
        event_bus: EventBus,
        state_store: StateStore,
        settings: LightsSettings,
        *,
        backend_factory: MidiOutputBackendFactory | None = None,
        osc_backend_factory: OscOutputBackendFactory | None = None,
        monitor_interval: float = 2.0,
    ) -> None:
        super().__init__(event_bus, state_store)
        self._settings = settings
        self._backend_factory = backend_factory or MidoMidiOutputBackend
        if monitor_interval <= 0:
            raise ValueError("Lighting MIDI monitor interval must be positive.")
        self._osc_backend_factory = osc_backend_factory or UdpOscOutputBackend
        self._monitor_interval = monitor_interval
        self._backend: MidiOutputBackendContract | None = None
        self._port: MidiOutputPortContract | None = None
        self._osc_output: OscOutputContract | None = None
        self._subscriptions: list[Subscription] = []
        self._pending_song: Song | None = None
        self._timeline_task: asyncio.Task[None] | None = None
        self._connection_task: asyncio.Task[None] | None = None
        self._stop_event = asyncio.Event()
        self._operation_lock = asyncio.Lock()
        self._status = PluginStatus.STOPPED
        self._connection_status = ConnectionStatus.DISCONNECTED
        self._detail: str | None = None
        self._last_error: str | None = None
        self._last_activity_at: datetime | None = None
        self._last_cue: LightingCue | None = None
        self._last_osc_cue: OscLightingCue | None = None
        self._last_cue_at: datetime | None = None
        self._logger = get_logger(self.name)

    async def start(self) -> None:
        self._stop_event.clear()
        self._status = PluginStatus.STARTING
        self._backend = self._backend_factory()
        self._subscriptions.extend(
            [
                await self.event_bus.subscribe(EventType.SONG_STARTED, self._on_song_event),
                await self.event_bus.subscribe(EventType.SONG_RESTARTED, self._on_song_event),
                await self.event_bus.subscribe(EventType.TIMER_STARTED, self._on_timer_started),
                await self.event_bus.subscribe(
                    EventType.TIMER_RESUMED, self._on_timer_repositioned
                ),
                await self.event_bus.subscribe(EventType.TIMER_SEEKED, self._on_timer_repositioned),
                await self.event_bus.subscribe(
                    EventType.TIMER_STOP_REQUESTED, self._on_timer_stopped
                ),
                await self.event_bus.subscribe(
                    EventType.TIMER_RESET_REQUESTED, self._on_timer_stopped
                ),
                await self.event_bus.subscribe(EventType.TIMER_STOPPED, self._on_timer_stopped),
                await self.event_bus.subscribe(EventType.TIMER_FAILED, self._on_timer_stopped),
            ]
        )
        self._status = PluginStatus.RUNNING
        if self._settings.enabled:
            await self._connect()
        else:
            await self._set_connection(
                ConnectionStatus.DISCONNECTED,
                self._not_configured_detail(),
            )
        self._connection_task = asyncio.create_task(
            self._supervise_connection(),
            name="stagepilot-lights-connection",
        )

    async def stop(self) -> None:
        self._status = PluginStatus.STOPPING
        self._stop_event.set()
        connection_task = self._connection_task
        self._connection_task = None
        if connection_task is not None:
            connection_task.cancel()
            with suppress(asyncio.CancelledError):
                await connection_task
        await self._cancel_timeline()
        for subscription in self._subscriptions:
            await self.event_bus.unsubscribe(subscription)
        self._subscriptions.clear()
        async with self._operation_lock:
            await self._close_port_locked()
            self._backend = None
        self._status = PluginStatus.STOPPED
        await self._set_connection(
            ConnectionStatus.DISCONNECTED,
            "Disconnected. Lighting output stopped.",
        )

    async def health(self) -> PluginHealth:
        return PluginHealth(
            name=self.name,
            version=self.version,
            status=self._status,
            last_error=self._last_error,
            last_activity_at=self._last_activity_at,
        )

    async def snapshot(self, *, refresh: bool = False) -> LightsSnapshot:
        if refresh:
            await self._refresh_connection()
        midi_active = self._settings.method is LightingMethod.MIDI
        names = await self._list_output_names() if midi_active else []
        return LightsSnapshot(
            enabled=self._settings.enabled,
            method=self._settings.method,
            osc_host=self._settings.osc_host,
            osc_port=self._settings.osc_port,
            output_name=self._settings.output_name,
            channel=self._settings.channel,
            pulse_ms=self._settings.pulse_ms,
            connection_status=self._connection_status,
            detail=self._detail,
            outputs=[
                LightingOutputSummary(
                    name=name,
                    ambiguous=names.count(name) > 1,
                    selected=name == self._settings.output_name,
                    connected=(
                        name == self._settings.output_name
                        and self._port is not None
                        and not self._port.closed
                    ),
                )
                for name in names
            ],
            last_cue=self._last_cue,
            last_osc_cue=self._last_osc_cue,
            last_cue_at=self._last_cue_at,
        )

    async def reconfigure(self, settings: LightsSettings) -> ActionOutcome:
        await self._cancel_timeline()
        async with self._operation_lock:
            await self._close_port_locked()
            self._settings = settings
        if not settings.enabled:
            self._status = PluginStatus.RUNNING
            self._last_error = None
            await self._set_connection(
                ConnectionStatus.DISCONNECTED,
                "Disconnected. Lighting output is disabled.",
            )
            return ActionOutcome(True, "Lighting output disabled.")
        connected = await self._connect()
        return ActionOutcome(
            connected,
            self._detail or ("Lighting output ready." if connected else "Connection failed."),
        )

    async def replace_cue_map(self, cue_map: SongLightingCueMap) -> None:
        cue_maps = dict(self._settings.cue_maps)
        if cue_map.cues or cue_map.osc_cues:
            cue_maps[cue_map.song_key] = cue_map
        else:
            cue_maps.pop(cue_map.song_key, None)
        self._settings = self._settings.model_copy(update={"cue_maps": cue_maps}, deep=True)

    async def test_osc_cue(self, address: str, args: list[OscArgument]) -> ActionOutcome:
        if self._settings.method is not LightingMethod.OSC:
            return ActionOutcome(False, "Lighting is set to MIDI. Switch to OSC and save first.")
        try:
            address = validate_osc_address(address)
        except ValueError as exc:
            return ActionOutcome(False, str(exc))
        if (self._osc_output is None or self._osc_output.closed) and not await self._connect():
            return ActionOutcome(False, self._detail or "Disconnected. OSC output is unavailable.")
        cue = OscLightingCue(at_seconds=0, address=address, args=args, label="Test cue")
        try:
            await self._send_osc(cue)
        except Exception as exc:
            await self._record_error(exc, "OSC test cue failed.")
            return ActionOutcome(False, self._detail or "OSC test cue failed.")
        target = f"{self._settings.osc_host}:{self._settings.osc_port}"
        await self._set_connection(ConnectionStatus.CONNECTED, f"Sent {address} to {target}.")
        return ActionOutcome(True, f"Sent {address} to {target} (UDP, delivery not confirmed).")

    async def test_cue(self, note: int, velocity: int) -> ActionOutcome:
        if self._settings.method is not LightingMethod.MIDI:
            return ActionOutcome(False, "Lighting is set to OSC. Switch to MIDI and save first.")
        cue = LightingCue(at_seconds=0, note=note, velocity=velocity, label="Test cue")
        if (self._port is None or self._port.closed) and not await self._connect():
            return ActionOutcome(False, self._detail or "Lighting MIDI output is unavailable.")
        try:
            await self._send_pulse(cue)
        except Exception as exc:
            await self._record_error(exc, "Lighting test cue failed.")
            return ActionOutcome(False, self._detail or "Lighting test cue failed.")
        return ActionOutcome(True, f"Sent lighting test cue on MIDI note {note}.")

    async def _on_song_event(self, event: StagePilotEvent) -> None:
        if not isinstance(event.payload, SongPayload):
            return
        await self._cancel_timeline()
        self._pending_song = event.payload.song

    async def _on_timer_started(self, event: StagePilotEvent) -> None:
        if not isinstance(event.payload, TimerPayload):
            return
        await self._cancel_timeline()
        song = event.payload.song or self._pending_song
        if song is None:
            return
        song_key = song.source_song_id or song.id
        cue_map = self._settings.cue_maps.get(song_key)
        active = self._active_cues(cue_map)
        if not self._settings.enabled or not active:
            return
        if not self._output_ready() and not await self._connect():
            return
        cues = [cue for cue in active if cue.at_seconds <= event.payload.duration_seconds]
        if not cues:
            return
        self._timeline_task = asyncio.create_task(
            self._run_timeline(cues, event.payload.started_at),
            name=f"stagepilot-lights-{song_key}",
        )

    async def _on_timer_repositioned(self, event: StagePilotEvent) -> None:
        """Resume/seek: restart the timeline at the song position, upcoming cues only."""
        payload = event.payload
        if not isinstance(payload, TimerPayload):
            return
        await self._cancel_timeline()
        if not payload.running or payload.position_seconds is None:
            return  # stopped: send nothing
        position = max(0.0, payload.position_seconds)
        song = payload.song or self._pending_song
        if song is None:
            return
        song_key = song.source_song_id or song.id
        active = self._active_cues(self._settings.cue_maps.get(song_key))
        if not self._settings.enabled or not active:
            return
        total = position + payload.duration_seconds
        cues = [cue for cue in active if position <= cue.at_seconds <= total]
        if not cues:
            return
        if not self._output_ready() and not await self._connect():
            return
        self._timeline_task = asyncio.create_task(
            self._run_timeline(cues, payload.started_at, position),
            name=f"stagepilot-lights-{song_key}",
        )

    async def _on_timer_stopped(self, _event: StagePilotEvent) -> None:
        await self._cancel_timeline()

    async def _run_timeline(
        self,
        cues: Sequence[AnyLightingCue],
        started_at: datetime | None,
        offset: float = 0.0,
    ) -> None:
        elapsed_before_confirmation = (
            max(0.0, (datetime.now(UTC) - started_at).total_seconds())
            if started_at is not None
            else 0.0
        )
        started = asyncio.get_running_loop().time() - elapsed_before_confirmation - offset
        tasks = [
            asyncio.create_task(self._send_at(started, cue), name=f"lighting-cue-{cue.id}")
            for cue in cues
        ]
        try:
            await asyncio.gather(*tasks)
        except asyncio.CancelledError:
            raise
        except Exception as exc:
            await self._record_error(exc, "A lighting cue could not be sent.")
        finally:
            for task in tasks:
                task.cancel()
            await asyncio.gather(*tasks, return_exceptions=True)

    async def _send_at(self, started: float, cue: AnyLightingCue) -> None:
        delay = max(0.0, started + cue.at_seconds - asyncio.get_running_loop().time())
        if delay:
            await asyncio.sleep(delay)
        if isinstance(cue, OscLightingCue):
            await self._send_osc(cue)
        else:
            await self._send_pulse(cue)

    def _active_cues(self, cue_map: SongLightingCueMap | None) -> Sequence[AnyLightingCue]:
        """Only the saved method's cues; the other method's cues stay saved but idle."""

        if cue_map is None:
            return []
        if self._settings.method is LightingMethod.OSC:
            return cue_map.osc_cues
        return cue_map.cues

    def _output_ready(self) -> bool:
        if self._settings.method is LightingMethod.OSC:
            return self._osc_output is not None and not self._osc_output.closed
        return self._port is not None and not self._port.closed

    def _not_configured_detail(self) -> str:
        if self._settings.method is LightingMethod.OSC:
            return "Disconnected. Lighting OSC output is not configured."
        return "Lighting MIDI output is not configured."

    async def _send_osc(self, cue: OscLightingCue) -> None:
        if self._settings.method is not LightingMethod.OSC:
            return  # never send OSC while MIDI is the saved method
        output = self._osc_output
        if output is None or output.closed:
            raise RuntimeError("Disconnected. The OSC output closed before a cue was sent.")
        await asyncio.to_thread(output.send_message, cue.address, cue.args)
        self._last_osc_cue = cue
        self._last_cue_at = datetime.now(UTC)
        self._last_activity_at = self._last_cue_at

    async def _send_pulse(self, cue: LightingCue) -> None:
        if self._settings.method is not LightingMethod.MIDI:
            return  # never send MIDI while OSC is the saved method
        port = self._port
        if port is None or port.closed:
            raise RuntimeError("Lighting MIDI output disconnected before a cue was sent.")
        await asyncio.to_thread(
            port.send_note_on,
            self._settings.channel,
            cue.note,
            cue.velocity,
        )
        try:
            await asyncio.sleep(self._settings.pulse_ms / 1_000)
        finally:
            try:
                await self._send_note_off(port, cue.note)
            except asyncio.CancelledError:
                raise
            except Exception:
                self._logger.warning("lighting_note_off_failed", note=cue.note)
        self._last_cue = cue
        self._last_cue_at = datetime.now(UTC)
        self._last_activity_at = self._last_cue_at

    async def _send_note_off(self, port: MidiOutputPortContract, note: int) -> None:
        task = asyncio.create_task(
            asyncio.to_thread(port.send_note_off, self._settings.channel, note)
        )
        try:
            await asyncio.shield(task)
        except asyncio.CancelledError:
            await task
            raise

    async def _cancel_timeline(self) -> None:
        task = self._timeline_task
        self._timeline_task = None
        if task is None:
            return
        task.cancel()
        with suppress(asyncio.CancelledError):
            await task

    async def _connect_osc(self) -> bool:
        settings = self._settings
        target = f"{settings.osc_host}:{settings.osc_port}"
        async with self._operation_lock:
            await self._close_port_locked()
            try:
                self._osc_output = await asyncio.to_thread(
                    self._osc_backend_factory().open_output,
                    settings.osc_host,
                    settings.osc_port,
                )
            except Exception as exc:
                self._status = PluginStatus.ERROR
                self._last_error = self._public_error(exc, f"OSC output to {target} failed.")
                await self._set_connection(ConnectionStatus.ERROR, self._last_error)
                return False
        self._status = PluginStatus.RUNNING
        self._last_error = None
        self._last_activity_at = datetime.now(UTC)
        # UDP has no handshake: "ready" means the socket opened, not that anything listens.
        await self._set_connection(
            ConnectionStatus.CONNECTED,
            f"OSC ready for {target}. Nothing sent yet; UDP does not confirm delivery.",
        )
        return True

    async def _connect(self) -> bool:
        if not self._settings.enabled:
            return False
        if self._settings.method is LightingMethod.OSC:
            return await self._connect_osc()
        if not self._settings.output_name:
            self._status = PluginStatus.RUNNING
            self._last_error = None
            await self._set_connection(
                ConnectionStatus.DISCONNECTED,
                "No lighting MIDI output is selected.",
            )
            return False
        try:
            output_names = await asyncio.to_thread(self._require_backend().list_output_names)
        except Exception as exc:
            await self._record_error(exc, "Lighting MIDI outputs could not be listed.")
            return False
        matches = output_names.count(self._settings.output_name)
        if matches == 0:
            self._status = PluginStatus.RUNNING
            self._last_error = None
            await self._set_connection(
                ConnectionStatus.DISCONNECTED,
                f'Lighting MIDI output "{self._settings.output_name}" is unavailable; retrying.',
            )
            return False
        if matches > 1:
            self._status = PluginStatus.ERROR
            self._last_error = (
                f'Multiple lighting MIDI outputs are named "{self._settings.output_name}".'
            )
            await self._set_connection(ConnectionStatus.ERROR, self._last_error)
            return False
        await self._set_connection(
            ConnectionStatus.CONNECTING,
            f"Connecting to lighting MIDI output {self._settings.output_name}.",
        )
        async with self._operation_lock:
            await self._close_port_locked()
            try:
                backend = self._require_backend()
                self._port = await asyncio.to_thread(
                    backend.open_output,
                    self._settings.output_name,
                )
            except Exception as exc:
                self._status = PluginStatus.ERROR
                self._last_error = self._public_error(exc, "Lighting MIDI output failed.")
                await self._set_connection(ConnectionStatus.ERROR, self._last_error)
                return False
        self._status = PluginStatus.RUNNING
        self._last_error = None
        self._last_activity_at = datetime.now(UTC)
        await self._set_connection(
            ConnectionStatus.CONNECTED,
            f"Connected to lighting MIDI output {self._settings.output_name}.",
        )
        return True

    async def _refresh_connection(self) -> None:
        if not self._settings.enabled:
            return
        if not self._output_ready():
            await self._connect()

    async def _supervise_connection(self) -> None:
        while not self._stop_event.is_set():
            try:
                await asyncio.wait_for(
                    self._stop_event.wait(),
                    timeout=self._monitor_interval,
                )
                return
            except TimeoutError:
                pass
            if not self._settings.enabled or self._settings.method is not LightingMethod.MIDI:
                continue  # OSC over UDP has nothing to monitor
            try:
                names = await asyncio.to_thread(self._require_backend().list_output_names)
            except Exception as exc:
                await self._record_error(exc, "Lighting MIDI outputs could not be listed.")
                continue

            configured_name = self._settings.output_name
            port = self._port
            matches = names.count(configured_name) if configured_name else 0
            if not configured_name or matches != 1 or port is None or port.closed:
                async with self._operation_lock:
                    await self._close_port_locked()
                if matches > 1:
                    self._status = PluginStatus.ERROR
                    self._last_error = (
                        f'Multiple lighting MIDI outputs are named "{configured_name}".'
                    )
                    await self._set_connection(ConnectionStatus.ERROR, self._last_error)
                else:
                    self._status = PluginStatus.RUNNING
                    self._last_error = None
                    await self._set_connection(
                        ConnectionStatus.DISCONNECTED,
                        (
                            f'Lighting MIDI output "{configured_name}" is unavailable; retrying.'
                            if configured_name
                            else "No lighting MIDI output is selected."
                        ),
                    )
                if configured_name and matches == 1:
                    await self._connect()

    async def _list_output_names(self) -> list[str]:
        try:
            return await asyncio.to_thread(self._require_backend().list_output_names)
        except Exception as exc:
            await self._record_error(exc, "Lighting MIDI outputs could not be listed.")
            return []

    async def _close_port_locked(self) -> None:
        port = self._port
        self._port = None
        if port is not None and not port.closed:
            await asyncio.to_thread(port.close)
        osc_output = self._osc_output
        self._osc_output = None
        if osc_output is not None and not osc_output.closed:
            await asyncio.to_thread(osc_output.close)

    async def _record_error(self, exc: BaseException, fallback: str) -> None:
        detail = self._public_error(exc, fallback)
        self._status = PluginStatus.ERROR
        self._last_error = detail
        self._last_activity_at = datetime.now(UTC)
        self._logger.error("lighting_midi_failed", detail=detail)
        await self._set_connection(ConnectionStatus.ERROR, detail)

    async def _set_connection(self, status: ConnectionStatus, detail: str) -> None:
        self._connection_status = status
        self._detail = detail
        await self.event_bus.publish(
            new_event(
                EventType.CONNECTION_CHANGED,
                source=self.name,
                payload=ConnectionPayload(
                    integration="lights",
                    status=status,
                    detail=detail,
                ),
            )
        )

    def _require_backend(self) -> MidiOutputBackendContract:
        if self._backend is None:
            self._backend = self._backend_factory()
        return self._backend

    @staticmethod
    def _public_error(exc: BaseException, fallback: str) -> str:
        if isinstance(exc, (ValueError, RuntimeError)):
            return str(exc)
        return fallback
