"""StagePilot REST endpoints."""

from __future__ import annotations

import asyncio
from datetime import date, datetime
from typing import Literal, NoReturn, cast
from uuid import UUID
from zoneinfo import ZoneInfo

from fastapi import APIRouter, HTTPException, Request
from pydantic import ValidationError

from stagepilot.api.remote_retry import RemoteRetryRoute
from stagepilot.core.config import (
    LightingCue,
    LightingMethod,
    LightsSettings,
    MidiSource,
    OscLightingCue,
    ProPresenterSettings,
    ServiceSource,
    Settings,
    SongLightingCueMap,
)
from stagepilot.core.events import (
    ActionName,
    EventType,
    ServicePlanSelectionPayload,
    new_event,
)
from stagepilot.core.lights import LightsSnapshot
from stagepilot.core.midi import MidiController, MidiInputSnapshot
from stagepilot.core.propresenter import ProPresenterSnapshot
from stagepilot.core.runtime import Runtime
from stagepilot.core.settings import (
    CredentialStoreError,
    PersistentPlanningCenterSettings,
    PersistentSettings,
    SettingsFileError,
)
from stagepilot.models.api import (
    ActionResponse,
    HealthResponse,
    LightingCaptureRequest,
    LightingCaptureResponse,
    LightingCaptureUndoRequest,
    LightingCueMapRequest,
    LightingCueTestRequest,
    LightingOscCueTestRequest,
    LightingOutputResponse,
    LightingPadsRequest,
    LightsOperationResponse,
    LightsSettingsRequest,
    LightsStatusResponse,
    MidiCueSimulationRequest,
    MidiCueSimulationResponse,
    MidiInputResponse,
    MidiInputSelectionRequest,
    MidiInputSelectionResponse,
    MidiInputsResponse,
    MidiMonitorMessageResponse,
    MidiMonitorResponse,
    PendingPlanSelectionResponse,
    PlanningCenterOAuthCallbackRequest,
    PlanningCenterOAuthStartResponse,
    PlanningCenterOAuthStatusResponse,
    PlanningCenterServiceTypeResponse,
    PlanningCenterSettingsUpdateRequest,
    PlanningCenterStatusResponse,
    PlanningCenterTestRequest,
    PlanningCenterTestResponse,
    PlanSelectionRequest,
    PlanSelectionResponse,
    ProPresenterLookResponse,
    ProPresenterOperationResponse,
    ProPresenterSettingsRequest,
    ProPresenterStatusResponse,
    ProPresenterTimerResponse,
    SettingsResponse,
)
from stagepilot.models.state import (
    ApplicationState,
    ApplicationStatus,
    ConnectionStatus,
    PluginStatus,
    ServiceLoadStatus,
)
from stagepilot.planning_center_oauth import (
    PlanningCenterOAuthError,
    PlanningCenterOAuthService,
)
from stagepilot.plugins.planning_center.errors import (
    PlanningCenterAuthenticationError,
    PlanningCenterConfigurationError,
    PlanningCenterError,
    PlanningCenterPermissionError,
    PlanningCenterRateLimitError,
)
from stagepilot.plugins.planning_center.plugin import ALL_SERVICE_TYPES_ID
from stagepilot.plugins.playback_api.plugin import DiscoveryConflict
from stagepilot.plugins.propresenter.errors import ProPresenterError
from stagepilot.services.presentation_source import PresentationBusyError, PresentationPluginFactory
from stagepilot.services.service_source import CREDENTIAL_BUSY_MESSAGE, ServiceSourceBusyError

router = APIRouter(prefix="/api/v1", route_class=RemoteRetryRoute)


def _runtime(request: Request) -> Runtime:
    return request.app.state.runtime  # type: ignore[no-any-return]


def _midi_controller(runtime: Runtime) -> MidiController | None:
    inputs = runtime.playback_input
    if inputs is not None:
        return inputs.midi
    return runtime.midi_controller


def _current_local_date(timezone_name: str) -> date:
    return datetime.now(ZoneInfo(timezone_name)).date()


def _runtime_settings_without_midi(settings: Settings) -> dict[str, object]:
    payload = cast(dict[str, object], settings.model_dump(mode="python"))
    payload.pop("midi", None)
    return payload


async def _apply_service_source(runtime: Runtime) -> str | None:
    """Make the running Services plugin match the saved settings; return a warning on failure."""

    controller = runtime.service_source
    if controller is None:
        return None
    try:
        health = await controller.sync()
    except ServiceSourceBusyError as exc:
        raise HTTPException(409, str(exc)) from exc
    if health is not None and health.status is PluginStatus.ERROR:
        return health.last_error or "Services could not be started."
    return None


async def _apply_presentation(runtime: Runtime) -> str | None:
    """Make the running Presentation output match the saved settings; warn on failure."""

    controller = runtime.presentation
    if controller is None:
        return None
    try:
        health = await controller.sync()
    except PresentationBusyError as exc:
        raise HTTPException(409, str(exc)) from exc
    if health is not None and health.status is PluginStatus.ERROR:
        return health.last_error or "Presentation could not be started."
    return None


async def _refuse_presentation_change(runtime: Runtime, requested: PersistentSettings) -> None:
    """Reject an output switch the environment pins, or one that would disturb a running show."""

    effective = runtime.settings_service.effective_runtime_settings()
    wanted = PresentationPluginFactory.wanted
    if wanted(requested) is wanted(effective):
        return
    pin = runtime.settings_service.timer_output_pin()
    if pin is not None:
        raise HTTPException(
            409,
            f"This is set by the {pin} environment variable and cannot be changed here.",
        )
    if runtime.presentation is not None:
        try:
            await runtime.presentation.ensure_idle()
        except PresentationBusyError as exc:
            raise HTTPException(409, str(exc)) from exc


async def _apply_planning_center_credentials(runtime: Runtime) -> None:
    """Hand the current credentials (OAuth token or PAT) to the running Planning Center plugin."""

    plugin = runtime.planning_center
    if plugin is not None:
        await plugin.reconfigure(
            runtime.settings_service.effective_runtime_settings().planning_center
        )
    else:
        await _apply_service_source(runtime)


