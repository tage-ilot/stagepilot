from __future__ import annotations

import asyncio
import json
from datetime import UTC, date, datetime
from pathlib import Path
from unittest.mock import AsyncMock

import httpx
import pytest

from stagepilot.core.actions import ActionOutcome
from stagepilot.core.config import PlanningCenterSettings
from stagepilot.core.event_bus import EventBus
from stagepilot.core.events import ActionName, EventType, ServicePayload, StagePilotEvent, new_event
from stagepilot.core.settings import (
    MemoryCredentialStore,
    MemorySettingsStore,
    SettingsFileStore,
    SettingsService,
)
from stagepilot.core.state import StateStore
from stagepilot.models.state import (
    ApplicationState,
    ConnectionStatus,
    ServicePlan,
    Song,
    TimerStatus,
)
from stagepilot.plugins.planning_center.client import PlanningCenterClient
from stagepilot.services.planning_center_lengths import (
    DENIED_MESSAGE,
    PlanningCenterLengthService,
)
from stagepilot.services.state_service import StateService
from test_playback_integration import FakePlayback, application, controller, wait_for


class Rig:
    def __init__(self, *, path: Path | None = None, count: int = 7) -> None:
        self.plan = ServicePlan(
            id="plan",
            title="Sunday",
            date=date.today(),
            service_type="Weekend",
            service_type_id="type",
            songs=[
                Song(id=str(i), title=f"Song {i}", duration_seconds=100, order=i + 1)
                for i in range(count)
            ],
        )
        self.settings = SettingsService(
            SettingsFileStore(path) if path else MemorySettingsStore(),
            MemoryCredentialStore(),
            environ={},
        )
        self.settings.load()
        self.state = StateStore(
            ApplicationState(plan=self.plan, planning_center_status=ConnectionStatus.CONNECTED)
        )
        self.remote = {song.id: 100 for song in self.plan.songs}
        self.requests: list[httpx.Request] = []
        self.failures: dict[str, int | str] = {}
        self.kind: dict[str, str] = {}
        self.dispatcher = AsyncMock()
        self.dispatcher.dispatch.side_effect = self.reload
        self.service = PlanningCenterLengthService(
            self.settings,
            self.state,
            self.dispatcher,
            client_factory=self.client,
            reload_timeout=0.1,
        )

    async def update(self, plan: ServicePlan, lengths: list[float | None]) -> None:
        preview = self.service.preview_plan(plan, lengths, "scan")
        await self.service.confirm(preview.token, "scan")

    def client(self) -> PlanningCenterClient:
        return PlanningCenterClient(
            PlanningCenterSettings(app_id="app-secret", secret="token-secret"),
            transport=httpx.MockTransport(self.handle),
        )

    def handle(self, request: httpx.Request) -> httpx.Response:
        self.requests.append(request)
        assert request.url.path.startswith("/services/v2/service_types/type/plans/plan/items/")
        item_id = request.url.path.rsplit("/", 1)[-1]
        if request.method == "PATCH":
            # The write-ahead record is already durably saved before any request.
            assert any(
                record.item_id == item_id
                for record in self.settings.snapshot().planning_center_length_undo
            )
            failure = self.failures.get(item_id)
            if failure == "timeout":
                raise httpx.ReadTimeout("token-secret", request=request)
            if isinstance(failure, int):
                return httpx.Response(failure, headers={"Retry-After": "0"})
            payload = json.loads(request.content)
            assert set(payload) == {"data"}
            assert set(payload["data"]) == {"type", "attributes"}
            assert payload["data"]["type"] == "Item"
            assert set(payload["data"]["attributes"]) == {"length"}
            self.remote[item_id] = payload["data"]["attributes"]["length"]
        return httpx.Response(
            200,
            json={
                "data": {
                    "type": "Item",
                    "id": item_id,
                    "attributes": {
                        "title": f"Song {item_id}",
                        "length": self.remote[item_id],
                        "item_type": self.kind.get(item_id, "song"),
                        "sequence": int(item_id),
                    },
                }
            },
        )

    async def reload(self, action: ActionName, source: str) -> ActionOutcome:
        assert action is ActionName.RELOAD_PLAN
        assert source == "planning_center_lengths"

        def mutate(state: ApplicationState) -> None:
            assert state.plan is not None
            for song in state.plan.songs:
                song.duration_seconds = self.remote[song.id]
            state.last_successful_plan_reload_at = datetime.now(UTC)

        await self.state.mutate(mutate)
        return ActionOutcome(True, "Loaded")


