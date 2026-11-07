//! Remote routes: the Rust counterparts of `api/remote_auth.py` (login, session, logout,
//! revoke-sessions, users CRUD), `api/remote_feature.py` (the product Remote Access controls) and
//! the retry journal of `api/remote_retry.py`.
//!
//! Status codes, `detail` strings and bodies follow Python. Remote auth responses carry
//! `Cache-Control: no-store`; a validation failure there is always the fixed text
//! "Invalid remote authentication request." (never an echo of the input). The Remote Access
//! controls sit behind the same route class in Python, so they get the same treatment.

// The handlers return `Result<_, Response>` and the early-return `Response` is the error.
#![allow(clippy::result_large_err)]

use crate::diagnostics;
use crate::remote_types::{RemoteAccess, RemoteContext, RemoteWiring, COOKIE_NAME};
use crate::runtime::Runtime;
use crate::validation::Json as JsonReply;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path, Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::Router;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use stagepilot_core::auth::{csrf_token, digest, RemoteAuthError, RemoteRole, SESSION_SECONDS};
use std::sync::Arc;
use std::time::SystemTime;
use uuid::Uuid;

type Rt = Arc<Runtime>;

/// The one-shot POST routes whose keyed retries are journalled (`remote_retry.ONE_SHOTS`).
const ONE_SHOTS: [&str; 6] = [
    "/api/v1/midi/cue-simulation",
    "/api/v1/lights/test",
    "/api/v1/planning-center/plan/reload",
    "/api/v1/planning-center/plan-selection",
    "/api/v1/planning-center/plans/select",
    "/api/v1/propresenter/test",
];
const MAX_BODY: usize = 64 * 1024 * 1024;
const INVALID_REQUEST: &str = "Invalid remote authentication request.";

pub fn routes() -> Router<Rt> {
    Router::new()
        .route("/api/v1/remote-auth/login", post(login))
        .route("/api/v1/remote-auth/session", get(session))
        .route("/api/v1/remote-auth/logout", post(logout))
        .route("/api/v1/remote-auth/revoke-sessions", post(logout))
        .route("/api/v1/remote-auth/users", get(users).post(create_user))
        .route(
            "/api/v1/remote-auth/users/{user_id}",
            patch(update_user).delete(delete_user),
        )
        .route("/api/v1/remote-access", get(status))
        .route("/api/v1/remote-access/bootstrap", post(bootstrap))
        .route("/api/v1/remote-access/enable", post(enable))
        .route("/api/v1/remote-access/reset", post(reset))
        .route("/api/v1/remote-access/disable", post(disable))
        .route("/api/v1/remote-access/regenerate", post(regenerate))
        // The bundle cap (2 MB) is checked by the handler, so the extractor's own 2 MB body
        // limit must not answer first.
        .route(
            "/api/v1/diagnostics/send",
            post(diagnostics::send).layer(DefaultBodyLimit::max(8 * 1024 * 1024)),
        )
}

// ---- response helpers --------------------------------------------------------------------------

