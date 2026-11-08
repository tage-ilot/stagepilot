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
    /// Learn which Playback song is which plan song while songs play (until a scan gives the
    /// exact order). Older settings files do not have it and get the default, on.
    #[serde(default = "default_true")]
    pub learn_order: bool,
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
            learn_order: true,
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

/// Rules for the `effects` of one song (finite numbers, ranges, caps). Fixture names follow the
/// look-message address rule; property names are the last address part, so no `/`.
fn validate_effects(effects: &[LightingEffect], errors: &mut Errors, path: &[Value]) {
    if effects.len() > MAX_EFFECTS_PER_SONG {
        too_long(errors, path, "effects", effects.len(), MAX_EFFECTS_PER_SONG);
    }
    let mut step_total = 0usize;
    for (i, effect) in effects.iter().enumerate() {
        let mut ep = path.to_vec();
        ep.push(json!("effects"));
        ep.push(json!(i));
        check_range(
            errors,
            &ep,
            "start_seconds",
            effect.start_seconds,
            Some(0.0),
            Some(86_399.0),
            None,
        );
        check_range(
            errors,
            &ep,
            "end_seconds",
            effect.end_seconds,
            Some(effect.start_seconds.max(0.0)),
            Some(86_400.0),
            None,
        );
        check_range(
            errors,
            &ep,
            "period_seconds",
            effect.period_seconds,
            Some(MIN_EFFECT_PERIOD_SECONDS),
            Some(MAX_EFFECT_PERIOD_SECONDS),
            None,
        );
        if effect.members.is_empty() || effect.members.len() > MAX_EFFECT_MEMBERS {
            if effect.members.is_empty() {
                value_error(
                    errors,
                    &ep,
                    "An effect needs at least one member.",
                    json!(effect.members.len()),
                );
            } else {
                too_long(
                    errors,
                    &ep,
                    "members",
                    effect.members.len(),
                    MAX_EFFECT_MEMBERS,
                );
            }
        }
        if effect.cycle.is_empty() {
            value_error(
                errors,
                &ep,
                "An effect needs at least one cycle step.",
                json!(0),
            );
        }
        if effect.cycle.len() > MAX_EFFECT_CYCLE_STEPS {
            too_long(
                errors,
                &ep,
                "cycle",
                effect.cycle.len(),
                MAX_EFFECT_CYCLE_STEPS,
            );
        }
        step_total += effect.cycle.len();
        for member in &effect.members {
            let address = format!("/fixture/{}/overrides/x", member.fixture);
            if member.fixture.is_empty()
                || member.fixture.contains('/')
                || validate_osc_message_address(&address).is_err()
            {
                value_error(
                    errors,
                    &ep,
                    "An effect member needs a fixture name without '/'.",
                    json!(member.fixture),
                );
            }
            check_range(
                errors,
                &ep,
                "offset_seconds",
                member.offset_seconds,
                Some(0.0),
                Some(effect.period_seconds.clamp(0.0, MAX_EFFECT_PERIOD_SECONDS)),
                None,
            );
        }
        for step in &effect.cycle {
            check_range(
                errors,
                &ep,
                "at_seconds",
                step.at_seconds,
                Some(0.0),
                Some(effect.period_seconds.clamp(0.0, MAX_EFFECT_PERIOD_SECONDS)),
                None,
            );
            check_range(
                errors,
                &ep,
                "fade_seconds",
                step.fade_seconds,
                Some(0.0),
                Some(MAX_FADE_SECONDS),
                None,
            );
            if step.property.is_empty() || step.property.contains('/') || step.property.len() > 128
            {
                value_error(
                    errors,
                    &ep,
                    "An effect step needs a property name without '/'.",
                    json!(step.property),
                );
            }
            if step.value.is_empty() || step.value.len() > MAX_EFFECT_VALUES {
                value_error(
                    errors,
                    &ep,
                    "An effect step needs 1 to 8 numbers.",
                    json!(step.value.len()),
                );
            }
            if step.value.iter().any(|v| !v.is_finite()) {
                value_error(
                    errors,
                    &ep,
                    "An effect step value must be a finite number.",
                    Value::Null,
                );
            }
        }
    }
    if step_total > MAX_EFFECT_CYCLE_STEPS_PER_SONG {
        too_long(
            errors,
            path,
            "effects",
            step_total,
            MAX_EFFECT_CYCLE_STEPS_PER_SONG,
        );
    }
}

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

