from __future__ import annotations

import asyncio
import base64
import hashlib
import json
import threading
import time
from collections.abc import AsyncIterator, Callable
from contextlib import asynccontextmanager, suppress
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, cast
from unittest.mock import AsyncMock

import httpx
import pytest
from fastapi import FastAPI

from stagepilot.core.config import (
    IntegrationModes,
    MidiSettings,
    MidiSource,
    PlaybackApiSettings,
    Settings,
)
from stagepilot.core.events import ActionName
from stagepilot.core.settings import (
    MemoryCredentialStore,
    SettingsFileError,
    SettingsFileStore,
    SettingsService,
)
from stagepilot.main import create_app
from stagepilot.plugins.midi_playback.models import MidiMessage
from stagepilot.plugins.playback_api.client import ConnectionOptions, Observer, PlaybackClient
from stagepilot.plugins.playback_api.find import DiscoveryResult, ScanCandidate
from stagepilot.plugins.playback_api.normalizer import Heartbeat, PlaybackEvent
from stagepilot.plugins.playback_api.plugin import PlaybackInputPlugin
from test_midi_config_api import FakeMidiBackend
from test_playback_api_transport import frame, heartbeat


@dataclass
class FakePlayback:
    songs: list[int] = field(default_factory=lambda: [900, 17, 42])
    index: int = 1
    playing: bool = False
    position: float = 0
    lengths: list[float | None] = field(default_factory=lambda: [279.53, 265.85, 260.17])
    version: int = 8
    commands: list[str] = field(default_factory=list)
    silent: bool = False
    heartbeat_timeout: float = 0.25
    disconnect_on_command: bool = False
    version_on_command: bool = False
    wrap: bool = False
    port: int = 0
    tasks: set[asyncio.Task[None]] = field(default_factory=set)
    writers: set[asyncio.StreamWriter] = field(default_factory=set)

    async def serve(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        self.writers.add(writer)
        sender: asyncio.Task[None] | None = None
        try:
            request = (await reader.readuntil(b"\r\n\r\n")).decode()
            headers = dict(line.split(": ", 1) for line in request.split("\r\n") if ": " in line)
            accepted = base64.b64encode(
                hashlib.sha1(
                    (
                        headers["Sec-WebSocket-Key"] + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
                    ).encode(),
                    usedforsecurity=False,
                ).digest()
            ).decode()
            writer.write(
                (
                    "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n"
                    "Connection: Upgrade\r\n" + f"Sec-WebSocket-Accept: {accepted}\r\n"
                    "Sec-WebSocket-Protocol: pr-protocol\r\n\r\n"
                ).encode()
            )
            await writer.drain()

            async def send() -> None:
                while True:
                    if not self.silent:
                        writer.write(
                            frame(
                                heartbeat(
                                    self.songs[self.index],
                                    self.position,
                                    self.playing,
                                    self.version,
                                ).encode()
                            )
                        )
                        await writer.drain()
                    await asyncio.sleep(0.05)

            sender = asyncio.create_task(send())
            while True:
                header = await reader.readexactly(2)
                size = header[1] & 127
                assert header[1] & 128 and size < 126
                mask = await reader.readexactly(4)
                payload = await reader.readexactly(size)
                payload = bytes(value ^ mask[i % 4] for i, value in enumerate(payload))
                if header[0] & 15 != 1:
                    continue
                message = json.loads(payload)
                command = next(iter(message))
                assert command in {
                    "transportPreviousSong",
                    "transportNextSong",
                    "waveformSeek",
                    "transportReturnToStart",
                }
                self.commands.append(command)
                if self.disconnect_on_command:
                    return
                if self.version_on_command:
                    self.version += 1
                if command == "waveformSeek":
                    assert message[command] == {"sequenceTime": 86400.0}
                    if self.lengths[self.index] is not None:
                        self.position = self.lengths[self.index] or 0
                    await self.broadcast(payload.decode())
                    continue
                if command == "transportReturnToStart":
                    self.position = 0
                    await self.broadcast(payload.decode())
                    continue
                delta = -1 if command == "transportPreviousSong" else 1
                self.index = (
                    (self.index + delta) % len(self.songs)
                    if self.wrap
                    else max(0, min(len(self.songs) - 1, self.index + delta))
                )
        except (asyncio.IncompleteReadError, ConnectionError):
            pass
        finally:
            if sender:
                sender.cancel()
                with suppress(asyncio.CancelledError, ConnectionError):
                    await sender
            writer.close()
            await writer.wait_closed()
            self.writers.discard(writer)

    def accept(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        task = asyncio.create_task(self.serve(reader, writer))
        self.tasks.add(task)

    async def broadcast(self, raw: str) -> None:
        for writer in tuple(self.writers):
            writer.write(frame(raw.encode()))
            await writer.drain()

    def client(self, options: ConnectionOptions, observer: Observer) -> PlaybackClient:
        return PlaybackClient(
            options,
            observer=observer,
            finder=lambda _: DiscoveryResult(
                "127.0.0.1", self.port, "loopback", Heartbeat(900, 0, False, False, 8)
            ),
            reconnect_delays=(0.02,),
            heartbeat_timeout=self.heartbeat_timeout,
        )


async def wait_for(predicate: Callable[[], bool]) -> None:
    async with asyncio.timeout(10):
        for _ in range(2000):
            if predicate():
                return
            await asyncio.sleep(0.005)
        raise AssertionError("Fake endpoint condition did not become true")


def controller(app: FastAPI) -> PlaybackInputPlugin:
    return cast(PlaybackInputPlugin, app.state.runtime.playback_input)


@asynccontextmanager
async def application(
    fake: FakePlayback,
    *,
    saved: bool = False,
    midi: FakeMidiBackend | None = None,
    settings_service: SettingsService | None = None,
    fast_transport: bool = True,
) -> AsyncIterator[tuple[FastAPI, httpx.AsyncClient]]:
    server = await asyncio.start_server(fake.accept, "127.0.0.1", 0)
    fake.port = server.sockets[0].getsockname()[1]
    settings = Settings(
        playback_api=PlaybackApiSettings(
            song_order=fake.songs if saved else [],
            captured_version=8 if saved else None,
            fast_transport=fast_transport,
        ),
        midi=MidiSettings(enabled=True, input_name="Playback MIDI"),
    )
    app = create_app(
        settings_service.load() if settings_service else settings,
        settings_service=settings_service,
        playback_client_factory=fake.client,
        midi_backend_factory=(lambda: midi) if midi else None,
    )
    inputs = controller(app)
    inputs._step_timeout = 0.6
    try:
        async with (
            app.router.lifespan_context(app),
            httpx.AsyncClient(
                transport=httpx.ASGITransport(app=app), base_url="http://testserver"
            ) as client,
        ):
            await wait_for(lambda: inputs.status.connected)
            yield app, client
    finally:
        server.close()
        await server.wait_closed()
        for writer in tuple(fake.writers):
            writer.close()
        for task in fake.tasks:
            task.cancel()
        await asyncio.gather(*fake.tasks, return_exceptions=True)


@pytest.mark.parametrize("initial", [0, 1, 2])
async def test_loopback_discovery_ends_restore_persist_and_suppress(initial: int) -> None:
    fake = FakePlayback(index=initial)
    async with application(fake) as (app, client):
        inputs = controller(app)
        dispatcher = AsyncMock()
        inputs.mapper._dispatcher = dispatcher
        operation = asyncio.create_task(
            client.post("/api/v1/playback-api/discover-song-order", json={"confirm": True})
        )
        await wait_for(lambda: inputs.discovery == "running")
        inputs._observe(
            inputs._generation,
            inputs.status,
            (
                PlaybackEvent("song.started", 1, 900),
                PlaybackEvent("song.paused", 1, 900),
                PlaybackEvent("song.stopped", 1, 900),
            ),
        )
        result = await operation
        assert result.status_code == 200
        data = result.json()
        assert data["discovery"] == "done"
        assert data["song_order"] == [900, 17, 42]
        assert data["captured_version"] == 8 and data["captured_at"]
        assert not data["stale"] and fake.index == initial
        await asyncio.sleep(0.25)
        dispatcher.dispatch.assert_not_awaited()
        dispatcher.dispatch_song_position.assert_not_awaited()
        events = (await client.get("/api/v1/playback-api/events")).json()["events"]
        assert any(item["discovery"] for item in events)
        persisted = app.state.runtime.settings_service.snapshot().playback_api
        assert persisted.song_order == fake.songs and persisted.captured_version == 8
        assert set(fake.commands) == {
            "transportPreviousSong",
            "transportNextSong",
            "waveformSeek",
            "transportReturnToStart",
        }
        assert data["song_lengths"] == fake.lengths
        assert data["lengths_measured_at"]
        assert persisted.song_lengths == fake.lengths
        assert fake.position == 0


@pytest.mark.parametrize(
    "payload", [None, {}, {"confirm": False}, {"confirm": 1}, {"confirm": "true"}]
)
async def test_confirmation_is_server_enforced(payload: object) -> None:
    fake = FakePlayback()
    async with application(fake) as (_, client):
        assert (
            await client.post("/api/v1/playback-api/discover-song-order", json=payload)
        ).status_code == 400
        assert not fake.commands


async def test_conflicts_playing_disconnected_running_and_settings() -> None:
    fake = FakePlayback(playing=True)
    async with application(fake) as (app, client):
        inputs = controller(app)
        response = await client.post(
            "/api/v1/playback-api/discover-song-order", json={"confirm": True}
        )
        assert response.status_code == 409 and "Stop Playback" in response.text
        fake.playing = False
        await wait_for(
            lambda: inputs.status.heartbeat is not None and not inputs.status.heartbeat.playing
        )
        task = asyncio.create_task(inputs.discover_song_order())
        await wait_for(lambda: inputs.discovery == "running")
        assert (
            await client.post("/api/v1/playback-api/discover-song-order", json={"confirm": True})
        ).status_code == 409
        assert (await client.post("/api/v1/playback-api/find")).status_code == 409
        saved = app.state.runtime.settings_service.snapshot()
        assert (
            await client.put("/api/v1/settings", json=saved.model_dump(mode="json"))
        ).status_code == 409
        assert (
            await client.put("/api/v1/playback-api/settings", json={"enabled": False})
        ).status_code == 409
        assert app.state.runtime.settings_service.snapshot() == saved
        await task
        assert (
            await client.put("/api/v1/playback-api/settings", json={"enabled": False})
        ).status_code == 200
        assert (
            await client.post("/api/v1/playback-api/discover-song-order", json={"confirm": True})
        ).status_code == 409


@pytest.mark.parametrize("failure", ["disconnect", "silence", "version", "wrap", "limit"])
async def test_failure_never_persists_partial_result(failure: str) -> None:
    fake = FakePlayback()
    async with application(fake, saved=True) as (app, client):
        inputs = controller(app)
        before = app.state.runtime.settings_service.snapshot().playback_api
        fake.disconnect_on_command = failure == "disconnect"
        fake.silent = failure == "silence"
        fake.version_on_command = failure == "version"
        fake.wrap = failure == "wrap"
        if failure == "limit":
            inputs._max_steps = 1
        result = await client.post(
            "/api/v1/playback-api/discover-song-order", json={"confirm": True}
        )
        assert result.status_code == 200 and result.json()["discovery"] == "failed"
        assert result.json()["last_error"]
        assert app.state.runtime.settings_service.snapshot().playback_api == before
        assert not inputs.mapper.discovery


async def test_live_mapping_known_restart_stale_unknown_and_monitor_bound() -> None:
    fake = FakePlayback(index=0)
    async with application(fake, saved=True) as (app, client):
        inputs = controller(app)
        fake.playing = True
        await wait_for(lambda: app.state.runtime.state_store._state.current_song is not None)
        state = (await client.get("/api/v1/state")).json()
        assert state["current_song_index"] == 0
        fake.position = 3
        await asyncio.sleep(0.25)
        fake.playing = False
        await asyncio.sleep(0.25)
        fake.position = 0
        await asyncio.sleep(0.25)
        fake.playing = True
        await asyncio.sleep(0.25)
        state = (await client.get("/api/v1/state")).json()
        assert any(event["type"] == "song.restarted" for event in state["recent_events"])
        dispatcher = AsyncMock()
        inputs.mapper._dispatcher = dispatcher
        fake.index = 1
        await wait_for(lambda: dispatcher.dispatch_song_position.await_count == 1)
        dispatcher.dispatch_song_position.assert_awaited_once_with(2, source="playback_api")
        fake.version = 9
        fake.index = 2
        await wait_for(lambda: inputs.mapper.stale)
        await asyncio.sleep(0.25)
        assert dispatcher.dispatch_song_position.await_count == 1
        fake.playing = False
        await wait_for(lambda: dispatcher.dispatch.await_count == 1)
        dispatcher.dispatch.assert_awaited_with(ActionName.STOP_TIMER, source="playback_api")
        fake.songs[2] = 12345
        await asyncio.sleep(0.25)
        assert inputs.mapper.stale
        # Normal receive operations have never sent an application command.
        assert fake.commands == []
        from stagepilot.plugins.playback_api.normalizer import PlaybackEvent

        for i in range(150):
            inputs._observe(inputs._generation, inputs.status, (PlaybackEvent("seek", float(i)),))
        monitor = (await client.get("/api/v1/playback-api/events")).json()
        assert len(monitor["events"]) == monitor["capacity"] == 100
        assert monitor["events"][-1]["event"]["type"] == "seek"


async def test_reconnect_revalidates_and_source_switch_closes_old_listeners() -> None:
    fake = FakePlayback(index=0)
    midi = FakeMidiBackend(["Playback MIDI"])
    async with application(fake, saved=True, midi=midi) as (app, client):
        inputs = controller(app)
        dispatcher = AsyncMock()
        inputs.mapper._dispatcher = dispatcher
        for writer in tuple(fake.writers):
            writer.close()
        await wait_for(lambda: not inputs.status.connected)
        await wait_for(lambda: inputs.status.connected)
        assert not inputs.mapper.stale
        dispatcher.dispatch_song_position.assert_not_awaited()
        old_client = inputs.client
        assert old_client is not None
        old_observer = old_client._observer
        saved = (await client.get("/api/v1/settings")).json()["settings"]
        saved["integration_modes"]["midi_source"] = "real"
        result = await client.put("/api/v1/settings", json=saved)
        assert result.status_code == 200 and not result.json()["restart_required"]
        await wait_for(lambda: bool(midi.ports))
        assert not old_client.status.connected
        assert (await client.get("/api/v1/midi/inputs")).json()["enabled"]
        assert old_observer is not None
        from stagepilot.plugins.playback_api.normalizer import PlaybackEvent

        old_observer(old_client.status, (PlaybackEvent("song.started", 1, 900),))
        dispatcher.dispatch_song_position.assert_not_awaited()
        await client.post("/api/v1/midi/cue-simulation", json={"cue": "start_next"})
        assert (await client.get("/api/v1/state")).json()["current_song_index"] == 0
        old_port = midi.ports[-1]
        saved["integration_modes"]["midi_source"] = "playback_api"
        assert (await client.put("/api/v1/settings", json=saved)).status_code == 200
        assert old_port.closed
        await wait_for(lambda: inputs.status.connected)
        state_before = (await client.get("/api/v1/state")).json()["current_song_index"]
        old_port.callback(MidiMessage(type="note_on", channel=1, note=112, velocity=2))
        await asyncio.sleep(0.25)
        assert (await client.get("/api/v1/state")).json()["current_song_index"] == state_before
        assert (
            await client.post("/api/v1/midi/cue-simulation", json={"cue": "start_next"})
        ).status_code == 409


async def test_settings_override_disable_find_no_order_edit_or_controls() -> None:
    fake = FakePlayback()
    async with application(fake, saved=True) as (app, client):
        result = await client.put(
            "/api/v1/playback-api/settings",
            json={
                "host": "192.0.2.10",
                "port": 8081,
                "auto_scan": False,
            },
        )
        assert result.status_code == 200
        inputs = controller(app)
        assert inputs.client is not None
        assert inputs.client.options.host_override == "192.0.2.10"
        assert inputs.client.options.port == 8081 and not inputs.client.options.auto_scan
        assert (await client.post("/api/v1/playback-api/find")).status_code == 200
        assert (
            await client.put("/api/v1/playback-api/settings", json={"song_order": [1]})
        ).status_code == 422
        saved = (await client.get("/api/v1/settings")).json()["settings"]
        saved["playback_api"]["song_order"] = [1]
        assert (await client.put("/api/v1/settings", json=saved)).status_code == 400
        for command in ["play", "pause", "select", "seek", "confirm-song-order"]:
            path = f"/api/v1/playback-api/{command}"
            assert path not in app.openapi()["paths"]
            # A built dashboard's GET-only static catch-all returns 405; without
            # a dashboard, the same absent control route returns 404.
            assert (await client.post(path)).status_code in {404, 405}
        assert (
            await client.put("/api/v1/playback-api/settings", json={"host": "http://bad/"})
        ).status_code == 422
        assert fake.commands == []


async def test_shutdown_cancels_discovery_without_saving() -> None:
    fake = FakePlayback()
    async with application(fake) as (app, _):
        inputs = controller(app)
        fake.silent = True
        task = asyncio.create_task(inputs.discover_song_order())
        await wait_for(lambda: inputs.discovery == "running")
        await inputs.stop()
        assert task.cancelled()
        assert inputs.discovery == "failed" and not inputs.mapper.discovery
        assert inputs.client is not None and not inputs.client.status.connected
        assert inputs._consumer is None
        assert app.state.runtime.settings_service.snapshot().playback_api.song_order == []


async def test_default_status_remote_connections_diagnostic_and_disable() -> None:
    app = create_app(Settings())
    async with (
        app.router.lifespan_context(app),
        httpx.AsyncClient(
            transport=httpx.ASGITransport(app=app), base_url="http://testserver"
        ) as client,
    ):
        inputs = controller(app)
        await wait_for(lambda: inputs.status.last_error is not None)
        data = (await client.get("/api/v1/playback-api/status")).json()
        assert data["selected"] and data["enabled"] and not data["connected"]
        assert "remote connections may be off" in data["last_error"]
        assert data["song_order"] == [] and data["stale"]
        response = await client.put("/api/v1/playback-api/settings", json={"enabled": False})
        assert response.status_code == 200 and not response.json()["enabled"]
        assert not inputs.status.connected
        # Scan never needs prior setup: it turns Playback back on by itself.
        inputs._scanner = lambda **_: []
        assert (await client.post("/api/v1/playback-api/find")).status_code == 200
        await wait_for(lambda: inputs.scan_state == "not_found")


async def test_persistence_failure_leaves_live_configuration_untouched(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fake = FakePlayback()
    async with application(fake) as (app, client):
        inputs = controller(app)
        previous = inputs.client

        def fail_save(_: object) -> None:
            raise SettingsFileError("Settings could not be saved atomically.")

        monkeypatch.setattr(app.state.runtime.settings_service._store, "save", fail_save)
        result = await client.put("/api/v1/playback-api/settings", json={"enabled": False})
        assert result.status_code == 503
        assert inputs.client is previous and inputs.settings.playback_api.enabled
        result = await client.post(
            "/api/v1/playback-api/discover-song-order", json={"confirm": True}
        )
        assert result.json()["discovery"] == "failed" and inputs.mapper.song_order == ()


async def test_pending_start_dropped_across_disconnect_without_invalidating_order() -> None:
    fake = FakePlayback()
    async with application(fake, saved=True) as (app, _):
        inputs = controller(app)
        dispatcher = AsyncMock()
        inputs.mapper._dispatcher = dispatcher
        queued = inputs.mapper.observe(
            inputs.status.heartbeat, (PlaybackEvent("song.started", 1, 17),)
        )
        inputs.mapper.discard_pending()
        # First reconnect heartbeat validates, but cannot replay the old envelope.
        inputs.mapper.observe(Heartbeat(17, 0, False, False, 8), ())
        assert not inputs.mapper.stale
        for item in queued:
            await inputs.mapper.dispatch(item)
        dispatcher.dispatch_song_position.assert_not_awaited()


async def test_migrated_install_starts_playback_and_retains_midi_alternative(
    tmp_path: Path,
) -> None:
    path = tmp_path / "settings.json"
    path.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "integration_modes": {
                    "service_source": "demo",
                    "midi_source": "simulated",
                    "timer_output": "simulated",
                },
                "midi": {
                    "enabled": True,
                    "input_name": "Playback MIDI",
                    "channel": 9,
                    "note": 110,
                    "debounce_ms": 321,
                },
            }
        )
    )
    service = SettingsService(SettingsFileStore(path), MemoryCredentialStore(), environ={})
    fake = FakePlayback()
    midi = FakeMidiBackend(["Playback MIDI"])
    async with application(fake, midi=midi, settings_service=service) as (app, client):
        data = (await client.get("/api/v1/playback-api/status")).json()
        assert data["selected"] and data["enabled"] and data["connected"]
        assert not midi.ports  # API mode never starts MIDI by itself.
        stored = json.loads(path.read_text())
        assert stored["schema_version"] == 2
        assert stored["midi"]["channel"] == 9 and stored["midi"]["debounce_ms"] == 321
        saved = (await client.get("/api/v1/settings")).json()["settings"]
        saved["integration_modes"]["midi_source"] = "real"
        assert (await client.put("/api/v1/settings", json=saved)).status_code == 200
        await wait_for(lambda: bool(midi.ports))
        assert controller(app).settings.midi.channel == 9
        assert controller(app).settings.midi.note == 110


