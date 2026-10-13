from __future__ import annotations

import asyncio
import json
from datetime import UTC, datetime
from pathlib import Path
from unittest.mock import AsyncMock

import pytest
from pydantic import ValidationError

from stagepilot.core.config import LightingCue, PlaybackApiSettings, ProPresenterSettings
from stagepilot.core.settings import SettingsFileStore
from stagepilot.plugins.lights import LightsPlugin
from stagepilot.plugins.propresenter.plugin import ProPresenterPlugin
from test_lights_plugin import FakeOutputBackend, lights_settings
from test_playback_integration import FakePlayback, application, controller, wait_for
from test_propresenter_plugin import FakeProPresenterClient


def test_old_settings_file_loads_without_lengths(tmp_path: Path) -> None:
    path = tmp_path / "settings.json"
    path.write_text(json.dumps({"schema_version": 2, "playback_api": {"song_order": [9, 8]}}))
    saved = SettingsFileStore(path).load()
    assert saved is not None
    assert saved.playback_api.song_lengths == []
    assert saved.playback_api.lengths_measured_at is None


@pytest.mark.parametrize("lengths", [[1.0], [1.0, 2.0, 3.0], [float("nan"), None], [-1, None]])
def test_lengths_are_aligned_and_valid(lengths: list[float | None]) -> None:
    with pytest.raises(ValidationError):
        PlaybackApiSettings(song_order=[9, 8], song_lengths=lengths)


def test_order_replacement_clears_old_lengths() -> None:
    saved = PlaybackApiSettings(
        song_order=[9, 8], song_lengths=[123.4, None], lengths_measured_at=datetime.now(UTC)
    )
    replacement = saved.model_copy(update={"song_order": [8, 9]})
    assert replacement.song_lengths == []
    assert replacement.lengths_measured_at is None
    assert saved.model_copy(update={"host": "localhost"}).song_lengths == saved.song_lengths


async def test_no_effect_seek_saves_none_and_order_and_emits_no_actions() -> None:
    fake = FakePlayback(index=2, lengths=[279.53, None, 260.17])
    async with application(fake) as (app, client):
        inputs = controller(app)
        dispatcher = AsyncMock()
        inputs.mapper._dispatcher = dispatcher
        data = (
            await client.post("/api/v1/playback-api/discover-song-order", json={"confirm": True})
        ).json()
        assert data["discovery"] == "done"
        assert data["song_order"] == fake.songs
        assert data["song_lengths"] == fake.lengths
        assert data["progress"] >= 3 * len(fake.songs)
        assert fake.index == 2 and fake.position == 0
        await asyncio.sleep(0.15)
        dispatcher.dispatch.assert_not_awaited()
        dispatcher.dispatch_song_position.assert_not_awaited()
        events = (await client.get("/api/v1/playback-api/events")).json()["events"]
        own = [item for item in events if item["event"]["type"] in {"seek", "song.stopped"}]
        assert own and all(item["discovery"] for item in own)
        assert set(fake.commands) <= {
            "transportPreviousSong",
            "transportNextSong",
            "waveformSeek",
            "transportReturnToStart",
        }


@pytest.mark.parametrize("initial", [0, 1, 2])
async def test_cancel_mid_measurement_saves_no_partial_lengths(initial: int) -> None:
    fake = FakePlayback(index=initial, lengths=[None, 123, 234])
    async with application(fake) as (app, _):
        inputs = controller(app)
        before = app.state.runtime.settings_service.snapshot().playback_api
        task = asyncio.create_task(inputs.discover_song_order())
        await wait_for(lambda: "waveformSeek" in fake.commands)
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task
        assert app.state.runtime.settings_service.snapshot().playback_api == before
        assert fake.position == 0
        assert fake.index == initial
        assert not inputs.mapper.discovery


@pytest.mark.parametrize("change", ["version", "identity", "unknown"])
async def test_stale_order_clears_persisted_lengths(change: str) -> None:
    fake = FakePlayback(index=0)
    async with application(fake) as (app, client):
        inputs = controller(app)
        await fake.broadcast('{"contentLoadSetlist":{"setlistData":{"setlistID":122}}}')
        await wait_for(lambda: inputs.status.setlist_id == 122)
        await inputs.discover_song_order()
        assert inputs.settings.playback_api.song_lengths
        if change == "version":
            fake.version += 1
        elif change == "identity":
            await fake.broadcast('{"contentLoadSetlist":{"setlistData":{"setlistID":123}}}')
        else:
            fake.songs[0] = 12345
        await wait_for(lambda: inputs.mapper.stale)
        assert inputs.settings.playback_api.song_lengths == []
        saved = app.state.runtime.settings_service.snapshot().playback_api
        assert saved.song_lengths == [] and saved.lengths_measured_at is None
        data = (await client.get("/api/v1/playback-api/status")).json()
        assert data["song_lengths"] == [] and data["lengths_measured_at"] is None


