"""Playback receive configuration; the sole control exception is confirmed discovery."""

from __future__ import annotations

import json
import time
from datetime import datetime
from typing import Any, Literal

from fastapi import APIRouter, Body, HTTPException, Request
from pydantic import BaseModel, ConfigDict, Field

from stagepilot.api.remote_retry import RemoteRetryRoute
from stagepilot.core.config import MidiSource, PlaybackApiSettings
from stagepilot.core.settings import SettingsFileError
from stagepilot.plugins.planning_center.errors import (
    PlanningCenterApiError,
    PlanningCenterAuthenticationError,
    PlanningCenterPermissionError,
)
from stagepilot.plugins.planning_center.models import PlanningCenterServiceType
from stagepilot.plugins.playback_api.arbiter import PlaybackConnection, Source, SourceStatus
from stagepilot.plugins.playback_api.client import ConnectionOptions
from stagepilot.plugins.playback_api.normalizer import PlaybackEvent
from stagepilot.plugins.playback_api.plugin import (
    LOCAL_NETWORK_SETTINGS_URL,
    DiscoveryConflict,
    PlaybackInputPlugin,
)
from stagepilot.services.planning_center_lengths import (
    DENIED_MESSAGE,
    LengthPreview,
    LengthUpdateResult,
)

router = APIRouter(prefix="/api/v1/playback-api", route_class=RemoteRetryRoute)


class PlaybackSettingsRequest(BaseModel):
    model_config = ConfigDict(extra="forbid")
    enabled: bool = True
    host: str | None = None
    port: int = Field(default=8080, ge=1, le=65535)
    auto_scan: bool = True
    fast_transport: bool = True


class ScanCandidateModel(BaseModel):
    name: str
    host: str


class ScanResult(BaseModel):
    state: Literal["idle", "scanning", "found", "not_found"]
    candidates: list[ScanCandidateModel]
    reason: str | None
    current: int
    total: int
    phase: str | None = None
    error_class: str | None = None
    typed: bool = False
    elapsed: float = 0.0
    networks: list[str] = Field(default_factory=list)
    hosts_probed: int = 0
    hosts_total: int = 0
    details: str | None = None
    settings_url: str | None = None


class FindRequest(BaseModel):
    model_config = ConfigDict(extra="forbid")
    host: str | None = Field(default=None, max_length=255)


class PlaybackPlanSong(BaseModel):
    title: str
    duration_seconds: int | None


class PlaybackStatusResponse(BaseModel):
    planning_center_lengths: LengthUpdateResult
    planning_center_undo_available: bool
    planning_center_connected: bool
    plan_title: str | None
    plan_date: str | None
    scan: ScanResult
    selected: bool
    enabled: bool
    connected: bool
    active_source: Source
    sources: dict[str, SourceStatus]
    reason: str
    host: str | None
    port: int
    source: Literal["manual", "loopback", "lan"] | None
    last_error: str | None
    playing: bool
    setlist_cloud_version: int | None
    discovery: Literal["idle", "running", "failed", "done"]
    progress: int
    discovery_song: int
    discovery_total: int
    song_order: list[int]
    song_lengths: list[float | None]
    discovery_duration_seconds: float | None
    planning_center_update_service_type_id: str | None
    planning_center_update_reason: str | None
    lengths_measured_at: datetime | None
    plan_songs: list[PlaybackPlanSong]
    song_count: int
    plan_song_count: int
    captured_version: int | None
    captured_at: datetime | None
    stale: bool
    plan_changed: bool = False
    lengths_applied: bool = False
    settings: PlaybackSettingsRequest


class PlaybackMonitorEntry(BaseModel):
    event: PlaybackEvent
    discovery: bool
    at: datetime


class PlaybackEventsResponse(BaseModel):
    events: list[PlaybackMonitorEntry]
    capacity: int = 100


def _controller(request: Request) -> PlaybackInputPlugin:
    controller: PlaybackInputPlugin | None = request.app.state.runtime.playback_input
    if controller is None:
        raise HTTPException(409, "Playback API input is unavailable.")
    return controller


