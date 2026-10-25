//! Settings models: port of `core/config.py` (the parts that are persisted) with the same JSON
//! names, defaults, bounds and cross-field rules.
//!
//! Serde handles shape and types. Value rules that pydantic enforces (`ge`, `le`, `min_length`,
//! patterns, cross-field validators) are in the `validate` methods, which collect pydantic-style
//! errors (`type`, `loc`, `msg`, `input`, `ctx`) so the HTTP layer can answer 422 like FastAPI.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use ts_rs::TS;
use uuid::Uuid;

/// One pydantic-style validation error.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Detail {
    #[serde(rename = "type")]
    pub kind: String,
    pub loc: Vec<Value>,
    pub msg: String,
    pub input: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ctx: Option<Value>,
}

pub type Errors = Vec<Detail>;

pub fn path_with(path: &[Value], key: &str) -> Vec<Value> {
    let mut p = path.to_vec();
    p.push(Value::String(key.to_string()));
    p
}

pub fn check_range<T: Copy + PartialOrd + Into<f64> + Serialize>(
    errors: &mut Errors,
    path: &[Value],
    key: &str,
    value: T,
    ge: Option<f64>,
    le: Option<f64>,
    gt: Option<f64>,
) {
    let v: f64 = value.into();
    let fmt = |n: f64| {
        if n.fract() == 0.0 && n.abs() < 1e15 {
            format!("{}", n as i64)
        } else {
            format!("{n}")
        }
    };
    let ctxv = |name: &str, n: f64| {
        if n.fract() == 0.0 {
            json!({ name: n as i64 })
        } else {
            json!({ name: n })
        }
    };
    let input = serde_json::to_value(value).unwrap_or(Value::Null);
    if let Some(n) = ge {
        if v < n {
            errors.push(Detail {
                kind: "greater_than_equal".into(),
                loc: path_with(path, key),
                msg: format!("Input should be greater than or equal to {}", fmt(n)),
                input: input.clone(),
                ctx: Some(ctxv("ge", n)),
            });
            return;
        }
    }
    if let Some(n) = gt {
        if v <= n {
            errors.push(Detail {
                kind: "greater_than".into(),
                loc: path_with(path, key),
                msg: format!("Input should be greater than {}", fmt(n)),
                input: input.clone(),
                ctx: Some(ctxv("gt", n)),
            });
            return;
        }
    }
    if let Some(n) = le {
        if v > n {
            errors.push(Detail {
                kind: "less_than_equal".into(),
                loc: path_with(path, key),
                msg: format!("Input should be less than or equal to {}", fmt(n)),
                input,
                ctx: Some(ctxv("le", n)),
            });
        }
    }
}

pub fn check_len(
    errors: &mut Errors,
    path: &[Value],
    key: &str,
    value: &str,
    min: usize,
    max: usize,
) {
    let n = value.chars().count();
    if n < min {
        errors.push(Detail {
            kind: "string_too_short".into(),
            loc: path_with(path, key),
            msg: format!(
                "String should have at least {min} character{}",
                if min == 1 { "" } else { "s" }
            ),
            input: Value::String(value.to_string()),
            ctx: Some(json!({ "min_length": min })),
        });
    } else if n > max {
        errors.push(Detail {
            kind: "string_too_long".into(),
            loc: path_with(path, key),
            msg: format!(
                "String should have at most {max} character{}",
                if max == 1 { "" } else { "s" }
            ),
            input: Value::String(value.to_string()),
            ctx: Some(json!({ "max_length": max })),
        });
    }
}

pub fn value_error(errors: &mut Errors, path: &[Value], message: &str, input: Value) {
    errors.push(Detail {
        kind: "value_error".into(),
        loc: path.to_vec(),
        msg: format!("Value error, {message}"),
        input,
        // FastAPI serialises the raised ValueError object as an empty object.
        ctx: Some(json!({ "error": {} })),
    });
}