async def _ensure_show_idle(runtime: Runtime, message: str) -> None:
    controller = runtime.service_source
    if controller is None:
        return
    try:
        await controller.ensure_idle(message)
    except ServiceSourceBusyError as exc:
        raise HTTPException(409, str(exc)) from exc


async def _refuse_service_source_change(
    runtime: Runtime, requested: ServiceSource, effective: ServiceSource
) -> None:
    """Reject a source switch the environment pins, or one that would disturb a running show."""

    if requested is effective:
        return
    pin = runtime.settings_service.service_source_pin()
    if pin is not None:
        raise HTTPException(
            409,
            f"This is set by the {pin} environment variable and cannot be changed here.",
        )
    controller = runtime.service_source
    if controller is not None:
        try:
            await controller.ensure_idle()
        except ServiceSourceBusyError as exc:
            raise HTTPException(409, str(exc)) from exc


def _settings_response(
    runtime: Runtime,
    *,
    persisted: bool = False,
    restart_required: bool = False,
) -> SettingsResponse:
    settings = (
        runtime.settings_service.snapshot()
        if persisted
        else runtime.settings_service.effective_snapshot()
    )
    return SettingsResponse(
        settings=settings,
        planning_center_secret_saved=runtime.settings_service.credential_saved,
        warning=runtime.settings_service.warning,
        restart_required=restart_required,
    )


def _raise_planning_center_http_error(exc: PlanningCenterError) -> NoReturn:
    status_code = 502
    headers: dict[str, str] | None = None
    if isinstance(exc, PlanningCenterConfigurationError):
        status_code = 409
    elif isinstance(exc, PlanningCenterAuthenticationError):
        status_code = 401
    elif isinstance(exc, PlanningCenterPermissionError):
        status_code = 403
    elif isinstance(exc, PlanningCenterRateLimitError):
        status_code = 429
        if exc.retry_after_seconds is not None:
            headers = {"Retry-After": str(exc.retry_after_seconds)}
    raise HTTPException(status_code=status_code, detail=str(exc), headers=headers) from exc


def _production_service_ready(state: ApplicationState, current_date: date) -> bool:
    plan = state.plan
    return (
        state.planning_center_status is ConnectionStatus.CONNECTED
        and state.service_load.status is ServiceLoadStatus.LOADED
        and not state.service_load.is_stale
        and plan is not None
        and state.service_load.target_date == plan.date
        and plan.date >= current_date
        and bool(plan.songs)
    )


def _midi_inputs_response(snapshot: MidiInputSnapshot) -> MidiInputsResponse:
    return MidiInputsResponse(
        enabled=snapshot.enabled,
        channel=snapshot.channel,
        note=snapshot.note,
        configured_input_name=snapshot.configured_input_name,
        selected_input_name=snapshot.selected_input_name,
        inputs=[
            MidiInputResponse(
                id=value.id,
                name=value.name,
                ambiguous=value.ambiguous,
                selected=value.selected,
                connected=value.connected,
            )
            for value in snapshot.inputs
        ],
        mappings=dict(snapshot.mappings),
    )


def _propresenter_response(snapshot: ProPresenterSnapshot) -> ProPresenterStatusResponse:
    return ProPresenterStatusResponse(
        enabled=snapshot.enabled,
        host=snapshot.host,
        port=snapshot.port,
        timer_name=snapshot.timer_name,
        look_id=snapshot.look_id,
        request_timeout_seconds=snapshot.request_timeout_seconds,
        connection_status=snapshot.connection_status,
        detail=snapshot.detail,
        timers=[
            ProPresenterTimerResponse(
                id=timer.id,
                name=timer.name,
                index=timer.index,
                is_countdown=timer.is_countdown,
                state=timer.state,
            )
            for timer in snapshot.timers
        ],
        selected_timer_id=snapshot.selected_timer_id,
        timer_found=snapshot.timer_found,
        looks=[
            ProPresenterLookResponse(id=look.id, name=look.name, index=look.index)
            for look in snapshot.looks
        ],
        current_look_id=snapshot.current_look_id,
        look_found=snapshot.look_found,
        last_checked_at=snapshot.last_checked_at,
    )


def _lights_response(snapshot: LightsSnapshot) -> LightsStatusResponse:
    return LightsStatusResponse(
        enabled=snapshot.enabled,
        method=snapshot.method,
        osc_host=snapshot.osc_host,
        osc_port=snapshot.osc_port,
        output_name=snapshot.output_name,
        channel=snapshot.channel,
        pulse_ms=snapshot.pulse_ms,
        connection_status=snapshot.connection_status,
        detail=snapshot.detail,
        outputs=[
            LightingOutputResponse(
                name=output.name,
                ambiguous=output.ambiguous,
                selected=output.selected,
                connected=output.connected,
            )
            for output in snapshot.outputs
        ],
        last_cue=snapshot.last_cue,
        last_osc_cue=snapshot.last_osc_cue,
        last_cue_at=snapshot.last_cue_at,
    )


async def _propresenter_status(
    runtime: Runtime,
    *,
    refresh: bool = False,
) -> ProPresenterStatusResponse:
    controller = runtime.propresenter_controller
    if controller is None:
        settings = runtime.settings_service.effective_runtime_settings().propresenter
        return ProPresenterStatusResponse(
            enabled=False,
            host=settings.host,
            port=settings.port,
            timer_name=settings.timer_name,
            look_id=settings.look_id,
            request_timeout_seconds=settings.request_timeout_seconds,
            connection_status=ConnectionStatus.DISCONNECTED,
            detail="The ProPresenter plugin is disabled.",
            timers=[],
            selected_timer_id=None,
            timer_found=False,
            looks=[],
            current_look_id=None,
            look_found=settings.look_id is None,
            last_checked_at=None,
        )
    return _propresenter_response(await controller.snapshot(refresh=refresh))