async def test_scan_does_not_change_plan_or_timer_duration() -> None:
    fake = FakePlayback(index=0, lengths=[1.5, 2.5, 3.5])
    async with application(fake) as (app, client):
        before = (await client.get("/api/v1/state")).json()
        assert before["plan"] is not None
        expected = before["plan"]["songs"][0]["duration_seconds"]
        data = (
            await client.post("/api/v1/playback-api/discover-song-order", json={"confirm": True})
        ).json()
        assert data["song_lengths"] == [1.5, 2.5, 3.5]
        after = (await client.get("/api/v1/state")).json()
        assert after["plan"] == before["plan"]
        assert after["timer"] == before["timer"]
        assert data["plan_songs"][0]["duration_seconds"] == expected
        fake.playing = True
        await wait_for(
            lambda: app.state.runtime.state_store._state.timer.duration_seconds is not None
        )
        state = (await client.get("/api/v1/state")).json()
        assert state["timer"]["duration_seconds"] == expected
        assert expected != fake.lengths[0]


async def test_unexpected_measurement_failure_does_not_fail_order(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fake = FakePlayback(index=0)
    async with application(fake) as (app, _):
        inputs = controller(app)
        assert inputs.client is not None
        monkeypatch.setattr(
            inputs.client, "measure_song_length", AsyncMock(side_effect=RuntimeError("read failed"))
        )
        await inputs.discover_song_order()
        assert inputs.discovery == "done"
        saved = app.state.runtime.settings_service.snapshot().playback_api
        assert saved.song_order == fake.songs
        assert saved.song_lengths == [None, None, None]


async def test_full_seek_scan_sends_no_propresenter_or_lights_commands() -> None:
    fake = FakePlayback(index=1)
    async with application(fake) as (app, _):
        inputs = controller(app)
        output = FakeProPresenterClient()
        backend = FakeOutputBackend()
        presenter = ProPresenterPlugin(
            inputs.event_bus,
            inputs.state_store,
            ProPresenterSettings(enabled=True, health_check_interval_seconds=300),
            client_factory=lambda _settings: output,
        )
        lights = LightsPlugin(
            inputs.event_bus,
            inputs.state_store,
            lights_settings(LightingCue(at_seconds=0, note=72, velocity=110)),
            backend_factory=lambda: backend,
        )
        await presenter.start()
        await lights.start()
        output.calls.clear()
        try:
            await inputs.discover_song_order()
            await asyncio.sleep(0.15)
            assert inputs.discovery == "done"
            assert inputs.settings.playback_api.song_lengths == fake.lengths
            assert not [
                call for call in output.calls if call[0] in {"start", "stop", "set", "reset"}
            ]
            assert backend.ports and not any(port.messages for port in backend.ports)
            assert set(fake.commands) <= {
                "transportPreviousSong",
                "transportNextSong",
                "waveformSeek",
                "transportReturnToStart",
            }
        finally:
            await lights.stop()
            await presenter.stop()


# --- Stale-setlist audit: every trigger must blank lengths and remove the PC update ---


async def _scanned(client: object, app: object | None = None) -> dict[str, object]:
    if app is not None:
        await _connect_pc(app)
    data = (
        await client.post(  # type: ignore[attr-defined]
            "/api/v1/playback-api/discover-song-order", json={"confirm": True}
        )
    ).json()
    assert data["discovery"] == "done" and not data["stale"] and not data["plan_changed"]
    return data  # type: ignore[no-any-return]


async def _connect_pc(app: object) -> None:
    from stagepilot.models.state import ConnectionStatus

    def mutate(state: object) -> None:
        state.planning_center_status = ConnectionStatus.CONNECTED  # type: ignore[attr-defined]

    await app.state.runtime.state_store.mutate(mutate)  # type: ignore[attr-defined]


async def _swap_plan(app: object, **update: object) -> None:
    def mutate(state: object) -> None:
        state.plan = state.plan.model_copy(update=update)  # type: ignore[attr-defined]

    await app.state.runtime.state_store.mutate(mutate)  # type: ignore[attr-defined]


async def _assert_update_removed(client: object, reason: str) -> None:
    data = (await client.get("/api/v1/playback-api/status")).json()  # type: ignore[attr-defined]
    assert data["song_lengths"] == [] and data["lengths_measured_at"] is None
    assert reason in data["planning_center_update_reason"]
    preview = await client.post("/api/v1/playback-api/planning-center-lengths/preview")  # type: ignore[attr-defined]
    assert preview.status_code == 409
    confirm = await client.post(  # type: ignore[attr-defined]
        "/api/v1/playback-api/planning-center-lengths/confirm",
        json={"token": "x", "confirm": True},
    )
    assert confirm.status_code == 409


@pytest.mark.parametrize("change", ["unknown_song", "version", "setlist_id"])
async def test_playback_side_change_removes_update(change: str) -> None:
    fake = FakePlayback(index=0)
    async with application(fake) as (app, client):
        inputs = controller(app)
        await fake.broadcast('{"contentLoadSetlist":{"setlistData":{"setlistID":122}}}')
        await wait_for(lambda: inputs.status.setlist_id == 122)
        await _scanned(client, app)
        if change == "unknown_song":
            fake.songs[0] = 12345
        elif change == "version":
            fake.version += 1
        else:
            await fake.broadcast('{"contentLoadSetlist":{"setlistData":{"setlistID":123}}}')
        await wait_for(lambda: inputs.mapper.stale)
        await _assert_update_removed(client, "The setlist changed")
        assert (await client.get("/api/v1/playback-api/status")).json()["stale"]


async def test_unannounced_setlist_id_is_unknown_not_changed() -> None:
    fake = FakePlayback(index=0)
    async with application(fake) as (app, client):
        inputs = controller(app)
        await _scanned(client, app)  # No contentLoadSetlist ever announced an ID.
        await asyncio.sleep(0.3)
        assert not inputs.mapper.stale
        data = (await client.get("/api/v1/playback-api/status")).json()
        assert data["song_lengths"] == fake.lengths and not data["stale"]


@pytest.mark.parametrize("change", ["plan_id", "song_ids", "category"])
async def test_planning_center_side_change_removes_update(change: str) -> None:
    fake = FakePlayback(index=0)
    async with application(fake) as (app, client):
        inputs = controller(app)
        await _scanned(client, app)
        plan = (await app.state.runtime.state_store.snapshot()).plan
        assert plan is not None
        if change == "plan_id":
            await _swap_plan(app, id=plan.id + "-other")
        elif change == "song_ids":
            songs = [s.model_copy(update={"id": s.id + "x"}) for s in plan.songs]
            await _swap_plan(app, songs=songs)
        else:
            inputs.settings.playback_api = inputs.settings.playback_api.model_copy(
                update={"planning_center_update_service_type_id": "another-type"}
            )
        data = (await client.get("/api/v1/playback-api/status")).json()
        assert data["plan_changed"] and not data["stale"]
        await _assert_update_removed(client, "The Services plan changed")


async def test_plan_reload_with_same_items_and_new_durations_stays_available() -> None:
    fake = FakePlayback(index=0)
    async with application(fake) as (app, client):
        await _scanned(client, app)
        plan = (await app.state.runtime.state_store.snapshot()).plan
        assert plan is not None
        songs = [s.model_copy(update={"duration_seconds": 5}) for s in plan.songs]
        await _swap_plan(app, songs=songs)
        data = (await client.get("/api/v1/playback-api/status")).json()
        assert not data["plan_changed"] and data["song_lengths"] == fake.lengths


async def test_confirm_refuses_to_write_after_plan_changes_mid_dialog() -> None:
    fake = FakePlayback(index=0)
    async with application(fake) as (app, client):
        inputs = controller(app)
        await _scanned(client, app)
        plan = (await app.state.runtime.state_store.snapshot()).plan
        assert plan is not None
        await _swap_plan(app, id=plan.id + "-other")
        assert not await inputs.planning_center_lengths.plan_current()


async def test_restart_with_saved_scan_then_different_setlist_is_stale(tmp_path: Path) -> None:
    from stagepilot.core.settings import MemoryCredentialStore, SettingsService

    store = SettingsFileStore(tmp_path / "settings.json")
    service = SettingsService(store, MemoryCredentialStore(), environ={})
    fake = FakePlayback(index=0)
    async with application(fake, settings_service=service) as (app, client):
        await fake.broadcast('{"contentLoadSetlist":{"setlistData":{"setlistID":122}}}')
        await wait_for(lambda: controller(app).status.setlist_id == 122)
        await _scanned(client, app)
    other = FakePlayback(index=0, songs=[5, 6, 7])
    service2 = SettingsService(store, MemoryCredentialStore(), environ={})
    async with application(other, settings_service=service2) as (app, client):
        inputs = controller(app)
        await _connect_pc(app)
        await wait_for(lambda: inputs.mapper.stale and inputs.status.connected)
        await _assert_update_removed(client, "Scan songs")
