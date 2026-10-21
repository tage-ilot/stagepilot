//! The `/api/v1/playback-api/*` routes: the Rust counterpart of `backend/.../api/playback.py`.
//!
//! Status codes, `detail` strings and response bodies follow Python. The Planning Center length
//! writer is a separate service (phase 5): its routes answer exactly what Python answers while
//! Planning Center is not connected, and its restore refuses when there is something to restore.

use crate::runtime::Runtime;
use crate::validation::{http_error, invalid_body, ok, unprocessable, Json};
use axum::body::Bytes;
use axum::extract::State;
use axum::routing::{get, post, put};
use axum::Router;
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Map, Value};
use stagepilot_core::config::{MidiSource, PlaybackApiSettings};
use stagepilot_core::state::ConnectionStatus;
use stagepilot_playback::api::{
    LengthUpdateResult, PlaybackConnectionResponse, PlaybackEventsResponse, PlaybackMonitorEntry,
    PlaybackPlanSong, PlaybackSettingsRequest, PlaybackStatusResponse, ScanCandidateModel,
    ScanResult, SourceStatus, SourcesStatus,
};
use stagepilot_playback::arrangement::align_lengths;
use stagepilot_playback::client::ConnectionOptions;
use stagepilot_playback::find::DiscoverySource;
use stagepilot_playback::plugin::{PlaybackError, LOCAL_NETWORK_SETTINGS_URL};
use std::sync::Arc;

type Rt = Arc<Runtime>;

const PC_UNAVAILABLE: &str =
    "Planning Center could not read this plan. Song order and lengths are saved. Try again later.";

pub fn routes() -> Router<Rt> {
    Router::new()
        .route("/api/v1/playback-api/status", get(status))
        .route("/api/v1/playback-api/connection", get(connection))
        .route("/api/v1/playback-api/events", get(events))
        .route("/api/v1/playback-api/arrangement", put(arrangement))
        .route("/api/v1/playback-api/find", post(find))
        .route("/api/v1/playback-api/find/cancel", post(cancel_find))
        .route("/api/v1/playback-api/settings", put(settings))
        .route("/api/v1/playback-api/discover-song-order", post(discover))
        .route(
            "/api/v1/playback-api/planning-center-lengths/categories",
            get(length_categories),
        )
        .route(
            "/api/v1/playback-api/planning-center-lengths/category",
            put(choose_length_category),
        )
        .route(
            "/api/v1/playback-api/planning-center-lengths/preview",
            post(preview_lengths),
        )
        .route(
            "/api/v1/playback-api/planning-center-lengths/confirm",
            post(confirm_lengths),
        )
        .route(
            "/api/v1/playback-api/planning-center-lengths/restore",
            post(restore_lengths),
        )
}

// ---- pydantic-shaped body validation ----------------------------------------------------------

fn at(loc: &[Value], extra: Value) -> Vec<Value> {
    let mut out = vec![json!("body")];
    out.extend_from_slice(loc);
    out.push(extra);
    out
}

fn err(kind: &str, loc: Vec<Value>, msg: &str, input: &Value) -> Value {
    json!({"type": kind, "loc": loc, "msg": msg, "input": input})
}

fn field_loc(name: &str) -> Vec<Value> {
    vec![json!("body"), json!(name)]
}

fn missing(name: &str, whole: &Value) -> Value {
    err("missing", field_loc(name), "Field required", whole)
}

/// Pydantic lax `bool`.
fn lax_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => match n.as_i64() {
            Some(0) => Some(false),
            Some(1) => Some(true),
            _ => None,
        },
        Value::String(s) => match s.to_lowercase().as_str() {
            "0" | "off" | "f" | "false" | "n" | "no" => Some(false),
            "1" | "on" | "t" | "true" | "y" | "yes" => Some(true),
            _ => None,
        },
        _ => None,
    }
}

