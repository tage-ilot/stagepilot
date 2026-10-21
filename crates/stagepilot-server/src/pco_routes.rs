//! `/api/v1/planning-center/*` routes: port of the Planning Center handlers in `api/routes.py`
//! (status, OAuth start/callback/status/disconnect, settings, test, service types, plan reload,
//! pending selection and plan selection), plus the live Services-source switch
//! (`services/service_source.py`). Status codes and JSON bodies follow the Python backend.

use crate::demo::DemoPlugin;
use crate::runtime::Runtime;
use crate::validation::{http_error, invalid_body, ok, unprocessable, Json};
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, HeaderValue};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Map, Value};
use stagepilot_core::config::{ConnectionMethod, ServiceSource};
use stagepilot_core::events::{new_event, ActionName, EventPayload, EventType};
use stagepilot_core::settings::{PersistentPlanningCenterSettings, UpdateError};
use stagepilot_core::state::{PluginStatus, ServiceLoadStatus, TimerStatus};
use stagepilot_pco::errors::PcoError;
use stagepilot_pco::models::{
    PendingPlanSelectionResponse, PlanningCenterOAuthStartResponse,
    PlanningCenterOAuthStatusResponse, PlanningCenterServiceTypeResponse,
    PlanningCenterStatusResponse, PlanningCenterTestResponse,
};
use stagepilot_pco::oauth::{blocking, CompleteError, OAuthError};
use stagepilot_pco::plugin::{ALL_SERVICE_TYPES_ID, PLUGIN_NAME};
use std::sync::Arc;

type Rt = Arc<Runtime>;

pub const BUSY_MESSAGE: &str = "Stop the timer before switching between Demo and Planning Center.";
pub const CREDENTIAL_BUSY_MESSAGE: &str =
    "Stop the timer before changing the Planning Center connection.";

fn to_value<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn respond(json: Json) -> Response {
    json.into_response()
}

// ---- request body validation (FastAPI/pydantic error shapes) ---------------------------------------

struct Body {
    map: Map<String, Value>,
    input: Value,
    errors: Vec<Value>,
    seen: Vec<&'static str>,
}

impl Body {
    fn parse(raw: &[u8]) -> Result<Self, Vec<Value>> {
        let parsed: Option<Value> = serde_json::from_slice(raw).ok();
        let Some(Value::Object(map)) = &parsed else {
            return Err(invalid_body(raw, parsed.as_ref()));
        };
        Ok(Self {
            input: Value::Object(map.clone()),
            map: map.clone(),
            errors: vec![],
            seen: vec![],
        })
    }

    fn loc(key: &str) -> Value {
        json!(["body", key])
    }

    fn missing(&mut self, key: &'static str) {
        self.errors
            .push(json!({"type": "missing", "loc": Self::loc(key),
            "msg": "Field required", "input": self.input}));
    }

    fn not_string(&mut self, key: &'static str, value: &Value) {
        self.errors
            .push(json!({"type": "string_type", "loc": Self::loc(key),
            "msg": "Input should be a valid string", "input": value}));
    }

    /// A plain `str` field (`required` or `Optional`) with optional length bounds.
    fn string(
        &mut self,
        key: &'static str,
        min: usize,
        max: Option<usize>,
        required: bool,
    ) -> Option<String> {
        self.seen.push(key);
        match self.map.get(key).cloned() {
            None => {
                if required {
                    self.missing(key);
                }
                None
            }
            Some(Value::Null) if !required => None,
            Some(Value::String(text)) => {
                let n = text.chars().count();
                if n < min {
                    self.errors.push(json!({"type": "string_too_short", "loc": Self::loc(key),
                        "msg": format!("String should have at least {min} character{}", if min == 1 { "" } else { "s" }),
                        "input": text, "ctx": {"min_length": min}}));
                    None
                } else if max.is_some_and(|m| n > m) {
                    let m = max.unwrap_or(0);
                    self.errors.push(json!({"type": "string_too_long", "loc": Self::loc(key),
                        "msg": format!("String should have at most {m} character{}", if m == 1 { "" } else { "s" }),
                        "input": text, "ctx": {"max_length": m}}));
                    None
                } else {
                    Some(text)
                }
            }
            Some(other) => {
                self.not_string(key, &other);
                None
            }
        }
    }