/// Times of fitted ramps and effects are kept to a millisecond (a ramp is fitted to samples a
/// few milliseconds apart; 0.1 s would move its corners).
pub fn round_millis(value: f64) -> f64 {
    (value * 1000.0).round_ties_even() / 1000.0
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
    /// A "look": several OSC messages sent together at this row's time. When non-empty they are
    /// sent instead of `address`/`args`. Left out of the saved file while empty, so a plain cue
    /// saves exactly as before.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<OscMessage>>")]
    pub messages: Vec<OscMessage>,
    /// Where the row came from. Left out of the saved file while `manual`.
    #[serde(default, skip_serializing_if = "OscCueSource::is_manual")]
    #[ts(optional, as = "Option<OscCueSource>")]
    pub source: OscCueSource,
    /// Set when someone changed a captured look by hand, so a later recording does not replace it
    /// silently. Left out of the saved file while false.
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional, as = "Option<bool>")]
    pub edited: bool,
    /// Seconds the row's messages take to reach their values, starting at `at_seconds`. Every
    /// numeric message ramps linearly from that address's value just before the row; options,
    /// text and anything that is not a plain number step at once. `0` (the default, left out of
    /// the saved file) is an instant change, exactly as rows always were.
    #[serde(default, skip_serializing_if = "is_zero")]
    #[ts(optional, as = "Option<f64>")]
    pub fade_seconds: f64,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

/// One OSC message of a look row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OscMessage {
    pub address: String,
    #[serde(default)]
    pub args: Vec<OscArgument>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum OscCueSource {
    #[default]
    Manual,
    Captured,
}

impl OscCueSource {
    pub fn is_manual(&self) -> bool {
        *self == OscCueSource::Manual
    }
}

/// Most messages one look row may hold (a rig has about 106 fixture addresses; headroom).
pub const MAX_OSC_MESSAGES_PER_ROW: usize = 256;
/// Most OSC rows one song may hold. A busy 5-minute song on a 66-fixture rig is about 12,000
/// rows at full fidelity (2026-11-08), so the cap leaves room for songs several times busier.
/// A recording that still does not fit is simplified (see `stagepilot_lights::lightkey_fit`),
/// never dropped.
pub const MAX_OSC_ROWS_PER_SONG: usize = 50_000;
/// Most arguments one look message may carry, and the longest text argument (M-01).
pub const MAX_OSC_ARGS_PER_MESSAGE: usize = 8;
pub const MAX_OSC_STRING_CHARS: usize = 256;
/// Most look messages one song may hold in total.
pub const MAX_OSC_MESSAGES_PER_SONG: usize = 600_000;
/// Longest a row (or an effect step) may take to reach its value. A captured ramp is at most
/// 60 s; hand-made fades may be longer.
pub const MAX_FADE_SECONDS: f64 = 600.0;
/// Most effects (detected loops) one song may hold. A busy rig has a handful per group; 1,000
/// leaves room for every group changing its loop many times in a song.
pub const MAX_EFFECTS_PER_SONG: usize = 1_000;
/// Most fixtures one effect may drive (a rig has about 106 addresses; same bound as a look row).
pub const MAX_EFFECT_MEMBERS: usize = 256;
/// Most cycle steps one effect may hold (a loop of 60 s fitted at 25 ms would be 2,400 raw
/// points; the fit keeps tens).
pub const MAX_EFFECT_CYCLE_STEPS: usize = 4_096;
/// Most cycle steps in all effects of one song.
pub const MAX_EFFECT_CYCLE_STEPS_PER_SONG: usize = 250_000;
/// Longest period of an effect, seconds; the shortest is `MIN_EFFECT_PERIOD_SECONDS`.
pub const MAX_EFFECT_PERIOD_SECONDS: f64 = 600.0;
pub const MIN_EFFECT_PERIOD_SECONDS: f64 = 0.1;
/// Most numbers one effect step may carry (a colour has 3; same bound as message arguments).
pub const MAX_EFFECT_VALUES: usize = MAX_OSC_ARGS_PER_MESSAGE;