/// Pydantic lax `int` (bool, integral float and numeric string accepted).
fn lax_int(value: &Value) -> Result<i64, (&'static str, &'static str)> {
    match value {
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i)
            } else if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 && f.abs() < 9.0e18 {
                    Ok(f as i64)
                } else {
                    Err((
                        "int_from_float",
                        "Input should be a valid integer, got a number with a fractional part",
                    ))
                }
            } else {
                Err(("int_parsing", "Input should be a valid integer"))
            }
        }
        Value::String(s) => s.trim().parse::<i64>().map_err(|_| {
            (
                "int_parsing",
                "Input should be a valid integer, unable to parse string as an integer",
            )
        }),
        _ => Err(("int_type", "Input should be a valid integer")),
    }
}

fn body_object(raw: &[u8]) -> Result<Map<String, Value>, Json> {
    let parsed: Option<Value> = serde_json::from_slice(raw).ok();
    match parsed {
        Some(Value::Object(map)) => Ok(map),
        other => Err(unprocessable(invalid_body(raw, other.as_ref()))),
    }
}

fn extras(map: &Map<String, Value>, known: &[&str], errors: &mut Vec<Value>) {
    for (key, value) in map {
        if !known.contains(&key.as_str()) {
            errors.push(err(
                "extra_forbidden",
                field_loc(key),
                "Extra inputs are not permitted",
                value,
            ));
        }
    }
}

/// `PlaybackSettingsRequest`.
struct SettingsRequest {
    enabled: bool,
    host: Option<String>,
    port: u16,
    auto_scan: bool,
    fast_transport: bool,
}

fn settings_request(raw: &[u8]) -> Result<SettingsRequest, Json> {
    let map = body_object(raw)?;
    let whole = Value::Object(map.clone());
    let mut errors = Vec::new();
    let flag = |name: &str, default: bool, errors: &mut Vec<Value>| match map.get(name) {
        None => default,
        Some(v) => lax_bool(v).unwrap_or_else(|| {
            errors.push(err(
                "bool_parsing",
                field_loc(name),
                "Input should be a valid boolean, unable to interpret input",
                v,
            ));
            default
        }),
    };
    let enabled = flag("enabled", true, &mut errors);
    let host = match map.get("host") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(v) => {
            errors.push(err(
                "string_type",
                field_loc("host"),
                "Input should be a valid string",
                v,
            ));
            None
        }
    };
    let port = match map.get("port") {
        None => 8080,
        Some(v) => match lax_int(v) {
            Ok(p) if p < 1 => {
                let mut e = err(
                    "greater_than_equal",
                    field_loc("port"),
                    "Input should be greater than or equal to 1",
                    v,
                );
                e["ctx"] = json!({"ge": 1});
                errors.push(e);
                8080
            }
            Ok(p) if p > 65535 => {
                let mut e = err(
                    "less_than_equal",
                    field_loc("port"),
                    "Input should be less than or equal to 65535",
                    v,
                );
                e["ctx"] = json!({"le": 65535});
                errors.push(e);
                8080
            }
            Ok(p) => p as u16,
            Err((kind, msg)) => {
                errors.push(err(kind, field_loc("port"), msg, v));
                8080
            }
        },
    };
    let auto_scan = flag("auto_scan", true, &mut errors);
    let fast_transport = flag("fast_transport", true, &mut errors);
    extras(
        &map,
        &["enabled", "host", "port", "auto_scan", "fast_transport"],
        &mut errors,
    );
    let _ = whole;
    if errors.is_empty() {
        Ok(SettingsRequest {
            enabled,
            host,
            port,
            auto_scan,
            fast_transport,
        })
    } else {
        Err(unprocessable(errors))
    }
}

/// A list field validated like pydantic: `list[StrictInt]` or `list[str]`, at most 200 items.
fn list_field(
    map: &Map<String, Value>,
    name: &str,
    strict_int: bool,
    errors: &mut Vec<Value>,
) -> Vec<Value> {
    let Some(value) = map.get(name) else {
        return vec![];
    };
    let Value::Array(items) = value else {
        errors.push(err(
            "list_type",
            field_loc(name),
            "Input should be a valid list",
            value,
        ));
        return vec![];
    };
    let mut good = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let loc = at(&[json!(name)], json!(i))[..].to_vec();
        let accepted = if strict_int {
            item.is_i64() || item.is_u64()
        } else {
            item.is_string()
        };
        if accepted {
            good.push(item.clone());
        } else if strict_int {
            errors.push(err(
                "int_type",
                loc,
                "Input should be a valid integer",
                item,
            ));
        } else {
            errors.push(err(
                "string_type",
                loc,
                "Input should be a valid string",
                item,
            ));
        }
    }
    if items.len() > 200 {
        let mut e = err(
            "too_long",
            field_loc(name),
            &format!(
                "List should have at most 200 items after validation, not {}",
                items.len()
            ),
            value,
        );
        e["ctx"] = json!({"field_type": "List", "max_length": 200, "actual_length": items.len()});
        errors.push(e);
    }
    good
}