    /// A `SecretStr` field: length is checked after validation, so the error types differ.
    fn secret(
        &mut self,
        key: &'static str,
        min: usize,
        max: Option<usize>,
        required: bool,
    ) -> Option<String> {
        self.seen.push(key);
        match self.map.get(key).cloned() {
            None => {
                if required {
                    self.missing(key);
                }
                None
            }
            Some(Value::Null) if !required => None,
            Some(Value::String(text)) => {
                let n = text.chars().count();
                if n < min {
                    self.errors.push(json!({"type": "too_short", "loc": Self::loc(key),
                        "msg": format!("Value should have at least {min} item{} after validation, not {n}", if min == 1 { "" } else { "s" }),
                        "input": text,
                        "ctx": {"field_type": "Value", "min_length": min, "actual_length": n}}));
                    None
                } else if max.is_some_and(|m| n > m) {
                    let m = max.unwrap_or(0);
                    self.errors.push(json!({"type": "too_long", "loc": Self::loc(key),
                        "msg": format!("Value should have at most {m} item{} after validation, not {n}", if m == 1 { "" } else { "s" }),
                        "input": text,
                        "ctx": {"field_type": "Value", "max_length": m, "actual_length": n}}));
                    None
                } else {
                    Some(text)
                }
            }
            Some(other) => {
                self.not_string(key, &other);
                None
            }
        }
    }

    fn boolean(&mut self, key: &'static str, default: bool) -> bool {
        self.seen.push(key);
        match self.map.get(key).cloned() {
            None => default,
            Some(Value::Bool(b)) => b,
            Some(other) => {
                self.errors.push(json!({"type": "bool_parsing", "loc": Self::loc(key),
                    "msg": "Input should be a valid boolean, unable to interpret input", "input": other}));
                default
            }
        }
    }

    /// `extra="forbid"`: unknown keys are reported after the declared fields.
    fn forbid_extras(&mut self) {
        let extras: Vec<(String, Value)> = self
            .map
            .iter()
            .filter(|(k, _)| !self.seen.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (key, value) in extras {
            self.errors
                .push(json!({"type": "extra_forbidden", "loc": ["body", key],
                "msg": "Extra inputs are not permitted", "input": value}));
        }
    }

    fn finish(self) -> Result<(), Vec<Value>> {
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors)
        }
    }
}

// ---- error mapping -----------------------------------------------------------------------------------

fn pco_error(error: &PcoError) -> Response {
    let status = match error {
        PcoError::Configuration(_) => 409,
        PcoError::Authentication(_) => 401,
        PcoError::Permission(_) => 403,
        PcoError::RateLimit { .. } => 429,
        _ => 502,
    };
    let mut response = respond(http_error(status, &error.to_string()));
    if let PcoError::RateLimit {
        retry_after_seconds: Some(seconds),
    } = error
    {
        if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
    }
    response
}

/// 401 means "this connection is genuinely dead, sign in again"; 503 means "transient".
fn oauth_error(error: &OAuthError) -> Response {
    respond(http_error(
        if error.permanent { 401 } else { 503 },
        &error.message,
    ))
}

fn complete_error(error: &CompleteError) -> Response {
    match error {
        CompleteError::OAuth(e) => oauth_error(e),
        CompleteError::Credential(e) => respond(http_error(503, &e.0)),
    }
}

fn persistence_error(message: &str) -> Response {
    respond(http_error(503, message))
}

// ---- status -----------------------------------------------------------------------------------------------