async def test_rounding_null_beyond_and_only_item_length() -> None:
    rig = Rig()
    before = await rig.state.snapshot()
    await rig.update(rig.plan, [100, 101, 102, 111, 151.1, None, 100, 999])
    assert rig.remote == {"0": 100, "1": 101, "2": 102, "3": 111, "4": 151, "5": 100, "6": 100}
    assert [r.url.path for r in rig.requests if r.method == "PATCH"] == [
        "/services/v2/service_types/type/plans/plan/items/1",
        "/services/v2/service_types/type/plans/plan/items/2",
        "/services/v2/service_types/type/plans/plan/items/3",
        "/services/v2/service_types/type/plans/plan/items/4",
    ]
    assert rig.service.result.reload == "verified"
    assert "Updated 4 songs" in rig.service.result.message
    assert "already the same" in rig.service.result.message
    rig.dispatcher.dispatch.assert_awaited_once()
    assert (await rig.state.snapshot()).timer == before.timer


async def test_conflict_and_non_song_leave_others_writable() -> None:
    rig = Rig(count=3)
    rig.remote["0"] = 90
    rig.kind["1"] = "header"
    await rig.update(rig.plan, [150, 150, 150])
    assert rig.remote == {"0": 90, "1": 100, "2": 150}
    assert "Someone changed" in rig.service.result.message
    assert len(rig.settings.snapshot().planning_center_length_undo) == 1


@pytest.mark.parametrize("failure", [401, 403, 429, 500, "timeout"])
async def test_failure_reports_partial_and_stops_then_later_success(
    failure: int | str, caplog: pytest.LogCaptureFixture
) -> None:
    rig = Rig(count=3)
    rig.failures["1"] = failure
    await rig.update(rig.plan, [151, 151, 151])
    assert rig.remote == {"0": 151, "1": 100, "2": 100}
    assert rig.service.result.status == "partial"
    assert [item.status for item in rig.service.result.items] == ["updated", "failed", "failed"]
    if failure in (401, 403):
        assert DENIED_MESSAGE in rig.service.result.message
    assert len([r for r in rig.requests if r.method == "PATCH" and r.url.path.endswith("/1")]) == (
        2 if failure == 429 else 1
    )
    assert "token-secret" not in rig.service.result.model_dump_json() + caplog.text
    rig.failures.clear()
    loaded = (await rig.state.snapshot()).plan
    assert loaded is not None
    await rig.update(loaded, [151, 151, 151])
    assert rig.remote == {"0": 151, "1": 151, "2": 151}


async def test_none_needed_no_requests_and_unknown_no_overwrite() -> None:
    rig = Rig(count=2)
    rig.plan.songs[1].duration_seconds = None
    await rig.update(rig.plan, [100, 150])
    assert not rig.requests
    rig.dispatcher.dispatch.assert_not_awaited()
    assert "No Planning Center times needed changing" in rig.service.result.message


async def test_undo_persists_restart_conflicts_and_clears(tmp_path: Path) -> None:
    path = tmp_path / "settings.json"
    rig = Rig(path=path, count=2)
    await rig.update(rig.plan, [151, 152])
    restarted = SettingsService(SettingsFileStore(path), MemoryCredentialStore(), environ={})
    restarted.load()
    rig.settings = restarted
    rig.service = PlanningCenterLengthService(
        restarted, rig.state, rig.dispatcher, client_factory=rig.client
    )
    assert rig.service.undo_available
    rig.remote["1"] = 999
    await rig.service.restore()
    assert rig.remote == {"0": 100, "1": 999}
    assert not rig.service.undo_available
    assert SettingsFileStore(path).load().planning_center_length_undo == []
    assert "Song 1: Someone changed" in rig.service.result.message
    assert rig.service.result.reload == "verified"