fn default_true() -> bool {
    true
}

// ---- enums ------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ServiceSource {
    Demo,
    PlanningCenter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MidiSource {
    Simulated,
    Real,
    PlaybackApi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MidiTransport {
    Local,
    Network,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TimerOutput {
    Simulated,
    Propresenter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LightingMethod {
    Osc,
    Midi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PadColor {
    Rose,
    Teal,
    Amber,
    Blue,
    Violet,
    Green,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum LogLevel {
    #[serde(rename = "DEBUG")]
    Debug,
    #[serde(rename = "INFO")]
    Info,
    #[serde(rename = "WARNING")]
    Warning,
    #[serde(rename = "ERROR")]
    Error,
    #[serde(rename = "CRITICAL")]
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ConnectionMethod {
    Oauth,
    Manual,
}

// ---- integration modes ------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct IntegrationModes {
    #[serde(default = "IntegrationModes::default_service")]
    pub service_source: ServiceSource,
    #[serde(default = "IntegrationModes::default_midi")]
    pub midi_source: MidiSource,
    #[serde(default = "IntegrationModes::default_timer")]
    pub timer_output: TimerOutput,
}

impl IntegrationModes {
    fn default_service() -> ServiceSource {
        ServiceSource::Demo
    }
    fn default_midi() -> MidiSource {
        MidiSource::PlaybackApi
    }
    fn default_timer() -> TimerOutput {
        TimerOutput::Simulated
    }
}

impl Default for IntegrationModes {
    fn default() -> Self {
        Self {
            service_source: ServiceSource::Demo,
            midi_source: MidiSource::PlaybackApi,
            timer_output: TimerOutput::Simulated,
        }
    }
}

// ---- MIDI -------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MidiVelocityMappings {
    #[serde(default = "d100")]
    pub start_next: Option<u8>,
    #[serde(default = "d101")]
    pub restart_current: Option<u8>,
    #[serde(default = "d102")]
    pub previous: Option<u8>,
    #[serde(default = "d103")]
    pub next: Option<u8>,
    #[serde(default = "d104")]
    pub reload_plan: Option<u8>,
    #[serde(default = "d105")]
    pub stop_timer: Option<u8>,
}

fn d100() -> Option<u8> {
    Some(100)
}
fn d101() -> Option<u8> {
    Some(101)
}
fn d102() -> Option<u8> {
    Some(102)
}
fn d103() -> Option<u8> {
    Some(103)
}
fn d104() -> Option<u8> {
    Some(104)
}
fn d105() -> Option<u8> {
    Some(105)
}

impl Default for MidiVelocityMappings {
    fn default() -> Self {
        Self {
            start_next: d100(),
            restart_current: d101(),
            previous: d102(),
            next: d103(),
            reload_plan: d104(),
            stop_timer: d105(),
        }
    }
}

impl MidiVelocityMappings {
    pub fn configured(&self) -> Vec<(&'static str, u8)> {
        [
            ("start_next", self.start_next),
            ("restart_current", self.restart_current),
            ("previous", self.previous),
            ("next", self.next),
            ("reload_plan", self.reload_plan),
            ("stop_timer", self.stop_timer),
        ]
        .into_iter()
        .filter_map(|(name, v)| v.map(|v| (name, v)))
        .collect()
    }

    pub fn validate(&self, path: &[Value], errors: &mut Errors) {
        for (name, value) in self.configured() {
            check_range(errors, path, name, value, Some(1.0), Some(127.0), None);
        }
        let velocities: Vec<u8> = self.configured().into_iter().map(|(_, v)| v).collect();
        if velocities.iter().any(|v| *v < 100) {
            value_error(
                errors,
                path,
                "MIDI action velocities must be between 100 and 127; velocities 1 through 99 identify service-plan songs.",
                serde_json::to_value(self).unwrap_or(Value::Null),
            );
        } else {
            let unique: std::collections::BTreeSet<_> = velocities.iter().collect();
            if unique.len() != velocities.len() {
                value_error(
                    errors,
                    path,
                    "Every configured MIDI action must use a distinct velocity.",
                    serde_json::to_value(self).unwrap_or(Value::Null),
                );
            }
        }
    }
}

fn trim_or_none(value: &Option<String>) -> Option<String> {
    value
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MidiSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "MidiSettings::transport_default")]
    pub transport: MidiTransport,
    #[serde(default)]
    pub input_name: Option<String>,
    #[serde(default = "one_u8")]
    pub channel: u8,
    #[serde(default = "note_default")]
    pub note: u8,
    #[serde(default)]
    pub mappings: MidiVelocityMappings,
    #[serde(default = "debounce_default")]
    pub debounce_ms: u16,
}

fn one_u8() -> u8 {
    1
}
fn note_default() -> u8 {
    112
}
fn debounce_default() -> u16 {
    250
}

impl MidiSettings {
    fn transport_default() -> MidiTransport {
        MidiTransport::Local
    }
}

impl Default for MidiSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            transport: MidiTransport::Local,
            input_name: None,
            channel: 1,
            note: 112,
            mappings: MidiVelocityMappings::default(),
            debounce_ms: 250,
        }
    }
}