@router.get("/health", response_model=HealthResponse)
async def health(request: Request) -> HealthResponse:
    runtime = _runtime(request)
    playback_ready = True
    if runtime.playback_input is not None:
        await runtime.playback_input._publish_connection()
        playback_ready = (
            runtime.playback_input.settings.integration_modes.midi_source is MidiSource.SIMULATED
            or runtime.playback_input.connection.connected
        )
    state = await runtime.state_store.snapshot()
    plugins = await runtime.plugin_manager.health()
    service_ready = (
        state.service_load.status is not ServiceLoadStatus.ERROR
        if runtime.settings.uses_demo_service
        else _production_service_ready(
            state,
            _current_local_date(runtime.settings.timezone),
        )
    )
    healthy = (
        state.application_status is ApplicationStatus.RUNNING
        and all(plugin.status is PluginStatus.RUNNING for plugin in plugins)
        and service_ready
        and playback_ready
    )
    return HealthResponse(
        status="healthy" if healthy else "degraded",
        version=runtime.settings.version,
        application_status=state.application_status,
        plugins=plugins,
        started_at=runtime.started_at,
    )


@router.get("/health/live")
async def liveness() -> dict[str, str]:
    """Report that the HTTP process and event loop can serve requests."""

    return {"status": "alive"}


@router.get("/state", response_model=ApplicationState)
async def state(request: Request) -> ApplicationState:
    runtime = _runtime(request)
    if runtime.playback_input is not None:
        await runtime.playback_input._publish_connection()
    return await runtime.state_store.snapshot()


@router.get("/settings", response_model=SettingsResponse)
async def settings(request: Request) -> SettingsResponse:
    return _settings_response(_runtime(request))


@router.put("/settings", response_model=SettingsResponse)
async def update_settings(
    settings: PersistentSettings,
    request: Request,
) -> SettingsResponse:
    runtime = _runtime(request)
    previous = runtime.settings_service.effective_runtime_settings()
    await _refuse_service_source_change(
        runtime,
        settings.integration_modes.service_source,
        previous.integration_modes.service_source,
    )
    await _refuse_presentation_change(runtime, settings)
    if runtime.playback_input is not None:
        if runtime.playback_input.discovery == "running":
            raise HTTPException(409, "Wait for Discover Song Order before changing settings.")
        saved_order = runtime.settings_service.snapshot().playback_api
        if (
            settings.playback_api.song_order != saved_order.song_order
            or settings.playback_api.captured_version != saved_order.captured_version
            or settings.playback_api.captured_at != saved_order.captured_at
            or settings.playback_api.setlist_id != saved_order.setlist_id
        ):
            raise HTTPException(400, "Song order can only be changed by Discover Song Order.")
    try:
        if runtime.playback_input is not None:
            await runtime.playback_input.save_settings(settings)
        else:
            runtime.settings_service.save(settings)
    except SettingsFileError as exc:
        raise HTTPException(status_code=503, detail=str(exc)) from exc
    except DiscoveryConflict as exc:
        raise HTTPException(409, str(exc)) from exc
    except ValueError as exc:
        raise HTTPException(400, str(exc)) from exc
    updated = runtime.settings_service.effective_runtime_settings()
    service_warning = await _apply_service_source(runtime)
    presentation_warning = await _apply_presentation(runtime)
    if presentation_warning is not None:
        service_warning = (
            f"{service_warning} {presentation_warning}" if service_warning else presentation_warning
        )
    restart_required = True
    controller = _midi_controller(runtime)
    # The Services source and the Presentation output are applied live above, so they never
    # need a restart.
    previous = previous.model_copy(
        update={
            "integration_modes": previous.integration_modes.model_copy(
                update={
                    "service_source": updated.integration_modes.service_source,
                    "timer_output": updated.integration_modes.timer_output,
                }
            ),
            "propresenter": updated.propresenter,
        }
    )
    only_midi_runtime_settings_changed = _runtime_settings_without_midi(
        previous
    ) == _runtime_settings_without_midi(updated)
    if (
        runtime.playback_input is None
        and controller is not None
        and updated.integration_modes.midi_source is MidiSource.REAL
        and updated.midi.enabled
        and only_midi_runtime_settings_changed
    ):
        outcome = await controller.reconfigure(updated.midi)
        restart_required = not outcome.accepted
    if (
        runtime.playback_input is not None
        and previous.model_dump(exclude={"midi", "playback_api", "integration_modes"})
        == updated.model_dump(exclude={"midi", "playback_api", "integration_modes"})
        and previous.integration_modes.service_source == updated.integration_modes.service_source
    ):
        restart_required = False
    response = _settings_response(runtime, restart_required=restart_required)
    if service_warning is not None:
        response.warning = (
            f"{response.warning} {service_warning}".strip() if response.warning else service_warning
        )
    return response


@router.get("/planning-center/status", response_model=PlanningCenterStatusResponse)
async def planning_center_status(request: Request) -> PlanningCenterStatusResponse:
    runtime = _runtime(request)
    state = await runtime.state_store.snapshot()
    settings = runtime.settings_service.effective_snapshot().planning_center
    runtime_settings = runtime.settings_service.effective_runtime_settings().planning_center
    oauth = runtime.planning_center_oauth
    oauth_status = (
        oauth.status(connection_method=settings.connection_method) if oauth is not None else None
    )
    return PlanningCenterStatusResponse(
        connection_status=state.planning_center_status,
        configured=(
            runtime_settings.is_configured and runtime_settings.service_type_id is not None
        ),
        app_id=settings.app_id,
        service_type_id=settings.service_type_id,
        planning_center_secret_saved=runtime.settings_service.credential_saved,
        connection_method=settings.connection_method,
        oauth_connected=oauth_status.connected if oauth_status else False,
        oauth_needs_reconnect=oauth_status.needs_reconnect if oauth_status else False,
        detail=state.service_load.message,
    )


def _oauth(request: Request) -> PlanningCenterOAuthService:
    service = _runtime(request).planning_center_oauth
    if service is None or not service.configured:
        raise HTTPException(
            status_code=503,
            detail="This build of StagePilot cannot sign in to Planning Center.",
        )
    return service


def _raise_oauth_http_error(exc: PlanningCenterOAuthError) -> NoReturn:
    # 401 means "this connection is genuinely dead, sign in again"; 503
    # means "transient, try again" -- deliberately distinct so the UI never
    # shows the alarming reconnect state for a passing network blip (the
    # same discipline as the Remote Access credential fixes).
    raise HTTPException(status_code=401 if exc.permanent else 503, detail=str(exc))