/// One fixture that plays an effect, shifted in phase (a chase is the same loop at different
/// offsets).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EffectMember {
    /// Fixture name as in `/fixture/<name>/overrides/<property>`.
    pub fixture: String,
    /// The fixture plays the cycle this many seconds later than the effect's start.
    #[serde(default, skip_serializing_if = "is_zero")]
    #[ts(optional, as = "Option<f64>")]
    pub offset_seconds: f64,
}

/// One ramp of an effect's cycle: from `at_seconds` (inside the cycle) the property reaches
/// `value` over `fade_seconds`, starting from the value the previous step of the same property
/// ended on (the first step starts from the last step's value, so the loop closes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EffectStep {
    pub at_seconds: f64,
    /// Property name: the last part of `/fixture/<name>/overrides/<property>`.
    pub property: String,
    /// The value reached (one number per argument: 1 for a dimmer, 3 for a colour).
    pub value: Vec<f64>,
    #[serde(default, skip_serializing_if = "is_zero")]
    #[ts(optional, as = "Option<f64>")]
    pub fade_seconds: f64,
}

/// A repeating pattern found in the recording, stored as ONE item: from `start_seconds` to
/// `end_seconds` every member fixture plays `cycle` over and over, `period_seconds` long. The
/// effect drives the properties named in `cycle` of its members for that window; outside it the
/// rows apply. It is edited as a unit and is never changed by editing rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightingEffect {
    #[serde(default = "Uuid::new_v4")]
    #[ts(as = "String")]
    pub id: Uuid,
    #[serde(default)]
    pub label: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub period_seconds: f64,
    pub members: Vec<EffectMember>,
    pub cycle: Vec<EffectStep>,
    /// Where the effect came from. Left out of the saved file while `manual`.
    #[serde(default, skip_serializing_if = "OscCueSource::is_manual")]
    #[ts(optional, as = "Option<OscCueSource>")]
    pub source: OscCueSource,
    /// Set when someone changed a captured effect by hand. Left out while false.
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional, as = "Option<bool>")]
    pub edited: bool,
}

impl OscLightingCue {
    /// What the row sends: its `messages`, or else the single address and arguments.
    pub fn outgoing(&self) -> Vec<OscMessage> {
        if self.messages.is_empty() {
            vec![OscMessage {
                address: self.address.clone(),
                args: self.args.clone(),
            }]
        } else {
            self.messages.clone()
        }
    }
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

/// Address rule for a look message. Same as `validate_osc_address`, except that a
/// `/fixture/...` address may contain spaces: Lightkey fixture names do ("Moving Head 1").
pub fn validate_osc_message_address(value: &str) -> Result<(), &'static str> {
    if value.starts_with("/fixture/") {
        if value.chars().any(|c| (c as u32) < 32) {
            return Err("An OSC address cannot contain control characters.");
        }
        if value.chars().count() > 256 {
            return Err("An OSC address can be at most 256 characters.");
        }
        return Ok(());
    }
    validate_osc_address(value)
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
    /// How loud the room was while the song was recorded (levels only, never audio). Absent
    /// until a recording captured it, so older files load and save back unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub loudness: Option<SongLoudness>,
    /// Captured look rows of this song were changed by hand (a whole row deleted, for one):
    /// Record asks before replacing them. Left out while false; a new recording clears it.
    #[serde(default, skip_serializing_if = "flag_unset")]
    #[ts(optional, as = "Option<bool>")]
    pub captured_edited: bool,
    /// How much of the song the captured rows cover. Absent on older cue maps and on songs with
    /// no capture: captured rows without it count as an incomplete capture and are re-captured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub capture: Option<CaptureStatus>,
    /// Repeating patterns found in the recording (see [`LightingEffect`]). Absent on older cue
    /// maps and on songs with none: they load and save back unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<LightingEffect>>")]
    pub effects: Vec<LightingEffect>,
}

