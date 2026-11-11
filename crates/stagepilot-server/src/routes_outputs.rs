//! `/api/v1/*` routes for MIDI input, ProPresenter and lights. Status codes, JSON and
//! message texts follow `api/routes.py`; the contract replays prove the shapes against Python.

use crate::runtime::Runtime;
use crate::validation::{self, http_error, ok, unprocessable, Json};
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::routing::{get, post, put};
use axum::Router;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use stagepilot_core::config::{
    round_tenths, value_error, Detail, Errors, LightingCue, LightingMethod, LightkeyMode,
    LightkeySettings, LightsSettings, OscLightingCue, ProPresenterSettings, SongLightingCueMap,
};
use stagepilot_core::settings_errors::parse_body;
use stagepilot_core::state::{ConnectionStatus, TimerStatus};
use stagepilot_lights::lightkey::LightkeyActionResponse;
use stagepilot_lights::models::{
    LightingCaptureRequest, LightingCaptureResponse, LightingCaptureUndoRequest,
    LightingCueMapRequest, LightingCueTestRequest, LightingOscCueTestRequest, LightingPadsRequest,
    LightsOperationResponse, LightsSettingsRequest, LightsStatusResponse,
};
use stagepilot_midi::models::{
    cue_action, MidiCueSimulationRequest, MidiCueSimulationResponse, MidiInputSelectionRequest,
    MidiInputSelectionResponse, MidiMonitorResponse,
};
use stagepilot_propresenter::{
    ProPresenterOperationResponse, ProPresenterSettingsRequest, ProPresenterStatusResponse,
};
use std::sync::Arc;
use uuid::Uuid;

type Rt = Arc<Runtime>;

/// Run a handler's work to its end even when the client that asked for it goes away.
///
/// When a client disconnects, hyper drops the handler future at whatever `.await` it was on.
/// For work that leaves lights held, or half-applied settings, behind that is not safe, so the
/// work runs in its own task. The handler still waits for the result while the client is there;
/// if the client is gone the wait is dropped and the task carries on to its end.
async fn detached<F>(work: F) -> Json
where
    F: std::future::Future<Output = Json> + Send + 'static,
{
    match tokio::spawn(work).await {
        Ok(response) => response,
        Err(_) => http_error(
            500,
            "That change stopped unexpectedly. Check the Lighting status.",
        ),
    }
}

const MIDI_BUSY: &str = "Stop the timer before changing the MIDI input.";
const MIDI_DISABLED: &str = "The MIDI Playback plugin is disabled.";
const PP_DISABLED: &str = "The ProPresenter plugin is disabled.";

/// Largest cue-map PUT body: a song at the per-song caps. A look row is about 400 bytes of JSON
/// and a message about 130 (measured: 600,000 messages and 50,000 rows are 65 MB compact);
/// 160 bytes a message and 4 MB of slack leave room for a pretty-printed or longer-named song.
const CUE_MAP_BODY_LIMIT: usize = stagepilot_core::config::MAX_OSC_ROWS_PER_SONG * 400
    + stagepilot_core::config::MAX_OSC_MESSAGES_PER_SONG * 160
    + 4 * 1024 * 1024;

pub fn routes() -> Router<Rt> {
    Router::new()
        .route("/api/v1/midi/inputs", get(midi_inputs).post(midi_inputs))
        .route("/api/v1/midi/inputs/refresh", post(midi_inputs))
        .route("/api/v1/midi/messages", get(midi_messages))
        .route("/api/v1/midi/input-selection", post(midi_input_selection))
        .route("/api/v1/midi/cue-simulation", post(midi_cue_simulation))
        .route("/api/v1/propresenter", get(propresenter_status))
        .route("/api/v1/propresenter/test", post(propresenter_test))
        .route(
            "/api/v1/propresenter/timers/refresh",
            post(propresenter_refresh),
        )
        .route("/api/v1/propresenter/settings", post(propresenter_settings))
        .route("/api/v1/lights", get(lights_status))
        .route("/api/v1/lights/outputs/refresh", post(lights_refresh))
        .route("/api/v1/lights/settings", post(lights_settings))
        .route(
            "/api/v1/lights/cue-map",
            // A song at the caps is over 60 MB; axum's default is 2 MB (K-01).
            put(lights_cue_map).layer(DefaultBodyLimit::max(CUE_MAP_BODY_LIMIT)),
        )
        .route("/api/v1/lights/pads", put(lights_pads))
        .route("/api/v1/lights/capture", post(lights_capture))
        .route("/api/v1/lights/capture/undo", post(lights_capture_undo))
        .route("/api/v1/lights/test", post(lights_test))
        .route("/api/v1/lights/test-osc", post(lights_test_osc))
        .route("/api/v1/lights/lightkey", get(lightkey_status))
        .route("/api/v1/lights/lightkey/settings", put(lightkey_settings))
        .route("/api/v1/lights/lightkey/simplify", post(lightkey_simplify))
        .route(
            "/api/v1/lights/lightkey/recording",
            post(lightkey_recording),
        )
        .route("/api/v1/lights/lightkey/release", post(lightkey_release))
        .route(
            "/api/v1/lights/lightkey/read-fixtures",
            post(lightkey_read_fixtures),
        )
        .route(
            "/api/v1/lights/lightkey/discovery/scan",
            post(lightkey_discovery_scan),
        )
        .route(
            "/api/v1/lights/lightkey/discovery/use",
            post(lightkey_discovery_use),
        )
        .route(
            "/api/v1/lights/lightkey/discovery/dismiss",
            post(lightkey_discovery_dismiss),
        )
        .route("/api/v1/lights/lightkey/audio", get(lightkey_audio))
        .route(
            "/api/v1/lights/lightkey/audio/meter",
            post(lightkey_audio_meter),
        )
        .route("/api/v1/lights/loudness/clear", post(lights_loudness_clear))
}

fn to_value<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn details(errors: &Errors, prefix: &[Value]) -> Vec<Value> {
    errors
        .iter()
        .map(|e: &Detail| {
            let mut v = to_value(e);
            if let Some(loc) = v.get_mut("loc").and_then(Value::as_array_mut) {
                for (i, p) in prefix.iter().enumerate() {
                    loc.insert(i, p.clone());
                }
            }
            v
        })
        .collect()
}

/// Parse a JSON object request body the way FastAPI does: a non-object body, then a field error,
/// is a 422 with pydantic's `detail` shape.
fn body<T: DeserializeOwned>(raw: &Bytes) -> Result<(T, Value), Json> {
    let parsed: Option<Value> = serde_json::from_slice(raw).ok();
    let Some(value @ Value::Object(_)) = parsed.clone() else {
        return Err(unprocessable(validation::invalid_body(
            raw,
            parsed.as_ref(),
        )));
    };
    match parse_body::<T>(&value) {
        Ok(model) => Ok((model, value)),
        Err(detail) => Err(unprocessable(detail)),
    }
}

fn body_loc(field: &str) -> Vec<Value> {
    vec![json!("body"), json!(field)]
}

fn field_error(kind: &str, field: &str, msg: String, input: Value, ctx: Value) -> Value {
    json!({"type": kind, "loc": body_loc(field), "msg": msg, "input": input, "ctx": ctx})
}

fn range_error(
    errors: &mut Vec<Value>,
    field: &str,
    value: f64,
    ge: Option<f64>,
    le: Option<f64>,
    input: Value,
) {
    let fmt = |n: f64| {
        if n.fract() == 0.0 {
            format!("{}", n as i64)
        } else {
            format!("{n}")
        }
    };
    if let Some(ge) = ge {
        if value < ge {
            errors.push(field_error(
                "greater_than_equal",
                field,
                format!("Input should be greater than or equal to {}", fmt(ge)),
                input,
                json!({"ge": ge as i64}),
            ));
            return;
        }
    }
    if let Some(le) = le {
        if value > le {
            errors.push(field_error(
                "less_than_equal",
                field,
                format!("Input should be less than or equal to {}", fmt(le)),
                input,
                json!({"le": le as i64}),
            ));
        }
    }
}