async def scan_done(client: httpx.AsyncClient, state: str) -> dict[str, Any]:
    for _ in range(400):
        data = (await client.get("/api/v1/playback-api/status")).json()
        if data["scan"]["state"] == state:
            return cast(dict[str, Any], data)
        await asyncio.sleep(0.02)
    raise AssertionError(f"scan never reached {state}")


async def test_scan_from_fresh_install_connects_with_one_click() -> None:
    fake = FakePlayback()
    app = create_app(Settings(), playback_client_factory=fake.client)
    server = await asyncio.start_server(fake.accept, "127.0.0.1", 0)
    fake.port = server.sockets[0].getsockname()[1]
    try:
        async with (
            app.router.lifespan_context(app),
            httpx.AsyncClient(
                transport=httpx.ASGITransport(app=app), base_url="http://testserver"
            ) as client,
        ):
            inputs = controller(app)
            inputs._scanner = lambda **_: [ScanCandidate("Example Mac", "127.0.0.1", "loopback")]
            assert (await client.post("/api/v1/playback-api/find")).status_code == 200
            data = await scan_done(client, "found")
            await wait_for(lambda: inputs.status.connected)
            data = (await client.get("/api/v1/playback-api/status")).json()
            assert data["connected"] and data["active_source"] == "playback_api"
            assert data["reason"].startswith("Connected to Playback")
            assert data["settings"]["host"] == "127.0.0.1" and data["selected"]
            assert data["scan"]["candidates"] == [{"name": "Example Mac", "host": "127.0.0.1"}]
    finally:
        server.close()
        await server.wait_closed()
        for writer in tuple(fake.writers):
            writer.close()
        await asyncio.gather(*fake.tasks, return_exceptions=True)


