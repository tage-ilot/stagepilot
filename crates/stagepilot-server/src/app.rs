//! The axum application: the `/api/v1/*` routes, the `/ws` state stream and static dashboard
//! serving. Response bodies and status codes follow the Python backend.

use crate::demo::today;
use crate::remote_types::RemoteContext;
use crate::runtime::Runtime;
use crate::validation::{self, http_error, ok, unprocessable, Json};
use axum::body::Bytes;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Extension;
use axum::Router;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use stagepilot_core::auth::verify_dashboard_pin;
use stagepilot_core::config::{MidiSource, ServiceSource};
use stagepilot_core::events::ActionName;
use stagepilot_core::settings::{PersistentSettings, UpdateError};
use stagepilot_core::state::{ApplicationState, ApplicationStatus, ConnectionStatus, PluginStatus};
use stagepilot_core::state::{ServiceLoadStatus, TimerStatus};
use stagepilot_playback::plugin::PlaybackError;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};

pub const COOKIE_NAME: &str = "stagepilot_dashboard_session";
const DESKTOP_ORIGINS: [&str; 3] = [
    "http://tauri.localhost",
    "https://tauri.localhost",
    "tauri://localhost",
];
const PUBLIC_PATHS: [&str; 6] = [
    "/api/v1/access",
    "/api/v1/dashboard-auth/status",
    "/api/v1/dashboard-auth/login",
    "/api/v1/health",
    "/api/v1/health/live",
    "/api/v1/health/ready",
];
const SERVICE_BUSY: &str = "Stop the timer before switching between Demo and Planning Center.";
const PRESENTATION_BUSY: &str =
    "Stop the timer before switching Presentation between simulated and ProPresenter.";
/// How long the state stream waits to gather a burst of changes into one snapshot.
const COALESCE: std::time::Duration = std::time::Duration::from_millis(40);
const MIDI_BUSY: &str = "Stop the timer before changing the MIDI input.";

type Rt = Arc<Runtime>;

pub struct AppOptions {
    pub web_root: Option<PathBuf>,
}

/// `STAGEPILOT_WEB_ROOT`, then `frontend/dist` next to the repository root.
pub fn default_web_root() -> Option<PathBuf> {
    let configured = std::env::var_os("STAGEPILOT_WEB_ROOT").map(PathBuf::from);
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.join("stagepilot_web")));
    [configured, beside, Some(PathBuf::from("frontend/dist"))]
        .into_iter()
        .flatten()
        .find(|p| p.join("index.html").is_file())
}

pub fn build_app(runtime: Rt, options: AppOptions) -> Router {
    let api = Router::new()
        .route("/api/v1/access", get(access))
        .route("/api/v1/health", get(health))
        .route("/api/v1/health/live", get(liveness))
        .route("/api/v1/state", get(state))
        .route("/api/v1/settings", get(get_settings).put(put_settings))
        .route("/api/v1/dashboard-auth/status", get(dashboard_status))
        .route("/api/v1/dashboard-auth/login", post(dashboard_login))
        .route("/api/v1/dashboard-auth/settings", post(dashboard_settings))
        .route("/api/v1/actions/{action}", post(perform_action))
        .route("/api/v1/planning-center/plan/reload", post(reload_plan))
        .merge(crate::pco_routes::router())
        // MIDI, ProPresenter and lights (phase 4).
        .merge(crate::routes_outputs::routes())
        // Remote access, remote auth and diagnostics (phase 6b-A).
        .merge(crate::remote_routes::routes())
        .route("/ws", get(ws_state))
        .merge(crate::playback_routes::routes())
        .with_state(runtime.clone());

    let router = match options.web_root.filter(|r| r.join("index.html").is_file()) {
        Some(root) => {
            let root = Arc::new(root);
            api.fallback(move |request: Request| {
                let root = root.clone();
                async move { crate::static_files::serve(root, request).await }
            })
        }
        None => api.fallback(not_found),
    };
    router
        .method_not_allowed_fallback(method_not_allowed)
        // Keyed retries of remote one-shot requests (`RemoteRetryRoute`); inside the PIN guard.
        .layer(middleware::from_fn(crate::remote_routes::remote_retry))
        .layer(middleware::from_fn_with_state(runtime, dashboard_guard))
        .layer(cors())
}