fn length_error(errors: &mut Vec<Value>, field: &str, text: &str, min: usize, max: usize) {
    let n = text.chars().count();
    if n < min {
        errors.push(field_error(
            "string_too_short",
            field,
            format!(
                "String should have at least {min} character{}",
                if min == 1 { "" } else { "s" }
            ),
            json!(text),
            json!({"min_length": min}),
        ));
    } else if n > max {
        errors.push(field_error(
            "string_too_long",
            field,
            format!(
                "String should have at most {max} character{}",
                if max == 1 { "" } else { "s" }
            ),
            json!(text),
            json!({"max_length": max}),
        ));
    }
}

fn finish(errors: Vec<Value>) -> Result<(), Json> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(unprocessable(errors))
    }
}

// ---- MIDI --------------------------------------------------------------------------------------

async fn midi_inputs(State(rt): State<Rt>) -> Json {
    let Some(midi) = rt.outputs.midi() else {
        let factory = rt.outputs.backends.midi.clone();
        let ports = tokio::task::spawn_blocking(move || {
            stagepilot_midi::plugin::list_system_inputs(&factory)
        })
        .await
        .unwrap_or_default();
        let settings = &rt.settings.midi;
        let mappings: serde_json::Map<String, Value> = settings
            .mappings
            .configured()
            .into_iter()
            .map(|(name, velocity)| (name.to_string(), json!(velocity)))
            .collect();
        return ok(json!({
            "enabled": false,
            "channel": settings.channel,
            "note": settings.note,
            "configured_input_name": settings.input_name,
            "selected_input_name": null,
            // Read-only: the system's ports, so the setup panel is not empty while MIDI is off.
            "inputs": ports,
            "mappings": mappings,
        }));
    };
    ok(to_value(&midi.input_snapshot(true).await))
}

async fn midi_messages(State(rt): State<Rt>) -> Json {
    let messages = match rt.outputs.midi() {
        Some(midi) => midi.recent_messages().await,
        None => Vec::new(),
    };
    ok(to_value(&MidiMonitorResponse { messages }))
}

fn selection_request(raw: &Bytes) -> Result<MidiInputSelectionRequest, Json> {
    let parsed: Option<Value> = serde_json::from_slice(raw).ok();
    let Some(Value::Object(map)) = &parsed else {
        return Err(unprocessable(validation::invalid_body(
            raw,
            parsed.as_ref(),
        )));
    };
    let input = Value::Object(map.clone());
    let valid = |s: &str| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    match map.get("input_id") {
        None => Err(unprocessable(vec![
            json!({"type": "missing", "loc": ["body", "input_id"],
            "msg": "Field required", "input": input}),
        ])),
        Some(Value::Null) => Ok(MidiInputSelectionRequest { input_id: None }),
        Some(Value::String(id)) if valid(id) => Ok(MidiInputSelectionRequest {
            input_id: Some(id.clone()),
        }),
        Some(Value::String(id)) if id.len() != 64 => {
            let kind = if id.len() < 64 {
                ("string_too_short", "at least", "min_length")
            } else {
                ("string_too_long", "at most", "max_length")
            };
            Err(unprocessable(vec![
                json!({"type": kind.0, "loc": ["body", "input_id"],
                "msg": format!("String should have {} 64 characters", kind.1),
                "input": id, "ctx": {kind.2: 64}}),
            ]))
        }
        Some(Value::String(id)) => Err(unprocessable(vec![
            json!({"type": "string_pattern_mismatch",
            "loc": ["body", "input_id"],
            "msg": "String should match pattern '^[a-f0-9]+$'",
            "input": id, "ctx": {"pattern": "^[a-f0-9]+$"}}),
        ])),
        Some(other) => Err(unprocessable(vec![json!({"type": "string_type",
            "loc": ["body", "input_id"],
            "msg": "Input should be a valid string", "input": other})])),
    }
}

async fn midi_input_selection(State(rt): State<Rt>, raw: Bytes) -> Json {
    let request = match selection_request(&raw) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let _serialised = rt.reconcile.lock().await;
    let Some(midi) = rt.outputs.midi() else {
        return http_error(409, MIDI_DISABLED);
    };
    let before = midi.input_snapshot(false).await;
    let current_id = before
        .inputs
        .iter()
        .find(|i| i.selected)
        .map(|i| i.id.clone());
    if request.input_id != current_id && rt.store.snapshot().timer.status == TimerStatus::Running {
        return http_error(409, MIDI_BUSY);
    }
    let outcome = midi.select_input(request.input_id.as_deref()).await;
    if !outcome.accepted {
        return http_error(409, &outcome.message);
    }
    let snapshot = midi.input_snapshot(false).await;
    if let Err(error) = rt
        .settings_service
        .persist_midi_input(snapshot.selected_input_name.clone())
    {
        return http_error(503, &error.0);
    }
    ok(to_value(&MidiInputSelectionResponse {
        accepted: true,
        message: outcome.message,
        midi: snapshot,
    }))
}

async fn midi_cue_simulation(State(rt): State<Rt>, raw: Bytes) -> Json {
    let (request, _): (MidiCueSimulationRequest, Value) = match body(&raw) {
        Ok(parsed) => parsed,
        Err(response) => return response,
    };
    let Some(midi) = rt.outputs.midi() else {
        return http_error(409, MIDI_DISABLED);
    };
    let outcome = midi.simulate_cue(request.cue).await;
    ok(to_value(&MidiCueSimulationResponse {
        cue: request.cue,
        action: cue_action(request.cue),
        accepted: outcome.accepted,
        message: outcome.message,
        state: rt.store.snapshot(),
    }))
}

// ---- ProPresenter ------------------------------------------------------------------------------

fn disabled_status(rt: &Runtime) -> ProPresenterStatusResponse {
    let settings = rt
        .settings_service
        .effective_runtime_settings()
        .propresenter;
    ProPresenterStatusResponse {
        enabled: false,
        host: settings.host,
        port: settings.port,
        timer_name: settings.timer_name,
        look_found: settings.look_id.is_none(),
        look_id: settings.look_id,
        request_timeout_seconds: settings.request_timeout_seconds,
        connection_status: ConnectionStatus::Disconnected,
        detail: Some(PP_DISABLED.into()),
        timers: vec![],
        selected_timer_id: None,
        timer_found: false,
        looks: vec![],
        current_look_id: None,
        last_checked_at: None,
    }
}

async fn propresenter_status(State(rt): State<Rt>) -> Json {
    let status = match rt.outputs.propresenter() {
        Some(plugin) => plugin.snapshot(false).await,
        None => disabled_status(&rt),
    };
    ok(to_value(&status))
}

fn operation(accepted: bool, message: String, status: ProPresenterStatusResponse) -> Json {
    ok(to_value(&ProPresenterOperationResponse {
        accepted,
        message,
        propresenter: status,
    }))
}

async fn propresenter_test(State(rt): State<Rt>) -> Json {
    let Some(plugin) = rt.outputs.propresenter() else {
        return operation(false, PP_DISABLED.into(), disabled_status(&rt));
    };
    let snapshot = plugin.test_connection().await;
    let accepted =
        snapshot.connection_status == ConnectionStatus::Connected && snapshot.timer_found;
    let message = snapshot.detail.clone().unwrap_or_else(|| {
        if accepted {
            "ProPresenter connection test succeeded.".into()
        } else {
            "ProPresenter connection test failed.".into()
        }
    });
    operation(accepted, message, snapshot)
}

async fn propresenter_refresh(State(rt): State<Rt>) -> Json {
    let Some(plugin) = rt.outputs.propresenter() else {
        return operation(false, PP_DISABLED.into(), disabled_status(&rt));
    };
    let snapshot = plugin.refresh_timers().await;
    let accepted =
        snapshot.connection_status == ConnectionStatus::Connected && snapshot.timer_found;
    let message = snapshot
        .detail
        .clone()
        .unwrap_or_else(|| "ProPresenter timers refreshed.".into());
    operation(accepted, message, snapshot)
}

fn validate_pp_request(request: &ProPresenterSettingsRequest, raw: &Value) -> Result<(), Json> {
    let mut errors = Vec::new();
    length_error(&mut errors, "host", &request.host, 1, 255);
    length_error(&mut errors, "timer_name", &request.timer_name, 1, 255);
    range_error(
        &mut errors,
        "port",
        f64::from(request.port),
        Some(1.0),
        Some(65535.0),
        json!(request.port),
    );
    if let Some(look) = &request.look_id {
        let n = look.chars().count();
        if n > 256 {
            errors.push(field_error(
                "string_too_long",
                "look_id",
                "String should have at most 256 characters".into(),
                json!(look),
                json!({"max_length": 256}),
            ));
        }
    }
    let t = request.request_timeout_seconds;
    if t <= 0.0 {
        errors.push(field_error(
            "greater_than",
            "request_timeout_seconds",
            "Input should be greater than 0".into(),
            raw["request_timeout_seconds"].clone(),
            json!({"gt": 0}),
        ));
    } else if t > 60.0 {
        errors.push(field_error(
            "less_than_equal",
            "request_timeout_seconds",
            "Input should be less than or equal to 60".into(),
            raw["request_timeout_seconds"].clone(),
            json!({"le": 60.0}),
        ));
    }
    finish(errors)
}