fn status_of(code: u16) -> StatusCode {
    StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

/// A Remote route response: JSON (or empty for 204) plus `Cache-Control: no-store`.
fn reply(code: u16, body: Option<Value>) -> Response {
    let mut response = match body {
        Some(value) => JsonReply(status_of(code), value).into_response(),
        // FastAPI's JSONResponse still labels an empty 204 body as JSON.
        None => {
            let mut empty = status_of(code).into_response();
            empty.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            );
            empty
        }
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// `HTTPException` raised by the route body itself (no `no-store` header, like Python).
fn http_fail(code: u16, detail: &str) -> Response {
    JsonReply(status_of(code), json!({ "detail": detail })).into_response()
}

/// `RemoteAuthError` through `RemoteRoute`: its status and detail, `no-store`, and `Retry-After`
/// on a 429.
fn auth_fail(error: RemoteAuthError) -> Response {
    let mut response =
        JsonReply(status_of(error.status), json!({ "detail": error.detail })).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if error.status == 429 {
        headers.insert(header::RETRY_AFTER, HeaderValue::from_static("300"));
    }
    response
}

/// An unhandled exception in Python is Starlette's plain-text 500.
fn internal_error() -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response()
}

/// Remote routes answer a body validation error with a fixed 422 (no input echoed).
fn invalid() -> Response {
    // The 422 is raised from inside the route handler, so it carries no `no-store` header.
    http_fail(422, INVALID_REQUEST)
}

fn context(request: &Request) -> Option<RemoteContext> {
    request.extensions().get::<RemoteContext>().cloned()
}

async fn body_of(request: Request) -> Result<Vec<u8>, Response> {
    axum::body::to_bytes(request.into_body(), MAX_BODY)
        .await
        .map(|b| b.to_vec())
        .map_err(|_| http_fail(413, "Request body too large."))
}

// ---- request bodies (pydantic models) ----------------------------------------------------------

struct Credentials {
    email: String,
    password: String,
}

fn string_field(map: &Map<String, Value>, key: &str, min: usize, max: usize) -> Option<String> {
    let text = map.get(key)?.as_str()?;
    let chars = text.chars().count();
    (min..=max).contains(&chars).then(|| text.to_string())
}

fn object_of(raw: &[u8]) -> Option<Map<String, Value>> {
    match serde_json::from_slice::<Value>(raw).ok()? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

fn only_keys(map: &Map<String, Value>, allowed: &[&str]) -> bool {
    map.keys().all(|k| allowed.contains(&k.as_str()))
}

/// `LoginRequest` (`extra="forbid"`, email 3..=254, password 1..=1024).
fn parse_login(raw: &[u8]) -> Option<Credentials> {
    let map = object_of(raw)?;
    only_keys(&map, &["email", "password"]).then_some(())?;
    Some(Credentials {
        email: string_field(&map, "email", 3, 254)?,
        password: string_field(&map, "password", 1, 1024)?,
    })
}

struct NewUser {
    credentials: Credentials,
    role: RemoteRole,
}

/// `CreateUserRequest`: login fields, password 12..=1024, a role.
fn parse_create(raw: &[u8]) -> Option<NewUser> {
    let map = object_of(raw)?;
    only_keys(&map, &["email", "password", "role"]).then_some(())?;
    Some(NewUser {
        credentials: Credentials {
            email: string_field(&map, "email", 3, 254)?,
            password: string_field(&map, "password", 12, 1024)?,
        },
        role: RemoteRole::parse(map.get("role")?.as_str()?)?,
    })
}

struct UserChange {
    role: Option<RemoteRole>,
    enabled: Option<bool>,
    password: Option<String>,
}

/// `UpdateUserRequest`: every field optional (`null` counts as absent).
fn parse_update(raw: &[u8]) -> Option<UserChange> {
    let map = object_of(raw)?;
    only_keys(&map, &["role", "enabled", "password"]).then_some(())?;
    let present = |key: &str| map.get(key).filter(|v| !v.is_null());
    Some(UserChange {
        role: match present("role") {
            Some(v) => Some(RemoteRole::parse(v.as_str()?)?),
            None => None,
        },
        enabled: match present("enabled") {
            Some(v) => Some(v.as_bool()?),
            None => None,
        },
        password: match present("password") {
            Some(_) => Some(string_field(&map, "password", 12, 1024)?),
            None => None,
        },
    })
}

// ---- access gates ------------------------------------------------------------------------------

fn access_of(wiring: &RemoteWiring) -> Result<Arc<RemoteAccess>, Response> {
    wiring
        .access
        .clone()
        .ok_or_else(|| http_fail(503, "Remote access is not configured."))
}

/// `_admin`: a remote caller must be an Operator; local access already passed the LAN PIN or
/// Tauri middleware.
fn admin(rt: &Runtime, request: &Request) -> Result<Arc<RemoteAccess>, Response> {
    if let Some(context) = context(request) {
        let operator = context
            .session
            .as_ref()
            .is_some_and(|s| s.role == RemoteRole::Operator);
        if !operator {
            return Err(http_fail(403, "Operator access required."));
        }
    }
    access_of(&rt.remote_wiring())
}

fn remote_only(request: &Request) -> Result<RemoteContext, Response> {
    context(request)
        .ok_or_else(|| http_fail(403, "Use the dedicated remote ingress for remote sessions."))
}

fn mutation(request: &Request) -> Result<(), Response> {
    let header_ok = request
        .headers()
        .get("x-stagepilot-remote")
        .is_some_and(|v| v.as_bytes() == b"1");
    if header_ok {
        Ok(())
    } else {
        Err(http_fail(403, "Remote settings request required."))
    }
}

// ---- remote auth -------------------------------------------------------------------------------

fn session_cookie(token: &str) -> String {
    format!(
        "{COOKIE_NAME}={token}; HttpOnly; Max-Age={}; Path=/; SameSite=strict; Secure",
        SESSION_SECONDS as u64
    )
}

fn deleted_cookie() -> String {
    format!(
        "{COOKIE_NAME}=\"\"; expires={}; HttpOnly; Max-Age=0; Path=/; SameSite=strict; Secure",
        httpdate::fmt_http_date(SystemTime::now())
    )
}

fn with_cookie(mut response: Response, cookie: &str) -> Response {
    if let Ok(value) = HeaderValue::from_str(cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

async fn login(request: Request) -> Response {
    let context = context(&request);
    let raw = match body_of(request).await {
        Ok(raw) => raw,
        Err(response) => return response,
    };
    let Some(body) = parse_login(&raw) else {
        return invalid();
    };
    let Some(context) = context else {
        return http_fail(403, "Use the dedicated remote ingress for remote sessions.");
    };
    let previous = context.token.clone();
    let result = context
        .access
        .call(move |store| store.login(&body.email, &body.password, previous.as_deref()))
        .await;
    match result {
        Ok(token) => with_cookie(
            reply(
                200,
                Some(json!({"authenticated": true, "csrf_token": csrf_token(&token)})),
            ),
            &session_cookie(&token),
        ),
        Err(error) => auth_fail(error),
    }
}

async fn session(request: Request) -> Response {
    let context = match remote_only(&request) {
        Ok(context) => context,
        Err(response) => return response,
    };
    let (Some(session), Some(token)) = (&context.session, &context.token) else {
        return http_fail(401, "Remote session required.");
    };
    reply(
        200,
        Some(json!({
            "user": {
                "id": session.user_id,
                "email": session.email,
                "role": session.role.as_str(),
            },
            "expires_at": session.expires_at,
            "csrf_token": csrf_token(token),
        })),
    )
}

async fn logout(request: Request) -> Response {
    let context = match remote_only(&request) {
        Ok(context) => context,
        Err(response) => return response,
    };
    if let Some(token) = context.token.clone().filter(|t| !t.is_empty()) {
        let all = request.uri().path().ends_with("/revoke-sessions");
        if let Err(error) = context
            .access
            .call(move |store| store.revoke(&token, all))
            .await
        {
            return auth_fail(error);
        }
    }
    with_cookie(reply(204, None), &deleted_cookie())
}

async fn users(State(rt): State<Rt>, request: Request) -> Response {
    let access = match admin(&rt, &request) {
        Ok(access) => access,
        Err(response) => return response,
    };
    match access.call(|store| store.users()).await {
        Ok(list) => reply(200, Some(serde_json::to_value(list).unwrap_or(Value::Null))),
        Err(error) => auth_fail(error),
    }
}

async fn create_user(State(rt): State<Rt>, request: Request) -> Response {
    let gate = admin_after_body(&rt, request, parse_create).await;
    let (access, body) = match gate {
        Ok(pair) => pair,
        Err(response) => return response,
    };
    let result = access
        .call(move |store| {
            store.create_user(
                &body.credentials.email,
                &body.credentials.password,
                body.role,
                false,
            )
        })
        .await;
    match result {
        Ok(user) => reply(201, Some(serde_json::to_value(user).unwrap_or(Value::Null))),
        Err(error) => auth_fail(error),
    }
}

/// Body validation comes before the route body in FastAPI, so a bad body is a 422 even for a
/// caller who would then be refused.
async fn admin_after_body<T>(
    rt: &Runtime,
    request: Request,
    parse: impl FnOnce(&[u8]) -> Option<T>,
) -> Result<(Arc<RemoteAccess>, T), Response> {
    let (parts, body) = request.into_parts();
    let raw = axum::body::to_bytes(body, MAX_BODY)
        .await
        .map_err(|_| http_fail(413, "Request body too large."))?;
    let parsed = parse(&raw).ok_or_else(invalid)?;
    let access = admin(rt, &Request::from_parts(parts, Body::empty()))?;
    Ok((access, parsed))
}

async fn update_user(
    State(rt): State<Rt>,
    Path(user_id): Path<String>,
    request: Request,
) -> Response {
    let gate = admin_after_body(&rt, request, parse_update).await;
    let (access, change) = match gate {
        Ok(pair) => pair,
        Err(response) => return response,
    };
    let result = access
        .call(move |store| {
            store.update_user(
                &user_id,
                change.role,
                change.enabled,
                change.password.as_deref(),
                false,
            )
        })
        .await;
    match result {
        Ok(()) => reply(204, None),
        Err(error) => auth_fail(error),
    }
}

async fn delete_user(
    State(rt): State<Rt>,
    Path(user_id): Path<String>,
    request: Request,
) -> Response {
    let access = match admin(&rt, &request) {
        Ok(access) => access,
        Err(response) => return response,
    };
    match access
        .call(move |store| store.update_user(&user_id, None, None, None, true))
        .await
    {
        Ok(()) => reply(204, None),
        Err(error) => auth_fail(error),
    }
}

// ---- remote access (product controls) ----------------------------------------------------------

fn unavailable_status() -> Value {
    json!({
        "available": false,
        "enabled": false,
        "state": "off",
        "url": null,
        "message": "Remote Access is unavailable on this installation.",
        "temporary_url": true,
        "permanently_revoked": false,
    })
}

/// The status object, with the defaults and `needs_operator` the route adds.
async fn current_status(
    access: &Arc<RemoteAccess>,
    wiring: &RemoteWiring,
) -> Result<Value, Response> {
    let mut value = if let Some(manager) = &wiring.manager {
        manager.status().await
    } else if let Some(feature) = wiring.feature.clone() {
        access
            .run(move || Ok(feature.status()))
            .await
            .map_err(auth_fail)?
    } else {
        unavailable_status()
    };
    let users = access
        .call(|store| store.users())
        .await
        .map_err(auth_fail)?;
    if let Value::Object(map) = &mut value {
        map.entry("provisioned").or_insert(Value::Bool(true));
        map.entry("credential_available")
            .or_insert(Value::Bool(true));
        map.entry("permanently_revoked")
            .or_insert(Value::Bool(false));
        map.insert(
            "needs_operator".into(),
            Value::Bool(!users.iter().any(|u| u.enabled && u.role == "Operator")),
        );
    }
    Ok(value)
}

async fn status(State(rt): State<Rt>, request: Request) -> Response {
    let access = match admin(&rt, &request) {
        Ok(access) => access,
        Err(response) => return response,
    };
    status_reply(&access, &rt.remote_wiring()).await
}

async fn status_reply(access: &Arc<RemoteAccess>, wiring: &RemoteWiring) -> Response {
    match current_status(access, wiring).await {
        Ok(value) => reply(200, Some(value)),
        Err(response) => response,
    }
}

async fn bootstrap(State(rt): State<Rt>, request: Request) -> Response {
    let remote_caller = context(&request).is_some();
    let headers = request.headers().clone();
    let gate = admin_after_body(&rt, request, parse_login).await;
    let (access, body) = match gate {
        Ok(pair) => pair,
        Err(response) => return response,
    };
    if headers
        .get("x-stagepilot-remote")
        .is_none_or(|v| v.as_bytes() != b"1")
    {
        return http_fail(403, "Remote settings request required.");
    }
    if remote_caller {
        return http_fail(403, "Create the first Operator from local StagePilot.");
    }
    match access
        .call(move |store| store.bootstrap(&body.email, &body.password))
        .await
    {
        Ok(user) => reply(201, Some(serde_json::to_value(user).unwrap_or(Value::Null))),
        Err(error) => auth_fail(error),
    }
}

async fn enable(State(rt): State<Rt>, request: Request) -> Response {
    let access = match admin(&rt, &request) {
        Ok(access) => access,
        Err(response) => return response,
    };
    if let Err(response) = mutation(&request) {
        return response;
    }
    let wiring = rt.remote_wiring();
    if let Some(manager) = &wiring.manager {
        match manager.enable().await {
            Ok(()) => {}
            Err(crate::remote_types::ManagerError::Provider(text)) => {
                return http_fail(503, &enable_error_message(&text));
            }
            Err(crate::remote_types::ManagerError::Value(_)) => return not_provisioned(),
        }
    } else {
        let Some(feature) = wiring.feature.clone() else {
            return http_fail(503, "Remote Access is unavailable on this installation.");
        };
        match access.run(move || set_enabled(&feature, true)).await {
            Ok(true) => {}
            Ok(false) => return not_provisioned(),
            Err(error) => return auth_fail(error),
        }
    }
    status_reply(&access, &wiring).await
}

fn not_provisioned() -> Response {
    http_fail(
        409,
        "Remote Access is not provisioned or is already managed.",
    )
}

/// `RemoteFeature.set_enabled`: `Ok(false)` is the `ValueError` ("managed elsewhere").
fn set_enabled(
    feature: &crate::remote_feature::RemoteFeature,
    enabled: bool,
) -> Result<bool, RemoteAuthError> {
    use crate::remote_feature::FeatureError;
    match feature.set_enabled(enabled) {
        Ok(()) => Ok(true),
        Err(FeatureError::Value(_)) => Ok(false),
        Err(FeatureError::Io(error)) => {
            tracing::warn!(%error, "remote_feature_failed");
            Err(RemoteAuthError {
                status: 503,
                detail: "Remote access storage is unavailable.".into(),
            })
        }
    }
}

async fn reset(State(rt): State<Rt>, request: Request) -> Response {
    let access = match admin(&rt, &request) {
        Ok(access) => access,
        Err(response) => return response,
    };
    if let Err(response) = mutation(&request) {
        return response;
    }
    let wiring = rt.remote_wiring();
    let Some(manager) = wiring.manager.clone().filter(|m| m.supports_reset()) else {
        return http_fail(
            503,
            "Resetting the installation identity is unavailable on this installation.",
        );
    };
    if manager.reset_identity().await.is_err() {
        return internal_error();
    }
    status_reply(&access, &wiring).await
}

/// `_enable_error_message`: fixed, end-user wording for a sanitised provider error.
pub fn enable_error_message(text: &str) -> String {
    if text.contains("enrollment limit") {
        "This computer has reached its enrollment limit for now. Try again later."
    } else if text.contains("Could not reach")
        || (text.contains("unavailable") && text.contains("Internet"))
    {
        "Could not reach the enrollment service. Check your Internet connection and try again."
    } else if text.contains("credential is unavailable") || text.contains("credential was revoked")
    {
        "Your previous Remote Access setup could not be restored and needs to be reset. This is \
         safe and won't affect local StagePilot."
    } else if text.contains("Hostname ownership conflict") {
        "Your previous Remote setup is still registered. Try again in a minute; if it keeps \
         happening, reset Remote Access."
    } else if text.contains("not trusted") {
        "Remote Access is unavailable on this installation."
    } else {
        "Remote Access could not be enabled right now; local StagePilot is unaffected. Try again \
         shortly."
    }
    .to_string()
}

async fn disable(State(rt): State<Rt>, request: Request) -> Response {
    let access = match admin(&rt, &request) {
        Ok(access) => access,
        Err(response) => return response,
    };
    if let Err(response) = mutation(&request) {
        return response;
    }
    let wiring = rt.remote_wiring();
    if let Some(manager) = &wiring.manager {
        match manager.disable().await {
            Ok(()) => {}
            Err(crate::remote_types::ManagerError::Provider(_)) => {
                return http_fail(
                    503,
                    "Remote public access is closed locally; provider revocation will retry on \
                     restart.",
                );
            }
            Err(crate::remote_types::ManagerError::Value(_)) => return internal_error(),
        }
    } else {
        let Some(feature) = wiring.feature.clone() else {
            return http_fail(503, "Remote Access is unavailable on this installation.");
        };
        match access.run(move || set_enabled(&feature, false)).await {
            Ok(_) => {}
            Err(error) => return auth_fail(error),
        }
    }
    let generation = Uuid::new_v4().to_string();
    if let Err(error) = access
        .call(move |store| store.installation_generation(&generation))
        .await
    {
        return auth_fail(error);
    }
    status_reply(&access, &wiring).await
}

async fn regenerate(State(rt): State<Rt>, request: Request) -> Response {
    let access = match admin(&rt, &request) {
        Ok(access) => access,
        Err(response) => return response,
    };
    if let Err(response) = mutation(&request) {
        return response;
    }
    let wiring = rt.remote_wiring();
    let Some(manager) = wiring.manager.clone().filter(|m| m.supports_regenerate()) else {
        return http_fail(
            503,
            "Regenerating the Remote link is unavailable on this installation.",
        );
    };
    match manager.regenerate().await {
        Ok(()) => {}
        Err(crate::remote_types::ManagerError::Provider(_)) => {
            return http_fail(
                503,
                "Remote link regeneration is unavailable; local StagePilot is unaffected.",
            );
        }
        Err(crate::remote_types::ManagerError::Value(_)) => return internal_error(),
    }
    status_reply(&access, &wiring).await
}

// ---- retry journal (remote_retry.py) -----------------------------------------------------------

fn idempotency_key_ok(key: &str) -> bool {
    (16..=128).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Session-scoped, durable at-most-once dispatch for keyed Remote one-shot requests.
pub async fn remote_retry(request: Request, next: Next) -> Response {
    let selected = request.uri().path().starts_with("/api/v1/actions/")
        || ONE_SHOTS.contains(&request.uri().path());
    let key = request
        .headers()
        .get("idempotency-key")
        .map(|v| v.to_str().unwrap_or("\u{0}").to_string());
    let context = context(&request);
    let (Some(context), Some(key)) = (context, key) else {
        return next.run(request).await;
    };
    if request.method() != Method::POST || !selected {
        return next.run(request).await;
    }
    if !idempotency_key_ok(&key) || request.headers().get_all("idempotency-key").iter().count() != 1
    {
        return http_fail(422, "Invalid action request identifier.");
    }
    let (Some(token), Some(session)) = (context.token.clone(), context.session.clone()) else {
        return http_fail(401, "Remote email/password session required.");
    };
    let path = request.uri().path().to_string();
    let (parts, body) = request.into_parts();
    let bytes = match axum::body::to_bytes(body, 16_385).await {
        Ok(bytes) if bytes.len() <= 16_384 => bytes,
        _ => return http_fail(413, "Action request too large."),
    };
    let mut hasher = Sha256::new();
    hasher.update(path.as_bytes());
    hasher.update([0u8]);
    hasher.update(&bytes);
    let fingerprint = hex::encode(hasher.finalize());
    let journal_key = digest(&format!("{token}:{key}"));
    let (claim_key, claim_fingerprint) = (journal_key.clone(), fingerprint);
    let expires = session.expires_at;
    let previous = context
        .access
        .call(move |store| store.claim_action(&claim_key, &claim_fingerprint, expires))
        .await;
    match previous {
        Err(error) => return http_fail(error.status, &error.detail),
        Ok(Some((code, payload))) => {
            let mut response = (status_of(code), payload).into_response();
            let headers = response.headers_mut();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            );
            headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            return response;
        }
        Ok(None) => {}
    }
    let response = next
        .run(Request::from_parts(parts, Body::from(bytes)))
        .await;
    if response.status().as_u16() >= 400 {
        // An `HTTPException` leaves the claim pending in Python (the journal is only finished
        // for a handler that returned), so a retry reports an uncertain outcome.
        return response;
    }
    let (head, body) = response.into_parts();
    let payload = match axum::body::to_bytes(body, MAX_BODY).await {
        Ok(payload) => payload,
        Err(_) => return internal_error(),
    };
    // Persist before replying; uncertain operations are never re-dispatched.
    let (finish_key, finish_payload, code) = (journal_key, payload.to_vec(), head.status.as_u16());
    if let Err(error) = context
        .access
        .call(move |store| store.finish_action(&finish_key, code, &finish_payload))
        .await
    {
        return http_fail(error.status, &error.detail);
    }
    let mut response = Response::from_parts(head, Body::from(payload));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