impl MidiSettings {
    pub fn normalise(&mut self) {
        self.input_name = trim_or_none(&self.input_name);
    }

    pub fn validate(&self, path: &[Value], errors: &mut Errors) {
        if let Some(name) = &self.input_name {
            check_len(errors, path, "input_name", name, 0, 512);
        }
        check_range(
            errors,
            path,
            "channel",
            self.channel,
            Some(1.0),
            Some(16.0),
            None,
        );
        check_range(
            errors,
            path,
            "note",
            self.note,
            Some(0.0),
            Some(127.0),
            None,
        );
        check_range(
            errors,
            path,
            "debounce_ms",
            self.debounce_ms,
            Some(0.0),
            Some(2000.0),
            None,
        );
        self.mappings.validate(&path_with(path, "mappings"), errors);
    }
}

// ---- playback ---------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(untagged)]
#[ts(export)]
pub enum SetlistId {
    Int(i64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PlaybackApiSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default = "port_8080")]
    pub port: u16,
    #[serde(default = "default_true")]
    pub auto_scan: bool,
    #[serde(default = "default_true")]
    pub fast_transport: bool,
    #[serde(default)]
    pub planning_center_update_service_type_id: Option<String>,
    #[serde(default)]
    pub song_order: Vec<i64>,
    #[serde(default)]
    pub captured_version: Option<i64>,
    #[serde(default)]
    pub setlist_id: Option<SetlistId>,
    #[serde(default)]
    pub scan_plan_key: Option<String>,
    #[serde(default)]
    pub lengths_applied_key: Option<String>,
    #[serde(default)]
    pub captured_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub song_lengths: Vec<Option<f64>>,
    #[serde(default)]
    pub skipped_song_ids: Vec<i64>,
    #[serde(default)]
    pub skipped_plan_song_ids: Vec<String>,
    #[serde(default)]
    pub discovery_duration_seconds: Option<f64>,
    #[serde(default)]
    pub lengths_measured_at: Option<DateTime<Utc>>,
}

fn port_8080() -> u16 {
    8080
}

impl Default for PlaybackApiSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            host: None,
            port: 8080,
            auto_scan: true,
            fast_transport: true,
            planning_center_update_service_type_id: None,
            song_order: vec![],
            captured_version: None,
            setlist_id: None,
            scan_plan_key: None,
            lengths_applied_key: None,
            captured_at: None,
            song_lengths: vec![],
            skipped_song_ids: vec![],
            skipped_plan_song_ids: vec![],
            discovery_duration_seconds: None,
            lengths_measured_at: None,
        }
    }
}