async fn propresenter_settings(State(rt): State<Rt>, raw: Bytes) -> Json {
    if let Ok(value) = serde_json::from_slice::<Value>(&raw) {
        let Some(map) = value.as_object() else {
            return propresenter_settings_body(rt, raw).await;
        };
        let missing: Vec<Value> = ["host", "port", "timer_name", "request_timeout_seconds"]
            .iter()
            .filter(|k| !map.contains_key(**k))
            .map(|k| json!({"type": "missing", "loc": ["body", k], "msg": "Field required", "input": value}))
            .collect();
        if !missing.is_empty() {
            return unprocessable(missing);
        }
    }
    propresenter_settings_body(rt, raw).await
}

async fn propresenter_settings_body(rt: Rt, raw: Bytes) -> Json {
    let (request, value): (ProPresenterSettingsRequest, Value) = match body(&raw) {
        Ok(parsed) => parsed,
        Err(response) => return response,
    };
    if let Err(response) = validate_pp_request(&request, &value) {
        return response;
    }
    let _serialised = rt.reconcile.lock().await;
    let current = rt
        .settings_service
        .effective_runtime_settings()
        .propresenter;
    let mut updated = ProPresenterSettings {
        enabled: current.enabled,
        host: request.host,
        port: request.port,
        timer_name: request.timer_name,
        look_id: request.look_id,
        request_timeout_seconds: request.request_timeout_seconds,
        reconnect_initial_seconds: current.reconnect_initial_seconds,
        reconnect_max_seconds: current.reconnect_max_seconds,
        health_check_interval_seconds: current.health_check_interval_seconds,
    };
    updated.normalise();
    let mut errors = Vec::new();
    updated.validate(&[], &mut errors);
    if !errors.is_empty() {
        return unprocessable(details(&errors, &[]));
    }
    let Some(mut plugin) = rt.outputs.propresenter() else {
        if let Err(error) = rt.settings_service.persist_propresenter(updated) {
            return http_error(503, &error.0);
        }
        // Saved settings that enable the output start it right now; otherwise they are just kept.
        let warning = match rt.apply_presentation().await {
            Ok(warning) => warning,
            Err(message) => return http_error(409, &message),
        };
        return match rt.outputs.propresenter() {
            Some(started) => {
                let snapshot = started.snapshot(false).await;
                finish_pp_settings(&rt, started, snapshot, None).await
            }
            None => operation(
                warning.is_none(),
                warning.unwrap_or_else(|| "ProPresenter settings saved.".into()),
                disabled_status(&rt),
            ),
        };
    };
    // The same settings as the saved ones and the running ones, with the output connected: there
    // is nothing to apply. No reconnect, no Look triggered again (it would change the screen),
    // nothing written. The dashboard does this on every page load (F-P1).
    if updated == current && plugin.is_running_with(&updated) {
        let snapshot = plugin.snapshot(false).await;
        return finish_pp_settings(&rt, plugin, snapshot, None).await;
    }
    let snapshot = plugin.reconfigure(updated.clone()).await;
    // Apply the Look before saving it: a Look ProPresenter does not know is reported but never
    // kept, so the previous Look stays saved.
    let look_outcome = match updated.look_id.clone() {
        Some(look) => {
            let result = plugin.apply_look(&look).await;
            if matches!(
                result,
                Err(stagepilot_propresenter::PpError::LookNotFound(_))
            ) {
                updated.look_id = current.look_id.clone();
                plugin.set_look_id(current.look_id.clone());
            }
            Some(result)
        }
        None => None,
    };
    rt.outputs.adopt_presentation(updated.clone());
    if let Err(error) = rt.settings_service.persist_propresenter(updated.clone()) {
        return http_error(503, &error.0);
    }
    plugin = rt.outputs.propresenter().unwrap_or(plugin);
    finish_pp_settings(&rt, plugin, snapshot, look_outcome).await
}

async fn finish_pp_settings(
    rt: &Runtime,
    plugin: Arc<stagepilot_propresenter::ProPresenterPlugin>,
    mut snapshot: ProPresenterStatusResponse,
    look_outcome: Option<Result<ProPresenterStatusResponse, stagepilot_propresenter::PpError>>,
) -> Json {
    let _ = rt;
    let look_id = match look_outcome {
        None => None,
        Some(Ok(applied)) => {
            let applied_look = applied.look_id.clone();
            snapshot = applied;
            applied_look
        }
        Some(Err(error)) if error.is_propresenter_error() => {
            let snapshot = plugin.snapshot(false).await;
            return operation(
                false,
                format!("ProPresenter settings were saved, but the Look was not applied: {error}"),
                snapshot,
            );
        }
        Some(Err(error)) => return http_error(500, &error.to_string()),
    };
    let accepted = snapshot.connection_status == ConnectionStatus::Connected
        && snapshot.timer_found
        && snapshot.look_found
        && look_id
            .as_ref()
            .is_none_or(|look| snapshot.current_look_id.as_ref() == Some(look));
    let message = snapshot.detail.clone().unwrap_or_else(|| {
        if accepted {
            "ProPresenter session settings applied.".into()
        } else {
            "ProPresenter session settings were saved, but readiness checks failed.".into()
        }
    });
    operation(accepted, message, snapshot)
}

// ---- lights ------------------------------------------------------------------------------------

async fn lights_snapshot(rt: &Runtime, refresh: bool) -> LightsStatusResponse {
    rt.outputs.lights.snapshot(refresh).await
}

async fn lights_operation(rt: &Runtime, accepted: bool, message: String) -> Json {
    ok(to_value(&LightsOperationResponse {
        accepted,
        message,
        lights: lights_snapshot(rt, false).await,
    }))
}

async fn lights_status(State(rt): State<Rt>) -> Json {
    ok(to_value(&lights_snapshot(&rt, false).await))
}

async fn lights_refresh(State(rt): State<Rt>) -> Json {
    ok(to_value(&lights_snapshot(&rt, true).await))
}

fn current_lights(rt: &Runtime) -> LightsSettings {
    rt.settings_service.effective_runtime_settings().lights
}

fn persist_lights(rt: &Runtime, updated: LightsSettings) -> Result<(), Json> {
    rt.settings_service
        .persist_lights(updated)
        .map_err(|error| http_error(503, &error.0))
}

fn validate_lights_request(request: &LightsSettingsRequest) -> Result<(), Json> {
    let mut errors = Vec::new();
    if let Some(host) = &request.osc_host {
        length_error(&mut errors, "osc_host", host, 1, 253);
    }
    if let Some(port) = request.osc_port {
        range_error(
            &mut errors,
            "osc_port",
            f64::from(port),
            Some(1.0),
            Some(65535.0),
            json!(port),
        );
    }
    if let Some(name) = &request.output_name {
        let n = name.chars().count();
        if n > 512 {
            errors.push(field_error(
                "string_too_long",
                "output_name",
                "String should have at most 512 characters".into(),
                json!(name),
                json!({"max_length": 512}),
            ));
        }
    }
    range_error(
        &mut errors,
        "channel",
        f64::from(request.channel),
        Some(1.0),
        Some(15.0),
        json!(request.channel),
    );
    range_error(
        &mut errors,
        "pulse_ms",
        f64::from(request.pulse_ms),
        Some(10.0),
        Some(2000.0),
        json!(request.pulse_ms),
    );
    finish(errors)
}

async fn lights_settings(State(rt): State<Rt>, raw: Bytes) -> Json {
    detached(lights_settings_work(rt, raw)).await
}