async def test_scan_switches_explicit_midi_install_and_keeps_midi_settings() -> None:
    settings = Settings(
        integration_modes=IntegrationModes(midi_source=MidiSource.REAL),
        midi=MidiSettings(enabled=True, input_name="Playback MIDI", channel=9),
    )
    midi = FakeMidiBackend(["Playback MIDI"])
    fake = FakePlayback()
    app = create_app(
        settings, playback_client_factory=fake.client, midi_backend_factory=lambda: midi
    )
    server = await asyncio.start_server(fake.accept, "127.0.0.1", 0)
    fake.port = server.sockets[0].getsockname()[1]
    try:
        async with (
            app.router.lifespan_context(app),
            httpx.AsyncClient(
                transport=httpx.ASGITransport(app=app), base_url="http://testserver"
            ) as client,
        ):
            inputs = controller(app)
            assert not inputs.selected
            inputs._scanner = lambda **_: [ScanCandidate("Example Mac", "127.0.0.1", "loopback")]
            assert (await client.post("/api/v1/playback-api/find")).status_code == 200
            await scan_done(client, "found")
            await wait_for(lambda: inputs.status.connected)
            assert inputs.selected
            saved = app.state.runtime.settings_service.snapshot()
            assert saved.midi.channel == 9 and saved.midi.input_name == "Playback MIDI"
    finally:
        server.close()
        await server.wait_closed()
        for writer in tuple(fake.writers):
            writer.close()
        await asyncio.gather(*fake.tasks, return_exceptions=True)


