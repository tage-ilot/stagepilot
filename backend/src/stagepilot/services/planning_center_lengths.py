"""Optional plan-item-only writes, write-ahead undo, and idle-only reload verification."""

from __future__ import annotations

import asyncio
import json
import math
import time
from collections.abc import Awaitable, Callable
from contextlib import suppress
from datetime import UTC, datetime
from typing import Literal
from uuid import uuid4
from zoneinfo import ZoneInfo

from pydantic import BaseModel, Field

from stagepilot.core.actions import ActionDispatcher
from stagepilot.core.events import ActionName
from stagepilot.core.logging import get_logger
from stagepilot.core.settings import PlanningCenterLengthUndo, SettingsService
from stagepilot.core.state import StateStore
from stagepilot.models.state import ConnectionStatus, ServicePlan, TimerStatus
from stagepilot.plugins.planning_center.client import PlanningCenterClient
from stagepilot.plugins.planning_center.errors import (
    PlanningCenterApiError,
    PlanningCenterConfigurationError,
    PlanningCenterLengthConflictError,
    PlanningCenterPermissionError,
    PlanningCenterRateLimitError,
)
from stagepilot.plugins.planning_center.models import (
    PlanAmbiguousResult,
    PlanLoadedResult,
    PlanningCenterServiceType,
)
from stagepilot.plugins.planning_center.preferences import preferred_candidate

DENIED_MESSAGE = (
    "Planning Center did not allow this change. The account or token used by "
    "StagePilot can't edit this plan."
)


class LengthItemResult(BaseModel):
    item_id: str
    title: str
    old_length: int | None = None
    new_length: int | None = None
    status: Literal["updated", "restored", "skipped", "failed"]
    reason: str | None = None


class LengthUpdateResult(BaseModel):
    operation: Literal["update", "restore"] = "update"
    status: Literal["idle", "running", "done", "partial", "failed"] = "idle"
    message: str = ""
    items: list[LengthItemResult] = Field(default_factory=list)
    reload: Literal["not_needed", "verified", "deferred", "failed", "different_plan"] = "not_needed"


def plan_key(plan: ServicePlan) -> str:
    """Identity of the plan the scanned lengths were measured for (not its durations)."""
    return json.dumps([plan.service_type_id, plan.id, [song.id for song in plan.songs]])


async def _always_current() -> bool:
    return True


def duration(value: int | None) -> str:
    if value is None:
        return "unknown"
    minutes, seconds = divmod(value, 60)
    return f"{minutes}:{seconds:02d}"


class LengthPreview(BaseModel):
    token: str
    category: str
    plan_title: str
    plan_date: str
    items: list[LengthItemResult]
    message: str = ""