/// The state of a song's captured rows (`SongLightingCueMap::capture`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CaptureStatus {
    /// A pass that recorded all of the song but at most 10 s in total (wherever those seconds
    /// are: a link drop, a seek, a pause, a late start) and at least one change. Only a complete
    /// capture is replayed by Auto.
    pub complete: bool,
    /// Seconds of the song the pass recorded in total (the union of the recorded stretches).
    pub coverage_seconds: f64,
    /// When the pass was saved, RFC 3339 UTC.
    pub recorded_at: String,
    /// The Lightkey project (base name) that was open when the recording began, as the helper
    /// reported it. Absent on older captures and when the helper did not say; such a capture
    /// never triggers the "different project" notice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<String>")]
    pub project: Option<String>,
    /// Set when the song was too busy to keep as recorded and was simplified to fit. Absent on
    /// a normal capture and on older files (an older build ignores the key).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<CaptureSimplified>")]
    pub simplified: Option<CaptureSimplified>,
    /// How many lights the capture's starting look holds (the state of the whole rig when the
    /// recording began, stored at the first look so the replay recreates the picture whatever
    /// presets are on). `Some(0)`: the pass tried but the relay's state could not be read.
    /// Absent: the capture was recorded before the starting look was saved (older files and
    /// older builds), so its house lights are not in it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<u32>")]
    pub start_lights: Option<u32>,
}

/// How a too-busy recording was simplified so that it fits the per-song caps
/// (`CaptureStatus::simplified`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CaptureSimplified {
    /// `loosened` (ramps fitted at a looser tolerance), `thinned` (steps closer than
    /// `thin_seconds` merged) or `trimmed` (the least important changes dropped).
    pub stage: String,
    /// Tolerance multiplier of the fit (1 = exact).
    pub tolerance_scale: f64,
    /// Steps closer than this many seconds were merged (0 = not thinned).
    pub thin_seconds: f64,
    /// Changes dropped by the last-resort trim (0 = none).
    #[serde(default)]
    pub trimmed: u32,
    /// Recorded changes the saved capture still reproduces, and how many were recorded.
    pub kept_changes: u32,
    pub recorded_changes: u32,
}

/// What a song's cue map says about capturing (the chip on the songs strip).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CaptureLabel {
    NotCaptured,
    Incomplete,
    FullyCaptured,
    ManuallyCued,
    ManuallyCuedAndFullyCaptured,
}

impl SongLightingCueMap {
    pub fn has_captured_rows(&self) -> bool {
        self.osc_cues
            .iter()
            .any(|row| row.source == OscCueSource::Captured)
            || self
                .effects
                .iter()
                .any(|effect| effect.source == OscCueSource::Captured)
    }

    /// Hand-built rows: manual look rows and MIDI cues.
    pub fn has_manual_rows(&self) -> bool {
        !self.cues.is_empty()
            || self
                .osc_cues
                .iter()
                .any(|row| row.source != OscCueSource::Captured)
            || self
                .effects
                .iter()
                .any(|effect| effect.source != OscCueSource::Captured)
    }

    /// Captured rows exist and a complete pass produced them.
    pub fn is_fully_captured(&self) -> bool {
        self.has_captured_rows() && self.capture.as_ref().is_some_and(|c| c.complete)
    }

    /// Captured rows recorded before the starting look was saved: they hold only the fixtures that
    /// changed, so the house lights are missing. Replay is unchanged; the Lighting screen says
    /// to record the song again.
    pub fn recorded_before_start_look(&self) -> bool {
        self.has_captured_rows()
            && self
                .capture
                .as_ref()
                .is_none_or(|c| c.start_lights.is_none())
    }

    pub fn capture_label(&self) -> CaptureLabel {
        let captured = self.has_captured_rows();
        let complete = self.is_fully_captured();
        let manual = self.has_manual_rows();
        match (captured, complete, manual) {
            (true, true, true) => CaptureLabel::ManuallyCuedAndFullyCaptured,
            (true, true, false) => CaptureLabel::FullyCaptured,
            (true, false, _) => CaptureLabel::Incomplete,
            (false, _, true) => CaptureLabel::ManuallyCued,
            (false, _, false) => CaptureLabel::NotCaptured,
        }
    }
}