async def test_scan_not_found_wrong_host_multiple_cancel_and_validation() -> None:
    app = create_app(Settings())
    async with (
        app.router.lifespan_context(app),
        httpx.AsyncClient(
            transport=httpx.ASGITransport(app=app), base_url="http://testserver"
        ) as client,
    ):
        inputs = controller(app)
        seen: list[dict[str, object]] = []

        def none(**kwargs: Any) -> list[ScanCandidate]:
            seen.append(kwargs)
            return []

        inputs._scanner = none
        response = await client.post("/api/v1/playback-api/find", json={"host": "192.0.2.77"})
        assert response.status_code == 200
        data = await scan_done(client, "not_found")
        assert seen[-1]["host_override"] == "192.0.2.77"
        assert "Couldn't find Playback" in data["reason"] and not data["connected"]
        assert data["settings"]["host"] is None  # nothing remembered on failure
        two = [ScanCandidate("One", "192.0.2.1", "lan"), ScanCandidate("Two", "192.0.2.2", "lan")]
        inputs._scanner = lambda **_: two
        await client.post("/api/v1/playback-api/find")
        data = await scan_done(client, "found")
        assert [c["name"] for c in data["scan"]["candidates"]] == ["One", "Two"]
        assert data["settings"]["host"] is None  # the user picks; nothing saved yet
        started = threading.Event()

        def slow(**kwargs: Any) -> list[ScanCandidate]:
            started.set()
            kwargs["cancel"].wait(5)
            return []

        inputs._scanner = slow
        await client.post("/api/v1/playback-api/find")
        await asyncio.to_thread(started.wait, 2)
        data = (await client.post("/api/v1/playback-api/find/cancel")).json()
        assert data["scan"]["state"] == "idle"
        for bad in ({"host": "http://bad/"}, {"host": "a b"}, {"nope": 1}):
            assert (await client.post("/api/v1/playback-api/find", json=bad)).status_code == 422