def scan_extras(controller: PlaybackInputPlugin) -> dict[str, Any]:
    last = controller.last_scan or {}
    elapsed = controller.scan_elapsed
    if controller.scan_state == "scanning" and controller.scan_started_at is not None:
        elapsed = time.monotonic() - controller.scan_started_at
    blocked = controller.scan_error_class == "permission_denied"
    return {
        "phase": controller.scan_phase if controller.scan_state == "scanning" else None,
        "error_class": controller.scan_error_class,
        "typed": controller.scan_typed,
        "elapsed": round(elapsed, 1),
        "networks": [str(n) for n in last.get("networks", [])],  # type: ignore[attr-defined]
        "hosts_probed": int(last.get("hosts_probed", 0)),  # type: ignore[call-overload]
        "hosts_total": int(last.get("hosts_total", 0)),  # type: ignore[call-overload]
        "details": json.dumps(last, default=str, sort_keys=True) if last else None,
        "settings_url": LOCAL_NETWORK_SETTINGS_URL if blocked else None,
    }


async def status_response(controller: PlaybackInputPlugin) -> PlaybackStatusResponse:
    state = await controller.state_store.snapshot()
    status = controller.status
    config = controller.settings.playback_api
    outdated = controller.mapper.stale or await controller.plan_changed()
    heartbeat = status.heartbeat
    connection = controller.connection
    return PlaybackStatusResponse(
        planning_center_lengths=controller.planning_center_lengths.result,
        planning_center_undo_available=controller.planning_center_lengths.undo_available,
        planning_center_connected=state.planning_center_status.value == "connected",
        planning_center_update_reason=await update_reason(controller),
        planning_center_update_service_type_id=config.planning_center_update_service_type_id
        or controller.settings.planning_center.service_type_id,
        discovery_duration_seconds=config.discovery_duration_seconds,
        plan_title=state.plan.title if state.plan else None,
        plan_date=state.plan.date.isoformat() if state.plan else None,
        scan=ScanResult(
            state=controller.scan_state,
            candidates=[
                ScanCandidateModel(name=c.name, host=c.host) for c in controller.scan_candidates
            ],
            reason=controller.scan_reason,
            current=controller.scan_current,
            total=controller.scan_total,
            **scan_extras(controller),
        ),
        selected=controller.selected,
        enabled=config.enabled,
        connected=connection.connected,
        active_source=connection.active_source,
        sources=connection.sources,
        reason=connection.reason,
        host=status.host,
        port=status.port,
        source=status.source,
        last_error=controller.discovery_error
        or (None if connection.connected else status.last_error),
        playing=heartbeat.playing if heartbeat else False,
        setlist_cloud_version=heartbeat.setlist_version if heartbeat else None,
        discovery=controller.discovery,
        progress=controller.progress,
        discovery_song=controller.discovery_song,
        discovery_total=controller.discovery_total,
        song_order=list(controller.mapper.song_order),
        song_lengths=config.song_lengths if not outdated else [],
        lengths_measured_at=config.lengths_measured_at if not outdated else None,
        plan_songs=[
            PlaybackPlanSong(title=song.title, duration_seconds=song.duration_seconds)
            for song in state.plan.songs
        ]
        if state.plan
        else [],
        song_count=len(controller.mapper.song_order),
        plan_song_count=len(state.plan.songs) if state.plan else 0,
        captured_version=controller.mapper.captured_version,
        captured_at=config.captured_at,
        stale=controller.mapper.stale,
        plan_changed=await controller.plan_changed(),
        lengths_applied=await controller.lengths_applied(),
        settings=PlaybackSettingsRequest(
            enabled=config.enabled,
            host=config.host,
            port=config.port,
            auto_scan=config.auto_scan,
            fast_transport=config.fast_transport,
        ),
    )


@router.get("/status", response_model=PlaybackStatusResponse)
async def status(request: Request) -> PlaybackStatusResponse:
    await _controller(request)._publish_connection()
    return await status_response(_controller(request))


@router.get("/connection", response_model=PlaybackConnection)
async def connection(request: Request) -> PlaybackConnection:
    controller = _controller(request)
    await controller._publish_connection()
    return controller.connection


@router.get("/events", response_model=PlaybackEventsResponse)
async def events(request: Request) -> PlaybackEventsResponse:
    return PlaybackEventsResponse(
        events=[
            PlaybackMonitorEntry(event=item.event, discovery=item.discovery, at=item.at)
            for item in _controller(request).events
        ]
    )