fn cors() -> CorsLayer {
    let origins = [
        "http://127.0.0.1:5173",
        "http://localhost:5173",
        "http://tauri.localhost",
        "https://tauri.localhost",
        "tauri://localhost",
    ]
    .map(HeaderValue::from_static);
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_credentials(true)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
        ])
        .allow_headers([
            header::CONTENT_TYPE,
            header::HeaderName::from_static("x-stagepilot-remote"),
            header::HeaderName::from_static("x-csrf-token"),
            header::HeaderName::from_static("idempotency-key"),
        ])
}

async fn not_found() -> Json {
    http_error(404, "Not Found")
}

async fn method_not_allowed() -> Json {
    http_error(405, "Method Not Allowed")
}

// ---- dashboard PIN guard ----------------------------------------------------------------------

fn loopback(peer: Option<SocketAddr>) -> bool {
    peer.is_some_and(|p| p.ip().is_loopback())
}

fn desktop_origin(headers: &HeaderMap, peer: Option<SocketAddr>) -> bool {
    headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|origin| DESKTOP_ORIGINS.contains(&origin))
        && loopback(peer)
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|line| line.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
}

fn peer_of(request: &Request) -> Option<SocketAddr> {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0)
}

async fn dashboard_guard(State(rt): State<Rt>, request: Request, next: Next) -> Response {
    if request.extensions().get::<RemoteContext>().is_some() {
        // Only the trusted remote wrapper sets this, after remote authorisation.
        return next.run(request).await;
    }
    let path = request.uri().path().to_string();
    let protected = (path.starts_with("/api/v1") || path == "/ws")
        && !PUBLIC_PATHS.contains(&path.as_str())
        && !desktop_origin(request.headers(), peer_of(&request));
    if !protected
        || !rt.dashboard_auth_enforced
        || !rt.settings_service.snapshot().web_dashboard_pin_enabled
        || rt
            .sessions
            .valid(cookie_value(request.headers(), COOKIE_NAME).as_deref())
    {
        return next.run(request).await;
    }
    if path == "/ws" {
        // Starlette refuses the handshake when the socket is closed before accept.
        return StatusCode::FORBIDDEN.into_response();
    }
    http_error(401, "Dashboard PIN required.").into_response()
}

// ---- health, state ----------------------------------------------------------------------------

async fn liveness() -> Json {
    ok(json!({"status": "alive"}))
}

fn production_service_ready(state: &ApplicationState, current: chrono::NaiveDate) -> bool {
    let Some(plan) = &state.plan else {
        return false;
    };
    state.planning_center_status == ConnectionStatus::Connected
        && state.service_load.status == ServiceLoadStatus::Loaded
        && !state.service_load.is_stale
        && state.service_load.target_date == Some(plan.date)
        && plan.date >= current
        && !plan.songs.is_empty()
}

fn local_date(timezone: &str) -> chrono::NaiveDate {
    timezone
        .parse::<chrono_tz::Tz>()
        .map(|tz| Utc::now().with_timezone(&tz).date_naive())
        .unwrap_or_else(|_| today())
}

async fn health(State(rt): State<Rt>) -> Json {
    let state = rt.store.snapshot();
    let plugins = rt.plugins.health().await;
    let service_ready = if rt.settings.uses_demo_service() {
        state.service_load.status != ServiceLoadStatus::Error
    } else {
        production_service_ready(
            &state,
            // The pco contract profile pins the clock (Python patches `routes._current_local_date`);
            // `PcoOptions::today` is only ever set by tests and that profile.
            rt.pco
                .today
                .as_ref()
                .map(|f| f(rt.settings.timezone.parse().unwrap_or(chrono_tz::UTC)))
                .unwrap_or_else(|| local_date(&rt.settings.timezone)),
        )
    };
    // `runtime.playback_input.connection.connected`: the simulated source is always ready, real
    // MIDI needs its link, and the Playback API needs a live Playback link unless Playback is
    // switched off (then nothing is waiting for it).
    let playback_ready = match rt.settings.integration_modes.midi_source {
        MidiSource::Simulated => true,
        MidiSource::Real => rt.outputs.link.connected(),
        MidiSource::PlaybackApi => {
            rt.playback.publish_connection().await;
            !rt.playback.config().enabled || rt.playback.connection().connected
        }
    };
    let healthy = state.application_status == ApplicationStatus::Running
        && plugins.iter().all(|p| p.status == PluginStatus::Running)
        && service_ready
        && playback_ready;
    ok(json!({
        "status": if healthy { "healthy" } else { "degraded" },
        "version": rt.settings.version,
        "application_status": state.application_status,
        "plugins": plugins,
        "started_at": rt.started_at,
    }))
}