pub async fn status(State(rt): State<Rt>) -> Response {
    let state = rt.store.snapshot();
    let effective = rt.pco.effective();
    let method = rt
        .settings_service
        .effective_snapshot()
        .planning_center
        .connection_method;
    let oauth = rt.pco.oauth.status(method);
    let body = PlanningCenterStatusResponse {
        connection_status: state.planning_center_status,
        configured: effective.is_configured() && effective.service_type_id.is_some(),
        app_id: effective.app_id.as_ref().map(|a| a.expose().to_string()),
        service_type_id: effective.service_type_id.clone(),
        planning_center_secret_saved: rt.settings_service.credential_saved(),
        connection_method: method,
        oauth_connected: oauth.connected,
        oauth_needs_reconnect: oauth.needs_reconnect,
        detail: state.service_load.message.clone(),
    };
    respond(ok(to_value(&body)))
}

// ---- OAuth ------------------------------------------------------------------------------------------------

fn oauth_unavailable(rt: &Runtime) -> Option<Response> {
    if rt.pco.oauth.configured() {
        None
    } else {
        Some(respond(http_error(
            503,
            "This build of StagePilot cannot sign in to Planning Center.",
        )))
    }
}

pub async fn oauth_start(State(rt): State<Rt>) -> Response {
    if let Some(unavailable) = oauth_unavailable(&rt) {
        return unavailable;
    }
    match rt.pco.oauth.start().await {
        Ok((authorize_url, state)) => respond(ok(to_value(&PlanningCenterOAuthStartResponse {
            authorize_url,
            state,
        }))),
        Err(error) => oauth_error(&error),
    }
}

fn status_body(rt: &Runtime, method: ConnectionMethod) -> Value {
    let s = rt.pco.oauth.status(method);
    to_value(&PlanningCenterOAuthStatusResponse {
        connection_method: s.connection_method,
        connected: s.connected,
        needs_reconnect: s.needs_reconnect,
        expires_at: s.expires_at,
        scope: s.scope,
    })
}

async fn ensure_show_idle(rt: &Runtime, message: &str) -> Option<Response> {
    if rt.store.snapshot().timer.status == TimerStatus::Running {
        Some(respond(http_error(409, message)))
    } else {
        None
    }
}

/// Hand the current credentials to the running plugin, or apply the saved source when there is none.
#[allow(clippy::result_large_err)]
async fn apply_credentials(rt: &Rt) -> Result<(), Response> {
    match rt.pco.plugin() {
        Some(plugin) => {
            plugin.reconfigure(rt.pco.effective()).await;
            Ok(())
        }
        None => apply_service_source(rt)
            .await
            .map(|_| ())
            .map_err(|message| respond(http_error(409, message))),
    }
}

async fn update_connection_method(rt: &Rt, method: ConnectionMethod) -> Result<(), String> {
    let settings = rt.settings_service.clone();
    blocking(move || {
        let mut snapshot = settings.snapshot();
        snapshot.planning_center.connection_method = method;
        settings.save(snapshot).map_err(|e| e.0)
    })
    .await
    .map_err(|e| e.message)?
}