async fn lights_settings_work(rt: Rt, raw: Bytes) -> Json {
    let (request, _): (LightsSettingsRequest, Value) = match body(&raw) {
        Ok(parsed) => parsed,
        Err(response) => return response,
    };
    if let Err(response) = validate_lights_request(&request) {
        return response;
    }
    // Serialize with the recording sink, capture, undo and simplify (C-01): all of them read,
    // merge and persist the whole lights settings.
    let _lock = rt.outputs.cue_lock.lock().await;
    let current = current_lights(&rt);
    // MIDI and OSC are independent: saving one method never touches the other's settings. A MIDI
    // save cannot overwrite the saved OSC host and port, and an OSC save cannot reset the saved
    // MIDI output, channel or note length.
    let use_osc = request.method == LightingMethod::Osc;
    let mut updated = LightsSettings {
        enabled: request.enabled,
        method: request.method,
        output_name: if use_osc {
            current.output_name.clone()
        } else {
            request.output_name.clone()
        },
        channel: if use_osc {
            current.channel
        } else {
            request.channel
        },
        pulse_ms: if use_osc {
            current.pulse_ms
        } else {
            request.pulse_ms
        },
        osc_host: if use_osc {
            request
                .osc_host
                .clone()
                .filter(|h| !h.is_empty())
                .unwrap_or_else(|| current.osc_host.clone())
        } else {
            current.osc_host.clone()
        },
        osc_port: if use_osc {
            request
                .osc_port
                .filter(|p| *p != 0)
                .unwrap_or(current.osc_port)
        } else {
            current.osc_port
        },
        cue_maps: current.cue_maps.clone(),
        pads: current.pads.clone(),
        transport: current.transport,
        // The Lightkey block is not part of this request: carry it over untouched.
        lightkey: current.lightkey.clone(),
        cue_revisions: current.cue_revisions.clone(),
    };
    updated.normalise();
    let mut errors = Vec::new();
    updated.validate(&[], &mut errors);
    if !errors.is_empty() {
        return unprocessable(details(&errors, &[]));
    }
    // The same as what is saved: nothing to write. The plugin itself decides whether what is
    // running already matches (and then touches nothing).
    if updated != current {
        if let Err(response) = persist_lights(&rt, updated.clone()) {
            return response;
        }
    }
    let outcome = rt.outputs.lights.reconfigure(updated).await;
    lights_operation(&rt, outcome.accepted, outcome.message).await
}