@pytest.mark.parametrize("fast", [True, False])
async def test_fake_transport_replay_latency_and_single_dispatch(fast: bool) -> None:
    from stagepilot.core.actions import ActionOutcome

    fake = FakePlayback(index=0, heartbeat_timeout=2)
    async with application(fake, saved=True, fast_transport=fast) as (app, _):
        inputs = controller(app)
        dispatcher = AsyncMock()
        dispatcher.dispatch_song_position.return_value = ActionOutcome(True, "accepted")
        dispatcher.dispatch.return_value = ActionOutcome(True, "accepted")
        inputs._dispatcher.dispatcher = dispatcher  # Keep the real mapper and arbiter.
        fake.silent = True
        await asyncio.sleep(0.06)  # Let the prior automatic heartbeat finish.
        latencies = []
        for raw, kind, playing, position in [
            ('{"transportPlay":{"playing":true}}', "song.started", True, 0.5),
            ('{"transportPlay":{"playing":false}}', "song.paused", False, 0.5),
            ('{"transportReturnToStart":{}}', "song.stopped", False, 0),
        ]:
            before = len(inputs.events)

            def new_event(count: int = before) -> bool:
                return len(inputs.events) > count

            sent = time.monotonic()
            await fake.broadcast(raw)
            if fast:
                await wait_for(new_event)
                assert inputs.events[-1].event.type == kind
                assert inputs.events[-1].event.provisional
                assert inputs.events[-1].event.timestamp - sent < 0.15
            await asyncio.sleep(0.6)  # Deliberately late authoritative heartbeat.
            await fake.broadcast(heartbeat(900, position, playing, 8))
            await wait_for(new_event)
            event = inputs.events[-1].event
            assert event.type == kind and event.provisional is fast
            latency = event.timestamp - sent
            latencies.append(round(latency, 3))
            if not fast:
                assert 0.55 <= latency < 1.5
            await wait_for(lambda: inputs._queue.empty())
        await wait_for(lambda: dispatcher.dispatch.await_count == 2)
        dispatcher.dispatch_song_position.assert_awaited_once_with(1, source="playback_api")
        assert [call.args[0] for call in dispatcher.dispatch.await_args_list] == [
            ActionName.STOP_TIMER,
            ActionName.STOP_TIMER,
        ]
        assert len([entry for entry in inputs.events if entry.event.type == "song.started"]) == 1
        print(f"Replay fast={fast}: play/pause/return latency seconds={latencies}")
        assert fake.commands == []  # No controls sent to Playback by StagePilot.