@router.post("/planning-center/oauth/start", response_model=PlanningCenterOAuthStartResponse)
async def start_planning_center_oauth(request: Request) -> PlanningCenterOAuthStartResponse:
    service = _oauth(request)
    try:
        authorize_url, state = await service.start()
    except PlanningCenterOAuthError as exc:
        _raise_oauth_http_error(exc)
    return PlanningCenterOAuthStartResponse(authorize_url=authorize_url, state=state)


@router.post("/planning-center/oauth/callback", response_model=PlanningCenterOAuthStatusResponse)
async def complete_planning_center_oauth(
    payload: PlanningCenterOAuthCallbackRequest,
    request: Request,
) -> PlanningCenterOAuthStatusResponse:
    runtime = _runtime(request)
    service = _oauth(request)
    if runtime.planning_center is not None:
        await _ensure_show_idle(runtime, CREDENTIAL_BUSY_MESSAGE)
    try:
        await service.complete(
            state=payload.state,
            code=payload.code.get_secret_value(),
            redirect_uri=payload.redirect_uri,
        )
    except PlanningCenterOAuthError as exc:
        _raise_oauth_http_error(exc)
    except CredentialStoreError as exc:
        raise HTTPException(status_code=503, detail=str(exc)) from exc
    try:
        await asyncio.to_thread(runtime.settings_service.update_connection_method, "oauth")
    except (CredentialStoreError, SettingsFileError) as exc:
        raise HTTPException(status_code=503, detail=str(exc)) from exc

    # A brand-new OAuth connect has no service type saved yet. Without this,
    # the dashboard is left showing a stale "connection: error" and no plan
    # loads until the operator manually walks through Load service types ->
    # choose one -> Save settings -> Load plan -- even though the OAuth sign-in
    # itself already succeeded. Default an unset service type to "All service
    # types" (the same option available in the manual flow) and, when the
    # Planning Center service source is already active, immediately apply it
    # to the running plugin so the connection status and plan load without
    # any further clicks. This mirrors what `update_planning_center_settings`
    # already does for a manual save; it is just now also triggered here.
    current_planning_center = runtime.settings_service.snapshot().planning_center
    if current_planning_center.service_type_id is None:
        updated_planning_center = current_planning_center.model_copy(
            update={"service_type_id": ALL_SERVICE_TYPES_ID}
        )
        try:
            await asyncio.to_thread(
                runtime.settings_service.update_planning_center,
                updated_planning_center,
            )
        except (CredentialStoreError, SettingsFileError) as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
    await _apply_planning_center_credentials(runtime)

    return PlanningCenterOAuthStatusResponse.model_validate(
        service.status(connection_method="oauth").model_dump()
    )


@router.get("/planning-center/oauth/status", response_model=PlanningCenterOAuthStatusResponse)
async def planning_center_oauth_status(request: Request) -> PlanningCenterOAuthStatusResponse:
    runtime = _runtime(request)
    method = runtime.settings_service.effective_snapshot().planning_center.connection_method
    service = runtime.planning_center_oauth
    if service is None:
        return PlanningCenterOAuthStatusResponse(
            connection_method=method, connected=False, needs_reconnect=False
        )
    return PlanningCenterOAuthStatusResponse.model_validate(
        service.status(connection_method=method).model_dump()
    )


@router.post("/planning-center/oauth/disconnect", response_model=PlanningCenterOAuthStatusResponse)
async def disconnect_planning_center_oauth(
    request: Request,
) -> PlanningCenterOAuthStatusResponse:
    runtime = _runtime(request)
    service = _oauth(request)
    if runtime.planning_center is not None:
        await _ensure_show_idle(runtime, CREDENTIAL_BUSY_MESSAGE)
    try:
        await asyncio.to_thread(service.disconnect)
        # Fall back to the manual path; any existing PAT secret is left
        # untouched, so a user who previously used it is simply restored.
        await asyncio.to_thread(runtime.settings_service.update_connection_method, "manual")
    except (CredentialStoreError, SettingsFileError) as exc:
        raise HTTPException(status_code=503, detail=str(exc)) from exc
    await _apply_planning_center_credentials(runtime)
    return PlanningCenterOAuthStatusResponse.model_validate(
        service.status(connection_method="manual").model_dump()
    )


@router.post("/planning-center/settings", response_model=SettingsResponse)
async def update_planning_center_settings(
    settings: PlanningCenterSettingsUpdateRequest,
    request: Request,
) -> SettingsResponse:
    runtime = _runtime(request)
    previous_planning_center = runtime.settings_service.snapshot().planning_center
    public_settings = PersistentPlanningCenterSettings(
        app_id=settings.app_id,
        # The manual-settings form never changes which auth path is active:
        # only the dedicated OAuth connect/disconnect routes do, so an
        # OAuth-connected installation editing its service type here is not
        # silently knocked back to the PAT path.
        connection_method=previous_planning_center.connection_method,
        service_type_id=settings.service_type_id,
        plan_title_preference=settings.plan_title_preference,
        preferred_service_time=settings.preferred_service_time,
        upcoming_lookahead_days=settings.upcoming_lookahead_days,
        request_timeout_seconds=settings.request_timeout_seconds,
    )
    # Only fields that actually invalidate the existing Planning Center HTTP
    # client (the app id, or the PAT secret itself) require restarting the
    # packaged backend. Everything else (service type, plan title
    # preference, lookahead window, timeout) can be applied in place, which
    # avoids an unnecessary full sidecar restart -- and the port-rebind
    # window that comes with it -- on every settings save.
    app_id_changed = public_settings.app_id != previous_planning_center.app_id
    secret_changed = settings.secret is not None or settings.remove_secret
    if (app_id_changed or secret_changed) and runtime.planning_center is not None:
        # Rebuilding the Planning Center client replaces the plan source under a live show.
        await _ensure_show_idle(runtime, CREDENTIAL_BUSY_MESSAGE)
    restart_required = False
    try:
        await asyncio.to_thread(
            runtime.settings_service.update_planning_center,
            public_settings,
            secret=settings.secret,
            remove_secret=settings.remove_secret,
        )
    except (CredentialStoreError, SettingsFileError) as exc:
        raise HTTPException(status_code=503, detail=str(exc)) from exc

    reconfigure_error: str | None = None
    plugin = runtime.planning_center
    if plugin is not None:
        # reconfigure() rebuilds the HTTP client in place, so a changed app id, secret or removed
        # secret takes effect immediately.
        updated_settings = runtime.settings_service.effective_runtime_settings().planning_center
        outcome = await plugin.reconfigure(updated_settings)
        if not outcome.accepted:
            reconfigure_error = outcome.message
    elif runtime.service_source is not None:
        # Planning Center is the saved source but no plugin is running (a previous start failed).
        reconfigure_error = await _apply_service_source(runtime)

    response = _settings_response(runtime, persisted=True, restart_required=restart_required)
    if reconfigure_error is not None:
        response.warning = (
            f"{response.warning} {reconfigure_error}".strip()
            if response.warning
            else reconfigure_error
        )
    return response