/// Most loudness values one song keeps (0.25 s steps cover a 1000 s song).
pub const MAX_LOUDNESS_VALUES: usize = 4_000;
/// Recordings averaged into one loudness envelope; capped so it can still adapt.
pub const MAX_LOUDNESS_TAKES: u32 = 8;

/// A song's loudness envelope: one byte per `step_seconds` slot. `0` means "no data" (a pause or
/// a missed slot); `1..=255` maps -60..0 dBFS. dBFS is a digital level, not sound pressure (SPL).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SongLoudness {
    pub step_seconds: f64,
    pub values: Vec<u8>,
    /// How many recordings are averaged in (1 to 8).
    pub takes: u32,
}

fn flag_unset(value: &bool) -> bool {
    !*value
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
    /// Which songs capture and which replay. `auto` (default): the link decides per song when
    /// its timer starts. `replay` and `record` pin it for every song. `record` is also what the
    /// Record switch (`/lights/lightkey/recording`) sets, so it is never saved by the settings
    /// route. `replay` stays the master switch: `false` replays nothing in any mode.
    #[serde(default, skip_serializing_if = "LightkeyMode::is_auto")]
    #[ts(optional, as = "Option<LightkeyMode>")]
    pub mode: LightkeyMode,
    /// How finely a recording is kept as timeline rows (see `LightkeyDetail`). Left out of the
    /// saved file while `normal`, so an older file saves back unchanged.
    #[serde(default, skip_serializing_if = "LightkeyDetail::is_normal")]
    #[ts(optional, as = "Option<LightkeyDetail>")]
    pub detail: LightkeyDetail,
    /// Name of the audio input that measures loudness while recording. Unset means the system
    /// default input; a name that is no longer present falls back to the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub audio_device: Option<String>,
    /// The Lighting computer the automatic search last connected to (`http://host:port`). Used
    /// only while `api_url` is blank: the address in use is this one, else the OSC host on port
    /// 8770. Left out of the saved file while empty, so an older file saves back unchanged (an
    /// rc.7/rc.8 build reads a file that has it as an unknown key and drops it).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub found_url: String,
}

impl Default for LightkeySettings {
    fn default() -> Self {
        Self {
            api_url: String::new(),
            replay: true,
            mode: LightkeyMode::default(),
            detail: LightkeyDetail::default(),
            audio_device: None,
            found_url: String::new(),
        }
    }
}

/// `LightkeySettings::mode`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum LightkeyMode {
    #[default]
    Auto,
    Replay,
    Record,
}

impl LightkeyMode {
    pub fn is_auto(&self) -> bool {
        *self == Self::Auto
    }
}

/// Detail of a recorded look: the grid its rows sit on and the closest spacing of two kept
/// changes of one address. Finer is smoother on replay and makes more rows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum LightkeyDetail {
    /// 0.25 s.
    Fine,
    /// 0.5 s.
    #[default]
    Normal,
    /// 1 s.
    Coarse,
}

impl LightkeyDetail {
    pub fn is_normal(&self) -> bool {
        *self == Self::Normal
    }

    /// The thinning interval and row grid, in seconds.
    pub fn interval_seconds(self) -> f64 {
        match self {
            Self::Fine => 0.25,
            Self::Normal => 0.5,
            Self::Coarse => 1.0,
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
    /// Per song, how many times its cue map has changed. The settings store bumps it on every
    /// save that changes a song's rows; a cue-map PUT that carries an older number is refused
    /// with 409, so an edit made on a stale list can never overwrite a fresh recording.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[ts(optional, as = "Option<BTreeMap<String, u32>>")]
    pub cue_revisions: BTreeMap<String, u32>,
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
            cue_revisions: BTreeMap::new(),
        }
    }
}