async fn state(State(rt): State<Rt>) -> Json {
    ok(to_value(&rt.store.snapshot()))
}

fn to_value<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

// ---- settings ---------------------------------------------------------------------------------

pub(crate) fn settings_response(rt: &Runtime, persisted: bool, restart_required: bool) -> Value {
    let settings = if persisted {
        rt.settings_service.snapshot()
    } else {
        rt.settings_service.effective_snapshot()
    };
    let degraded = rt.settings_service.degraded();
    json!({
        "settings": settings,
        "planning_center_secret_saved": rt.settings_service.credential_saved(),
        "warning": rt.settings_service.warning(),
        // Set for the whole run when the saved file was damaged or newer than this build: the
        // settings in use are defaults and the original is kept as settings.json.corrupt-*.
        "settings_degraded": degraded.is_some(),
        "settings_degraded_reason": degraded.map(|d| d.reason),
        "restart_required": restart_required,
    })
}

async fn get_settings(State(rt): State<Rt>) -> Json {
    ok(settings_response(&rt, false, false))
}

/// The part of the effective settings whose change needs a restart (everything the Python
/// route compares after it removes `midi`, `playback_api` and `integration_modes`, and after the
/// Services/Presentation settings it applies live).
fn restart_relevant(settings: &PersistentSettings) -> Value {
    let mut value = to_value(settings);
    if let Value::Object(map) = &mut value {
        for key in [
            "midi",
            "playback_api",
            "integration_modes",
            "propresenter",
            "planning_center_length_undo",
            "onboarding",
            "web_dashboard_pin_enabled",
            "schema_version",
        ] {
            map.remove(key);
        }
    }
    value
}