impl PlaybackApiSettings {
    pub fn normalise(&mut self) {
        self.host = trim_or_none(&self.host);
        // A scan of a new setlist replaces the song order but used to leave skips for songs that
        // are gone. Such a skip can never match a song, and keeping it made the whole settings
        // file invalid on the next launch (the app then ran on defaults). Drop it on load.
        let order = &self.song_order;
        self.skipped_song_ids.retain(|id| order.contains(id));
    }

    pub fn validate(&self, path: &[Value], errors: &mut Errors) {
        if let Some(host) = &self.host {
            check_len(errors, path, "host", host, 0, 255);
        }
        check_range(
            errors,
            path,
            "port",
            self.port,
            Some(1.0),
            Some(65535.0),
            None,
        );
        if self.song_order.len() > 200 {
            too_long(errors, path, "song_order", self.song_order.len(), 200);
        }
        if self.song_lengths.len() > 200 {
            too_long(errors, path, "song_lengths", self.song_lengths.len(), 200);
        }
        if let Some(d) = self.discovery_duration_seconds {
            check_range(
                errors,
                path,
                "discovery_duration_seconds",
                d,
                None,
                None,
                Some(0.0),
            );
        }
        let unique =
            |v: &[i64]| v.iter().collect::<std::collections::BTreeSet<_>>().len() == v.len();
        if !unique(&self.skipped_song_ids) {
            value_error(
                errors,
                &path_with(path, "skipped_song_ids"),
                "Skipped songs must be unique.",
                json!(self.skipped_song_ids),
            );
        }
        let ids: std::collections::BTreeSet<_> = self.skipped_plan_song_ids.iter().collect();
        if ids.len() != self.skipped_plan_song_ids.len() {
            value_error(
                errors,
                &path_with(path, "skipped_plan_song_ids"),
                "Skipped songs must be unique.",
                json!(self.skipped_plan_song_ids),
            );
        }
        if !unique(&self.song_order) {
            value_error(
                errors,
                &path_with(path, "song_order"),
                "Discovered song IDs must be unique and in setlist order.",
                json!(self.song_order),
            );
        }
        let whole = serde_json::to_value(self).unwrap_or(Value::Null);
        if !self.song_lengths.is_empty() && self.song_lengths.len() != self.song_order.len() {
            value_error(
                errors,
                path,
                "Song lengths must have the same length as song order.",
                whole.clone(),
            );
        } else if self
            .song_lengths
            .iter()
            .flatten()
            .any(|l| !(*l > 0.0 && *l <= 86400.0))
        {
            value_error(
                errors,
                path,
                "Song lengths must be finite positive seconds or null.",
                whole.clone(),
            );
        } else if !self
            .skipped_song_ids
            .iter()
            .all(|id| self.song_order.contains(id))
        {
            value_error(
                errors,
                path,
                "Skipped Playback songs must be songs in the saved order.",
                whole,
            );
        }
    }
}

fn too_long(errors: &mut Errors, path: &[Value], key: &str, len: usize, max: usize) {
    errors.push(Detail {
        kind: "too_long".into(),
        loc: path_with(path, key),
        msg: format!("List should have at most {max} items after validation, not {len}"),
        input: Value::Null,
        ctx: Some(json!({"field_type": "List", "max_length": max, "actual_length": len})),
    });
}

// ---- lights -----------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightingCue {
    #[serde(default = "Uuid::new_v4")]
    #[ts(as = "String")]
    pub id: Uuid,
    pub at_seconds: f64,
    pub note: u8,
    #[serde(default = "velocity_127")]
    pub velocity: u8,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub color: Option<PadColor>,
}

fn velocity_127() -> u8 {
    127
}