async fn lights_cue_map(State(rt): State<Rt>, raw: Bytes) -> Json {
    let (request, value): (LightingCueMapRequest, Value) = match body(&raw) {
        Ok(parsed) => parsed,
        Err(response) => return response,
    };
    let mut errors = Vec::new();
    let song_key = request.song_key.trim().to_string();
    let song_title = request.song_title.trim().to_string();
    for (field, text, max, original) in [
        ("song_key", &song_key, 256, &request.song_key),
        ("song_title", &song_title, 500, &request.song_title),
    ] {
        if text.is_empty() {
            errors.push(field_error(
                "value_error",
                field,
                "Value error, Value cannot be empty.".into(),
                json!(original),
                json!({"error": {}}),
            ));
        } else {
            length_error(&mut errors, field, text, 1, max);
        }
    }
    if let Err(response) = finish(errors) {
        return response;
    }
    let mut errors = Vec::new();
    for (i, cue) in request.osc_cues.iter().enumerate() {
        if !cue.messages.is_empty() {
            continue; // a look row sends its messages; the shared validation below checks them
        }
        let address = cue.address.trim();
        if let Err(message) = stagepilot_core::config::validate_osc_address(address) {
            errors.push(
                json!({"type": "value_error", "loc": ["body", "osc_cues", i, "address"],
                "msg": format!("Value error, {message}"), "input": address, "ctx": {"error": {}}}),
            );
        }
    }
    if let Err(response) = finish(errors) {
        return response;
    }
    // Validate the submitted cue lists with the same rules saved settings use.
    let probe = SongLightingCueMap {
        song_key: song_key.clone(),
        song_title: song_title.clone(),
        cues: request.cues.clone(),
        osc_cues: request.osc_cues.clone(),
        loudness: None,
        captured_edited: false,
        capture: None,
        effects: Vec::new(),
    };
    let mut check = LightsSettings::default();
    check.cue_maps.insert(song_key.clone(), probe);
    check.normalise();
    let mut errors = Vec::new();
    check.validate(&[], &mut errors);
    if !errors.is_empty() {
        let mut out = details(&errors, &[json!("body")]);
        for e in &mut out {
            if let Some(loc) = e.get_mut("loc").and_then(Value::as_array_mut) {
                if loc.get(1) == Some(&json!("cue_maps")) && loc.len() > 3 {
                    loc.drain(1..3);
                }
            }
        }
        return unprocessable(out);
    }
    let submitted = check
        .cue_maps
        .remove(&song_key)
        .unwrap_or(SongLightingCueMap {
            song_key: song_key.clone(),
            song_title: song_title.clone(),
            cues: vec![],
            osc_cues: vec![],
            loudness: None,
            captured_edited: false,
            capture: None,
            effects: Vec::new(),
        });
    let _ = value;

    let _lock = rt.outputs.cue_lock.lock().await;
    let current = current_lights(&rt);
    if let Some(sent) = request.revision {
        let saved = current.cue_revisions.get(&song_key).copied().unwrap_or(0);
        if sent != saved {
            return http_error(
                409,
                "This song's lights changed since you opened them (a recording finished or another edit was saved). Reload and try again.",
            );
        }
    }
    let existing = current.cue_maps.get(&song_key);
    // Replace only the submitted method's cues; the other method's cues are kept.
    let is_midi = request.method == LightingMethod::Midi;
    let merged = SongLightingCueMap {
        song_key: song_key.clone(),
        song_title: song_title.clone(),
        cues: if is_midi {
            submitted.cues.clone()
        } else {
            existing.map(|m| m.cues.clone()).unwrap_or_default()
        },
        osc_cues: if request.method == LightingMethod::Osc {
            submitted.osc_cues.clone()
        } else {
            existing.map(|m| m.osc_cues.clone()).unwrap_or_default()
        },
        loudness: existing.and_then(|m| m.loudness.clone()),
        captured_edited: false,
        capture: None,
        // Editing rows never changes or drops effects; only an explicit remove list (OSC) does.
        effects: existing
            .map(|m| {
                let remove = request.remove_effect_ids.as_deref().unwrap_or(&[]);
                m.effects
                    .iter()
                    .filter(|e| is_midi || !remove.iter().any(|id| *id == e.id.to_string()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default(),
    };
    let merged = SongLightingCueMap {
        captured_edited: captured_edited_after_save(existing, &merged, is_midi),
        capture: existing.and_then(|m| m.capture.clone()),
        ..merged
    };
    let merged = without_stale_capture(merged);
    let mut updated = current.clone();
    if !merged.cues.is_empty()
        || !merged.osc_cues.is_empty()
        || !merged.effects.is_empty()
        || merged.loudness.is_some()
    {
        updated.cue_maps.insert(song_key.clone(), merged.clone());
    } else {
        updated.cue_maps.remove(&song_key);
    }
    if let Err(response) = persist_lights(&rt, updated) {
        return response;
    }
    rt.outputs.lights.replace_cue_map(merged).await;
    let count = if is_midi {
        submitted.cues.len()
    } else {
        submitted.osc_cues.len()
    };
    let message = if count > 0 {
        format!("Saved {count} lighting cues for \"{song_title}\".")
    } else {
        format!("Removed the lighting cue map for \"{song_title}\".")
    };
    lights_operation(&rt, true, message).await
}

/// A capture status describes captured rows; with none left (cleared, deleted, undone) it is gone.
fn without_stale_capture(mut map: SongLightingCueMap) -> SongLightingCueMap {
    if !map.has_captured_rows() {
        map.capture = None;
    }
    map
}

/// The song-level "captured looks were changed by hand" flag after a cue-map save. Deleting a
/// whole captured row leaves no row to carry the per-row `edited` mark, so the flag keeps it. It
/// stays set until a recording replaces the captured rows, and clears when none are left.
fn captured_edited_after_save(
    existing: Option<&SongLightingCueMap>,
    saved: &SongLightingCueMap,
    midi_only: bool,
) -> bool {
    let Some(existing) = existing else {
        return false;
    };
    let captured = |map: &SongLightingCueMap| {
        map.osc_cues
            .iter()
            .filter(|row| row.source == stagepilot_core::config::OscCueSource::Captured)
            .map(|row| row.id)
            .collect::<std::collections::BTreeSet<_>>()
    };
    let before = captured(existing);
    let after = captured(saved);
    if after.is_empty() {
        return false;
    }
    existing.captured_edited || (!midi_only && before.iter().any(|id| !after.contains(id)))
}

async fn lights_pads(State(rt): State<Rt>, raw: Bytes) -> Json {
    let (request, value): (LightingPadsRequest, Value) = match body(&raw) {
        Ok(parsed) => parsed,
        Err(response) => return response,
    };
    if request.pads.len() > 64 {
        return unprocessable(vec![json!({"type": "too_long", "loc": ["body", "pads"],
            "msg": format!("List should have at most 64 items after validation, not {}", request.pads.len()),
            "input": value["pads"], "ctx": {"field_type": "List", "max_length": 64, "actual_length": request.pads.len()}})]);
    }
    let ids: std::collections::BTreeSet<_> = request.pads.iter().map(|p| p.id).collect();
    if ids.len() != request.pads.len() {
        let mut errors = Vec::new();
        value_error(
            &mut errors,
            &[json!("body")],
            "Every lighting pad must have a unique ID.",
            value.clone(),
        );
        return unprocessable(details(&errors, &[]));
    }
    let _lock = rt.outputs.cue_lock.lock().await;
    let current = current_lights(&rt);
    let mut updated = LightsSettings {
        pads: request.pads.clone(),
        ..current
    };
    updated.normalise();
    let mut errors = Vec::new();
    updated.validate(&[], &mut errors);
    if !errors.is_empty() {
        let mut out = details(&errors, &[json!("body")]);
        for e in &mut out {
            let idx = e["loc"].get(2).and_then(Value::as_u64);
            if let (Some(idx), true) = (idx, e["input"].is_null()) {
                e["input"] = value["pads"][idx as usize].clone();
            }
            if e["loc"].get(1) == Some(&json!("pads"))
                && e["loc"].as_array().is_some_and(|l| l.len() == 3)
            {
                // already body.pads.N
            }
        }
        return unprocessable(out);
    }
    if let Err(response) = persist_lights(&rt, updated) {
        return response;
    }
    lights_operation(
        &rt,
        true,
        format!("Saved {} lighting pads.", request.pads.len()),
    )
    .await
}

async fn lights_capture(State(rt): State<Rt>, raw: Bytes) -> Json {
    let (request, value): (LightingCaptureRequest, Value) = match body(&raw) {
        Ok(parsed) => parsed,
        Err(response) => return response,
    };
    let mut errors = Vec::new();
    if let Some(key) = &request.song_key {
        length_error(&mut errors, "song_key", key, 1, 256);
    }
    if let Some(title) = &request.song_title {
        length_error(&mut errors, "song_title", title, 1, 500);
    }
    if let Some(at) = request.at_seconds {
        range_error(
            &mut errors,
            "at_seconds",
            at,
            Some(0.0),
            Some(86_399.0),
            json!(at),
        );
    }
    if let Err(response) = finish(errors) {
        return response;
    }
    let present = [
        request.song_key.is_some(),
        request.song_title.is_some(),
        request.at_seconds.is_some(),
    ];
    if present.iter().any(|p| *p) && present.iter().any(|p| !*p) {
        let mut errors = Vec::new();
        value_error(
            &mut errors,
            &[json!("body")],
            "A capture needs song_key, song_title and at_seconds together.",
            value,
        );
        return unprocessable(details(&errors, &[]));
    }
    let _lock = rt.outputs.cue_lock.lock().await;
    let current = current_lights(&rt);
    let Some(pad) = current
        .pads
        .iter()
        .find(|p| p.id == request.pad_id)
        .cloned()
    else {
        return http_error(404, "That pad no longer exists.");
    };
    if pad.method != current.method {
        return http_error(
            409,
            &format!(
                "This pad is for {}; Lighting is set to {}.",
                method_name(pad.method),
                method_name(current.method)
            ),
        );
    }
    let fired = match pad.method {
        LightingMethod::Osc => {
            rt.outputs
                .lights
                .test_osc_cue(pad.address.as_deref().unwrap_or(""), pad.args.clone())
                .await
        }
        LightingMethod::Midi => {
            rt.outputs
                .lights
                .test_cue(pad.note.unwrap_or(0), pad.velocity)
                .await
        }
    };
    let mut cue_id: Option<Uuid> = None;
    let mut at_seconds: Option<f64> = None;
    if let (Some(song_key), Some(song_title), Some(at)) =
        (&request.song_key, &request.song_title, request.at_seconds)
    {
        // Read the cue map again after the send: a timer restart or a cue-map save may have
        // landed while the cue was going out, and there is no await from here to the write.
        let current = current_lights(&rt);
        let existing = current.cue_maps.get(song_key);
        let mut midi_cues = existing.map(|m| m.cues.clone()).unwrap_or_default();
        let mut osc_cues = existing.map(|m| m.osc_cues.clone()).unwrap_or_default();
        let at = round_tenths(at);
        match pad.method {
            LightingMethod::Osc => {
                let cue = OscLightingCue {
                    id: Uuid::new_v4(),
                    at_seconds: at,
                    address: pad.address.clone().unwrap_or_default(),
                    args: pad.args.clone(),
                    label: pad.label.clone(),
                    color: pad.color,
                    messages: Vec::new(),
                    source: Default::default(),
                    edited: false,
                    fade_seconds: 0.0,
                };
                cue_id = Some(cue.id);
                osc_cues.push(cue);
            }
            LightingMethod::Midi => {
                let cue = LightingCue {
                    id: Uuid::new_v4(),
                    at_seconds: at,
                    note: pad.note.unwrap_or(0),
                    velocity: pad.velocity,
                    label: pad.label.clone(),
                    color: pad.color,
                };
                cue_id = Some(cue.id);
                midi_cues.push(cue);
            }
        }
        at_seconds = Some(at);
        let mut updated = current.clone();
        updated.cue_maps.insert(
            song_key.clone(),
            SongLightingCueMap {
                song_key: song_key.trim().to_string(),
                song_title: song_title.trim().to_string(),
                cues: midi_cues,
                osc_cues,
                loudness: existing.and_then(|m| m.loudness.clone()),
                captured_edited: current
                    .cue_maps
                    .get(song_key)
                    .is_some_and(|m| m.captured_edited),
                capture: existing.and_then(|m| m.capture.clone()),
                effects: existing.map(|m| m.effects.clone()).unwrap_or_default(),
            },
        );
        updated.normalise();
        let merged = updated.cue_maps.get(song_key).cloned();
        if let Err(response) = persist_lights(&rt, updated) {
            return response;
        }
        if let Some(merged) = merged {
            rt.outputs.lights.replace_cue_map(merged).await;
        }
    }
    ok(to_value(&LightingCaptureResponse {
        accepted: fired.accepted,
        message: fired.message,
        lights: lights_snapshot(&rt, false).await,
        cue_id,
        at_seconds,
    }))
}

fn method_name(method: LightingMethod) -> &'static str {
    match method {
        LightingMethod::Osc => "OSC",
        LightingMethod::Midi => "MIDI",
    }
}

async fn lights_capture_undo(State(rt): State<Rt>, raw: Bytes) -> Json {
    let (request, _): (LightingCaptureUndoRequest, Value) = match body(&raw) {
        Ok(parsed) => parsed,
        Err(response) => return response,
    };
    let mut errors = Vec::new();
    length_error(&mut errors, "song_key", &request.song_key, 1, 256);
    if let Err(response) = finish(errors) {
        return response;
    }
    let _lock = rt.outputs.cue_lock.lock().await;
    let current = current_lights(&rt);
    let Some(existing) = current.cue_maps.get(&request.song_key).cloned() else {
        return lights_operation(&rt, false, "Nothing to undo.".into()).await;
    };
    let cues: Vec<_> = existing
        .cues
        .iter()
        .filter(|c| c.id != request.cue_id)
        .cloned()
        .collect();
    let osc_cues: Vec<_> = existing
        .osc_cues
        .iter()
        .filter(|c| c.id != request.cue_id)
        .cloned()
        .collect();
    if cues.len() == existing.cues.len() && osc_cues.len() == existing.osc_cues.len() {
        return lights_operation(
            &rt,
            false,
            "That capture was already changed or removed.".into(),
        )
        .await;
    }
    let merged = SongLightingCueMap {
        song_key: existing.song_key.clone(),
        song_title: existing.song_title.clone(),
        cues,
        osc_cues,
        loudness: existing.loudness.clone(),
        captured_edited: existing.captured_edited,
        capture: existing.capture.clone(),
        effects: existing.effects.clone(),
    };
    let merged = without_stale_capture(merged);
    let mut updated = current.clone();
    if !merged.cues.is_empty()
        || !merged.osc_cues.is_empty()
        || !merged.effects.is_empty()
        || merged.loudness.is_some()
    {
        updated
            .cue_maps
            .insert(request.song_key.clone(), merged.clone());
    } else {
        updated.cue_maps.remove(&request.song_key);
    }
    if let Err(response) = persist_lights(&rt, updated) {
        return response;
    }
    rt.outputs.lights.replace_cue_map(merged).await;
    lights_operation(&rt, true, "Capture undone.".into()).await
}

async fn lights_test(State(rt): State<Rt>, raw: Bytes) -> Json {
    let (request, _): (LightingCueTestRequest, Value) = match body(&raw) {
        Ok(parsed) => parsed,
        Err(response) => return response,
    };
    let mut errors = Vec::new();
    range_error(
        &mut errors,
        "note",
        f64::from(request.note),
        Some(0.0),
        Some(127.0),
        json!(request.note),
    );
    range_error(
        &mut errors,
        "velocity",
        f64::from(request.velocity),
        Some(1.0),
        Some(127.0),
        json!(request.velocity),
    );
    if let Err(response) = finish(errors) {
        return response;
    }
    let outcome = rt
        .outputs
        .lights
        .test_cue(request.note, request.velocity)
        .await;
    lights_operation(&rt, outcome.accepted, outcome.message).await
}

async fn lights_test_osc(State(rt): State<Rt>, raw: Bytes) -> Json {
    let (request, value): (LightingOscCueTestRequest, Value) = match body(&raw) {
        Ok(parsed) => parsed,
        Err(response) => return response,
    };
    let mut errors = Vec::new();
    length_error(&mut errors, "address", &request.address, 1, 256);
    if request.args.len() > 16 {
        errors.push(field_error(
            "too_long",
            "args",
            format!(
                "List should have at most 16 items after validation, not {}",
                request.args.len()
            ),
            value["args"].clone(),
            json!({"field_type": "List", "max_length": 16, "actual_length": request.args.len()}),
        ));
    }
    if let Err(response) = finish(errors) {
        return response;
    }
    if let Err(message) = stagepilot_core::config::validate_osc_address(request.address.trim()) {
        return unprocessable(vec![
            json!({"type": "value_error", "loc": ["body", "address"],
            "msg": format!("Value error, {message}"), "input": request.address, "ctx": {"error": {}}}),
        ]);
    }
    let outcome = rt
        .outputs
        .lights
        .test_osc_cue(request.address.trim(), request.args)
        .await;
    lights_operation(&rt, outcome.accepted, outcome.message).await
}

// ---- Lightkey ----------------------------------------------------------------------------------

async fn lightkey_response(rt: &Runtime, accepted: bool, message: String) -> Json {
    ok(to_value(&LightkeyActionResponse {
        accepted,
        message,
        lightkey: rt.outputs.lights.lightkey_status().await,
    }))
}

async fn lightkey_status(State(rt): State<Rt>) -> Json {
    ok(to_value(&rt.outputs.lights.lightkey_status().await))
}

/// `LightkeySettingsRequest`: every field optional, send only what changed.
struct LightkeySettingsRequest {
    api_url: Option<String>,
    replay: Option<bool>,
    mode: Option<stagepilot_core::config::LightkeyMode>,
    detail: Option<stagepilot_core::config::LightkeyDetail>,
    /// `Some("")` clears the choice (system default input).
    audio_device: Option<String>,
}

/// pydantic's lax `bool`: `true`/`false`, `0`/`1`, and the strings pydantic accepts. `Err` is the
/// pydantic error for a value that is not a boolean at all (`bool_type`) or a string/number that
/// cannot be read as one (`bool_parsing`).
fn lax_bool(field: &str, value: &Value) -> Result<bool, Value> {
    let parsing = || {
        Err(field_error(
            "bool_parsing",
            field,
            "Input should be a valid boolean, unable to interpret input".into(),
            value.clone(),
            Value::Null,
        ))
    };
    match value {
        Value::Bool(b) => Ok(*b),
        Value::Number(n) => match n.as_f64() {
            Some(0.0) => Ok(false),
            Some(1.0) => Ok(true),
            _ => parsing(),
        },
        Value::String(text) => match text.to_lowercase().as_str() {
            "0" | "off" | "f" | "false" | "n" | "no" => Ok(false),
            "1" | "on" | "t" | "true" | "y" | "yes" => Ok(true),
            _ => parsing(),
        },
        other => Err(field_error(
            "bool_type",
            field,
            "Input should be a valid boolean".into(),
            other.clone(),
            Value::Null,
        )),
    }
}

/// `field_error` without a `ctx` key, as pydantic omits it for these types.
fn plain_error(kind: &str, field: &str, msg: &str, input: &Value) -> Value {
    json!({"type": kind, "loc": body_loc(field), "msg": msg, "input": input})
}

fn parse_lightkey_settings(
    object: &serde_json::Map<String, Value>,
) -> Result<LightkeySettingsRequest, Vec<Value>> {
    let mut errors = Vec::new();
    let api_url = match object.get("api_url") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => Some(text.clone()),
        Some(other) => {
            errors.push(plain_error(
                "string_type",
                "api_url",
                "Input should be a valid string",
                other,
            ));
            None
        }
    };
    let replay = match object.get("replay") {
        None | Some(Value::Null) => None,
        Some(value) => match lax_bool("replay", value) {
            Ok(b) => Some(b),
            Err(error) => {
                errors.push(strip_ctx(error));
                None
            }
        },
    };
    // The frontend sends `detail_level` (status field of the same name); `detail` stays accepted.
    let mode = match object.get("mode") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => match text.as_str() {
            "auto" => Some(stagepilot_core::config::LightkeyMode::Auto),
            "replay" => Some(stagepilot_core::config::LightkeyMode::Replay),
            "record" => Some(stagepilot_core::config::LightkeyMode::Record),
            _ => {
                errors.push(plain_error(
                    "enum",
                    "mode",
                    "Input should be 'auto', 'replay' or 'record'",
                    &json!(text),
                ));
                None
            }
        },
        Some(other) => {
            errors.push(plain_error(
                "enum",
                "mode",
                "Input should be 'auto', 'replay' or 'record'",
                other,
            ));
            None
        }
    };
    let detail_key = if object.contains_key("detail_level") {
        "detail_level"
    } else {
        "detail"
    };
    let detail = match object.get(detail_key) {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => match text.as_str() {
            "fine" => Some(stagepilot_core::config::LightkeyDetail::Fine),
            "normal" => Some(stagepilot_core::config::LightkeyDetail::Normal),
            "coarse" => Some(stagepilot_core::config::LightkeyDetail::Coarse),
            _ => {
                errors.push(plain_error(
                    "enum",
                    detail_key,
                    "Input should be 'fine', 'normal' or 'coarse'",
                    &json!(text),
                ));
                None
            }
        },
        Some(other) => {
            errors.push(plain_error(
                "enum",
                detail_key,
                "Input should be 'fine', 'normal' or 'coarse'",
                other,
            ));
            None
        }
    };
    let audio_device = match object.get("audio_device") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) if text.chars().count() <= 200 => Some(text.clone()),
        Some(Value::String(text)) => {
            errors.push(plain_error(
                "string_too_long",
                "audio_device",
                "String should have at most 200 characters",
                &json!(text),
            ));
            None
        }
        Some(other) => {
            errors.push(plain_error(
                "string_type",
                "audio_device",
                "Input should be a valid string",
                other,
            ));
            None
        }
    };
    if errors.is_empty() {
        Ok(LightkeySettingsRequest {
            api_url,
            replay,
            mode,
            detail,
            audio_device,
        })
    } else {
        Err(errors)
    }
}