@router.post("/planning-center/test", response_model=PlanningCenterTestResponse)
async def test_planning_center(
    settings: PlanningCenterTestRequest,
    request: Request,
) -> PlanningCenterTestResponse:
    runtime = _runtime(request)
    try:
        service_types = await runtime.planning_center_setup.test_connection(
            app_id=settings.app_id,
            secret=settings.secret,
        )
    except PlanningCenterError as exc:
        _raise_planning_center_http_error(exc)
    return PlanningCenterTestResponse(
        message="Planning Center authentication succeeded.",
        service_types=[
            PlanningCenterServiceTypeResponse(id=value.id, name=value.name)
            for value in service_types
        ],
    )


@router.get(
    "/planning-center/service-types",
    response_model=list[PlanningCenterServiceTypeResponse],
)
async def planning_center_service_types(
    request: Request,
) -> list[PlanningCenterServiceTypeResponse]:
    runtime = _runtime(request)
    try:
        service_types = await runtime.planning_center_setup.list_service_types()
    except PlanningCenterError as exc:
        _raise_planning_center_http_error(exc)
    return [
        PlanningCenterServiceTypeResponse(id=value.id, name=value.name) for value in service_types
    ]


@router.get("/midi/inputs", response_model=MidiInputsResponse)
async def midi_inputs(request: Request) -> MidiInputsResponse:
    runtime = _runtime(request)
    controller = _midi_controller(runtime)
    if controller is None:
        return MidiInputsResponse(
            enabled=False,
            channel=runtime.settings.midi.channel,
            note=runtime.settings.midi.note,
            configured_input_name=runtime.settings.midi.input_name,
            selected_input_name=None,
            inputs=[],
            mappings=dict(runtime.settings.midi.mappings.configured()),
        )
    snapshot = await controller.input_snapshot(refresh=True)
    return _midi_inputs_response(snapshot)


@router.post("/midi/inputs/refresh", response_model=MidiInputsResponse)
async def refresh_midi_inputs(request: Request) -> MidiInputsResponse:
    return await midi_inputs(request)


@router.get("/midi/messages", response_model=MidiMonitorResponse)
async def midi_messages(request: Request) -> MidiMonitorResponse:
    controller = _midi_controller(_runtime(request))
    if controller is None:
        return MidiMonitorResponse(messages=[])
    messages = await controller.recent_messages()
    return MidiMonitorResponse(
        messages=[
            MidiMonitorMessageResponse(
                timestamp=message.timestamp,
                input_name=message.input_name,
                message_type=cast(Literal["note_on", "note_off"], message.message_type),
                channel=message.channel,
                note=message.note,
                note_name=message.note_name,
                velocity=message.velocity,
                disposition=message.disposition,
                detail=message.detail,
                action=message.action,
                simulated=message.simulated,
            )
            for message in messages
        ]
    )


@router.post(
    "/midi/input-selection",
    response_model=MidiInputSelectionResponse,
)
async def select_midi_input(
    selection: MidiInputSelectionRequest,
    request: Request,
) -> MidiInputSelectionResponse:
    runtime = _runtime(request)
    controller = _midi_controller(runtime)
    if controller is None:
        raise HTTPException(status_code=409, detail="The MIDI Playback plugin is disabled.")
    outcome = await controller.select_input(selection.input_id)
    if not outcome.accepted:
        raise HTTPException(status_code=409, detail=outcome.message)
    snapshot = await controller.input_snapshot()
    try:
        runtime.settings_service.persist_midi_input(snapshot.selected_input_name)
    except SettingsFileError as exc:
        raise HTTPException(status_code=503, detail=str(exc)) from exc
    return MidiInputSelectionResponse(
        accepted=True,
        message=outcome.message,
        midi=_midi_inputs_response(snapshot),
    )


@router.post(
    "/midi/cue-simulation",
    response_model=MidiCueSimulationResponse,
)
async def simulate_midi_cue(
    simulation: MidiCueSimulationRequest,
    request: Request,
) -> MidiCueSimulationResponse:
    runtime = _runtime(request)
    if runtime.playback_input is not None and runtime.playback_input.selected:
        raise HTTPException(status_code=409, detail="Select MIDI to simulate a cue.")
    controller = _midi_controller(runtime)
    if controller is None:
        raise HTTPException(status_code=409, detail="The MIDI Playback plugin is disabled.")
    outcome = await controller.simulate_cue(simulation.cue)
    return MidiCueSimulationResponse(
        cue=simulation.cue,
        action=simulation.cue.action,
        accepted=outcome.accepted,
        message=outcome.message,
        state=await runtime.state_store.snapshot(),
    )


@router.get("/propresenter", response_model=ProPresenterStatusResponse)
async def propresenter_status(request: Request) -> ProPresenterStatusResponse:
    return await _propresenter_status(_runtime(request))


@router.post("/propresenter/test", response_model=ProPresenterOperationResponse)
async def test_propresenter(request: Request) -> ProPresenterOperationResponse:
    runtime = _runtime(request)
    controller = runtime.propresenter_controller
    if controller is None:
        status = await _propresenter_status(runtime)
        return ProPresenterOperationResponse(
            accepted=False,
            message="The ProPresenter plugin is disabled.",
            propresenter=status,
        )
    snapshot = await controller.test_connection()
    accepted = snapshot.connection_status is ConnectionStatus.CONNECTED and snapshot.timer_found
    return ProPresenterOperationResponse(
        accepted=accepted,
        message=snapshot.detail
        or (
            "ProPresenter connection test succeeded."
            if accepted
            else "ProPresenter connection test failed."
        ),
        propresenter=_propresenter_response(snapshot),
    )