async fn put_settings(State(rt): State<Rt>, body: Bytes) -> Json {
    let _serialised = rt.reconcile.lock().await;
    let parsed: Option<Value> = serde_json::from_slice(&body).ok();
    let Some(value @ Value::Object(_)) = parsed.clone() else {
        return unprocessable(validation::invalid_body(&body, parsed.as_ref()));
    };
    let requested = match PersistentSettings::from_value(value) {
        Ok((settings, _migration)) => settings,
        Err(errors) => {
            let detail = errors
                .iter()
                .map(|e| {
                    let mut e = to_value(e);
                    if let Some(loc) = e.get_mut("loc").and_then(Value::as_array_mut) {
                        loc.insert(0, json!("body"));
                    }
                    e
                })
                .collect();
            return unprocessable(detail);
        }
    };
    let _serialised = rt.pco.lock.lock().await;
    let previous = rt.settings_service.effective_runtime_settings();
    let previous_effective = rt.settings_service.effective_snapshot();
    let timer_running = rt.store.snapshot().timer.status == TimerStatus::Running;

    if requested.integration_modes.service_source != previous.integration_modes.service_source {
        if let Some(pin) = rt.settings_service.service_source_pin() {
            return pinned(&pin);
        }
        if timer_running {
            return http_error(409, SERVICE_BUSY);
        }
    }
    let wants_propresenter = |s: &PersistentSettings| {
        s.integration_modes.timer_output == stagepilot_core::config::TimerOutput::Propresenter
    };
    if wants_propresenter(&requested)
        != (previous.integration_modes.timer_output
            == stagepilot_core::config::TimerOutput::Propresenter)
    {
        if let Some(pin) = rt.settings_service.timer_output_pin() {
            return pinned(&pin);
        }
        if timer_running {
            return http_error(409, PRESENTATION_BUSY);
        }
    }
    let source_changed =
        requested.integration_modes.midi_source != previous.integration_modes.midi_source;
    let enabled_changed = requested.midi.enabled != previous_effective.midi.enabled;
    let input_changed = requested.midi.input_name != previous_effective.midi.input_name;
    if source_changed || enabled_changed {
        if let Some(pin) = rt.settings_service.midi_pin(enabled_changed) {
            return pinned(&pin);
        }
    }
    if (source_changed || enabled_changed || input_changed) && timer_running {
        return http_error(409, MIDI_BUSY);
    }
    if rt.playback.discovery_snapshot().state.as_str() == "running" {
        return http_error(
            409,
            "Wait for Discover Song Order before changing settings.",
        );
    }
    let saved = rt.settings_service.snapshot().playback_api;
    if requested.playback_api.song_order != saved.song_order
        || requested.playback_api.captured_version != saved.captured_version
        || requested.playback_api.captured_at != saved.captured_at
        || requested.playback_api.setlist_id != saved.setlist_id
    {
        return http_error(
            400,
            "Song order can only be changed by Discover Song Order.",
        );
    }
    // Python hands the new settings to the Playback plugin, which saves and reconfigures.
    if let Err(error) = rt.playback.save_settings(requested.clone()).await {
        return match error {
            PlaybackError::Conflict(m) => http_error(409, &m),
            PlaybackError::Invalid(m) => http_error(400, &m),
            PlaybackError::Save(m) => http_error(503, &m),
        };
    }
    let updated = rt.settings_service.effective_snapshot();
    // Playback is always present in Python, so a restart is only needed when something other
    // than MIDI, Playback, the Services source and the Presentation output changed.
    let restart_required = restart_relevant(&previous_effective) != restart_relevant(&updated);
    // The Services source is applied live (as Python does), so it never needs a restart.
    let service_warning = match crate::pco_routes::apply_service_source(&rt).await {
        Ok(warning) => warning,
        Err(message) => return http_error(409, message),
    };
    let mut response = settings_response(&rt, false, restart_required);
    if let Some(warning) = service_warning {
        let existing = response["warning"].as_str().unwrap_or_default().to_string();
        response["warning"] = json!(format!("{existing} {warning}").trim().to_string());
    }
    // Follow the saved settings live, like the Python route: the Presentation output (simulated
    // <-> ProPresenter or a changed ProPresenter block) and the MIDI input.
    let presentation_warning = match rt.apply_presentation().await {
        Ok(warning) => warning,
        Err(message) => return http_error(409, &message),
    };
    rt.outputs
        .apply_midi(&rt.settings_service.effective_runtime_settings())
        .await;
    if let (Some(warning), Value::Object(map)) = (presentation_warning, &mut response) {
        let existing = map
            .get("warning")
            .and_then(Value::as_str)
            .map(str::to_string);
        map.insert(
            "warning".into(),
            json!(match existing {
                Some(existing) => format!("{existing} {warning}").trim().to_string(),
                None => warning,
            }),
        );
    }
    ok(response)
}

fn pinned(pin: &str) -> Json {
    http_error(
        409,
        &format!("This is set by the {pin} environment variable and cannot be changed here."),
    )
}

// ---- dashboard auth ---------------------------------------------------------------------------

fn auth_status(rt: &Runtime, headers: &HeaderMap, peer: Option<SocketAddr>) -> (bool, bool) {
    let required = rt.settings_service.snapshot().web_dashboard_pin_enabled;
    let authenticated = !required
        || desktop_origin(headers, peer)
        || rt
            .sessions
            .valid(cookie_value(headers, COOKIE_NAME).as_deref());
    (required, authenticated)
}