fn arrangement_request(raw: &[u8]) -> Result<(Vec<i64>, Vec<String>), Json> {
    let map = body_object(raw)?;
    let mut errors = Vec::new();
    let songs = list_field(&map, "skipped_song_ids", true, &mut errors);
    let plan = list_field(&map, "skipped_plan_song_ids", false, &mut errors);
    extras(
        &map,
        &["skipped_song_ids", "skipped_plan_song_ids"],
        &mut errors,
    );
    if !errors.is_empty() {
        return Err(unprocessable(errors));
    }
    Ok((
        songs.iter().filter_map(Value::as_i64).collect(),
        plan.iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
    ))
}

/// `CategoryChoice`: `service_type_id` 1..=128 characters of `[A-Za-z0-9_-]`.
fn category_choice(raw: &[u8]) -> Result<String, Json> {
    let map = body_object(raw)?;
    let whole = Value::Object(map.clone());
    let mut errors = Vec::new();
    let mut id = String::new();
    match map.get("service_type_id") {
        None => errors.push(missing("service_type_id", &whole)),
        Some(Value::String(s)) => {
            let n = s.chars().count();
            if n < 1 {
                let mut e = err(
                    "string_too_short",
                    field_loc("service_type_id"),
                    "String should have at least 1 character",
                    &json!(s),
                );
                e["ctx"] = json!({"min_length": 1});
                errors.push(e);
            } else if n > 128 {
                let mut e = err(
                    "string_too_long",
                    field_loc("service_type_id"),
                    "String should have at most 128 characters",
                    &json!(s),
                );
                e["ctx"] = json!({"max_length": 128});
                errors.push(e);
            } else if !s
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                let mut e = err(
                    "string_pattern_mismatch",
                    field_loc("service_type_id"),
                    "String should match pattern '^[A-Za-z0-9_-]+$'",
                    &json!(s),
                );
                e["ctx"] = json!({"pattern": "^[A-Za-z0-9_-]+$"});
                errors.push(e);
            } else {
                id = s.clone();
            }
        }
        Some(v) => errors.push(err(
            "string_type",
            field_loc("service_type_id"),
            "Input should be a valid string",
            v,
        )),
    }
    extras(&map, &["service_type_id"], &mut errors);
    if errors.is_empty() {
        Ok(id)
    } else {
        Err(unprocessable(errors))
    }
}

/// `LengthConfirmation`: `token: str`, `confirm: Literal[True]`.
fn length_confirmation(raw: &[u8]) -> Result<(), Json> {
    let map = body_object(raw)?;
    let whole = Value::Object(map.clone());
    let mut errors = Vec::new();
    match map.get("token") {
        None => errors.push(missing("token", &whole)),
        Some(Value::String(_)) => {}
        Some(v) => errors.push(err(
            "string_type",
            field_loc("token"),
            "Input should be a valid string",
            v,
        )),
    }
    match map.get("confirm") {
        None => errors.push(missing("confirm", &whole)),
        Some(Value::Bool(true)) => {}
        Some(v) => {
            let mut e = err(
                "literal_error",
                field_loc("confirm"),
                "Input should be True",
                v,
            );
            e["ctx"] = json!({"expected": "True"});
            errors.push(e);
        }
    }
    extras(&map, &["token", "confirm"], &mut errors);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(unprocessable(errors))
    }
}

/// `payload: object = Body(default=None)`: any JSON value, nothing at all, or a JSON error.
fn loose_body(raw: &[u8]) -> Result<Option<Value>, Json> {
    if raw.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    serde_json::from_slice::<Value>(raw)
        .map(Some)
        .map_err(|_| unprocessable(invalid_body(raw, None)))
}