@router.post(
    "/propresenter/timers/refresh",
    response_model=ProPresenterOperationResponse,
)
async def refresh_propresenter_timers(request: Request) -> ProPresenterOperationResponse:
    runtime = _runtime(request)
    controller = runtime.propresenter_controller
    if controller is None:
        status = await _propresenter_status(runtime)
        return ProPresenterOperationResponse(
            accepted=False,
            message="The ProPresenter plugin is disabled.",
            propresenter=status,
        )
    snapshot = await controller.refresh_timers()
    accepted = snapshot.connection_status is ConnectionStatus.CONNECTED and snapshot.timer_found
    return ProPresenterOperationResponse(
        accepted=accepted,
        message=snapshot.detail or "ProPresenter timers refreshed.",
        propresenter=_propresenter_response(snapshot),
    )


@router.post(
    "/propresenter/settings",
    response_model=ProPresenterOperationResponse,
)
async def update_propresenter_settings(
    settings: ProPresenterSettingsRequest,
    request: Request,
) -> ProPresenterOperationResponse:
    runtime = _runtime(request)
    controller = runtime.propresenter_controller
    current = runtime.settings_service.effective_runtime_settings().propresenter
    updated = ProPresenterSettings(
        enabled=current.enabled,
        host=settings.host,
        port=settings.port,
        timer_name=settings.timer_name,
        look_id=settings.look_id,
        request_timeout_seconds=settings.request_timeout_seconds,
        reconnect_initial_seconds=current.reconnect_initial_seconds,
        reconnect_max_seconds=current.reconnect_max_seconds,
        health_check_interval_seconds=current.health_check_interval_seconds,
    )
    if controller is None:
        try:
            runtime.settings_service.persist_propresenter(updated)
        except SettingsFileError as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        # Saved settings that enable the output start it right now; otherwise they are just kept.
        warning = await _apply_presentation(runtime)
        controller = runtime.propresenter_controller
        if controller is None:
            status = await _propresenter_status(runtime)
            return ProPresenterOperationResponse(
                accepted=warning is None,
                message=warning or "ProPresenter settings saved.",
                propresenter=status,
            )
    snapshot = await controller.reconfigure(updated)
    runtime.settings.propresenter = updated
    if runtime.presentation is not None:
        runtime.presentation.adopt(controller, updated)
    try:
        runtime.settings_service.persist_propresenter(updated)
    except SettingsFileError as exc:
        raise HTTPException(status_code=503, detail=str(exc)) from exc
    if updated.look_id is not None:
        try:
            snapshot = await controller.apply_look(updated.look_id)
        except ProPresenterError as exc:
            snapshot = await controller.snapshot()
            return ProPresenterOperationResponse(
                accepted=False,
                message=f"ProPresenter settings were saved, but the Look was not applied: {exc}",
                propresenter=_propresenter_response(snapshot),
            )
    accepted = (
        snapshot.connection_status is ConnectionStatus.CONNECTED
        and snapshot.timer_found
        and snapshot.look_found
        and (updated.look_id is None or snapshot.current_look_id == updated.look_id)
    )
    message = snapshot.detail or (
        "ProPresenter session settings applied."
        if accepted
        else "ProPresenter session settings were saved, but readiness checks failed."
    )
    return ProPresenterOperationResponse(
        accepted=accepted,
        message=message,
        propresenter=_propresenter_response(snapshot),
    )


@router.get("/lights", response_model=LightsStatusResponse)
async def lights_status(request: Request) -> LightsStatusResponse:
    controller = _runtime(request).lights_controller
    if controller is None:
        raise HTTPException(status_code=409, detail="The Lighting plugin is unavailable.")
    return _lights_response(await controller.snapshot())


@router.post("/lights/outputs/refresh", response_model=LightsStatusResponse)
async def refresh_lighting_outputs(request: Request) -> LightsStatusResponse:
    controller = _runtime(request).lights_controller
    if controller is None:
        raise HTTPException(status_code=409, detail="The Lighting plugin is unavailable.")
    return _lights_response(await controller.snapshot(refresh=True))


@router.post("/lights/settings", response_model=LightsOperationResponse)
async def update_lights_settings(
    settings: LightsSettingsRequest,
    request: Request,
) -> LightsOperationResponse:
    runtime = _runtime(request)
    controller = runtime.lights_controller
    if controller is None:
        raise HTTPException(status_code=409, detail="The Lighting plugin is unavailable.")
    current = runtime.settings_service.effective_runtime_settings().lights
    try:
        # MIDI and OSC are independent: saving one method never touches the other's
        # settings. A MIDI save cannot overwrite the saved OSC host and port, and an
        # OSC save cannot reset the saved MIDI output, channel or note length.
        use_osc = settings.method is LightingMethod.OSC
        updated = LightsSettings(
            enabled=settings.enabled,
            method=settings.method,
            output_name=current.output_name if use_osc else settings.output_name,
            channel=current.channel if use_osc else settings.channel,
            pulse_ms=current.pulse_ms if use_osc else settings.pulse_ms,
            osc_host=(settings.osc_host or current.osc_host) if use_osc else current.osc_host,
            osc_port=(settings.osc_port or current.osc_port) if use_osc else current.osc_port,
            cue_maps=current.cue_maps,
            pads=current.pads,
        )
    except ValidationError as exc:
        raise HTTPException(status_code=422, detail=exc.errors(include_url=False)) from exc
    try:
        runtime.settings_service.persist_lights(updated)
    except SettingsFileError as exc:
        raise HTTPException(status_code=503, detail=str(exc)) from exc
    runtime.settings.lights = updated
    outcome = await controller.reconfigure(updated)
    return LightsOperationResponse(
        accepted=outcome.accepted,
        message=outcome.message,
        lights=_lights_response(await controller.snapshot()),
    )