pub async fn oauth_callback(State(rt): State<Rt>, body: Bytes) -> Response {
    let (state, code, redirect_uri) = {
        let mut b = match Body::parse(&body) {
            Ok(b) => b,
            Err(errors) => return respond(unprocessable(errors)),
        };
        let state = b.string("state", 1, Some(512), true);
        let code = b.secret("code", 1, None, true);
        let redirect_uri = b.string("redirect_uri", 1, Some(256), true);
        b.forbid_extras();
        if let Err(errors) = b.finish() {
            return respond(unprocessable(errors));
        }
        (
            state.unwrap_or_default(),
            code.unwrap_or_default(),
            redirect_uri.unwrap_or_default(),
        )
    };
    let _serialised = rt.pco.lock.lock().await;
    if let Some(unavailable) = oauth_unavailable(&rt) {
        return unavailable;
    }
    if rt.pco.plugin().is_some() {
        if let Some(busy) = ensure_show_idle(&rt, CREDENTIAL_BUSY_MESSAGE).await {
            return busy;
        }
    }
    if let Err(error) = rt.pco.oauth.complete(&state, &code, &redirect_uri).await {
        return complete_error(&error);
    }
    if let Err(message) = update_connection_method(&rt, ConnectionMethod::Oauth).await {
        return persistence_error(&message);
    }
    // A brand-new OAuth connect has no service type saved yet: default it to "All service types"
    // and, when Planning Center is the active source, apply it so the plan loads without clicks.
    let current = rt.settings_service.snapshot().planning_center;
    if current.service_type_id.is_none() {
        let updated = PersistentPlanningCenterSettings {
            service_type_id: Some(ALL_SERVICE_TYPES_ID.to_string()),
            ..current
        };
        if let Err(message) = save_planning_center(&rt, updated, None, false).await {
            return persistence_error(&message);
        }
    }
    if let Err(response) = apply_credentials(&rt).await {
        return response;
    }
    respond(ok(status_body(&rt, ConnectionMethod::Oauth)))
}

pub async fn oauth_status(State(rt): State<Rt>) -> Response {
    let method = rt
        .settings_service
        .effective_snapshot()
        .planning_center
        .connection_method;
    respond(ok(status_body(&rt, method)))
}

pub async fn oauth_disconnect(State(rt): State<Rt>) -> Response {
    let _serialised = rt.pco.lock.lock().await;
    if let Some(unavailable) = oauth_unavailable(&rt) {
        return unavailable;
    }
    if rt.pco.plugin().is_some() {
        if let Some(busy) = ensure_show_idle(&rt, CREDENTIAL_BUSY_MESSAGE).await {
            return busy;
        }
    }
    let oauth = rt.pco.oauth.clone();
    match blocking(move || oauth.disconnect()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return persistence_error(&error.0),
        Err(error) => return persistence_error(&error.message),
    }
    // Fall back to the manual path; any existing PAT secret is left untouched.
    if let Err(message) = update_connection_method(&rt, ConnectionMethod::Manual).await {
        return persistence_error(&message);
    }
    if let Err(response) = apply_credentials(&rt).await {
        return response;
    }
    respond(ok(status_body(&rt, ConnectionMethod::Manual)))
}

// ---- manual settings -------------------------------------------------------------------------------------------

async fn save_planning_center(
    rt: &Rt,
    settings: PersistentPlanningCenterSettings,
    secret: Option<String>,
    remove_secret: bool,
) -> Result<(), String> {
    let service = rt.settings_service.clone();
    blocking(move || {
        service
            .update_planning_center(settings, secret.as_deref(), remove_secret)
            .map_err(|e| match e {
                UpdateError::File(e) => e.0,
                UpdateError::Credential(e) => e.0,
            })
    })
    .await
    .map_err(|e| e.message)?
}