impl SongLightingCueMap {
    /// Trim, round and sort one song's map exactly as saving the Lighting settings does.
    pub fn normalise(&mut self) {
        let map = self;
        map.song_key = map.song_key.trim().to_string();
        map.song_title = map.song_title.trim().to_string();
        for cue in &mut map.cues {
            cue.at_seconds = round_tenths(cue.at_seconds);
            cue.label = cue.label.trim().to_string();
        }
        for cue in &mut map.osc_cues {
            // Fitted (captured) rows and fades keep millisecond times; hand-made instant
            // rows stay on the 0.1 s grid.
            cue.at_seconds = if cue.fade_seconds > 0.0 || cue.source == OscCueSource::Captured {
                round_millis(cue.at_seconds)
            } else {
                round_tenths(cue.at_seconds)
            };
            if cue.fade_seconds.is_finite() {
                cue.fade_seconds = round_millis(cue.fade_seconds);
            }
            cue.address = cue.address.trim().to_string();
            cue.label = cue.label.trim().to_string();
            for message in &mut cue.messages {
                message.address = message.address.trim().to_string();
            }
        }
        map.cues.sort_by(|a, b| {
            a.at_seconds
                .total_cmp(&b.at_seconds)
                .then_with(|| a.id.to_string().cmp(&b.id.to_string()))
        });
        // Rows at the same time: an instant change before a ramp (a ramp starts from the
        // value the instant change set), then by id.
        map.osc_cues.sort_by(|a, b| {
            a.at_seconds
                .total_cmp(&b.at_seconds)
                .then_with(|| a.fade_seconds.total_cmp(&b.fade_seconds))
                .then_with(|| a.id.to_string().cmp(&b.id.to_string()))
        });
        for effect in &mut map.effects {
            effect.label = effect.label.trim().to_string();
            effect.start_seconds = round_millis(effect.start_seconds);
            effect.end_seconds = round_millis(effect.end_seconds);
            effect.period_seconds = round_millis(effect.period_seconds);
            for member in &mut effect.members {
                member.fixture = member.fixture.trim().to_string();
                member.offset_seconds = round_millis(member.offset_seconds);
            }
            for step in &mut effect.cycle {
                step.property = step.property.trim().to_string();
                step.at_seconds = round_millis(step.at_seconds);
                step.fade_seconds = round_millis(step.fade_seconds);
            }
        }
        map.effects.sort_by(|a, b| {
            a.start_seconds
                .total_cmp(&b.start_seconds)
                .then_with(|| a.id.to_string().cmp(&b.id.to_string()))
        });
    }

    /// Repair what can be repaired without losing anything and return what changed, in plain
    /// words (empty when nothing changed). A phase offset is periodic, so an offset past the
    /// period (or below zero) is the same phase as the offset reduced into `0..=period`. Nothing
    /// else is touched: every other rule violation is left for the validation to report.
    pub fn repair_lossless(&mut self) -> Vec<String> {
        let mut changes = Vec::new();
        for (i, effect) in self.effects.iter_mut().enumerate() {
            let period = round_millis(effect.period_seconds);
            for member in &mut effect.members {
                if let Some(fixed) = reduce_phase(member.offset_seconds, period) {
                    if fixed != member.offset_seconds {
                        changes.push(format!(
                            "effect {i}, {}: offset {} s reduced to {} s (same phase, period {} s)",
                            member.fixture, member.offset_seconds, fixed, period
                        ));
                        member.offset_seconds = fixed;
                    }
                }
            }
        }
        changes
    }
}

/// An effect member's phase offset reduced into `0..=period` (same phase), rounded to
/// milliseconds and never above the rounded period. `None` when the numbers cannot be reduced
/// (not finite, or a period outside the allowed range): that is not a lossless repair.
pub fn reduce_phase(offset: f64, period: f64) -> Option<f64> {
    if !offset.is_finite() || !period.is_finite() {
        return None;
    }
    let period = round_millis(period);
    if !(MIN_EFFECT_PERIOD_SECONDS..=MAX_EFFECT_PERIOD_SECONDS).contains(&period) {
        return None;
    }
    if (0.0..=period).contains(&offset) {
        return Some(offset);
    }
    let reduced = round_millis(offset.rem_euclid(period)).clamp(0.0, period);
    // `-0.0` and friends: a plain zero.
    Some(if reduced == 0.0 { 0.0 } else { reduced })
}