fn confirmed(body: &Option<Value>) -> bool {
    matches!(body, Some(Value::Object(m)) if m.get("confirm") == Some(&Value::Bool(true)))
}

fn truthy(value: &Option<Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

// ---- response bodies --------------------------------------------------------------------------

/// pydantic v2 JSON for an aware UTC datetime.
fn iso(at: DateTime<Utc>) -> String {
    let micros = at.timestamp_subsec_micros();
    let format = if micros == 0 {
        SecondsFormat::Secs
    } else {
        SecondsFormat::Micros
    };
    at.to_rfc3339_opts(format, true)
}

/// `json.dumps(value, sort_keys=True)` (default separators, ASCII output).
fn py_dumps(value: &Value) -> String {
    match value {
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(py_dumps).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            format!(
                "{{{}}}",
                keys.iter()
                    .map(|k| format!("{}: {}", py_dumps(&json!(k)), py_dumps(&map[*k])))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        Value::String(s) => {
            let mut out = String::from("\"");
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                        let mut buf = [0u16; 2];
                        for unit in c.encode_utf16(&mut buf) {
                            out.push_str(&format!("\\u{unit:04x}"));
                        }
                    }
                    c => out.push(c),
                }
            }
            out.push('"');
            out
        }
        other => other.to_string(),
    }
}

fn source_name(source: Option<DiscoverySource>) -> Option<String> {
    source.map(|s| {
        match s {
            DiscoverySource::Manual => "manual",
            DiscoverySource::Loopback => "loopback",
            DiscoverySource::Lan => "lan",
        }
        .to_string()
    })
}

fn sources_of(connection: &stagepilot_playback::arbiter::PlaybackConnection) -> SourcesStatus {
    let of = |s: &stagepilot_playback::arbiter::SourceStatus| SourceStatus {
        connected: s.connected,
        reason: s.reason.to_string(),
    };
    SourcesStatus {
        midi: of(&connection.midi),
        playback_api: of(&connection.playback_api),
    }
}

fn connection_json(rt: &Runtime) -> Value {
    let connection = rt.playback.connection();
    to_json(&PlaybackConnectionResponse {
        active_source: connection.active_source.as_str().to_string(),
        connected: connection.connected,
        reason: rt.playback.connection_reason(),
        sources: sources_of(&connection),
    })
}