/// Cue times are kept to 0.1 s (the Live capture and timeline resolution).
pub fn round_tenths(value: f64) -> f64 {
    (value * 10.0).round_ties_even() / 10.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
#[ts(export)]
pub enum OscArgument {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OscLightingCue {
    #[serde(default = "Uuid::new_v4")]
    #[ts(as = "String")]
    pub id: Uuid,
    pub at_seconds: f64,
    pub address: String,
    #[serde(default)]
    pub args: Vec<OscArgument>,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub color: Option<PadColor>,
}

pub fn validate_osc_address(value: &str) -> Result<(), &'static str> {
    if !value.starts_with('/') {
        return Err("An OSC address must start with /.");
    }
    if value.chars().any(|c| c.is_whitespace() || (c as u32) < 32) {
        return Err("An OSC address cannot contain spaces.");
    }
    if value.chars().count() > 256 {
        return Err("An OSC address can be at most 256 characters.");
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightingPad {
    #[serde(default = "Uuid::new_v4")]
    #[ts(as = "String")]
    pub id: Uuid,
    pub label: String,
    #[serde(default)]
    pub color: Option<PadColor>,
    pub method: LightingMethod,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub args: Vec<OscArgument>,
    #[serde(default)]
    pub note: Option<u8>,
    #[serde(default = "velocity_127")]
    pub velocity: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SongLightingCueMap {
    pub song_key: String,
    pub song_title: String,
    #[serde(default)]
    pub cues: Vec<LightingCue>,
    #[serde(default)]
    pub osc_cues: Vec<OscLightingCue>,
}

/// Default port of the `lightkey-api` agent (`DEFAULT_LIGHTKEY_API_PORT`).
pub const DEFAULT_LIGHTKEY_API_PORT: u16 = 8770;
/// Default UDP port of the `lightkey-api` OSC relay (`relay_port`), on the same host as the API.
pub const DEFAULT_LIGHTKEY_RELAY_PORT: u16 = 21700;

/// Saved Lightkey link settings, part of `LightsSettings` (`LightkeySettings`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightkeySettings {
    /// Empty means "derive from the Lighting OSC host": `http://<osc_host>:8770`.
    #[serde(default)]
    pub api_url: String,
    /// Replay is on by default and only ever suspended by this switch (or by recording).
    #[serde(default = "default_true")]
    pub replay: bool,
}

impl Default for LightkeySettings {
    fn default() -> Self {
        Self {
            api_url: String::new(),
            replay: true,
        }
    }
}

const API_URL_MESSAGE: &str =
    "Enter the Lightkey API address as host or host:port, e.g. 10.30.0.211:8770.";

/// Python `str.isspace` (also true for the four ASCII separators `\x1c`..`\x1f`).
fn py_space(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// The `api_url` validator of `LightkeySettings`: trim, drop trailing `/`, default the scheme to
/// `http://`, refuse spaces and paths. `Err` carries the validator's message.
pub fn clean_lightkey_api_url(value: &str) -> Result<String, &'static str> {
    let text = value
        .trim_matches(py_space)
        .trim_end_matches('/')
        .to_string();
    if text.is_empty() {
        return Ok(String::new());
    }
    let text = if text.starts_with("http://") || text.starts_with("https://") {
        text
    } else {
        format!("http://{text}")
    };
    let rest = text.split_once("://").map_or("", |(_, rest)| rest);
    if rest.is_empty() || text.chars().any(py_space) || rest.contains('/') {
        return Err(API_URL_MESSAGE);
    }
    Ok(text)
}

impl LightkeySettings {
    /// Validate `api_url` as pydantic would after the `before` validator ran. Paths are relative
    /// to `path` (the `lightkey` object).
    pub fn validate(&self, path: &[Value], errors: &mut Errors) {
        match clean_lightkey_api_url(&self.api_url) {
            Err(message) => value_error(
                errors,
                &path_with(path, "api_url"),
                message,
                json!(self.api_url),
            ),
            Ok(clean) => check_len(errors, path, "api_url", &clean, 0, 300),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightsSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "osc_method")]
    pub method: LightingMethod,
    #[serde(default = "transport_local")]
    pub transport: MidiTransport,
    #[serde(default)]
    pub output_name: Option<String>,
    #[serde(default = "one_u8")]
    pub channel: u8,
    #[serde(default = "pulse_default")]
    pub pulse_ms: u16,
    #[serde(default = "osc_host_default")]
    pub osc_host: String,
    #[serde(default = "osc_port_default")]
    pub osc_port: u16,
    #[serde(default)]
    pub cue_maps: BTreeMap<String, SongLightingCueMap>,
    #[serde(default)]
    pub pads: Vec<LightingPad>,
    #[serde(default)]
    pub lightkey: LightkeySettings,
}

fn osc_method() -> LightingMethod {
    LightingMethod::Osc
}
fn transport_local() -> MidiTransport {
    MidiTransport::Local
}
fn pulse_default() -> u16 {
    100
}
fn osc_host_default() -> String {
    "127.0.0.1".into()
}
fn osc_port_default() -> u16 {
    21_600
}

impl Default for LightsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            method: LightingMethod::Osc,
            transport: MidiTransport::Local,
            output_name: None,
            channel: 1,
            pulse_ms: 100,
            osc_host: osc_host_default(),
            osc_port: 21_600,
            cue_maps: BTreeMap::new(),
            pads: vec![],
            lightkey: LightkeySettings::default(),
        }
    }
}