@router.put("/lights/cue-map", response_model=LightsOperationResponse)
async def update_lighting_cue_map(
    cue_map: LightingCueMapRequest,
    request: Request,
) -> LightsOperationResponse:
    runtime = _runtime(request)
    controller = runtime.lights_controller
    if controller is None:
        raise HTTPException(status_code=409, detail="The Lighting plugin is unavailable.")
    current = runtime.settings_service.effective_runtime_settings().lights
    cue_maps = dict(current.cue_maps)
    existing = cue_maps.get(cue_map.song_key)
    # Replace only the submitted method's cues; the other method's cues are kept.
    is_midi = cue_map.method is LightingMethod.MIDI
    merged = SongLightingCueMap(
        song_key=cue_map.song_key,
        song_title=cue_map.song_title,
        cues=cue_map.cues
        if cue_map.method is LightingMethod.MIDI
        else (existing.cues if existing else []),
        osc_cues=cue_map.osc_cues
        if cue_map.method is LightingMethod.OSC
        else (existing.osc_cues if existing else []),
    )
    if merged.cues or merged.osc_cues:
        cue_maps[cue_map.song_key] = merged
    else:
        cue_maps.pop(cue_map.song_key, None)
    updated = current.model_copy(update={"cue_maps": cue_maps}, deep=True)
    try:
        runtime.settings_service.persist_lights(updated)
    except SettingsFileError as exc:
        raise HTTPException(status_code=503, detail=str(exc)) from exc
    runtime.settings.lights = updated
    await controller.replace_cue_map(merged)
    count = len(cue_map.cues if is_midi else cue_map.osc_cues)
    message = (
        f'Saved {count} lighting cues for "{cue_map.song_title}".'
        if count
        else f'Removed the lighting cue map for "{cue_map.song_title}".'
    )
    return LightsOperationResponse(
        accepted=True,
        message=message,
        lights=_lights_response(await controller.snapshot()),
    )


def _cue_map_lock(request: Request) -> asyncio.Lock:
    """One lock for every cue-map read-modify-write, so a capture, an undo and a cue-map save
    never interleave and drop each other's cues."""

    lock = getattr(request.app.state, "lights_cue_lock", None)
    if lock is None:
        lock = asyncio.Lock()
        request.app.state.lights_cue_lock = lock
    return cast(asyncio.Lock, lock)


@router.put("/lights/pads", response_model=LightsOperationResponse)
async def update_lighting_pads(
    body: LightingPadsRequest,
    request: Request,
) -> LightsOperationResponse:
    runtime = _runtime(request)
    controller = runtime.lights_controller
    if controller is None:
        raise HTTPException(status_code=409, detail="The Lighting plugin is unavailable.")
    async with _cue_map_lock(request):
        current = runtime.settings_service.effective_runtime_settings().lights
        try:
            updated = LightsSettings.model_validate(
                {
                    **current.model_dump(mode="json"),
                    "pads": [p.model_dump(mode="json") for p in body.pads],
                }
            )
        except ValidationError as exc:
            raise HTTPException(status_code=422, detail=exc.errors(include_url=False)) from exc
        try:
            runtime.settings_service.persist_lights(updated)
        except SettingsFileError as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        runtime.settings.lights = updated
    return LightsOperationResponse(
        accepted=True,
        message=f"Saved {len(body.pads)} lighting pads.",
        lights=_lights_response(await controller.snapshot()),
    )


@router.post("/lights/capture", response_model=LightingCaptureResponse)
async def capture_lighting_pad(
    body: LightingCaptureRequest,
    request: Request,
) -> LightingCaptureResponse:
    """Fire a pad through the saved method and, if asked, add it to the song's timeline."""

    runtime = _runtime(request)
    controller = runtime.lights_controller
    if controller is None:
        raise HTTPException(status_code=409, detail="The Lighting plugin is unavailable.")
    async with _cue_map_lock(request):
        current = runtime.settings_service.effective_runtime_settings().lights
        pad = next((item for item in current.pads if item.id == body.pad_id), None)
        if pad is None:
            raise HTTPException(status_code=404, detail="That pad no longer exists.")
        if pad.method is not current.method:
            raise HTTPException(
                status_code=409,
                detail=f"This pad is for {pad.method.value.upper()}; "
                f"Lighting is set to {current.method.value.upper()}.",
            )
        if pad.method is LightingMethod.OSC:
            assert pad.address is not None
            fired = await controller.test_osc_cue(pad.address, pad.args)
        else:
            assert pad.note is not None
            fired = await controller.test_cue(pad.note, pad.velocity)
        cue_id: UUID | None = None
        at_seconds: float | None = None
        message = fired.message
        if (
            body.song_key is not None
            and body.song_title is not None
            and body.at_seconds is not None
        ):
            # Read the cue map again after the send: a timer restart or a cue-map save may have
            # landed while the cue was going out, and there is no await from here to the write.
            current = runtime.settings_service.effective_runtime_settings().lights
            cue_maps = dict(current.cue_maps)
            existing = cue_maps.get(body.song_key)
            midi_cues = list(existing.cues) if existing else []
            osc_cues = list(existing.osc_cues) if existing else []
            if pad.method is LightingMethod.OSC:
                assert pad.address is not None
                osc_cue = OscLightingCue(
                    at_seconds=body.at_seconds,
                    address=pad.address,
                    args=pad.args,
                    label=pad.label,
                    color=pad.color,
                )
                osc_cues.append(osc_cue)
                cue_id, at_seconds = osc_cue.id, osc_cue.at_seconds
            else:
                assert pad.note is not None
                midi_cue = LightingCue(
                    at_seconds=body.at_seconds,
                    note=pad.note,
                    velocity=pad.velocity,
                    label=pad.label,
                    color=pad.color,
                )
                midi_cues.append(midi_cue)
                cue_id, at_seconds = midi_cue.id, midi_cue.at_seconds
            merged = SongLightingCueMap(
                song_key=body.song_key,
                song_title=body.song_title,
                cues=midi_cues,
                osc_cues=osc_cues,
            )
            cue_maps[body.song_key] = merged
            updated = current.model_copy(update={"cue_maps": cue_maps}, deep=True)
            try:
                runtime.settings_service.persist_lights(updated)
            except SettingsFileError as exc:
                raise HTTPException(status_code=503, detail=str(exc)) from exc
            runtime.settings.lights = updated
            await controller.replace_cue_map(merged)
    return LightingCaptureResponse(
        accepted=fired.accepted,
        message=message,
        lights=_lights_response(await controller.snapshot()),
        cue_id=cue_id,
        at_seconds=at_seconds,
    )