fn strip_ctx(mut error: Value) -> Value {
    if let Some(map) = error.as_object_mut() {
        map.remove("ctx");
    }
    error
}

/// The 422 of a bad address: `exc.errors(include_url=False, include_context=False)`, the "no
/// context" form (Python's default detail held a non-serialisable exception object). The `loc` is
/// the model's own (`["api_url"]`), without a `body` prefix, because the error comes from building
/// `LightkeySettings` inside the route, not from request parsing.
fn lightkey_validation_error(raw: &str, settings: &LightkeySettings) -> Option<Json> {
    let mut errors = Errors::new();
    settings.validate(&[], &mut errors);
    if errors.is_empty() {
        return None;
    }
    let detail: Vec<Value> = errors
        .into_iter()
        .map(|mut e| {
            e.ctx = None;
            if e.kind == "value_error" {
                e.input = json!(raw);
            }
            to_value(&e)
        })
        .collect();
    Some(unprocessable(detail))
}

async fn lightkey_settings(State(rt): State<Rt>, raw: Bytes) -> Json {
    detached(lightkey_settings_work(rt, raw)).await
}

async fn lightkey_settings_work(rt: Rt, raw: Bytes) -> Json {
    let request = match object_body(&raw) {
        Ok(object) => match parse_lightkey_settings(&object) {
            Ok(request) => request,
            Err(errors) => return unprocessable(errors),
        },
        Err(response) => return response,
    };
    let mut errors = Vec::new();
    if let Some(url) = &request.api_url {
        length_error(&mut errors, "api_url", url, 0, 300);
    }
    if let Err(response) = finish(errors) {
        return response;
    }
    let _lock = rt.outputs.cue_lock.lock().await;
    let current = current_lights(&rt);
    let api_url = request
        .api_url
        .clone()
        .unwrap_or_else(|| current.lightkey.api_url.clone());
    let candidate = LightkeySettings {
        api_url: stagepilot_core::config::clean_lightkey_api_url(&api_url)
            .unwrap_or_else(|_| api_url.clone()),
        // Choosing Auto or Replay also turns the master Replay switch back on, unless the request
        // says otherwise: "replay: false" is the one thing that silences every mode.
        replay: request.replay.unwrap_or(
            current.lightkey.replay
                || matches!(
                    request.mode,
                    Some(LightkeyMode::Auto | LightkeyMode::Replay)
                ),
        ),
        // Record is the Record switch (below), never a saved mode.
        mode: match request.mode {
            Some(mode @ (LightkeyMode::Auto | LightkeyMode::Replay)) => mode,
            _ => current.lightkey.mode,
        },
        detail: request.detail.unwrap_or(current.lightkey.detail),
        audio_device: match &request.audio_device {
            Some(name) => Some(name.trim().to_string()).filter(|name| !name.is_empty()),
            None => current.lightkey.audio_device.clone(),
        },
        // Written by the search only; a settings save keeps it.
        found_url: current.lightkey.found_url.clone(),
    };
    if let Some(response) = lightkey_validation_error(&api_url, &candidate) {
        return response;
    }
    let lightkey = candidate;
    let updated = LightsSettings {
        lightkey: lightkey.clone(),
        ..current
    };
    if let Err(response) = persist_lights(&rt, updated) {
        return response;
    }
    // The recorder's save takes this lock too: let go before touching it.
    drop(_lock);
    rt.outputs.lights.lightkey_apply_settings(lightkey).await;
    // Pinning Record arms the recorder; pinning Auto or Replay disarms it.
    let armed = rt.outputs.lights.lightkey_status().await.recording;
    match request.mode {
        Some(LightkeyMode::Record) if !armed => {
            let message = rt.outputs.lights.lightkey_set_recording(true).await;
            let recording = rt.outputs.lights.lightkey_status().await.recording;
            return lightkey_response(&rt, recording, message).await;
        }
        Some(LightkeyMode::Auto | LightkeyMode::Replay) if armed => {
            rt.outputs.lights.lightkey_set_recording(false).await;
        }
        _ => {}
    }
    lightkey_response(&rt, true, "Saved.".into()).await
}