impl LightsSettings {
    /// Apply the field normalisers (trimming, tenths rounding, cue ordering) of the Python
    /// validators. Called after deserialisation, before `validate`.
    pub fn normalise(&mut self) {
        self.output_name = trim_or_none(&self.output_name);
        self.osc_host = self.osc_host.trim().to_string();
        if let Ok(clean) = clean_lightkey_api_url(&self.lightkey.api_url) {
            self.lightkey.api_url = clean;
        }
        for pad in &mut self.pads {
            pad.label = pad.label.trim().to_string();
            pad.address = pad.address.as_ref().map(|a| a.trim().to_string());
            if pad.method == LightingMethod::Osc {
                pad.note = None;
            } else {
                pad.address = None;
                pad.args.clear();
            }
        }
        for map in self.cue_maps.values_mut() {
            map.song_key = map.song_key.trim().to_string();
            map.song_title = map.song_title.trim().to_string();
            for cue in &mut map.cues {
                cue.at_seconds = round_tenths(cue.at_seconds);
                cue.label = cue.label.trim().to_string();
            }
            for cue in &mut map.osc_cues {
                cue.at_seconds = round_tenths(cue.at_seconds);
                cue.address = cue.address.trim().to_string();
                cue.label = cue.label.trim().to_string();
            }
            map.cues.sort_by(|a, b| {
                a.at_seconds
                    .total_cmp(&b.at_seconds)
                    .then_with(|| a.id.to_string().cmp(&b.id.to_string()))
            });
            map.osc_cues.sort_by(|a, b| {
                a.at_seconds
                    .total_cmp(&b.at_seconds)
                    .then_with(|| a.id.to_string().cmp(&b.id.to_string()))
            });
        }
    }