fn parse_settings_update(
    raw: &[u8],
) -> Result<(PersistentPlanningCenterSettings, Option<String>, bool), Vec<Value>> {
    let mut b = Body::parse(raw)?;
    // The nested model first (declared fields, then its own `extra="forbid"`)...
    let mut model = Map::new();
    for key in [
        "app_id",
        "connection_method",
        "service_type_id",
        "plan_title_preference",
        "preferred_service_time",
        "upcoming_lookahead_days",
        "request_timeout_seconds",
    ] {
        if let Some(value) = b.map.get(key) {
            model.insert(key.to_string(), value.clone());
            b.seen.push(match key {
                "app_id" => "app_id",
                "connection_method" => "connection_method",
                "service_type_id" => "service_type_id",
                "plan_title_preference" => "plan_title_preference",
                "preferred_service_time" => "preferred_service_time",
                "upcoming_lookahead_days" => "upcoming_lookahead_days",
                _ => "request_timeout_seconds",
            });
        }
    }
    let original = Value::Object(model.clone());
    let mut parsed = match serde_path_to_error::deserialize::<_, PersistentPlanningCenterSettings>(
        original.clone(),
    ) {
        Ok(parsed) => Some(parsed),
        Err(error) => {
            let detail =
                stagepilot_core::settings_errors::map_serde_error(&error, &original, &original);
            let mut detail = to_value(&detail);
            if let Some(loc) = detail.get_mut("loc").and_then(Value::as_array_mut) {
                loc.insert(0, json!("body"));
            }
            b.errors.push(detail);
            None
        }
    };
    if let Some(settings) = parsed.as_mut() {
        settings.normalise();
        let mut errors = Vec::new();
        settings.validate(&[], &mut errors);
        for error in errors {
            let mut detail = to_value(&error);
            if let Some(loc) = detail.get_mut("loc").and_then(Value::as_array_mut) {
                loc.insert(0, json!("body"));
            }
            b.errors.push(detail);
        }
    }
    let secret = b.secret("secret", 0, None, false);
    let remove_secret = b.boolean("remove_secret", false);
    b.forbid_extras();
    if b.errors.is_empty() && secret.is_some() && remove_secret {
        b.errors.push(json!({"type": "value_error", "loc": ["body"],
            "msg": "Value error, A credential cannot be replaced and removed in one request.",
            "input": b.input, "ctx": {"error": {}}}));
    }
    let input_hidden = b.errors.is_empty();
    b.finish()?;
    debug_assert!(input_hidden);
    match parsed {
        Some(settings) => Ok((settings, secret, remove_secret)),
        None => Err(vec![]),
    }
}

pub async fn update_settings(State(rt): State<Rt>, body: Bytes) -> Response {
    let (requested, secret, remove_secret) = match parse_settings_update(&body) {
        Ok(parsed) => parsed,
        Err(errors) => return respond(unprocessable(errors)),
    };
    let _serialised = rt.pco.lock.lock().await;
    let previous = rt.settings_service.snapshot().planning_center;
    // The manual-settings form never changes which auth path is active: only the dedicated OAuth
    // connect/disconnect routes do.
    let public = PersistentPlanningCenterSettings {
        connection_method: previous.connection_method,
        ..requested
    };
    let app_id_changed = public.app_id != previous.app_id;
    let secret_changed = secret.is_some() || remove_secret;
    if (app_id_changed || secret_changed) && rt.pco.plugin().is_some() {
        if let Some(busy) = ensure_show_idle(&rt, CREDENTIAL_BUSY_MESSAGE).await {
            return busy;
        }
    }
    if let Err(message) = save_planning_center(&rt, public, secret, remove_secret).await {
        return persistence_error(&message);
    }
    let mut reconfigure_error: Option<String> = None;
    match rt.pco.plugin() {
        Some(plugin) => {
            let outcome = plugin.reconfigure(rt.pco.effective()).await;
            if !outcome.accepted {
                reconfigure_error = Some(outcome.message);
            }
        }
        None if rt
            .settings_service
            .effective_runtime_settings()
            .integration_modes
            .service_source
            == ServiceSource::PlanningCenter =>
        {
            // Planning Center is the saved source but no plugin is running (a start failed).
            match apply_service_source(&rt).await {
                Ok(warning) => reconfigure_error = warning,
                Err(message) => return respond(http_error(409, message)),
            }
        }
        None => {}
    }
    let mut response = crate::app::settings_response(&rt, true, false);
    if let Some(error) = reconfigure_error {
        let warning = match response.get("warning").and_then(Value::as_str) {
            Some(existing) if !existing.is_empty() => {
                format!("{existing} {error}").trim().to_string()
            }
            _ => error,
        };
        response["warning"] = json!(warning);
    }
    respond(ok(response))
}