/// A request body that must be a JSON object (FastAPI's 422 otherwise).
fn object_body(raw: &Bytes) -> Result<serde_json::Map<String, Value>, Json> {
    let parsed: Option<Value> = serde_json::from_slice(raw).ok();
    match parsed {
        Some(Value::Object(map)) => Ok(map),
        other => Err(unprocessable(validation::invalid_body(raw, other.as_ref()))),
    }
}

/// "Release lights": give every fixture the replay holds back to Lightkey.
async fn lightkey_release(State(rt): State<Rt>) -> Json {
    detached(lightkey_release_work(rt)).await
}

async fn lightkey_release_work(rt: Rt) -> Json {
    let (accepted, message) = rt.outputs.lights.lightkey_release_lights().await;
    lightkey_response(&rt, accepted, message).await
}

/// `POST /api/v1/lights/lightkey/read-fixtures`: ask the Lightkey helper to read the open
/// project's fixtures again (its `POST /fixtures/refresh`). Always 200 with
/// `{ok, message, project?, count?, changed?, code?}`; a refusal carries the helper's own
/// plain-language text. Takes no body.
async fn lightkey_read_fixtures(State(rt): State<Rt>) -> Json {
    ok(to_value(&rt.outputs.lights.lightkey_read_fixtures().await))
}

/// `POST /api/v1/lights/lightkey/simplify` `{song_key, detail_level?, apply?}`: the recording's
/// thin + merge step over a song's captured rows. Without `apply` it only counts ("412 rows ->
/// 190"); with it the simplified rows are saved through the same serialized cue-map path as a
/// recording. Manual rows are never touched.
async fn lightkey_simplify(State(rt): State<Rt>, raw: Bytes) -> Json {
    use stagepilot_lights::lightkey::LightkeySimplifyResponse;
    let object = match object_body(&raw) {
        Ok(object) => object,
        Err(response) => return response,
    };
    let song_key = match object.get("song_key") {
        Some(Value::String(text)) if !text.trim().is_empty() && text.len() <= 256 => {
            text.trim().to_string()
        }
        Some(other) => {
            return unprocessable(vec![plain_error(
                "string_type",
                "song_key",
                "Input should be a non-empty string of at most 256 characters",
                other,
            )])
        }
        None => {
            return unprocessable(vec![json!({"type": "missing", "loc": body_loc("song_key"),
                "msg": "Field required", "input": Value::Object(object.clone())})])
        }
    };
    // `detail_level` is still validated (an older client sends it) but no longer changes the result.
    if let Err(errors) = parse_lightkey_settings(&object) {
        return unprocessable(errors);
    }
    let apply = match object.get("apply") {
        None | Some(Value::Null) => false,
        Some(value) => match lax_bool("apply", value) {
            Ok(b) => b,
            Err(error) => return unprocessable(vec![strip_ctx(error)]),
        },
    };
    let _lock = rt.outputs.cue_lock.lock().await;
    let current = current_lights(&rt);
    let respond = |accepted: bool, applied: bool, message: String, counts: [usize; 4]| {
        let n = |v: usize| u32::try_from(v).unwrap_or(u32::MAX);
        ok(to_value(&LightkeySimplifyResponse {
            accepted,
            applied,
            message,
            rows_before: n(counts[0]),
            rows_after: n(counts[1]),
            messages_before: n(counts[2]),
            messages_after: n(counts[3]),
        }))
    };
    let Some(result) = current.cue_maps.get(&song_key).and_then(|map| {
        stagepilot_lights::lightkey_looks::simplify_song(map, SIMPLIFY_INTERVAL_SECONDS)
    }) else {
        return respond(
            false,
            false,
            "This song has no captured looks to simplify.".into(),
            [0; 4],
        );
    };
    let counts = [
        result.rows_before,
        result.rows_after,
        result.messages_before,
        result.messages_after,
    ];
    let summary = format!("{} rows -> {}", result.rows_before, result.rows_after);
    if result.rows_after >= result.rows_before {
        return respond(
            false,
            false,
            format!("Nothing to simplify ({summary})."),
            counts,
        );
    }
    if !stagepilot_lights::lightkey_looks::within_caps(&result.map) {
        return respond(
            false,
            false,
            format!("Still too many looks after simplifying ({summary})."),
            counts,
        );
    }
    if !apply {
        return respond(true, false, summary, counts);
    }
    let mut updated = current.clone();
    updated.cue_maps.insert(song_key.clone(), result.map);
    updated.normalise();
    let merged = updated.cue_maps.get(&song_key).cloned();
    if let Err(response) = persist_lights(&rt, updated) {
        return response;
    }
    if let Some(merged) = merged {
        rt.outputs.lights.replace_cue_map(merged).await;
    }
    respond(true, true, format!("Simplified: {summary}."), counts)
}