    pub fn validate(&self, path: &[Value], errors: &mut Errors) {
        if let Some(name) = &self.output_name {
            check_len(errors, path, "output_name", name, 0, 512);
        }
        check_range(
            errors,
            path,
            "channel",
            self.channel,
            Some(1.0),
            Some(15.0),
            None,
        );
        check_range(
            errors,
            path,
            "pulse_ms",
            self.pulse_ms,
            Some(10.0),
            Some(2000.0),
            None,
        );
        check_len(errors, path, "osc_host", &self.osc_host, 1, 253);
        if self.osc_host.chars().any(char::is_whitespace) {
            value_error(
                errors,
                &path_with(path, "osc_host"),
                "Enter an OSC host name or IP address without spaces.",
                json!(self.osc_host),
            );
        }
        check_range(
            errors,
            path,
            "osc_port",
            self.osc_port,
            Some(1.0),
            Some(65535.0),
            None,
        );
        if self.pads.len() > 64 {
            too_long(errors, path, "pads", self.pads.len(), 64);
        }
        self.lightkey.validate(&path_with(path, "lightkey"), errors);
        for (i, pad) in self.pads.iter().enumerate() {
            let p = {
                let mut p = path_with(path, "pads");
                p.push(json!(i));
                p
            };
            check_len(errors, &p, "label", &pad.label, 1, 60);
            match pad.method {
                LightingMethod::Osc => match &pad.address {
                    None => {
                        value_error(errors, &p, "An OSC pad needs an OSC address.", Value::Null)
                    }
                    Some(a) => {
                        if let Err(m) = validate_osc_address(a) {
                            value_error(errors, &p, m, json!(a));
                        }
                    }
                },
                LightingMethod::Midi => {
                    if pad.note.is_none() {
                        value_error(errors, &p, "A MIDI pad needs a MIDI note.", Value::Null);
                    }
                }
            }
            if let Some(n) = pad.note {
                check_range(errors, &p, "note", n, Some(0.0), Some(127.0), None);
            }
            check_range(
                errors,
                &p,
                "velocity",
                pad.velocity,
                Some(1.0),
                Some(127.0),
                None,
            );
        }
        let pad_ids: std::collections::BTreeSet<_> = self.pads.iter().map(|p| p.id).collect();
        for (key, map) in &self.cue_maps {
            if key != &map.song_key {
                value_error(
                    errors,
                    path,
                    "Every lighting cue-map key must match its song key.",
                    json!(key),
                );
            }
            let mp = {
                let mut p = path_with(path, "cue_maps");
                p.push(json!(key));
                p
            };
            check_len(errors, &mp, "song_key", &map.song_key, 1, 256);
            check_len(errors, &mp, "song_title", &map.song_title, 1, 500);
            for cue in &map.cues {
                check_range(
                    errors,
                    &mp,
                    "at_seconds",
                    cue.at_seconds,
                    Some(0.0),
                    Some(86_399.0),
                    None,
                );
                check_range(errors, &mp, "note", cue.note, Some(0.0), Some(127.0), None);
                check_range(
                    errors,
                    &mp,
                    "velocity",
                    cue.velocity,
                    Some(1.0),
                    Some(127.0),
                    None,
                );
            }
            for cue in &map.osc_cues {
                check_range(
                    errors,
                    &mp,
                    "at_seconds",
                    cue.at_seconds,
                    Some(0.0),
                    Some(86_399.0),
                    None,
                );
                if let Err(m) = validate_osc_address(&cue.address) {
                    value_error(errors, &mp, m, json!(cue.address));
                }
            }
            let ids =
                |v: Vec<Uuid>| v.iter().collect::<std::collections::BTreeSet<_>>().len() == v.len();
            if !ids(map.cues.iter().map(|c| c.id).collect())
                || !ids(map.osc_cues.iter().map(|c| c.id).collect())
            {
                value_error(
                    errors,
                    &mp,
                    "Every lighting cue must have a unique ID.",
                    Value::Null,
                );
            }
        }
        if pad_ids.len() != self.pads.len() {
            value_error(
                errors,
                path,
                "Every lighting pad must have a unique ID.",
                Value::Null,
            );
        }
    }
}

// ---- network MIDI / ProPresenter --------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NetworkMidiSettings {
    #[serde(default = "socket_default")]
    pub socket_path: String,
    #[serde(default = "two_f64")]
    pub request_timeout_seconds: f64,
}

fn socket_default() -> String {
    "/run/stagepilot-midi/midi.sock".into()
}
fn two_f64() -> f64 {
    2.0
}

impl Default for NetworkMidiSettings {
    fn default() -> Self {
        Self {
            socket_path: socket_default(),
            request_timeout_seconds: 2.0,
        }
    }
}