pub async fn test_connection(State(rt): State<Rt>, body: Bytes) -> Response {
    let (app_id, secret) = {
        let mut b = match Body::parse(&body) {
            Ok(b) => b,
            Err(errors) => return respond(unprocessable(errors)),
        };
        let app_id = b.string("app_id", 1, Some(255), false);
        let secret = b.secret("secret", 1, Some(255), false);
        b.forbid_extras();
        if let Err(errors) = b.finish() {
            return respond(unprocessable(errors));
        }
        (app_id, secret)
    };
    match rt
        .pco
        .setup
        .test_connection(app_id.as_deref(), secret.as_deref())
        .await
    {
        Ok(types) => respond(ok(to_value(&PlanningCenterTestResponse {
            authenticated: true,
            message: "Planning Center authentication succeeded.".to_string(),
            service_types: types
                .into_iter()
                .map(|t| PlanningCenterServiceTypeResponse {
                    id: t.id,
                    name: t.name,
                })
                .collect(),
        }))),
        Err(error) => pco_error(&error),
    }
}

pub async fn service_types(State(rt): State<Rt>) -> Response {
    match rt.pco.setup.list_service_types().await {
        Ok(types) => respond(ok(Value::Array(
            types
                .into_iter()
                .map(|t| {
                    to_value(&PlanningCenterServiceTypeResponse {
                        id: t.id,
                        name: t.name,
                    })
                })
                .collect(),
        ))),
        Err(error) => pco_error(&error),
    }
}

// ---- plan reload and selection ------------------------------------------------------------------------------------

pub async fn pending_selection(State(rt): State<Rt>) -> Response {
    let state = rt.store.snapshot();
    let pending = state.service_load.status == ServiceLoadStatus::Ambiguous;
    respond(ok(to_value(&PendingPlanSelectionResponse {
        pending,
        target_date: if pending {
            state.service_load.target_date.map(|d| d.to_string())
        } else {
            None
        },
        candidates: if pending {
            state.service_load.candidates.clone()
        } else {
            vec![]
        },
        message: if pending {
            state.service_load.message.clone()
        } else {
            None
        },
    })))
}

pub async fn select_plan(State(rt): State<Rt>, body: Bytes) -> Response {
    let plan_id = {
        let mut b = match Body::parse(&body) {
            Ok(b) => b,
            Err(errors) => return respond(unprocessable(errors)),
        };
        let plan_id = b.string("plan_id", 1, Some(128), true);
        if let Some(id) = &plan_id {
            if !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                b.errors.push(
                    json!({"type": "string_pattern_mismatch", "loc": ["body", "plan_id"],
                    "msg": "String should match pattern '^[A-Za-z0-9_-]+$'", "input": id,
                    "ctx": {"pattern": "^[A-Za-z0-9_-]+$"}}),
                );
            }
        }
        // `PlanSelectionRequest` ignores unknown keys (no `extra="forbid"`).
        if let Err(errors) = b.finish() {
            return respond(unprocessable(errors));
        }
        plan_id.unwrap_or_default()
    };
    let current = rt.store.snapshot();
    let is_candidate = current
        .service_load
        .candidates
        .iter()
        .any(|c| c.id == plan_id);
    if current.service_load.status != ServiceLoadStatus::Ambiguous || !is_candidate {
        return respond(http_error(
            409,
            "The selected plan is not a current Planning Center candidate.",
        ));
    }
    let report = rt
        .bus
        .publish(new_event(
            EventType::ServicePlanSelectionRequested,
            "api",
            EventPayload::ServicePlanSelection {
                plan_id: plan_id.clone(),
            },
        ))
        .await;
    if !report.failures.is_empty() {
        return respond(http_error(
            503,
            "Planning Center could not process the plan selection.",
        ));
    }
    let updated = rt.store.snapshot();
    match &updated.plan {
        Some(plan)
            if updated.service_load.status == ServiceLoadStatus::Loaded && plan.id == plan_id =>
        {
            respond(ok(json!({
                "accepted": true,
                "message": format!("Loaded \"{}\".", plan.title),
                "state": updated,
            })))
        }
        _ => respond(http_error(
            503,
            updated
                .service_load
                .message
                .as_deref()
                .filter(|m| !m.is_empty())
                .unwrap_or("Planning Center could not load the selected plan."),
        )),
    }
}