/// The grid the older thin-and-merge step simplifies on (the former Normal detail).
const SIMPLIFY_INTERVAL_SECONDS: f64 = 0.5;

async fn lightkey_recording(State(rt): State<Rt>, raw: Bytes) -> Json {
    detached(lightkey_recording_work(rt, raw)).await
}

async fn lightkey_recording_work(rt: Rt, raw: Bytes) -> Json {
    let object = match object_body(&raw) {
        Ok(object) => object,
        Err(response) => return response,
    };
    let recording = match object.get("recording") {
        None => {
            return unprocessable(vec![
                json!({"type": "missing", "loc": body_loc("recording"),
                "msg": "Field required", "input": Value::Object(object.clone())}),
            ])
        }
        Some(value) => match lax_bool("recording", value) {
            Ok(b) => b,
            Err(error) => return unprocessable(vec![strip_ctx(error)]),
        },
    };
    let message = rt.outputs.lights.lightkey_set_recording(recording).await;
    let status = rt.outputs.lights.lightkey_status().await;
    ok(to_value(&LightkeyActionResponse {
        accepted: status.recording == recording,
        message,
        lightkey: status,
    }))
}

// ---- Finding the Lighting computer -------------------------------------------------------------

/// The `address` of a discovery request body: a required, non-blank string.
fn discovery_address(raw: &Bytes) -> Result<String, Json> {
    let object = object_body(raw)?;
    match object.get("address") {
        None => Err(unprocessable(vec![
            json!({"type": "missing", "loc": body_loc("address"),
            "msg": "Field required", "input": Value::Object(object.clone())}),
        ])),
        Some(Value::String(text)) if !text.trim().is_empty() && text.len() <= 300 => {
            Ok(text.trim().to_string())
        }
        Some(other) => Err(unprocessable(vec![plain_error(
            "string_type",
            "address",
            "Input should be a non-empty string",
            other,
        )])),
    }
}

/// `POST /api/v1/lights/lightkey/discovery/scan` ("Search again"): look for the Lighting computer
/// now. Takes no body. Accepted only while nothing is connected.
async fn lightkey_discovery_scan(State(rt): State<Rt>) -> Json {
    let (accepted, message) = rt.outputs.lights.lightkey_discovery_scan();
    lightkey_response(&rt, accepted, message).await
}

/// `POST /api/v1/lights/lightkey/discovery/use` `{address}`: switch to a Lighting computer the
/// last search found (`Use this one`, `Use <ip>`). Any other address is a plain 422.
async fn lightkey_discovery_use(State(rt): State<Rt>, raw: Bytes) -> Json {
    let address = match discovery_address(&raw) {
        Ok(address) => address,
        Err(response) => return response,
    };
    match rt.outputs.lights.lightkey_discovery_use(&address).await {
        Ok(message) => lightkey_response(&rt, true, message).await,
        Err(message) => http_error(422, &message),
    }
}

/// `POST /api/v1/lights/lightkey/discovery/dismiss` `{address}` (`Keep waiting`): stop offering
/// that address until StagePilot restarts or the chosen address changes.
async fn lightkey_discovery_dismiss(State(rt): State<Rt>, raw: Bytes) -> Json {
    let address = match discovery_address(&raw) {
        Ok(address) => address,
        Err(response) => return response,
    };
    rt.outputs.lights.lightkey_discovery_dismiss(&address);
    lightkey_response(&rt, true, "Still waiting for the chosen address.".into()).await
}

// ---- Loudness ----------------------------------------------------------------------------------

/// The input picker's data: devices, the saved choice, the live meter. `available: false` with a
/// `reason` in a build or on a machine that cannot listen (the picker is then hidden).
async fn lightkey_audio(State(rt): State<Rt>) -> Json {
    audio_response(&rt, None).await
}

/// How long the audio routes wait for the device layer before answering with what is known.
const AUDIO_ROUTE_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// The picker's data. Listing and opening inputs can block (a microphone permission dialog), so
/// they run on a blocking thread with a timeout; a slow answer falls back to the cached view and
/// the thread finishes on its own (A-01).
async fn audio_response(rt: &Runtime, meter: Option<(Option<String>, bool)>) -> Json {
    let selected = current_lights(rt).lightkey.audio_device;
    let Some(hub) = rt.outputs.lights.lightkey_audio() else {
        return ok(json!({
            "available": false,
            "reason": stagepilot_lights::audio::UNAVAILABLE_TEXT,
            "devices": [],
            "selected": null,
            "note": null,
            "level": null,
            "metering": false,
        }));
    };
    let worker = hub.clone();
    let job = tokio::task::spawn_blocking(move || {
        if let Some((device, on)) = meter {
            worker.set_device(device);
            worker.set_meter(on);
        }
        worker.info()
    });
    let mut info = match tokio::time::timeout(AUDIO_ROUTE_WAIT, job).await {
        Ok(Ok(info)) => info,
        _ => {
            let mut info = hub.snapshot();
            info.reason.get_or_insert_with(|| {
                "The audio input is slow to answer (the system may be asking for microphone access)."
                    .into()
            });
            info
        }
    };
    info.selected = selected;
    ok(to_value(&info))
}

/// Turns the picker's meter on (it switches itself off after a few seconds without a repeat) or
/// off. Only the picker asks, and only while Record is chosen.
async fn lightkey_audio_meter(State(rt): State<Rt>, raw: Bytes) -> Json {
    let on = match object_body(&raw) {
        Ok(object) => match object.get("on") {
            Some(value) => match lax_bool("on", value) {
                Ok(on) => on,
                Err(error) => return unprocessable(vec![strip_ctx(error)]),
            },
            None => true,
        },
        Err(response) => return response,
    };
    audio_response(&rt, Some((current_lights(&rt).lightkey.audio_device, on))).await
}

/// Removes a song's loudness envelope (nothing else on its cue map).
async fn lights_loudness_clear(State(rt): State<Rt>, raw: Bytes) -> Json {
    let (request, _): (LoudnessClearRequest, Value) = match body(&raw) {
        Ok(parsed) => parsed,
        Err(response) => return response,
    };
    let mut errors = Vec::new();
    length_error(&mut errors, "song_key", &request.song_key, 1, 256);
    if let Err(response) = finish(errors) {
        return response;
    }
    let _lock = rt.outputs.cue_lock.lock().await;
    let current = current_lights(&rt);
    let Some(mut map) = current.cue_maps.get(&request.song_key).cloned() else {
        return lights_operation(&rt, false, "This song has no loudness.".into()).await;
    };
    if map.loudness.take().is_none() {
        return lights_operation(&rt, false, "This song has no loudness.".into()).await;
    }
    let mut updated = current.clone();
    if map.cues.is_empty() && map.osc_cues.is_empty() && map.effects.is_empty() {
        updated.cue_maps.remove(&request.song_key);
    } else {
        updated
            .cue_maps
            .insert(request.song_key.clone(), map.clone());
    }
    if let Err(response) = persist_lights(&rt, updated) {
        return response;
    }
    rt.outputs.lights.replace_cue_map(map).await;
    lights_operation(&rt, true, "Loudness cleared.".into()).await
}

#[derive(serde::Deserialize)]
struct LoudnessClearRequest {
    song_key: String,
}

#[cfg(test)]
mod detached_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn detached_work_finishes_when_the_waiting_handler_is_dropped() {
        let finished = Arc::new(AtomicBool::new(false));
        let flag = finished.clone();
        let handler = detached(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            flag.store(true, Ordering::SeqCst);
            ok(json!({"done": true}))
        });
        // The client goes away after 20 ms: the handler future is dropped mid-wait.
        assert!(tokio::time::timeout(Duration::from_millis(20), handler)
            .await
            .is_err());
        assert!(!finished.load(Ordering::SeqCst));
        for _ in 0..500 {
            if finished.load(Ordering::SeqCst) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the work was dropped with the handler");
    }

    #[tokio::test]
    async fn detached_work_that_panics_answers_a_plain_error() {
        let response = detached(async { panic!("boom") }).await;
        assert_eq!(response.0.as_u16(), 500);
    }
}