impl NetworkMidiSettings {
    pub fn normalise(&mut self) {
        self.socket_path = self.socket_path.trim().to_string();
    }
    pub fn validate(&self, path: &[Value], errors: &mut Errors) {
        if self.socket_path.is_empty() {
            value_error(
                errors,
                &path_with(path, "socket_path"),
                "Network MIDI socket path cannot be empty.",
                json!(""),
            );
        } else {
            check_len(errors, path, "socket_path", &self.socket_path, 1, 1024);
        }
        check_range(
            errors,
            path,
            "request_timeout_seconds",
            self.request_timeout_seconds,
            None,
            Some(30.0),
            Some(0.0),
        );
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProPresenterSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "pp_host")]
    pub host: String,
    #[serde(default = "pp_port")]
    pub port: u16,
    #[serde(default = "pp_timer")]
    pub timer_name: String,
    #[serde(default)]
    pub look_id: Option<String>,
    #[serde(default = "three_f64")]
    pub request_timeout_seconds: f64,
    #[serde(default = "one_f64")]
    pub reconnect_initial_seconds: f64,
    #[serde(default = "thirty_f64")]
    pub reconnect_max_seconds: f64,
    #[serde(default = "ten_f64")]
    pub health_check_interval_seconds: f64,
}

fn pp_host() -> String {
    "127.0.0.1".into()
}
fn pp_port() -> u16 {
    1025
}
fn pp_timer() -> String {
    "Song Countdown".into()
}
fn three_f64() -> f64 {
    3.0
}
fn one_f64() -> f64 {
    1.0
}
fn thirty_f64() -> f64 {
    30.0
}
fn ten_f64() -> f64 {
    10.0
}

impl Default for ProPresenterSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            host: pp_host(),
            port: 1025,
            timer_name: pp_timer(),
            look_id: None,
            request_timeout_seconds: 3.0,
            reconnect_initial_seconds: 1.0,
            reconnect_max_seconds: 30.0,
            health_check_interval_seconds: 10.0,
        }
    }
}

impl ProPresenterSettings {
    pub fn normalise(&mut self) {
        self.host = self.host.trim().to_string();
        self.timer_name = self.timer_name.trim().to_string();
        self.look_id = trim_or_none(&self.look_id);
    }
    pub fn validate(&self, path: &[Value], errors: &mut Errors) {
        for (key, v) in [("host", &self.host), ("timer_name", &self.timer_name)] {
            if v.is_empty() {
                value_error(
                    errors,
                    &path_with(path, key),
                    "Value cannot be empty.",
                    json!(""),
                );
            } else {
                check_len(errors, path, key, v, 1, 255);
            }
        }
        check_range(
            errors,
            path,
            "port",
            self.port,
            Some(1.0),
            Some(65535.0),
            None,
        );
        if let Some(l) = &self.look_id {
            check_len(errors, path, "look_id", l, 0, 256);
        }
        check_range(
            errors,
            path,
            "request_timeout_seconds",
            self.request_timeout_seconds,
            None,
            Some(60.0),
            Some(0.0),
        );
        check_range(
            errors,
            path,
            "reconnect_initial_seconds",
            self.reconnect_initial_seconds,
            None,
            Some(60.0),
            Some(0.0),
        );
        check_range(
            errors,
            path,
            "reconnect_max_seconds",
            self.reconnect_max_seconds,
            None,
            Some(300.0),
            Some(0.0),
        );
        check_range(
            errors,
            path,
            "health_check_interval_seconds",
            self.health_check_interval_seconds,
            None,
            Some(300.0),
            Some(0.0),
        );
        if self.reconnect_max_seconds < self.reconnect_initial_seconds {
            value_error(
                errors,
                path,
                "ProPresenter reconnect maximum must be greater than or equal to the initial delay.",
                Value::Null,
            );
        }
    }
}