@router.post("/lights/capture/undo", response_model=LightsOperationResponse)
async def undo_lighting_capture(
    body: LightingCaptureUndoRequest,
    request: Request,
) -> LightsOperationResponse:
    runtime = _runtime(request)
    controller = runtime.lights_controller
    if controller is None:
        raise HTTPException(status_code=409, detail="The Lighting plugin is unavailable.")
    async with _cue_map_lock(request):
        current = runtime.settings_service.effective_runtime_settings().lights
        existing = current.cue_maps.get(body.song_key)
        if existing is None:
            return LightsOperationResponse(
                accepted=False,
                message="Nothing to undo.",
                lights=_lights_response(await controller.snapshot()),
            )
        cues = [cue for cue in existing.cues if cue.id != body.cue_id]
        osc_cues = [cue for cue in existing.osc_cues if cue.id != body.cue_id]
        if len(cues) == len(existing.cues) and len(osc_cues) == len(existing.osc_cues):
            return LightsOperationResponse(
                accepted=False,
                message="That capture was already changed or removed.",
                lights=_lights_response(await controller.snapshot()),
            )
        merged = SongLightingCueMap(
            song_key=existing.song_key,
            song_title=existing.song_title,
            cues=cues,
            osc_cues=osc_cues,
        )
        cue_maps = dict(current.cue_maps)
        if merged.cues or merged.osc_cues:
            cue_maps[body.song_key] = merged
        else:
            cue_maps.pop(body.song_key, None)
        updated = current.model_copy(update={"cue_maps": cue_maps}, deep=True)
        try:
            runtime.settings_service.persist_lights(updated)
        except SettingsFileError as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        runtime.settings.lights = updated
        await controller.replace_cue_map(merged)
    return LightsOperationResponse(
        accepted=True,
        message="Capture undone.",
        lights=_lights_response(await controller.snapshot()),
    )


@router.post("/lights/test", response_model=LightsOperationResponse)
async def test_lighting_cue(
    cue: LightingCueTestRequest,
    request: Request,
) -> LightsOperationResponse:
    controller = _runtime(request).lights_controller
    if controller is None:
        raise HTTPException(status_code=409, detail="The Lighting plugin is unavailable.")
    outcome = await controller.test_cue(cue.note, cue.velocity)
    return LightsOperationResponse(
        accepted=outcome.accepted,
        message=outcome.message,
        lights=_lights_response(await controller.snapshot()),
    )


@router.post("/lights/test-osc", response_model=LightsOperationResponse)
async def test_lighting_osc_cue(
    cue: LightingOscCueTestRequest,
    request: Request,
) -> LightsOperationResponse:
    controller = _runtime(request).lights_controller
    if controller is None:
        raise HTTPException(status_code=409, detail="The Lighting plugin is unavailable.")
    outcome = await controller.test_osc_cue(cue.address, cue.args)
    return LightsOperationResponse(
        accepted=outcome.accepted,
        message=outcome.message,
        lights=_lights_response(await controller.snapshot()),
    )


@router.post("/actions/{action}", response_model=ActionResponse)
async def perform_action(action: ActionName, request: Request) -> ActionResponse:
    runtime = _runtime(request)
    outcome = await runtime.state_service.dispatch(action, source="api")
    return ActionResponse(
        action=action,
        accepted=outcome.accepted,
        message=outcome.message,
        state=await runtime.state_store.snapshot(),
    )


@router.post("/planning-center/plan/reload", response_model=ActionResponse)
async def reload_planning_center_plan(request: Request) -> ActionResponse:
    return await perform_action(ActionName.RELOAD_PLAN, request)


@router.get(
    "/planning-center/plans/pending-selection",
    response_model=PendingPlanSelectionResponse,
)
async def pending_planning_center_plan_selection(
    request: Request,
) -> PendingPlanSelectionResponse:
    state = await _runtime(request).state_store.snapshot()
    pending = state.service_load.status is ServiceLoadStatus.AMBIGUOUS
    target_date = state.service_load.target_date if pending else None
    return PendingPlanSelectionResponse(
        pending=pending,
        target_date=target_date.isoformat() if target_date is not None else None,
        candidates=state.service_load.candidates if pending else [],
        message=state.service_load.message if pending else None,
    )


@router.post(
    "/planning-center/plan-selection",
    response_model=PlanSelectionResponse,
)
@router.post(
    "/planning-center/plans/select",
    response_model=PlanSelectionResponse,
)
async def select_planning_center_plan(
    selection: PlanSelectionRequest,
    request: Request,
) -> PlanSelectionResponse:
    runtime = _runtime(request)
    current = await runtime.state_store.snapshot()
    candidate_ids = {candidate.id for candidate in current.service_load.candidates}
    if (
        current.service_load.status is not ServiceLoadStatus.AMBIGUOUS
        or selection.plan_id not in candidate_ids
    ):
        raise HTTPException(
            status_code=409,
            detail="The selected plan is not a current Planning Center candidate.",
        )

    report = await runtime.event_bus.publish(
        new_event(
            EventType.SERVICE_PLAN_SELECTION_REQUESTED,
            source="api",
            payload=ServicePlanSelectionPayload(plan_id=selection.plan_id),
        )
    )
    if report.failures:
        raise HTTPException(
            status_code=503,
            detail="Planning Center could not process the plan selection.",
        )

    updated = await runtime.state_store.snapshot()
    loaded_plan = updated.plan
    if (
        updated.service_load.status is not ServiceLoadStatus.LOADED
        or loaded_plan is None
        or loaded_plan.id != selection.plan_id
    ):
        raise HTTPException(
            status_code=503,
            detail=updated.service_load.message
            or "Planning Center could not load the selected plan.",
        )
    return PlanSelectionResponse(
        accepted=True,
        message=f'Loaded "{loaded_plan.title}".',
        state=updated,
    )