/// Check one song's map with the rules the settings loader uses (after the same normalising).
/// `Err` carries a plain-language reason.
pub fn check_cue_map(map: &SongLightingCueMap) -> Result<(), String> {
    let mut map = map.clone();
    map.normalise();
    let mut errors = Vec::new();
    validate_cue_map(&map.song_key.clone(), &map, &[json!("lights")], &mut errors);
    match errors.first() {
        None => Ok(()),
        Some(first) => Err(format!(
            "{} (at {}{})",
            first.msg,
            first
                .loc
                .iter()
                .skip(3)
                .map(|p| match p {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect::<Vec<_>>()
                .join("."),
            if errors.len() > 1 {
                format!("; {} more problems", errors.len() - 1)
            } else {
                String::new()
            }
        )),
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
        // The found address is written by the search, never typed: one that cannot be read
        // (a hand-edited file) is dropped rather than refusing the whole Lighting block.
        self.lightkey.found_url =
            clean_lightkey_api_url(&self.lightkey.found_url).unwrap_or_default();
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
            map.normalise();
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
            validate_cue_map(key, map, path, errors);
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

/// The rules for one song's cue map, shared by the settings loader and by every writer (a map
/// is checked with exactly these rules before it is saved). `path` is the Lighting section.
pub fn validate_cue_map(key: &str, map: &SongLightingCueMap, path: &[Value], errors: &mut Errors) {
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
        // A look row's own address is not sent, so it is only checked for a plain row.
        if cue.messages.is_empty() {
            if let Err(m) = validate_osc_address(&cue.address) {
                value_error(errors, &mp, m, json!(cue.address));
            }
        }
        check_range(
            errors,
            &mp,
            "fade_seconds",
            cue.fade_seconds,
            Some(0.0),
            Some(MAX_FADE_SECONDS),
            None,
        );
        if cue.messages.len() > MAX_OSC_MESSAGES_PER_ROW {
            too_long(
                errors,
                &mp,
                "messages",
                cue.messages.len(),
                MAX_OSC_MESSAGES_PER_ROW,
            );
        }
        for message in &cue.messages {
            if let Err(m) = validate_osc_message_address(&message.address) {
                value_error(errors, &mp, m, json!(message.address));
            }
            if message.args.len() > MAX_OSC_ARGS_PER_MESSAGE {
                too_long(
                    errors,
                    &mp,
                    "args",
                    message.args.len(),
                    MAX_OSC_ARGS_PER_MESSAGE,
                );
            }
            for arg in &message.args {
                if let OscArgument::String(text) = arg {
                    let n = text.chars().count();
                    if n > MAX_OSC_STRING_CHARS {
                        too_long(errors, &mp, "args", n, MAX_OSC_STRING_CHARS);
                    }
                }
            }
        }
    }
    if map.osc_cues.len() > MAX_OSC_ROWS_PER_SONG {
        too_long(
            errors,
            &mp,
            "osc_cues",
            map.osc_cues.len(),
            MAX_OSC_ROWS_PER_SONG,
        );
    }
    let message_total: usize = map.osc_cues.iter().map(|c| c.messages.len()).sum();
    if message_total > MAX_OSC_MESSAGES_PER_SONG {
        too_long(
            errors,
            &mp,
            "messages",
            message_total,
            MAX_OSC_MESSAGES_PER_SONG,
        );
    }
    validate_effects(&map.effects, errors, &mp);
    if let Some(loudness) = &map.loudness {
        check_range(
            errors,
            &mp,
            "step_seconds",
            loudness.step_seconds,
            Some(0.05),
            Some(10.0),
            None,
        );
        check_range(
            errors,
            &mp,
            "takes",
            loudness.takes,
            Some(1.0),
            Some(f64::from(MAX_LOUDNESS_TAKES)),
            None,
        );
        if loudness.values.len() > MAX_LOUDNESS_VALUES {
            too_long(
                errors,
                &mp,
                "values",
                loudness.values.len(),
                MAX_LOUDNESS_VALUES,
            );
        }
    }
    let ids = |v: Vec<Uuid>| v.iter().collect::<std::collections::BTreeSet<_>>().len() == v.len();
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