async def test_running_timer_defers_without_actions() -> None:
    rig = Rig(count=1)
    await rig.state.mutate(lambda state: setattr(state.timer, "status", TimerStatus.RUNNING))
    timer = (await rig.state.snapshot()).timer
    await rig.update(rig.plan, [151])
    assert rig.remote["0"] == 151
    assert rig.service.result.reload == "deferred"
    rig.dispatcher.dispatch.assert_not_awaited()
    assert (await rig.state.snapshot()).timer == timer


async def test_reload_failure_still_keeps_undo_and_changes() -> None:
    rig = Rig(count=1)
    rig.dispatcher.dispatch.side_effect = None
    rig.dispatcher.dispatch.return_value = ActionOutcome(True, "Requested")
    await rig.update(rig.plan, [151])
    assert rig.remote["0"] == 151 and rig.service.undo_available
    assert rig.service.result.reload == "failed"
    assert "has not reloaded" in rig.service.result.message
    rig.dispatcher.dispatch.assert_awaited_once()


async def test_idle_reload_uses_real_state_service_no_timer_or_lights_events() -> None:
    rig = Rig(count=1)
    bus = EventBus()
    state_service = StateService(bus, rig.state)
    await state_service.start()
    recorded: list[StagePilotEvent] = []
    await bus.subscribe(None, recorded.append)

    async def reload(_event: StagePilotEvent) -> None:
        loaded = rig.plan.model_copy(deep=True)
        loaded.songs[0].duration_seconds = rig.remote["0"]
        await bus.publish(
            new_event(EventType.SERVICE_LOADED, source="test", payload=ServicePayload(plan=loaded))
        )

    await bus.subscribe(EventType.SERVICE_RELOAD_REQUESTED, reload)
    rig.service.dispatcher = state_service
    try:
        timer = (await rig.state.snapshot()).timer
        await rig.update(rig.plan, [151])
        assert rig.service.result.reload == "verified"
        assert (await rig.state.snapshot()).timer == timer
        assert not any(
            "timer" in event.type.value or "light" in event.type.value for event in recorded
        )
        assert (
            len([event for event in recorded if event.type is EventType.SERVICE_RELOAD_REQUESTED])
            == 1
        )
        await rig.state.mutate(lambda state: setattr(state.timer, "status", TimerStatus.RUNNING))
        outcome = await state_service.dispatch(
            ActionName.RELOAD_PLAN, source="planning_center_lengths"
        )
        assert not outcome.accepted
    finally:
        await state_service.stop()


@pytest.mark.parametrize("mode", ["done", "failed", "cancelled", "unmeasured"])
async def test_scan_never_writes(mode: str) -> None:
    fake = FakePlayback(
        index=0, lengths=[None, None, None] if mode == "unmeasured" else [151, 152, 153]
    )
    async with application(fake) as (app, client):
        inputs = controller(app)
        rig = Rig(count=3)
        inputs.planning_center_lengths = rig.service
        if mode == "failed":
            inputs._step = AsyncMock(side_effect=RuntimeError("Navigation failed"))  # type: ignore[method-assign]
        task = asyncio.create_task(inputs.discover_song_order())
        if mode == "cancelled":
            await wait_for(lambda: bool(fake.commands))
            task.cancel()
            with pytest.raises(asyncio.CancelledError):
                await task
        else:
            await task
        assert not rig.requests
        assert not rig.service.undo_available
        data = (await client.get("/api/v1/playback-api/status")).json()
        if mode != "done":
            assert data["planning_center_update_reason"]
        else:
            assert data["song_lengths"] == fake.lengths