async def test_load_identity_saved_at_discovery_and_same_version_change_stales() -> None:
    fake = FakePlayback(index=0)
    async with application(fake) as (app, client):
        inputs = controller(app)
        await fake.broadcast(
            json.dumps(
                {
                    "contentLoadSetlist": {
                        "setlistData": {"setlistID": 7, "setlistName": "PRIVATE SERVICE PLAN"}
                    }
                }
            )
        )
        await wait_for(lambda: inputs.status.setlist_id == 7)
        data = (
            await client.post("/api/v1/playback-api/discover-song-order", json={"confirm": True})
        ).json()
        assert data["discovery"] == "done" and not data["stale"]
        saved = app.state.runtime.settings_service.snapshot().playback_api
        assert saved.setlist_id == inputs.mapper.setlist_id == 7
        await fake.broadcast(
            json.dumps(
                {
                    "contentLoadSetlist": {
                        "setlistData": {"setlistID": 8, "setlistName": "ANOTHER PRIVATE PLAN"}
                    }
                }
            )
        )
        await wait_for(lambda: inputs.mapper.stale)
        data = (await client.get("/api/v1/playback-api/status")).json()
        assert data["captured_version"] == data["setlist_cloud_version"] == 8
        assert data["stale"]
        events = (await client.get("/api/v1/playback-api/events")).text
        assert "PRIVATE" not in events and "PRIVATE" not in saved.model_dump_json()
        saved_data = (await client.get("/api/v1/settings")).json()["settings"]
        saved_data["playback_api"]["setlist_id"] = 8
        assert (await client.put("/api/v1/settings", json=saved_data)).status_code == 400