@router.post("/find", response_model=PlaybackStatusResponse)
async def find(request: Request, payload: object = Body(default=None)) -> PlaybackStatusResponse:
    """Scan, no setup needed: turns Playback on and selects it, then looks for it."""
    controller = _controller(request)
    try:
        body = FindRequest.model_validate(payload or {})
    except ValueError as exc:
        raise HTTPException(422, "Enter a host name or IP address.") from exc
    try:
        await controller.find(body.host)
    except DiscoveryConflict as exc:
        raise HTTPException(409, str(exc)) from exc
    except ValueError as exc:
        raise HTTPException(422, str(exc)) from exc
    return await status_response(controller)


@router.post("/find/cancel", response_model=PlaybackStatusResponse)
async def cancel_find(request: Request) -> PlaybackStatusResponse:
    controller = _controller(request)
    await controller.cancel_scan()
    return await status_response(controller)


@router.put("/settings", response_model=PlaybackStatusResponse)
async def settings(payload: PlaybackSettingsRequest, request: Request) -> PlaybackStatusResponse:
    controller = _controller(request)
    runtime = request.app.state.runtime
    if controller.discovery == "running":
        raise HTTPException(409, "Wait for Discover Song Order before changing settings.")
    try:
        ConnectionOptions(payload.enabled, payload.host, payload.port, payload.auto_scan)
    except ValueError as exc:
        raise HTTPException(422, str(exc)) from exc
    previous = runtime.settings_service.snapshot()
    updated = PlaybackApiSettings.model_validate(
        {**previous.playback_api.model_dump(), **payload.model_dump()}
    )
    # Turning Playback on is choosing it: one switch, no hidden second source setting.
    modes = previous.integration_modes
    if payload.enabled and modes.midi_source is not MidiSource.PLAYBACK_API:
        modes = modes.model_copy(update={"midi_source": MidiSource.PLAYBACK_API})
    try:
        await controller.save_settings(
            previous.model_copy(
                update={"playback_api": updated, "integration_modes": modes}, deep=True
            )
        )
    except SettingsFileError as exc:
        raise HTTPException(503, str(exc)) from exc
    except DiscoveryConflict as exc:
        raise HTTPException(409, str(exc)) from exc
    return await status_response(controller)


def scan_signature(controller: PlaybackInputPlugin) -> str:
    config = controller.settings.playback_api
    return json.dumps(
        [
            config.song_order,
            config.song_lengths,
            str(config.captured_at),
            config.captured_version,
            config.setlist_id,
            config.planning_center_update_service_type_id,
            config.scan_plan_key,
        ],
        sort_keys=True,
    )


async def update_reason(controller: PlaybackInputPlugin) -> str | None:
    state = await controller.state_store.snapshot()
    if state.planning_center_status.value != "connected":
        return "Connect Planning Center first."
    if controller.status.heartbeat and controller.status.heartbeat.playing:
        return "Stop Playback first."
    if controller.discovery == "failed":
        return "The song scan failed or was cancelled. Scan songs again."
    config = controller.settings.playback_api
    if controller.discovery == "running":
        return "Wait for the song scan to finish."
    if not config.captured_at or not config.song_order:
        return "Scan songs first to read the lengths from Playback."
    if controller.mapper.stale:
        return "The setlist changed. Scan songs again."
    if await controller.plan_changed():
        return "The Services plan changed since the scan. Scan songs again."
    if await controller.lengths_applied():
        return "Planning Center already has these lengths. Scan songs again for new ones."
    if not any(length is not None for length in config.song_lengths):
        return "The scan has no song lengths. Scan songs again."
    if controller.planning_center_lengths.result.status == "running":
        return "Wait for the Planning Center changes to finish."
    return None


class CategoryChoice(BaseModel):
    model_config = ConfigDict(extra="forbid")
    service_type_id: str = Field(min_length=1, max_length=128, pattern=r"^[A-Za-z0-9_-]+$")


class LengthConfirmation(BaseModel):
    model_config = ConfigDict(extra="forbid")
    token: str
    confirm: Literal[True]