async def test_write_ahead_persistence_failure_prevents_patch(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rig = Rig(count=1)
    monkeypatch.setattr(
        rig.settings, "save", lambda _settings: (_ for _ in ()).throw(OSError("disk full"))
    )
    await rig.update(rig.plan, [151])
    assert not any(request.method == "PATCH" for request in rig.requests)
    assert rig.service.result.status == "failed"
    rig.dispatcher.dispatch.assert_not_awaited()


async def test_rate_limit_retry_waits_once_and_checks_conflict(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rig = Rig(count=2)
    rig.failures["0"] = 429

    async def wait(delay: float) -> None:
        assert delay == 0
        rig.remote["0"] = 99

    sleep = AsyncMock(side_effect=wait)
    monkeypatch.setattr("stagepilot.plugins.planning_center.client.asyncio.sleep", sleep)
    await rig.update(rig.plan, [151, 152])
    sleep.assert_awaited_once_with(0)
    assert rig.remote == {"0": 99, "1": 152}
    assert len([r for r in rig.requests if r.method == "PATCH" and r.url.path.endswith("/0")]) == 1
    assert rig.service.result.items[0].status == "skipped"
    assert [r.item_id for r in rig.settings.snapshot().planning_center_length_undo] == ["1"]


async def test_restore_denied_retains_record_for_retry() -> None:
    rig = Rig(count=1)
    await rig.update(rig.plan, [151])
    rig.failures["0"] = 403
    await rig.service.restore()
    assert rig.service.undo_available
    assert DENIED_MESSAGE in rig.service.result.message
    rig.failures.clear()
    await rig.service.restore()
    assert not rig.service.undo_available
    assert rig.remote["0"] == 100


async def test_cancel_pc_write_preserves_write_ahead_and_can_restore() -> None:
    rig = Rig(count=1)
    entered = asyncio.Event()

    async def slow(request: httpx.Request) -> httpx.Response:
        if request.method == "PATCH":
            entered.set()
            await asyncio.Event().wait()
        return rig.handle(request)

    rig.service.client_factory = lambda: PlanningCenterClient(
        PlanningCenterSettings(app_id="app", secret="secret"),
        transport=httpx.MockTransport(slow),
    )
    operation = asyncio.create_task(rig.update(rig.plan, [151]))
    await entered.wait()
    operation.cancel()
    with pytest.raises(asyncio.CancelledError):
        await operation
    assert rig.service.result.status == "failed"
    assert rig.service.undo_available
    rig.service.client_factory = rig.client
    await rig.service.restore()
    assert not rig.service.undo_available


def test_old_settings_without_fields_load() -> None:
    from stagepilot.core.settings import PersistentSettings

    settings = PersistentSettings.model_validate({"playback_api": {"song_order": [4]}})
    assert settings.playback_api.planning_center_update_service_type_id is None
    assert not settings.planning_center_length_undo


@pytest.mark.parametrize(
    "length,expected",
    [
        (279.53, 280),
        (265.85, 266),
        (260.17, 260),
        (410.83, 411),
        (282.16, 282),
        (282.5, 283),
        (282.49, 282),
    ],
)
async def test_half_up_no_tolerance(length: float, expected: int) -> None:
    rig = Rig(count=1)
    await rig.update(rig.plan, [length])
    assert rig.remote["0"] == expected


async def test_preview_is_read_only_and_single_use() -> None:
    rig = Rig(count=2)
    preview = rig.service.preview_plan(rig.plan, [101, 100], "scan")
    assert not rig.requests
    assert preview.items[1].status == "skipped"
    await rig.service.confirm(preview.token, "scan")
    assert rig.remote["0"] == 101
    with pytest.raises(ValueError):
        await rig.service.confirm(preview.token, "scan")


async def test_changed_scan_invalidates_preview() -> None:
    rig = Rig(count=1)
    preview = rig.service.preview_plan(rig.plan, [101], "scan")
    with pytest.raises(ValueError):
        await rig.service.confirm(preview.token, "different")
    assert not rig.requests


async def test_different_loaded_plan_does_not_reload_or_switch() -> None:
    rig = Rig(count=1)
    other = rig.plan.model_copy(update={"id": "other", "service_type_id": "other"})
    await rig.state.mutate(lambda state: setattr(state, "plan", other))
    await rig.update(rig.plan, [101])
    assert rig.remote["0"] == 101
    assert rig.service.result.reload == "different_plan"
    assert "StagePilot is showing a different plan" in rig.service.result.message
    rig.dispatcher.dispatch.assert_not_awaited()
    loaded = (await rig.state.snapshot()).plan
    assert loaded is not None and loaded.id == "other"


async def test_applied_marker_only_after_a_complete_write_and_failed_write_allows_retry() -> None:
    rig = Rig(count=2)
    applied: list[str] = []
    rig.service.on_applied = applied.append
    rig.failures["1"] = 500
    await rig.update(rig.plan, [101, 101])
    assert rig.service.result.status in ("partial", "failed")
    assert applied == []  # A failed or partial write keeps the explicit retry available.
    del rig.failures["1"]
    await rig.update(rig.plan, [101, 101])
    assert applied == ["plan"]  # The retry completed, so now it is marked applied.


async def test_nothing_to_change_does_not_mark_applied() -> None:
    rig = Rig(count=1)
    applied: list[str] = []
    rig.service.on_applied = applied.append
    await rig.update(rig.plan, [100])
    assert applied == []


async def test_applied_marker_survives_restart_reload_and_undo_until_new_scan(
    tmp_path: Path,
) -> None:
    fake = FakePlayback(index=0)
    service = SettingsService(
        SettingsFileStore(tmp_path / "s.json"), MemoryCredentialStore(), environ={}
    )
    async with application(fake, settings_service=service) as (app, client):
        inputs = controller(app)
        await client.post("/api/v1/playback-api/discover-song-order", json={"confirm": True})
        assert not await inputs.lengths_applied()
        plan = (await app.state.runtime.state_store.snapshot()).plan
        assert plan is not None
        inputs.mark_lengths_applied(plan.id)
        data = (await client.get("/api/v1/playback-api/status")).json()
        assert data["lengths_applied"] and not data["stale"]
        from stagepilot.models.state import ConnectionStatus as CS

        def connect(state: ApplicationState) -> None:
            state.planning_center_status = CS.CONNECTED

        await app.state.runtime.state_store.mutate(connect)
        reason = (await client.get("/api/v1/playback-api/status")).json()
        assert "already has these lengths" in reason["planning_center_update_reason"]
        assert (
            await client.post("/api/v1/playback-api/planning-center-lengths/preview")
        ).status_code == 409
        # Reloading the plan alone does not bring the prompt back (same plan id).
        await app.state.runtime.state_store.mutate(lambda s: None)
        assert (await client.get("/api/v1/playback-api/status")).json()["lengths_applied"]
        # Undo/Restore does not clear the marker.
        assert (
            await client.post(
                "/api/v1/playback-api/planning-center-lengths/restore", json={"confirm": True}
            )
        ).status_code == 200
        assert (await client.get("/api/v1/playback-api/status")).json()["lengths_applied"]
    # Restart: the marker is persisted with the scan's captured_at and plan id.
    service2 = SettingsService(
        SettingsFileStore(tmp_path / "s.json"), MemoryCredentialStore(), environ={}
    )
    async with application(FakePlayback(index=0), settings_service=service2) as (app, client):
        assert (await client.get("/api/v1/playback-api/status")).json()["lengths_applied"]
        # A new successful scan (new captured_at) brings the action back.
        data = (
            await client.post("/api/v1/playback-api/discover-song-order", json={"confirm": True})
        ).json()
        assert not data["lengths_applied"] and not data["stale"]