// ---- live Services source ---------------------------------------------------------------------------------------------

/// Make the running Services plugin match the saved settings. The caller holds `rt.pco.lock`
/// (Python's `reconcile_lock`). `Ok(Some(warning))` when the new plugin failed to start; `Err` is
/// the 409 "stop the timer first".
pub async fn apply_service_source(rt: &Rt) -> Result<Option<String>, &'static str> {
    let settings = rt.settings_service.effective_runtime_settings();
    let target = settings.integration_modes.service_source;
    let current = if rt.plugins.has("demo") {
        ServiceSource::Demo
    } else {
        ServiceSource::PlanningCenter
    };
    if target == current && (target == ServiceSource::Demo || rt.plugins.has(PLUGIN_NAME)) {
        return Ok(None);
    }
    if rt.store.snapshot().timer.status == TimerStatus::Running {
        return Err(BUSY_MESSAGE);
    }
    let previous = rt
        .plugins
        .get("demo")
        .or_else(|| rt.plugins.get(PLUGIN_NAME));
    let new_plugin: Arc<dyn stagepilot_core::plugin::Plugin> = match target {
        ServiceSource::Demo => Arc::new(DemoPlugin::new(rt.bus.clone())),
        ServiceSource::PlanningCenter => {
            match rt.pco.build_plugin(rt.bus.clone(), rt.store.clone()) {
                Ok(plugin) => {
                    rt.pco.set_plugin(Some(plugin.clone()));
                    Arc::new(plugin)
                }
                Err(error) => return Ok(Some(error.to_string())),
            }
        }
    };
    let switching = previous
        .as_ref()
        .is_none_or(|p| p.name() != new_plugin.name());
    if let Some(previous) = &previous {
        rt.plugins.remove(previous.name()).await;
        if previous.name() == PLUGIN_NAME {
            rt.pco.set_plugin(None);
        }
        if switching {
            rt.state_service.clear_service().await;
        }
        let name = previous.name().to_string();
        rt.store.mutate(move |s| {
            s.plugins.remove(&name);
        });
    }
    let health = rt.plugins.add(new_plugin.clone()).await;
    let planning_center = new_plugin.name() == PLUGIN_NAME;
    match health {
        Ok(health) if health.status == PluginStatus::Error => {
            if planning_center {
                rt.pco.set_plugin(None);
            }
            Ok(Some(health.last_error.unwrap_or_else(|| {
                "Services could not be started.".to_string()
            })))
        }
        Ok(_) => Ok(None),
        Err(error) => {
            if planning_center {
                rt.pco.set_plugin(None);
            }
            Ok(Some(error))
        }
    }
}

/// `POST /planning-center/plan/reload` is the `reload_plan` action.
pub fn reload_action() -> ActionName {
    ActionName::ReloadPlan
}

pub fn router() -> axum::Router<Rt> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/api/v1/planning-center/status", get(status))
        .route("/api/v1/planning-center/oauth/start", post(oauth_start))
        .route(
            "/api/v1/planning-center/oauth/callback",
            post(oauth_callback),
        )
        .route("/api/v1/planning-center/oauth/status", get(oauth_status))
        .route(
            "/api/v1/planning-center/oauth/disconnect",
            post(oauth_disconnect),
        )
        .route("/api/v1/planning-center/settings", post(update_settings))
        .route("/api/v1/planning-center/test", post(test_connection))
        .route("/api/v1/planning-center/service-types", get(service_types))
        .route(
            "/api/v1/planning-center/plans/pending-selection",
            get(pending_selection),
        )
        .route("/api/v1/planning-center/plan-selection", post(select_plan))
        .route("/api/v1/planning-center/plans/select", post(select_plan))
}