class PlanningCenterLengthService:
    def __init__(
        self,
        settings: SettingsService,
        state: StateStore,
        dispatcher: ActionDispatcher,
        *,
        client_factory: Callable[[], PlanningCenterClient] | None = None,
        reload_timeout: float = 10,
        is_playing: Callable[[], bool] = lambda: False,
    ) -> None:
        self.settings = settings
        self.state = state
        self.dispatcher = dispatcher
        self.client_factory = client_factory or (
            lambda: PlanningCenterClient(settings.effective_runtime_settings().planning_center)
        )
        self.reload_timeout = reload_timeout
        self.is_playing = is_playing
        self.result = LengthUpdateResult()
        self._lock = asyncio.Lock()
        self._preview: tuple[LengthPreview, list[PlanningCenterLengthUndo], str, float] | None = (
            None
        )
        self._target_label = ""
        self.write_allowed: Callable[[], bool] = lambda: not self.is_playing()
        # False once the loaded plan differs from the one the lengths were measured for.
        self.plan_current: Callable[[], Awaitable[bool]] = _always_current
        # Called with the plan id after a complete, successful write of every changed item.
        self.on_applied: Callable[[str], None] = lambda _plan_id: None

    @property
    def undo_available(self) -> bool:
        return bool(self.settings.snapshot().planning_center_length_undo)

    def _persist(self, records: list[PlanningCenterLengthUndo]) -> None:
        self.settings.save(
            self.settings.snapshot().model_copy(update={"planning_center_length_undo": records})
        )

    async def categories(self) -> list[PlanningCenterServiceType]:
        async with self.client_factory() as client:
            return [
                category for category in await client.list_service_types() if not category.archived
            ]

    async def preview(
        self,
        category_id: str,
        lengths: list[float | None],
        signature: str,
    ) -> LengthPreview:
        async with self._lock:
            self._preview = None
            settings = self.settings.effective_runtime_settings()
            today = datetime.now(ZoneInfo(settings.timezone)).date()
            async with self.client_factory() as client:
                categories = [c for c in await client.list_service_types() if not c.archived]
                category = next((c for c in categories if c.id == category_id), None)
                if category is None:
                    raise ValueError("Choose an available Planning Center category.")
                result = await client.load_plan_for_date(
                    category,
                    today,
                    settings.timezone,
                    lookahead_days=settings.planning_center.upcoming_lookahead_days,
                )
                if isinstance(result, PlanAmbiguousResult):
                    chosen = preferred_candidate(settings.planning_center, result.candidates)
                    if chosen:
                        result = await client.resolve_selected_plan(
                            result.candidates, [category], today, selected_plan_id=chosen.id
                        )
                if not isinstance(result, PlanLoadedResult):
                    raise ValueError(
                        "No upcoming plan in this category."
                        if not isinstance(result, PlanAmbiguousResult)
                        else "More than one upcoming plan matches. "
                        "Set a preferred service time first."
                    )
            return self.preview_plan(result.plan, lengths, signature)

    def preview_plan(
        self,
        plan: ServicePlan,
        lengths: list[float | None],
        signature: str,
    ) -> LengthPreview:
        rows: list[LengthItemResult] = []
        candidates: list[PlanningCenterLengthUndo] = []
        assert plan.service_type_id
        for index, song in enumerate(plan.songs):
            measured = lengths[index] if index < len(lengths) else None
            target = math.floor(measured + 0.5) if measured is not None else None
            reason = None
            if target is None:
                reason = "No measured Playback length."
            elif song.duration_seconds is None:
                reason = "The plan length is unknown."
            elif target == song.duration_seconds:
                reason = "Already the same."
            elif any(
                (r.service_type_id, r.plan_id, r.item_id)
                == (plan.service_type_id, plan.id, song.id)
                and song.duration_seconds not in (r.new_length, r.pending_from_length)
                for r in self.settings.snapshot().planning_center_length_undo
            ):
                reason = "Someone changed this item; their edit was left alone."
            rows.append(
                LengthItemResult(
                    item_id=song.id,
                    title=song.title,
                    old_length=song.duration_seconds,
                    new_length=target,
                    status="skipped" if reason else "updated",
                    reason=reason,
                )
            )
            if not reason:
                assert target is not None and song.duration_seconds is not None
                candidates.append(
                    PlanningCenterLengthUndo(
                        service_type_id=plan.service_type_id,
                        plan_id=plan.id,
                        item_id=song.id,
                        title=song.title,
                        old_length=song.duration_seconds,
                        new_length=target,
                        written_at=datetime.now(UTC),
                    )
                )
        preview = LengthPreview(
            token=uuid4().hex,
            category=plan.service_type,
            plan_title=plan.title,
            plan_date=plan.date.isoformat(),
            items=rows,
            message="Planning Center already matches Playback to the second."
            if not candidates and all(r.reason == "Already the same." for r in rows)
            else "No song times can be changed."
            if not candidates
            else "",
        )
        self._preview = (preview, candidates, signature, time.monotonic())
        self._target_label = f"{plan.title} {plan.date.isoformat()}"
        return preview

    async def confirm(self, token: str, signature: str) -> None:
        async with self._lock:
            if self._preview is None:
                raise ValueError("Preview the plan again before updating.")
            preview, candidates, saved_signature, created = self._preview
            if (
                token != preview.token
                or not await self.plan_current()
                or signature != saved_signature
                or time.monotonic() - created > 300
                or not self.write_allowed()
            ):
                self._preview = None
                raise ValueError("The scan or Playback changed. Preview the plan again.")
            self._preview = None
            self.result = LengthUpdateResult(
                status="running",
                items=[r.model_copy() for r in preview.items if r.status == "skipped"],
            )
            await self._apply(candidates, restore=False)
            if (
                candidates
                and self.result.status == "done"
                and not any(item.status == "failed" for item in self.result.items)
            ):
                with suppress(Exception):  # Never turn a finished write into an error.
                    self.on_applied(candidates[0].plan_id)

    async def restore(self) -> None:
        async with self._lock:
            self.result = LengthUpdateResult(operation="restore", status="running")
            await self._apply(self.settings.snapshot().planning_center_length_undo, restore=True)

    async def _apply(self, candidates: list[PlanningCenterLengthUndo], *, restore: bool) -> None:
        changed: list[PlanningCenterLengthUndo] = []
        completed: list[PlanningCenterLengthUndo] = []
        client: PlanningCenterClient | None = None
        halted: str | None = None
        try:
            if candidates:
                client = self.client_factory()
            for record in candidates:
                expected = record.new_length if restore else record.old_length
                target = record.old_length if restore else record.new_length
                item_result = LengthItemResult(
                    item_id=record.item_id,
                    title=record.title,
                    old_length=expected,
                    new_length=target,
                    status="failed",
                )
                self.result.items.append(item_result)
                if halted:
                    item_result.reason = halted
                    continue
                assert client is not None
                attempted = False
                accepted = False
                undo = record
                previous_records = self.settings.snapshot().planning_center_length_undo
                try:
                    state = await self.state.snapshot()
                    if state.planning_center_status is not ConnectionStatus.CONNECTED:
                        raise PlanningCenterConfigurationError("Connect Planning Center first.")
                    if self.is_playing():
                        raise PlanningCenterConfigurationError(
                            "Playback started; stop it before changing plan times."
                        )
                    if not restore and not (self.write_allowed() and await self.plan_current()):
                        raise PlanningCenterConfigurationError(
                            "The scan or Playback changed; preview again."
                        )
                    current = await client.get_plan_item(
                        record.service_type_id, record.plan_id, record.item_id
                    )
                    songs, _ = client._extract_songs([current])
                    if (
                        restore
                        and record.pending_from_length is not None
                        and current.attributes.length == record.pending_from_length
                    ):
                        # A later uncertain PATCH never committed: restore the previous
                        # StagePilot value, not an unrelated remote edit.
                        expected = record.pending_from_length
                        item_result.old_length = expected
                    if not songs or current.attributes.length != expected:
                        item_result.status = "skipped"
                        item_result.reason = "Someone changed this item; their edit was left alone."
                        if restore:
                            completed.append(record)
                        continue
                    if self.is_playing():
                        raise PlanningCenterConfigurationError(
                            "Playback started; stop it before changing plan times."
                        )
                    if not restore:
                        # Record before PATCH; a crash/timeout can leave an uncertain write.
                        prior = next(
                            (
                                r
                                for r in previous_records
                                if (r.service_type_id, r.plan_id, r.item_id)
                                == (record.service_type_id, record.plan_id, record.item_id)
                            ),
                            None,
                        )
                        if prior and expected not in (
                            prior.new_length,
                            prior.pending_from_length,
                        ):
                            item_result.status = "skipped"
                            item_result.reason = (
                                "Someone changed this item; their edit was left alone."
                            )
                            continue
                        # Coalesce a sequence of explicit updates into one restore point.
                        # Fresh remote state distinguishes an uncertain committed write
                        # (prior.new_length) from one that never committed (prior.old_length).
                        undo = record.model_copy(
                            update={
                                "old_length": prior.old_length if prior else expected,
                                "pending_from_length": expected,
                            }
                        )
                        self._persist([r for r in previous_records if r != prior] + [undo])
                    attempted = True
                    response = await client.update_plan_item_length(
                        record.service_type_id,
                        record.plan_id,
                        record.item_id,
                        target,
                        expected_length=expected,
                        write_allowed=lambda: (
                            not self.is_playing() and (restore or self.write_allowed())
                        ),
                    )
                    accepted = True
                    get_logger("planning_center").info(
                        "planning_center_item_length_written",
                        item_id=record.item_id,
                        old_length=expected,
                        new_length=target,
                    )
                    changed.append(record)
                    item_result.status = "restored" if restore else "updated"
                    if response.attributes.length != target:
                        raise RuntimeError("Unconfirmed write")
                    verified = await client.get_plan_item(
                        record.service_type_id, record.plan_id, record.item_id
                    )
                    if verified.attributes.length != target:
                        raise RuntimeError("Unconfirmed write")

                    if not restore:
                        self._persist(
                            [
                                r.model_copy(update={"pending_from_length": None})
                                if r == undo
                                else r
                                for r in self.settings.snapshot().planning_center_length_undo
                            ]
                        )

                    if restore:
                        completed.append(record)
                except PlanningCenterLengthConflictError:
                    item_result.status = "skipped"
                    item_result.reason = "Someone changed this item; their edit was left alone."
                    if restore:
                        completed.append(record)
                    else:
                        self._persist(previous_records)
                except Exception as exc:
                    if isinstance(exc, PlanningCenterPermissionError):
                        halted = DENIED_MESSAGE
                    elif isinstance(exc, PlanningCenterRateLimitError):
                        halted = (
                            "Planning Center is rate limiting changes. "
                            "Stopped after one Retry-After retry."
                        )
                    elif isinstance(exc, PlanningCenterConfigurationError):
                        halted = str(exc)
                    elif isinstance(exc, PlanningCenterApiError):
                        halted = (
                            f"Planning Center returned HTTP {exc.status_code}; "
                            "this change was not confirmed."
                        )
                    else:
                        halted = (
                            "Planning Center times could not be changed or confirmed. "
                            "Check the plan before trying again; song order and lengths are saved."
                        )
                    item_result.reason = halted
                    # A definite HTTP refusal did not write. Uncertain writes retain undo.
                    if (
                        not restore
                        and attempted
                        and not accepted
                        and isinstance(
                            exc, (PlanningCenterPermissionError, PlanningCenterRateLimitError)
                        )
                    ):
                        self._persist(previous_records)
        except asyncio.CancelledError:
            self.result.status = "partial" if changed else "failed"
            self.result.reload = "failed" if changed else "not_needed"
            self.result.message = (
                "Planning Center changes were interrupted. Song order and lengths are saved. "
                "Check the plan; previous times are kept for any unconfirmed writes."
            )
            raise
        except Exception:
            halted = "Planning Center times could not be changed. Song order and lengths are saved."
            present = {item.item_id for item in self.result.items}
            self.result.items.extend(
                LengthItemResult(item_id=r.item_id, title=r.title, status="failed", reason=halted)
                for r in candidates
                if r.item_id not in present
            )
        finally:
            if client is not None:
                with suppress(Exception):
                    await client.close()
        if changed:
            await self._reload(changed, restore=restore)
        if restore and completed:
            try:
                self._persist(
                    [
                        r
                        for r in self.settings.snapshot().planning_center_length_undo
                        if r not in completed
                    ]
                )
            except Exception:
                halted = "Restored times could not be cleared from the undo record."
        failed = bool(halted) or any(item.status == "failed" for item in self.result.items)
        self.result.status = (
            "partial" if failed and changed else "failed" if failed or halted else "done"
        )
        self._summarize(halted)

    async def _reload(self, changed: list[PlanningCenterLengthUndo], *, restore: bool) -> None:
        state = await self.state.snapshot()
        loaded_changes = [
            r
            for r in changed
            if state.plan is not None
            and (r.plan_id, r.service_type_id) == (state.plan.id, state.plan.service_type_id)
        ]
        if not loaded_changes:
            self.result.reload = "different_plan"
            return
        changed = loaded_changes
        if state.timer.status is TimerStatus.RUNNING:
            self.result.reload = "deferred"
            return
        previous_reload = state.last_successful_plan_reload_at
        try:
            outcome = await self.dispatcher.dispatch(
                ActionName.RELOAD_PLAN, source="planning_center_lengths"
            )
            if not outcome.accepted:
                self.result.reload = "deferred"
                return
            async with asyncio.timeout(self.reload_timeout):
                while True:
                    state = await self.state.snapshot()
                    if state.last_successful_plan_reload_at != previous_reload:
                        actual = (
                            {song.id: song.duration_seconds for song in state.plan.songs}
                            if state.plan
                            else {}
                        )
                        self.result.reload = (
                            "verified"
                            if state.plan
                            and all(
                                (state.plan.id, state.plan.service_type_id)
                                == (r.plan_id, r.service_type_id)
                                and actual.get(r.item_id)
                                == (r.old_length if restore else r.new_length)
                                for r in changed
                            )
                            else "failed"
                        )
                        return
                    await asyncio.sleep(0.05)
        except Exception:
            self.result.reload = "failed"

    def _summarize(self, halted: str | None) -> None:
        changed = [item for item in self.result.items if item.status in ("updated", "restored")]
        unchanged = [item for item in self.result.items if item.status == "skipped"]
        verb = "Restored" if self.result.operation == "restore" else "Updated"
        parts = []
        if changed:
            parts.append(
                f"{verb} {len(changed)} songs in Planning Center: "
                + ", ".join(
                    f"{item.title} {duration(item.old_length)} -> {duration(item.new_length)}"
                    for item in changed
                )
                + "."
            )
        elif not halted:
            parts.append("No Planning Center times needed changing.")
        within = [item for item in unchanged if item.reason == "Already the same."]
        if within:
            parts.append(f"Left {len(within)} songs as they were (already the same).")
        parts.extend(
            f"{item.title}: {item.reason}"
            for item in self.result.items
            if item.reason and item not in within
        )
        if halted and not any(item.status == "failed" for item in self.result.items):
            parts.append(halted)
        if self.result.reload == "verified":
            parts.append("StagePilot reloaded the plan and verified the song times.")
        elif self.result.reload == "deferred":
            parts.append(
                "Times changed in Planning Center. The timer is running; "
                "reload the plan when it is idle."
            )
        elif self.result.reload == "different_plan":
            parts.append(
                f"Updated {self._target_label} in Planning Center. "
                "StagePilot is showing a different plan, so nothing changed here."
            )
        elif self.result.reload == "failed":
            parts.append(
                "Times changed in Planning Center, but StagePilot has not "
                "reloaded and verified them yet."
            )
        self.result.message = " ".join(parts)