def preview_error(exc: Exception) -> HTTPException:
    if isinstance(exc, (PlanningCenterPermissionError, PlanningCenterAuthenticationError)) or (
        isinstance(exc, PlanningCenterApiError) and exc.status_code in (401, 403)
    ):
        return HTTPException(403, DENIED_MESSAGE)
    if isinstance(exc, ValueError):
        return HTTPException(409, str(exc))
    return HTTPException(
        503,
        "Planning Center could not read this plan. "
        "Song order and lengths are saved. Try again later.",
    )


@router.get("/planning-center-lengths/categories", response_model=list[PlanningCenterServiceType])
async def length_categories(request: Request) -> list[PlanningCenterServiceType]:
    try:
        return await _controller(request).planning_center_lengths.categories()
    except Exception as exc:
        raise preview_error(exc) from None


@router.put("/planning-center-lengths/category", response_model=PlaybackStatusResponse)
async def choose_length_category(
    payload: CategoryChoice, request: Request
) -> PlaybackStatusResponse:
    controller = _controller(request)
    if (
        controller.discovery == "running"
        or controller.planning_center_lengths.result.status == "running"
    ):
        raise HTTPException(409, "Wait for the current operation to finish.")
    try:
        categories = await controller.planning_center_lengths.categories()
        if not any(c.id == payload.service_type_id for c in categories):
            raise ValueError("Choose an available Planning Center category.")
        previous = controller._service.snapshot()
        config = previous.playback_api.model_copy(
            update={"planning_center_update_service_type_id": payload.service_type_id}
        )
        controller._service.save(previous.model_copy(update={"playback_api": config}))
        controller.settings.playback_api = config
    except Exception as exc:
        raise preview_error(exc) from None
    return await status_response(controller)


@router.post("/planning-center-lengths/preview", response_model=LengthPreview)
async def preview_lengths(request: Request) -> LengthPreview:
    controller = _controller(request)
    reason = await update_reason(controller)
    if reason:
        raise HTTPException(409, reason)
    config = controller.settings.playback_api
    category = (
        config.planning_center_update_service_type_id
        or controller.settings.planning_center.service_type_id
    )
    if not category:
        raise HTTPException(409, "Choose a Planning Center category first.")
    try:
        return await controller.planning_center_lengths.preview(
            category, config.song_lengths, scan_signature(controller)
        )
    except Exception as exc:
        raise preview_error(exc) from None


@router.post("/planning-center-lengths/confirm", response_model=PlaybackStatusResponse)
async def confirm_lengths(payload: LengthConfirmation, request: Request) -> PlaybackStatusResponse:
    controller = _controller(request)
    reason = await update_reason(controller)
    if reason:
        raise HTTPException(409, reason)
    signature = scan_signature(controller)
    controller.planning_center_lengths.write_allowed = lambda: (
        not controller.mapper.stale
        and controller.discovery not in ("running", "failed")
        and scan_signature(controller) == signature
        and not (controller.status.heartbeat and controller.status.heartbeat.playing)
    )
    try:
        await controller.planning_center_lengths.confirm(payload.token, signature)
    except Exception as exc:
        raise preview_error(exc) from None
    return await status_response(controller)


@router.post("/planning-center-lengths/restore", response_model=PlaybackStatusResponse)
async def restore_lengths(
    request: Request, payload: object = Body(default=None)
) -> PlaybackStatusResponse:
    if not isinstance(payload, dict) or payload.get("confirm") is not True:
        raise HTTPException(400, "Confirm restoring Planning Center times with {confirm: true}.")
    controller = _controller(request)
    if (
        controller.discovery == "running"
        or controller.planning_center_lengths.result.status == "running"
    ):
        raise HTTPException(409, "Wait for the scan or Planning Center changes to finish.")
    if controller.status.heartbeat and controller.status.heartbeat.playing:
        raise HTTPException(409, "Stop Playback before restoring Planning Center times.")
    await controller.planning_center_lengths.restore()
    return await status_response(controller)


@router.post("/discover-song-order", response_model=PlaybackStatusResponse)
async def discover(
    request: Request, payload: object = Body(default=None)
) -> PlaybackStatusResponse:
    if not isinstance(payload, dict) or payload.get("confirm") is not True:
        raise HTTPException(400, "Confirm Discover Song Order with {confirm: true}.")
    controller = _controller(request)
    try:
        await controller.discover_song_order()
    except DiscoveryConflict as exc:
        raise HTTPException(409, str(exc)) from exc
    return await status_response(controller)