async def test_song_counts_follow_plan_reload_without_rediscovery() -> None:
    fake = FakePlayback(index=0)
    async with application(fake) as (app, client):
        inputs = controller(app)
        data = (
            await client.post("/api/v1/playback-api/discover-song-order", json={"confirm": True})
        ).json()
        assert data["song_count"] == len(fake.songs) == 3
        state = await inputs.state_store.snapshot()
        assert state.plan is not None
        assert data["plan_song_count"] == len(state.plan.songs)
        plan = state.plan.model_copy(deep=True)
        plan.songs = plan.songs[:1]
        await inputs.state_store.mutate(lambda current: setattr(current, "plan", plan))
        changed = (await client.get("/api/v1/playback-api/status")).json()
        assert changed["song_count"] == 3 and changed["plan_song_count"] == 1
        assert changed["song_order"] == data["song_order"] and not changed["stale"]


async def test_fast_transport_setting_reconfigures_client() -> None:
    fake = FakePlayback()
    async with application(fake) as (app, client):
        data = (
            await client.put("/api/v1/playback-api/settings", json={"fast_transport": False})
        ).json()
        assert data["settings"]["fast_transport"] is False
        inputs = controller(app)
        assert inputs.client is not None and inputs.client.options.fast_transport is False
        assert not app.state.runtime.settings_service.snapshot().playback_api.fast_transport