fn to_json<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn lengths_result(rt: &Runtime) -> LengthUpdateResult {
    rt.lengths_result
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

fn lengths_status(rt: &Runtime) -> String {
    lengths_result(rt).status
}

/// `update_reason`: why the Planning Center write is not available right now.
fn update_reason(rt: &Runtime) -> Option<String> {
    let state = rt.store.snapshot();
    if state.planning_center_status != ConnectionStatus::Connected {
        return Some("Connect Planning Center first.".into());
    }
    let status = rt.playback.status();
    if status.heartbeat.is_some_and(|h| h.playing) {
        return Some("Stop Playback first.".into());
    }
    let discovery = rt.playback.discovery_snapshot().state.as_str();
    if discovery == "failed" {
        return Some("The song scan failed or was cancelled. Scan songs again.".into());
    }
    let config = rt.playback.config();
    if discovery == "running" {
        return Some("Wait for the song scan to finish.".into());
    }
    if config.captured_at.is_none() || config.song_order.is_empty() {
        return Some("Scan songs first to read the lengths from Playback.".into());
    }
    if rt.playback.mapper_stale() {
        return Some("The setlist changed. Scan songs again.".into());
    }
    if rt.playback.plan_changed() {
        return Some("The Services plan changed since the scan. Scan songs again.".into());
    }
    if rt.playback.lengths_applied() {
        return Some(
            "Planning Center already has these lengths. Scan songs again for new ones.".into(),
        );
    }
    if !config.song_lengths.iter().any(Option::is_some) {
        return Some("The scan has no song lengths. Scan songs again.".into());
    }
    if lengths_status(rt) == "running" {
        return Some("Wait for the Planning Center changes to finish.".into());
    }
    None
}

fn status_json(rt: &Runtime) -> Value {
    let state = rt.store.snapshot();
    let status = rt.playback.status();
    let config = rt.playback.config();
    let persistent = rt.settings_service.snapshot();
    let connection = rt.playback.connection();
    let scan = rt.playback.scan_snapshot();
    let discovery = rt.playback.discovery_snapshot();
    let (song_order, captured_version) = rt.playback.mapper_view();
    let stale = rt.playback.mapper_stale();
    let plan_changed = rt.playback.plan_changed();
    let outdated = stale || plan_changed;
    let blocked = scan.error_class.as_deref() == Some("permission_denied");
    let last = scan.last_scan.clone().unwrap_or(Value::Null);
    let last_obj = last.as_object();
    let count = |key: &str| {
        last_obj
            .and_then(|m| m.get(key))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32
    };
    let networks: Vec<String> = last_obj
        .and_then(|m| m.get("networks"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let plan = state.plan.as_ref();
    let plan_ids: Vec<&str> = plan
        .map(|p| p.songs.iter().map(|s| s.id.as_str()).collect())
        .unwrap_or_default();
    let arrangement_current = config.skipped_plan_song_ids.is_empty()
        || (plan.is_some()
            && config
                .skipped_plan_song_ids
                .iter()
                .all(|id| plan_ids.contains(&id.as_str())));
    let update_service_type = config
        .planning_center_update_service_type_id
        .clone()
        .or_else(|| persistent.planning_center.service_type_id.clone());
    let response = PlaybackStatusResponse {
        planning_center_lengths: lengths_result(rt),
        planning_center_undo_available: !persistent.planning_center_length_undo.is_empty(),
        planning_center_connected: state.planning_center_status == ConnectionStatus::Connected,
        planning_center_update_reason: update_reason(rt),
        planning_center_update_service_type_id: update_service_type,
        discovery_duration_seconds: config.discovery_duration_seconds,
        plan_title: plan.map(|p| p.title.clone()),
        plan_date: plan.map(|p| p.date.to_string()),
        scan: ScanResult {
            state: scan.state.as_str().to_string(),
            candidates: scan
                .candidates
                .iter()
                .map(|c| ScanCandidateModel {
                    name: c.name.clone(),
                    host: c.host.clone(),
                })
                .collect(),
            reason: scan.reason.clone(),
            current: scan.current as u32,
            total: scan.total as u32,
            phase: if scan.state.as_str() == "scanning" {
                scan.phase.clone()
            } else {
                None
            },
            error_class: scan.error_class.clone(),
            typed: scan.typed,
            elapsed: (scan.elapsed * 10.0).round() / 10.0,
            networks,
            hosts_probed: count("hosts_probed"),
            hosts_total: count("hosts_total"),
            details: if last_obj.is_some_and(|m| !m.is_empty()) {
                Some(py_dumps(&last))
            } else {
                None
            },
            settings_url: blocked.then(|| LOCAL_NETWORK_SETTINGS_URL.to_string()),
        },
        selected: rt.playback.selected(),
        enabled: config.enabled,
        connected: connection.connected,
        active_source: connection.active_source.as_str().to_string(),
        sources: sources_of(&connection),
        reason: rt.playback.connection_reason(),
        host: status.host.clone(),
        port: status.port,
        source: source_name(status.source),
        last_error: discovery.error.clone().or_else(|| {
            if connection.connected {
                None
            } else {
                status.last_error.clone()
            }
        }),
        playing: status.heartbeat.is_some_and(|h| h.playing),
        setlist_cloud_version: status.heartbeat.and_then(|h| h.setlist_version),
        discovery: discovery.state.as_str().to_string(),
        progress: discovery.progress as u32,
        discovery_song: discovery.song as u32,
        discovery_total: discovery.total as u32,
        song_count: song_order.len() as u32,
        song_order,
        song_lengths: if outdated {
            vec![]
        } else {
            config.song_lengths.clone()
        },
        skipped_song_ids: config.skipped_song_ids.clone(),
        skipped_plan_song_ids: config.skipped_plan_song_ids.clone(),
        arrangement_current,
        lengths_measured_at: if outdated {
            None
        } else {
            config.lengths_measured_at.map(iso)
        },
        plan_songs: plan
            .map(|p| {
                p.songs
                    .iter()
                    .map(|s| PlaybackPlanSong {
                        id: s.id.clone(),
                        title: s.title.clone(),
                        duration_seconds: s.duration_seconds,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        plan_song_count: plan.map_or(0, |p| p.songs.len() as u32),
        captured_version,
        captured_at: config.captured_at.map(iso),
        stale,
        plan_changed,
        lengths_applied: rt.playback.lengths_applied(),
        settings: PlaybackSettingsRequest {
            enabled: config.enabled,
            host: config.host.clone(),
            port: config.port,
            auto_scan: config.auto_scan,
            fast_transport: config.fast_transport,
        },
    };
    to_json(&response)
}

fn failure(error: PlaybackError, invalid_status: u16) -> Json {
    match error {
        PlaybackError::Conflict(m) => http_error(409, &m),
        PlaybackError::Invalid(m) => http_error(invalid_status, &m),
        PlaybackError::Save(m) => http_error(503, &m),
    }
}

// ---- handlers ---------------------------------------------------------------------------------

async fn status(State(rt): State<Rt>) -> Json {
    rt.playback.publish_connection().await;
    ok(status_json(&rt))
}

async fn connection(State(rt): State<Rt>) -> Json {
    rt.playback.publish_connection().await;
    ok(connection_json(&rt))
}

async fn events(State(rt): State<Rt>) -> Json {
    let events = rt
        .playback
        .events()
        .iter()
        .map(|e| PlaybackMonitorEntry {
            event: e.event.clone(),
            discovery: e.discovery,
            at: iso(e.at),
        })
        .collect();
    ok(to_json(&PlaybackEventsResponse {
        events,
        capacity: 100,
    }))
}

async fn arrangement(State(rt): State<Rt>, body: Bytes) -> Json {
    let (songs, plan_songs) = match arrangement_request(&body) {
        Ok(parsed) => parsed,
        Err(response) => return response,
    };
    let unique =
        |v: &[String]| v.iter().collect::<std::collections::BTreeSet<_>>().len() == v.len();
    let unique_ints =
        |v: &[i64]| v.iter().collect::<std::collections::BTreeSet<_>>().len() == v.len();
    if !unique_ints(&songs) || !unique(&plan_songs) {
        return http_error(422, "Skipped songs must be unique.");
    }
    let (order, _) = rt.playback.mapper_view();
    if !songs.iter().all(|id| order.contains(id)) {
        return http_error(
            422,
            "A skipped Playback song is not in the scanned song order.",
        );
    }
    match rt.playback.save_arrangement(songs, plan_songs).await {
        Ok(()) => ok(status_json(&rt)),
        Err(error) => failure(error, 409),
    }
}

async fn find(State(rt): State<Rt>, body: Bytes) -> Json {
    let payload = match loose_body(&body) {
        Ok(p) => p,
        Err(response) => return response,
    };
    let host_error = || http_error(422, "Enter a host name or IP address.");
    let host = if truthy(&payload) {
        let Some(Value::Object(map)) = &payload else {
            return host_error();
        };
        if map.keys().any(|k| k != "host") {
            return host_error();
        }
        match map.get("host") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) if s.chars().count() <= 255 => Some(s.clone()),
            Some(_) => return host_error(),
        }
    } else {
        None
    };
    match rt.playback.find(host.as_deref()).await {
        Ok(_) => ok(status_json(&rt)),
        Err(error) => failure(error, 422),
    }
}

async fn cancel_find(State(rt): State<Rt>) -> Json {
    rt.playback.cancel_scan().await;
    ok(status_json(&rt))
}

async fn settings(State(rt): State<Rt>, body: Bytes) -> Json {
    let request = match settings_request(&body) {
        Ok(r) => r,
        Err(response) => return response,
    };
    if rt.playback.discovery_snapshot().state.as_str() == "running" {
        return http_error(
            409,
            "Wait for Discover Song Order before changing settings.",
        );
    }
    let options = ConnectionOptions {
        enabled: request.enabled,
        host_override: request.host.clone(),
        port: request.port,
        auto_scan: request.auto_scan,
        ..Default::default()
    };
    if let Err(error) = options.validate() {
        return http_error(422, &error.to_string());
    }
    let previous = rt.settings_service.snapshot();
    let mut updated: PlaybackApiSettings = previous.playback_api.clone();
    updated.enabled = request.enabled;
    updated.host = request.host.clone();
    updated.port = request.port;
    updated.auto_scan = request.auto_scan;
    updated.fast_transport = request.fast_transport;
    updated.normalise();
    let mut next = previous.clone();
    next.playback_api = updated;
    // Turning Playback on is choosing it: one switch, no hidden second source setting.
    if request.enabled && next.integration_modes.midi_source != MidiSource::PlaybackApi {
        next.integration_modes.midi_source = MidiSource::PlaybackApi;
    }
    match rt.playback.save_settings(next).await {
        Ok(()) => ok(status_json(&rt)),
        Err(error) => failure(error, 409),
    }
}

async fn discover(State(rt): State<Rt>, body: Bytes) -> Json {
    let payload = match loose_body(&body) {
        Ok(p) => p,
        Err(response) => return response,
    };
    if !confirmed(&payload) {
        return http_error(400, "Confirm Discover Song Order with {confirm: true}.");
    }
    match rt.playback.discover_song_order().await {
        Ok(()) => ok(status_json(&rt)),
        Err(error) => failure(error, 409),
    }
}

async fn length_categories() -> Json {
    http_error(503, PC_UNAVAILABLE)
}

fn busy(rt: &Runtime) -> bool {
    rt.playback.discovery_snapshot().state.as_str() == "running" || lengths_status(rt) == "running"
}

async fn choose_length_category(State(rt): State<Rt>, body: Bytes) -> Json {
    if let Err(response) = category_choice(&body) {
        return response;
    }
    if busy(&rt) {
        return http_error(409, "Wait for the current operation to finish.");
    }
    http_error(503, PC_UNAVAILABLE)
}

async fn preview_lengths(State(rt): State<Rt>) -> Json {
    if let Some(reason) = update_reason(&rt) {
        return http_error(409, &reason);
    }
    let config = rt.playback.config();
    let category = config
        .planning_center_update_service_type_id
        .clone()
        .or_else(|| {
            rt.settings_service
                .snapshot()
                .planning_center
                .service_type_id
        });
    if category.is_none_or(|c| c.is_empty()) {
        return http_error(409, "Choose a Planning Center category first.");
    }
    // Aligning needs the loaded plan; the Planning Center read itself is not ported (phase 5).
    if let Some(plan) = rt.store.snapshot().plan {
        let ids: Vec<&str> = plan.songs.iter().map(|s| s.id.as_str()).collect();
        let skipped_plan: Vec<&str> = config
            .skipped_plan_song_ids
            .iter()
            .map(String::as_str)
            .collect();
        let _ = align_lengths(
            &ids,
            &config.song_order,
            &config.song_lengths,
            &skipped_plan,
            &config.skipped_song_ids,
        );
    }
    http_error(503, PC_UNAVAILABLE)
}

async fn confirm_lengths(State(rt): State<Rt>, body: Bytes) -> Json {
    if let Err(response) = length_confirmation(&body) {
        return response;
    }
    if let Some(reason) = update_reason(&rt) {
        return http_error(409, &reason);
    }
    http_error(503, PC_UNAVAILABLE)
}

async fn restore_lengths(State(rt): State<Rt>, body: Bytes) -> Json {
    let payload = match loose_body(&body) {
        Ok(p) => p,
        Err(response) => return response,
    };
    if !confirmed(&payload) {
        return http_error(
            400,
            "Confirm restoring Planning Center times with {confirm: true}.",
        );
    }
    if busy(&rt) {
        return http_error(
            409,
            "Wait for the scan or Planning Center changes to finish.",
        );
    }
    if rt.playback.status().heartbeat.is_some_and(|h| h.playing) {
        return http_error(409, "Stop Playback before restoring Planning Center times.");
    }
    if !rt
        .settings_service
        .snapshot()
        .planning_center_length_undo
        .is_empty()
    {
        return http_error(
            503,
            "Restoring Planning Center times is not available in this build yet.",
        );
    }
    *rt.lengths_result.lock().unwrap_or_else(|p| p.into_inner()) = LengthUpdateResult {
        operation: "restore".into(),
        status: "done".into(),
        message: "No Planning Center times needed changing.".into(),
        items: vec![],
        reload: "not_needed".into(),
    };
    ok(status_json(&rt))
}