async fn dashboard_status(
    State(rt): State<Rt>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Json {
    let (required, authenticated) = auth_status(&rt, &headers, Some(peer));
    ok(json!({"required": required, "authenticated": authenticated}))
}

async fn dashboard_login(State(rt): State<Rt>, body: Bytes) -> Response {
    let pin = match validation::dashboard_login(&body) {
        Ok(pin) => pin,
        Err(detail) => return unprocessable(detail).into_response(),
    };
    let settings = rt.settings_service.snapshot();
    if settings.web_dashboard_pin_enabled
        && !verify_dashboard_pin(&pin, settings.web_dashboard_pin_hash.as_deref())
    {
        return http_error(401, "Incorrect dashboard PIN.").into_response();
    }
    let token = rt.sessions.create();
    let mut response = ok(json!({
        "required": settings.web_dashboard_pin_enabled,
        "authenticated": true,
    }))
    .into_response();
    let cookie = format!("{COOKIE_NAME}={token}; HttpOnly; Max-Age=43200; Path=/; SameSite=Strict");
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

async fn dashboard_settings(State(rt): State<Rt>, body: Bytes) -> Json {
    let update = match validation::dashboard_access(&body) {
        Ok(update) => update,
        Err(detail) => return unprocessable(detail),
    };
    let pin_hash = update
        .pin
        .as_deref()
        .map(stagepilot_core::auth::hash_dashboard_pin);
    if let Err(error) = rt
        .settings_service
        .update_dashboard_access(update.enabled, pin_hash)
    {
        return http_error(503, &error.0);
    }
    rt.sessions.clear();
    ok(settings_response(&rt, false, false))
}

async fn access(
    State(rt): State<Rt>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    remote: Option<Extension<RemoteContext>>,
    headers: HeaderMap,
) -> Response {
    if let Some(Extension(context)) = remote {
        return remote_access(&context);
    }
    let (required, local_authenticated) = auth_status(&rt, &headers, Some(peer));
    let desktop = desktop_origin(&headers, Some(peer));
    let authenticated = local_authenticated || !rt.dashboard_auth_enforced;
    let mut response = ok(json!({
        "mode": if desktop { "desktop" } else { "lan" },
        "authentication": if required && !desktop { "pin" } else { "none" },
        "authenticated": authenticated,
        "capabilities": {
            "canRead": authenticated,
            "canOperate": authenticated,
            "canConfigure": authenticated,
            "canActivateServices": authenticated,
            "canManageRemote": authenticated,
            "canBootstrapRemote": authenticated,
        },
        "user": null,
        "expires_at": null,
        "csrf_token": null,
    }))
    .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// `/api/v1/access` through the trusted remote ingress.
fn remote_access(context: &RemoteContext) -> Response {
    use stagepilot_core::auth::{csrf_token, RemoteRole};
    let session = context.session.as_ref();
    let authenticated = session.is_some();
    let operator = session.is_some_and(|s| s.role == RemoteRole::Operator);
    let mut response = ok(json!({
        "mode": "remote",
        "authentication": "password",
        "authenticated": authenticated,
        "capabilities": {
            "canRead": authenticated,
            "canOperate": operator,
            "canConfigure": operator,
            "canActivateServices": false,
            "canManageRemote": operator,
            "canBootstrapRemote": false,
        },
        "user": session.map(|s| json!({"email": s.email, "role": s.role.as_str()})),
        "expires_at": session.map(|s| s.expires_at),
        "csrf_token": match (session, &context.token) {
            (Some(_), Some(token)) => Some(csrf_token(token)),
            _ => None,
        },
    }))
    .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

// ---- actions ----------------------------------------------------------------------------------

fn bad_action(input: &str) -> Json {
    let expected = "'start_next', 'restart_current', 'previous', 'next', 'stop_timer', 'reload_plan' or 'reset_position'";
    unprocessable(vec![json!({
        "type": "enum",
        "loc": ["path", "action"],
        "msg": format!("Input should be {expected}"),
        "input": input,
        "ctx": {"expected": expected},
    })])
}

async fn run_action(rt: &Runtime, action: ActionName) -> Json {
    let outcome = rt.state_service.dispatch(action, "api").await;
    ok(json!({
        "action": action,
        "accepted": outcome.accepted,
        "message": outcome.message,
        "state": rt.store.snapshot(),
    }))
}

async fn perform_action(State(rt): State<Rt>, Path(action): Path<String>) -> Json {
    match ActionName::parse(&action) {
        Some(action) => run_action(&rt, action).await,
        None => bad_action(&action),
    }
}

async fn reload_plan(State(rt): State<Rt>) -> Json {
    run_action(&rt, ActionName::ReloadPlan).await
}

// ---- WebSocket --------------------------------------------------------------------------------

async fn ws_state(
    State(rt): State<Rt>,
    remote: Option<Extension<RemoteContext>>,
    upgrade: WebSocketUpgrade,
) -> Response {
    match remote {
        Some(Extension(context)) => {
            upgrade.on_upgrade(move |socket| remote_state_stream(rt, context, socket))
        }
        None => upgrade.on_upgrade(move |socket| state_stream(rt, socket)),
    }
}

async fn close_with(socket: &mut WebSocket, code: u16, reason: &'static str) {
    let _ = socket
        .send(Message::Close(Some(CloseFrame {
            code,
            reason: reason.into(),
        })))
        .await;
}

/// `api/websocket.py` for a remote caller: the session is re-checked before every send (4401
/// when it expired or was revoked, 4503 when the store is down) and the stream is receive-only
/// (4403 when the client sends anything).
async fn remote_state_stream(rt: Rt, context: RemoteContext, mut socket: WebSocket) {
    async fn valid(context: &RemoteContext, socket: &mut WebSocket) -> bool {
        let token = context.token.clone();
        match context
            .access
            .call(move |store| store.session(token.as_deref()))
            .await
        {
            Err(_) => {
                close_with(socket, 4503, "Remote storage unavailable").await;
                false
            }
            Ok(None) => {
                close_with(socket, 4401, "Remote session expired or revoked").await;
                false
            }
            Ok(Some(_)) => true,
        }
    }
    let mut updates = rt.store.subscribe();
    if !valid(&context, &mut socket).await {
        return;
    }
    let initial = rt.store.snapshot();
    let mut last_revision = initial.revision;
    updates.borrow_and_update();
    if socket.send(envelope(&initial)).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            changed = tokio::time::timeout(std::time::Duration::from_secs(1), updates.changed()) => {
                if !valid(&context, &mut socket).await {
                    return;
                }
                match changed {
                    Err(_) => continue,
                    Ok(Err(_)) => break,
                    Ok(Ok(())) => {}
                }
                tokio::time::sleep(COALESCE).await;
                let snapshot = updates.borrow_and_update().clone();
                if snapshot.revision <= last_revision {
                    continue;
                }
                last_revision = snapshot.revision;
                if socket.send(envelope(&snapshot)).await.is_err() {
                    return;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return,
                    Some(Ok(_)) => {
                        close_with(&mut socket, 4403, "State stream is receive-only").await;
                        return;
                    }
                }
            }
        }
    }
}

fn envelope(state: &ApplicationState) -> Message {
    Message::text(json!({"type": "state.snapshot", "data": state}).to_string())
}

async fn state_stream(rt: Rt, mut socket: WebSocket) {
    let mut updates = rt.store.subscribe();
    let initial = rt.store.snapshot();
    let mut last_revision = initial.revision;
    updates.borrow_and_update();
    if socket.send(envelope(&initial)).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            changed = updates.changed() => {
                if changed.is_err() {
                    break;
                }
                // One action publishes several state changes in a burst. Python's one-slot queue
                // hands the sender only the latest, so wait a moment and send that, not the
                // first intermediate revision.
                tokio::time::sleep(COALESCE).await;
                let snapshot = updates.borrow_and_update().clone();
                if snapshot.revision <= last_revision {
                    continue;
                }
                last_revision = snapshot.revision;
                if socket.send(envelope(&snapshot)).await.is_err() {
                    break;
                }
            }
            incoming = socket.recv() => {
                // The local handler ignores anything the client sends; it only watches for a close.
                match incoming {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                    Some(Ok(_)) => {}
                }
            }
        }
    }
    let _ = socket
        .send(Message::Close(Some(CloseFrame {
            code: 1000,
            reason: "".into(),
        })))
        .await;
}

#[allow(dead_code)]
fn _unused(_: DateTime<Utc>, _: ServiceSource, _: UpdateError) {}
