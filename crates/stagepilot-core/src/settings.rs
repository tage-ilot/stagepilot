//! Persistent settings, the on-disk store and the settings service: port of `core/settings.py`.
//!
//! The settings file is the same JSON file the Python app writes (`settings.json`, schema 2). A
//! file this code writes is read by the Python app and the reverse. Behaviour kept from Python:
//!
//! * path logic (`STAGEPILOT_SETTINGS_PATH`, `%APPDATA%`, `$XDG_CONFIG_HOME`);
//! * unknown keys are rejected on the top level and in `planning_center`, `onboarding` and
//!   `playback_api` (the models that set `extra="forbid"`); other sections ignore extras;
//! * the legacy `release_channel` key is dropped on load and never written back;
//! * a file with no `schema_version` is treated as version 1; `migrate_v1` moves untouched legacy
//!   installs to Playback and rewrites the file as version 2;
//! * the dashboard PIN hash lives in the file but is never serialised to API responses;
//! * writes are atomic (temp file in the same directory, fsync, rename).
//!
//! Added for the migration: before the first write over an existing file a one-time backup copy
//! `settings.json.pre-rust.bak` is made (never overwritten once it exists).

use crate::config::*;
use crate::settings_errors::map_serde_error;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use ts_rs::TS;

pub const SETTINGS_SCHEMA_VERSION: u8 = 2;
pub const KEYRING_SERVICE: &str = "StagePilot";
pub const KEYRING_ACCOUNT: &str = "planning-center-secret";
pub const KEYRING_ACCOUNT_OAUTH: &str = "planning-center-oauth";
pub const APP_VERSION: &str = "2.41.1";
pub const BACKUP_SUFFIX: &str = "pre-rust.bak";

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct SettingsFileError(pub String);

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct CredentialStoreError(pub String);

// ---- persistent models ------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PersistentPlanningCenterSettings {
    #[serde(default)]
    pub app_id: Option<String>,
    #[serde(default = "manual")]
    pub connection_method: ConnectionMethod,
    #[serde(default)]
    pub service_type_id: Option<String>,
    #[serde(default)]
    pub plan_title_preference: Option<String>,
    #[serde(default)]
    pub preferred_service_time: Option<String>,
    #[serde(default = "lookahead_default")]
    pub upcoming_lookahead_days: u16,
    #[serde(default = "pc_timeout_default")]
    pub request_timeout_seconds: f64,
}

fn manual() -> ConnectionMethod {
    ConnectionMethod::Manual
}
fn lookahead_default() -> u16 {
    30
}
fn pc_timeout_default() -> f64 {
    10.0
}

impl Default for PersistentPlanningCenterSettings {
    fn default() -> Self {
        Self {
            app_id: None,
            connection_method: ConnectionMethod::Manual,
            service_type_id: None,
            plan_title_preference: None,
            preferred_service_time: None,
            upcoming_lookahead_days: 30,
            request_timeout_seconds: 10.0,
        }
    }
}

fn strip_opt(v: &mut Option<String>) {
    *v = v
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
}

impl PersistentPlanningCenterSettings {
    pub fn normalise(&mut self) {
        strip_opt(&mut self.app_id);
        strip_opt(&mut self.service_type_id);
        strip_opt(&mut self.plan_title_preference);
        strip_opt(&mut self.preferred_service_time);
    }

    pub fn validate(&self, path: &[Value], errors: &mut Errors) {
        if let Some(v) = &self.app_id {
            check_len(errors, path, "app_id", v, 0, 255);
        }
        if let Some(v) = &self.service_type_id {
            check_len(errors, path, "service_type_id", v, 0, 128);
        }
        if let Some(v) = &self.plan_title_preference {
            check_len(errors, path, "plan_title_preference", v, 0, 255);
        }
        if let Some(v) = &self.preferred_service_time {
            let b = v.as_bytes();
            let ok = b.len() == 5
                && b[2] == b':'
                && b[..2].iter().all(u8::is_ascii_digit)
                && b[3..].iter().all(u8::is_ascii_digit)
                && matches!((b[0], b[1]), (b'0' | b'1', _) | (b'2', b'0'..=b'3'))
                && b[3] <= b'5';
            if !ok {
                errors.push(Detail {
                    kind: "string_pattern_mismatch".into(),
                    loc: path_with(path, "preferred_service_time"),
                    msg: "String should match pattern '^([01]\\d|2[0-3]):[0-5]\\d$'".into(),
                    input: json!(v),
                    ctx: Some(json!({"pattern": "^([01]\\d|2[0-3]):[0-5]\\d$"})),
                });
            }
        }
        check_range(
            errors,
            path,
            "upcoming_lookahead_days",
            self.upcoming_lookahead_days,
            Some(0.0),
            Some(365.0),
            None,
        );
        check_range(
            errors,
            path,
            "request_timeout_seconds",
            self.request_timeout_seconds,
            Some(1.0),
            Some(60.0),
            None,
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct OnboardingSettings {
    #[serde(default)]
    pub general_completed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlanningCenterLengthUndo {
    pub service_type_id: String,
    pub plan_id: String,
    pub item_id: String,
    pub title: String,
    pub old_length: i64,
    pub new_length: i64,
    pub written_at: DateTime<Utc>,
    #[serde(default)]
    pub pending_from_length: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PersistentSettings {
    #[serde(default)]
    pub planning_center_length_undo: Vec<PlanningCenterLengthUndo>,
    #[serde(default = "schema_default")]
    pub schema_version: u8,
    #[serde(default)]
    pub onboarding: OnboardingSettings,
    #[serde(default)]
    pub integration_modes: IntegrationModes,
    #[serde(default = "timezone_default")]
    pub timezone: String,
    #[serde(default = "log_level_default")]
    pub log_level: LogLevel,
    #[serde(default = "server_port_default")]
    pub server_port: u16,
    #[serde(default)]
    pub lan_access: bool,
    #[serde(default = "pin_enabled_default")]
    pub web_dashboard_pin_enabled: bool,
    /// Stored in the file, never serialised to API responses (`exclude=True` in Python).
    #[serde(default, skip_serializing)]
    #[ts(skip)]
    pub web_dashboard_pin_hash: Option<String>,
    #[serde(default)]
    pub planning_center: PersistentPlanningCenterSettings,
    #[serde(default)]
    pub midi: MidiSettings,
    #[serde(default)]
    pub playback_api: PlaybackApiSettings,
    #[serde(default)]
    pub network_midi: NetworkMidiSettings,
    #[serde(default)]
    pub lights: LightsSettings,
    #[serde(default)]
    pub propresenter: ProPresenterSettings,
}

fn schema_default() -> u8 {
    SETTINGS_SCHEMA_VERSION
}
fn timezone_default() -> String {
    "America/Los_Angeles".into()
}
fn log_level_default() -> LogLevel {
    LogLevel::Info
}
fn server_port_default() -> u16 {
    8765
}
fn pin_enabled_default() -> bool {
    true
}

impl Default for PersistentSettings {
    fn default() -> Self {
        Self {
            planning_center_length_undo: vec![],
            schema_version: SETTINGS_SCHEMA_VERSION,
            onboarding: OnboardingSettings::default(),
            integration_modes: IntegrationModes::default(),
            timezone: timezone_default(),
            log_level: LogLevel::Info,
            server_port: 8765,
            lan_access: false,
            web_dashboard_pin_enabled: true,
            web_dashboard_pin_hash: None,
            planning_center: PersistentPlanningCenterSettings::default(),
            midi: MidiSettings::default(),
            playback_api: PlaybackApiSettings::default(),
            network_midi: NetworkMidiSettings::default(),
            lights: LightsSettings::default(),
            propresenter: ProPresenterSettings::default(),
        }
    }
}

/// The outcome of the `migrate_v1` before-validator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migration {
    pub from_schema: u8,
    pub chose_midi: bool,
    pub midi_source: Option<String>,
}

pub fn valid_timezone(name: &str) -> bool {
    name.parse::<chrono_tz::Tz>().is_ok()
}

impl PersistentSettings {
    /// `migrate_v1` (before-validator): drop `release_channel`; a `schema_version` of exactly
    /// integer 1 moves an untouched install to Playback and becomes 2. Returns the migrated value
    /// and a description when a version migration happened.
    pub fn migrate_v1(value: Value) -> (Value, Option<Migration>) {
        let Value::Object(mut map) = value else {
            return (value, None);
        };
        map.remove("release_channel");
        let is_v1 = map
            .get("schema_version")
            .is_some_and(|v| v.as_i64() == Some(1) && v.is_i64() || v.as_u64() == Some(1));
        // `lights` saved before OSC existed has no method and keeps sending MIDI.
        if let Some(Value::Object(lights)) = map.get_mut("lights") {
            if !lights.contains_key("method") && !lights.is_empty() {
                lights.insert("method".into(), json!("midi"));
            }
        }
        if !is_v1 {
            return (Value::Object(map), None);
        }
        let modes = map.get("integration_modes").cloned().unwrap_or(json!({}));
        let Value::Object(mut modes) = modes else {
            return (Value::Object(map), None);
        };
        let chose_midi = modes.get("midi_source") == Some(&json!("real"))
            && map
                .get("midi")
                .and_then(|m| m.as_object())
                .and_then(|m| m.get("input_name"))
                .is_some_and(|n| match n {
                    Value::Null => false,
                    Value::String(s) => !s.is_empty(),
                    _ => true,
                });
        if !chose_midi {
            modes.insert("midi_source".into(), json!("playback_api"));
        }
        let midi_source = modes
            .get("midi_source")
            .and_then(|v| v.as_str())
            .map(String::from);
        map.insert("integration_modes".into(), Value::Object(modes));
        map.insert("schema_version".into(), json!(SETTINGS_SCHEMA_VERSION));
        (
            Value::Object(map),
            Some(Migration {
                from_schema: 1,
                chose_midi,
                midi_source,
            }),
        )
    }

    /// Validate a JSON value exactly like `PersistentSettings.model_validate`. The error list
    /// uses pydantic's shape (`type`, `loc`, `msg`, `input`).
    pub fn from_value(value: Value) -> Result<(Self, Option<Migration>), Errors> {
        let original = value.clone();
        if !value.is_object() {
            return Err(vec![Detail {
                kind: "model_type".into(),
                loc: vec![],
                msg: "Input should be a valid dictionary or instance of PersistentSettings".into(),
                input: value,
                ctx: Some(json!({"class_name": "PersistentSettings"})),
            }]);
        }
        let (migrated, migration) = Self::migrate_v1(value);
        let mut parsed: Self = serde_path_to_error::deserialize(migrated.clone())
            .map_err(|e| vec![map_serde_error(&e, &migrated, &original)])?;
        // schema_version is a Literal[2]
        if parsed.schema_version != SETTINGS_SCHEMA_VERSION {
            return Err(vec![Detail {
                kind: "literal_error".into(),
                loc: vec![json!("schema_version")],
                msg: "Input should be 2".into(),
                input: json!(parsed.schema_version),
                ctx: Some(json!({"expected": "2"})),
            }]);
        }
        parsed.normalise();
        let mut errors = Vec::new();
        parsed.validate(&mut errors);
        if errors.is_empty() {
            Ok((parsed, migration))
        } else {
            Err(errors)
        }
    }

    pub fn normalise(&mut self) {
        self.planning_center.normalise();
        self.midi.normalise();
        self.playback_api.normalise();
        self.network_midi.normalise();
        self.lights.normalise();
        self.propresenter.normalise();
    }

    pub fn validate(&self, errors: &mut Errors) {
        if !valid_timezone(&self.timezone) {
            value_error(
                errors,
                &[json!("timezone")],
                &format!("Unknown IANA timezone: {}", self.timezone),
                json!(self.timezone),
            );
        }
        check_range(
            errors,
            &[],
            "server_port",
            self.server_port,
            Some(1.0),
            Some(65535.0),
            None,
        );
        self.planning_center
            .validate(&[json!("planning_center")], errors);
        self.midi.validate(&[json!("midi")], errors);
        self.playback_api.validate(&[json!("playback_api")], errors);
        self.network_midi.validate(&[json!("network_midi")], errors);
        self.lights.validate(&[json!("lights")], errors);
        self.propresenter.validate(&[json!("propresenter")], errors);
    }

    /// The file payload: the API serialisation plus the PIN hash when set.
    pub fn to_file_value(&self) -> Value {
        let mut value = serde_json::to_value(self).unwrap_or(Value::Null);
        if let (Some(hash), Value::Object(map)) = (&self.web_dashboard_pin_hash, &mut value) {
            map.insert("web_dashboard_pin_hash".into(), json!(hash));
        }
        value
    }
}

// ---- runtime settings (the part of Python `Settings` this phase needs) ------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeSettings {
    pub version: String,
    pub bind_host: String,
    pub bind_port: u16,
    pub log_level: String,
    pub integration_modes: IntegrationModes,
    pub timezone: String,
    pub midi_enabled: bool,
    pub propresenter_enabled: bool,
    pub recent_event_limit: usize,
    pub recent_error_limit: usize,
}

impl RuntimeSettings {
    pub fn uses_demo_service(&self) -> bool {
        self.integration_modes.service_source == ServiceSource::Demo
    }

    /// `Settings(demo_mode=True)`: demo service, simulated MIDI and timer output.
    pub fn demo() -> Self {
        Self::from_persistent(&PersistentSettings {
            integration_modes: IntegrationModes {
                service_source: ServiceSource::Demo,
                midi_source: MidiSource::Simulated,
                timer_output: TimerOutput::Simulated,
            },
            ..PersistentSettings::default()
        })
    }

    pub fn from_persistent(p: &PersistentSettings) -> Self {
        Self {
            version: APP_VERSION.into(),
            bind_host: if p.lan_access { "0.0.0.0" } else { "127.0.0.1" }.into(),
            bind_port: p.server_port,
            log_level: serde_json::to_value(p.log_level)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| "INFO".into()),
            integration_modes: p.integration_modes.clone(),
            timezone: p.timezone.clone(),
            midi_enabled: p.integration_modes.midi_source != MidiSource::Simulated,
            propresenter_enabled: p.integration_modes.timer_output == TimerOutput::Propresenter,
            recent_event_limit: 100,
            recent_error_limit: 50,
        }
    }
}

fn truthy(value: &str) -> bool {
    matches!(value.to_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

/// The subset of `STAGEPILOT_*` overrides that belong to this phase (host, port, log level,
/// timezone, integration modes). PCO, MIDI, lights and ProPresenter overrides arrive with the
/// phases that port those integrations.
pub fn apply_environment(runtime: &mut RuntimeSettings, environ: &BTreeMap<String, String>) {
    if let Some(v) = environ.get("STAGEPILOT_HOST") {
        runtime.bind_host = v.clone();
    }
    if let Some(v) = environ
        .get("STAGEPILOT_PORT")
        .and_then(|v| v.trim().parse::<u16>().ok())
    {
        if v >= 1 {
            runtime.bind_port = v;
        }
    }
    if let Some(v) = environ.get("STAGEPILOT_LOG_LEVEL") {
        runtime.log_level = v.to_uppercase();
    }
    if let Some(v) = environ.get("STAGEPILOT_TIMEZONE") {
        if valid_timezone(v) {
            runtime.timezone = v.clone();
        }
    }
    let modes = &mut runtime.integration_modes;
    if let Some(v) = environ.get("STAGEPILOT_DEMO_MODE") {
        modes.service_source = if truthy(v) {
            ServiceSource::Demo
        } else {
            ServiceSource::PlanningCenter
        };
    }
    if let Some(v) = environ.get("STAGEPILOT_DEMO_SIMULATE_MIDI") {
        modes.midi_source = if truthy(v) {
            MidiSource::Simulated
        } else {
            MidiSource::Real
        };
    }
    if let Some(v) = environ.get("STAGEPILOT_DEMO_SIMULATE_PROPRESENTER") {
        modes.timer_output = if truthy(v) {
            TimerOutput::Simulated
        } else {
            TimerOutput::Propresenter
        };
    }
    if let Some(v) = environ.get("STAGEPILOT_MIDI_ENABLED") {
        let enabled = truthy(v);
        runtime.midi_enabled = enabled;
        if !environ.contains_key("STAGEPILOT_DEMO_SIMULATE_MIDI") {
            modes.midi_source = if enabled {
                MidiSource::Real
            } else {
                MidiSource::Simulated
            };
        }
    }
    if let Some(v) = environ.get("STAGEPILOT_PROPRESENTER_ENABLED") {
        let enabled = truthy(v);
        runtime.propresenter_enabled = enabled;
        if !environ.contains_key("STAGEPILOT_DEMO_SIMULATE_PROPRESENTER") {
            modes.timer_output = if enabled {
                TimerOutput::Propresenter
            } else {
                TimerOutput::Simulated
            };
        }
    }
    if let Some(v) = environ.get("STAGEPILOT_SERVICE_SOURCE") {
        match v.as_str() {
            "demo" => modes.service_source = ServiceSource::Demo,
            "planning_center" => modes.service_source = ServiceSource::PlanningCenter,
            _ => {}
        }
    }
    if let Some(v) = environ.get("STAGEPILOT_MIDI_SOURCE") {
        let source = match v.as_str() {
            "simulated" => Some(MidiSource::Simulated),
            "real" => Some(MidiSource::Real),
            "playback_api" => Some(MidiSource::PlaybackApi),
            _ => None,
        };
        if let Some(source) = source {
            modes.midi_source = source;
            runtime.midi_enabled = source == MidiSource::Real;
        }
    }
    if let Some(v) = environ.get("STAGEPILOT_TIMER_OUTPUT") {
        let output = match v.as_str() {
            "simulated" => Some(TimerOutput::Simulated),
            "propresenter" => Some(TimerOutput::Propresenter),
            _ => None,
        };
        if let Some(output) = output {
            modes.timer_output = output;
            runtime.propresenter_enabled = output == TimerOutput::Propresenter;
        }
    }
}

// ---- stores -----------------------------------------------------------------------------------

pub trait SettingsStore: Send + Sync {
    fn load(&self) -> Result<Option<PersistentSettings>, SettingsFileError>;
    fn save(&self, settings: &PersistentSettings) -> Result<(), SettingsFileError>;
}

pub fn default_settings_path(environ: &BTreeMap<String, String>) -> PathBuf {
    if let Some(v) = environ
        .get("STAGEPILOT_SETTINGS_PATH")
        .filter(|v| !v.is_empty())
    {
        return expand_user(v);
    }
    let home = || {
        environ
            .get("HOME")
            .or_else(|| environ.get("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_default()
    };
    let base = if cfg!(windows) {
        match environ.get("APPDATA") {
            Some(v) if !v.is_empty() => PathBuf::from(v),
            _ => home().join("AppData").join("Roaming"),
        }
    } else {
        match environ.get("XDG_CONFIG_HOME") {
            Some(v) => PathBuf::from(v),
            None => home().join(".config"),
        }
    };
    base.join("StagePilot").join("settings.json")
}

fn expand_user(value: &str) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(value)
}

pub fn process_environment() -> BTreeMap<String, String> {
    std::env::vars().collect()
}

pub struct SettingsFileStore {
    pub path: PathBuf,
    backed_up: Mutex<bool>,
}

impl SettingsFileStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            backed_up: Mutex::new(false),
        }
    }

    pub fn backup_path(&self) -> PathBuf {
        let mut name = self
            .path
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_default();
        name.push(format!(".{BACKUP_SUFFIX}"));
        self.path.with_file_name(name)
    }

    /// Copy the existing file aside once, before the first write of this process, unless a backup
    /// already exists (an older backup is never overwritten).
    fn backup_once(&self) -> Result<(), SettingsFileError> {
        let mut done = self.backed_up.lock().unwrap_or_else(|p| p.into_inner());
        if *done {
            return Ok(());
        }
        *done = true;
        let backup = self.backup_path();
        if self.path.is_file() && !backup.exists() {
            std::fs::copy(&self.path, &backup)
                .map_err(|_| SettingsFileError("Settings could not be saved atomically.".into()))?;
        }
        Ok(())
    }
}

const CORRUPT: &str = "Saved settings are corrupt or invalid; built-in defaults were used.";

impl SettingsStore for SettingsFileStore {
    fn load(&self) -> Result<Option<PersistentSettings>, SettingsFileError> {
        if !self.path.exists() {
            return Ok(None);
        }
        let text =
            std::fs::read_to_string(&self.path).map_err(|_| SettingsFileError(CORRUPT.into()))?;
        let mut payload: Value =
            serde_json::from_str(&text).map_err(|_| SettingsFileError(CORRUPT.into()))?;
        if let Value::Object(map) = &mut payload {
            map.entry("schema_version").or_insert(json!(1));
        }
        let (settings, migration) = PersistentSettings::from_value(payload)
            .map_err(|_| SettingsFileError(CORRUPT.into()))?;
        if let Some(m) = migration {
            tracing::warn!(
                "settings_migration schema={}->2 chose_midi={} midi_source={:?}",
                m.from_schema,
                m.chose_midi,
                m.midi_source
            );
            self.save(&settings)?;
        }
        Ok(Some(settings))
    }

    fn save(&self, settings: &PersistentSettings) -> Result<(), SettingsFileError> {
        use std::io::Write;
        let failure = || SettingsFileError("Settings could not be saved atomically.".into());
        self.backup_once()?;
        let parent = self
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let name = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "settings.json".into());
        let temporary = parent.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
        let result = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(&parent)?;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            let mut text = serde_json::to_string_pretty(&settings.to_file_value())
                .map_err(std::io::Error::other)?;
            text.push('\n');
            file.write_all(text.as_bytes())?;
            file.flush()?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, &self.path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
            return Err(failure());
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct MemorySettingsStore {
    settings: Mutex<Option<PersistentSettings>>,
}

impl MemorySettingsStore {
    pub fn new(settings: Option<PersistentSettings>) -> Self {
        Self {
            settings: Mutex::new(settings),
        }
    }
}

impl SettingsStore for MemorySettingsStore {
    fn load(&self) -> Result<Option<PersistentSettings>, SettingsFileError> {
        Ok(self
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone())
    }
    fn save(&self, settings: &PersistentSettings) -> Result<(), SettingsFileError> {
        *self.settings.lock().unwrap_or_else(|p| p.into_inner()) = Some(settings.clone());
        Ok(())
    }
}

// ---- credentials ------------------------------------------------------------------------------

pub trait CredentialStore: Send + Sync {
    fn get_secret(&self) -> Result<Option<String>, CredentialStoreError>;
    fn set_secret(&self, secret: &str) -> Result<(), CredentialStoreError>;
    fn remove_secret(&self) -> Result<(), CredentialStoreError>;
}

#[derive(Default)]
pub struct MemoryCredentialStore {
    secret: Mutex<Option<String>>,
}

impl MemoryCredentialStore {
    pub fn new(secret: Option<String>) -> Self {
        Self {
            secret: Mutex::new(secret),
        }
    }
}

impl CredentialStore for MemoryCredentialStore {
    fn get_secret(&self) -> Result<Option<String>, CredentialStoreError> {
        Ok(self
            .secret
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone())
    }
    fn set_secret(&self, secret: &str) -> Result<(), CredentialStoreError> {
        *self.secret.lock().unwrap_or_else(|p| p.into_inner()) = Some(secret.to_string());
        Ok(())
    }
    fn remove_secret(&self) -> Result<(), CredentialStoreError> {
        *self.secret.lock().unwrap_or_else(|p| p.into_inner()) = None;
        Ok(())
    }
}

/// OS keychain store. Same service (`StagePilot`) and accounts as the Python app, so a secret
/// saved by Python is read here and vice versa. Like Python, at most one keychain read per
/// launch (each read can raise a macOS password prompt); the in-memory copy follows our writes.
pub struct KeyringCredentialStore {
    account: String,
    cache: Mutex<Option<Option<String>>>,
}

impl KeyringCredentialStore {
    pub fn new(account: &str) -> Self {
        Self {
            account: account.to_string(),
            cache: Mutex::new(None),
        }
    }
    pub fn planning_center() -> Self {
        Self::new(KEYRING_ACCOUNT)
    }
    pub fn oauth() -> Self {
        Self::new(KEYRING_ACCOUNT_OAUTH)
    }

    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    fn entry(&self) -> Result<keyring::Entry, keyring::Error> {
        keyring::Entry::new(KEYRING_SERVICE, &self.account)
    }
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
impl CredentialStore for KeyringCredentialStore {
    fn get_secret(&self) -> Result<Option<String>, CredentialStoreError> {
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(value) = cache.as_ref() {
            return Ok(value.clone());
        }
        let value = match self.entry().and_then(|e| e.get_password()) {
            Ok(v) => Some(v),
            Err(keyring::Error::NoEntry) => None,
            Err(_) => {
                return Err(CredentialStoreError(
                    "The secure credential store is unavailable.".into(),
                ))
            }
        };
        *cache = Some(value.clone());
        Ok(value)
    }

    fn set_secret(&self, secret: &str) -> Result<(), CredentialStoreError> {
        self.entry()
            .and_then(|e| e.set_password(secret))
            .map_err(|_| {
                CredentialStoreError("The secure credential could not be saved.".into())
            })?;
        *self.cache.lock().unwrap_or_else(|p| p.into_inner()) = Some(Some(secret.to_string()));
        Ok(())
    }

    fn remove_secret(&self) -> Result<(), CredentialStoreError> {
        match self.entry().and_then(|e| e.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(_) => {
                return Err(CredentialStoreError(
                    "The secure credential could not be removed.".into(),
                ))
            }
        }
        *self.cache.lock().unwrap_or_else(|p| p.into_inner()) = Some(None);
        Ok(())
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
impl CredentialStore for KeyringCredentialStore {
    fn get_secret(&self) -> Result<Option<String>, CredentialStoreError> {
        Err(CredentialStoreError(
            "The secure credential store is unavailable.".into(),
        ))
    }
    fn set_secret(&self, _: &str) -> Result<(), CredentialStoreError> {
        Err(CredentialStoreError(
            "The secure credential could not be saved.".into(),
        ))
    }
    fn remove_secret(&self) -> Result<(), CredentialStoreError> {
        Err(CredentialStoreError(
            "The secure credential could not be removed.".into(),
        ))
    }
}

// ---- the service ------------------------------------------------------------------------------

struct ServiceState {
    persistent: PersistentSettings,
    runtime: Option<RuntimeSettings>,
    secret: Option<String>,
    warning: Option<String>,
    credential_saved: bool,
}

pub struct SettingsService {
    store: Box<dyn SettingsStore>,
    credentials: Box<dyn CredentialStore>,
    environ: BTreeMap<String, String>,
    state: Mutex<ServiceState>,
}

impl SettingsService {
    pub fn new(
        store: Box<dyn SettingsStore>,
        credentials: Box<dyn CredentialStore>,
        environ: BTreeMap<String, String>,
    ) -> Self {
        Self {
            store,
            credentials,
            environ,
            state: Mutex::new(ServiceState {
                persistent: PersistentSettings::default(),
                runtime: None,
                secret: None,
                warning: None,
                credential_saved: false,
            }),
        }
    }

    /// File-backed settings at the default path, keyring credentials, process environment.
    pub fn default_service() -> Self {
        let environ = process_environment();
        Self::new(
            Box::new(SettingsFileStore::new(default_settings_path(&environ))),
            Box::new(KeyringCredentialStore::planning_center()),
            environ,
        )
    }

    /// In-memory settings for explicitly configured instances (`SettingsService.ephemeral`).
    pub fn ephemeral(runtime: RuntimeSettings) -> Self {
        let persistent = PersistentSettings {
            integration_modes: runtime.integration_modes.clone(),
            timezone: runtime.timezone.clone(),
            ..PersistentSettings::default()
        };
        let service = Self::new(
            Box::new(MemorySettingsStore::new(Some(persistent.clone()))),
            Box::new(MemoryCredentialStore::default()),
            BTreeMap::new(),
        );
        {
            let mut s = service.lock();
            s.persistent = persistent;
            s.runtime = Some(runtime);
        }
        service
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ServiceState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn pin(&self, names: &[&str]) -> Option<String> {
        names
            .iter()
            .find(|n| self.environ.contains_key(**n))
            .map(|n| n.to_string())
    }

    pub fn service_source_pin(&self) -> Option<String> {
        self.pin(&["STAGEPILOT_SERVICE_SOURCE", "STAGEPILOT_DEMO_MODE"])
    }
    pub fn timer_output_pin(&self) -> Option<String> {
        self.pin(&[
            "STAGEPILOT_TIMER_OUTPUT",
            "STAGEPILOT_DEMO_SIMULATE_PROPRESENTER",
            "STAGEPILOT_PROPRESENTER_ENABLED",
        ])
    }
    pub fn midi_pin(&self, enabled: bool) -> Option<String> {
        let mut names = vec!["STAGEPILOT_MIDI_SOURCE", "STAGEPILOT_DEMO_SIMULATE_MIDI"];
        if enabled {
            names.push("STAGEPILOT_MIDI_ENABLED");
        }
        self.pin(&names)
    }

    pub fn warning(&self) -> Option<String> {
        self.lock().warning.clone()
    }
    pub fn credential_saved(&self) -> bool {
        self.lock().credential_saved
    }

    /// Load settings from the store and resolve precedence: saved file, then environment.
    pub fn load(&self) -> RuntimeSettings {
        let mut warnings = Vec::new();
        let saved = match self.store.load() {
            Ok(s) => s,
            Err(e) => {
                warnings.push(e.0);
                None
            }
        };
        let persistent = saved.unwrap_or_default();
        let mut secret = None;
        let pc = &persistent.planning_center;
        // No App ID means nothing usable is configured: do not touch the keychain at all (every
        // keychain read can prompt for the login password).
        if pc.app_id.is_some() || pc.connection_method == ConnectionMethod::Oauth {
            match self.credentials.get_secret() {
                Ok(s) => secret = s,
                Err(e) => warnings.push(e.0),
            }
        }
        let runtime = self.resolve(&persistent);
        let mut s = self.lock();
        s.persistent = persistent;
        s.credential_saved = secret.is_some();
        s.secret = secret;
        s.runtime = Some(runtime.clone());
        s.warning = if warnings.is_empty() {
            None
        } else {
            Some(warnings.join(" "))
        };
        runtime
    }

    fn resolve(&self, persistent: &PersistentSettings) -> RuntimeSettings {
        let mut runtime = RuntimeSettings::from_persistent(persistent);
        apply_environment(&mut runtime, &self.environ);
        runtime
    }

    pub fn snapshot(&self) -> PersistentSettings {
        self.lock().persistent.clone()
    }

    /// Persisted settings with the environment's effective values layered in (`effective_snapshot`).
    pub fn effective_snapshot(&self) -> PersistentSettings {
        let s = self.lock();
        let Some(runtime) = &s.runtime else {
            return s.persistent.clone();
        };
        let mut out = s.persistent.clone();
        out.integration_modes = runtime.integration_modes.clone();
        out.timezone = runtime.timezone.clone();
        if let Ok(level) = serde_json::from_value::<LogLevel>(json!(runtime.log_level)) {
            out.log_level = level;
        }
        out.server_port = runtime.bind_port;
        out.lan_access = runtime.bind_host == "0.0.0.0";
        out.propresenter.enabled =
            runtime.integration_modes.timer_output == TimerOutput::Propresenter;
        out
    }

    pub fn effective_runtime_settings(&self) -> RuntimeSettings {
        let mut s = self.lock();
        if s.runtime.is_none() {
            let r = self.resolve(&s.persistent.clone());
            s.runtime = Some(r);
        }
        s.runtime.clone().unwrap_or_else(RuntimeSettings::demo)
    }

    pub fn save(&self, settings: PersistentSettings) -> Result<(), SettingsFileError> {
        let mut settings = settings;
        let current = self.snapshot();
        if settings.web_dashboard_pin_hash.is_none() {
            settings.web_dashboard_pin_hash = current.web_dashboard_pin_hash.clone();
        }
        if current.onboarding.general_completed {
            settings.onboarding = current.onboarding.clone();
        }
        self.store.save(&settings)?;
        let runtime = self.resolve(&settings);
        let mut s = self.lock();
        s.persistent = settings;
        s.runtime = Some(runtime);
        Ok(())
    }

    pub fn update_dashboard_access(
        &self,
        enabled: bool,
        pin_hash: Option<String>,
    ) -> Result<(), SettingsFileError> {
        let mut updated = self.snapshot();
        updated.web_dashboard_pin_enabled = enabled;
        if pin_hash.is_some() {
            updated.web_dashboard_pin_hash = pin_hash;
        }
        self.save(updated)
    }

    /// Replace/remove the Planning Center secret together with the non-secret settings. If the
    /// credential backend fails the previous public settings are restored (as Python does).
    pub fn update_planning_center(
        &self,
        settings: PersistentPlanningCenterSettings,
        secret: Option<&str>,
        remove_secret: bool,
    ) -> Result<(), UpdateError> {
        let previous = self.snapshot();
        let mut updated = previous.clone();
        updated.planning_center = settings;
        self.store.save(&updated).map_err(UpdateError::File)?;
        let credential = if remove_secret {
            self.credentials.remove_secret().map(|_| Some(None))
        } else if let Some(secret) = secret {
            self.credentials
                .set_secret(secret)
                .map(|_| Some(Some(secret.to_string())))
        } else {
            Ok(None)
        };
        match credential {
            Err(e) => {
                let _ = self.store.save(&previous);
                Err(UpdateError::Credential(e))
            }
            Ok(change) => {
                let runtime = self.resolve(&updated);
                let mut s = self.lock();
                if let Some(new_secret) = change {
                    s.credential_saved = new_secret.is_some();
                    s.secret = new_secret;
                }
                s.persistent = updated;
                s.runtime = Some(runtime);
                Ok(())
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("{0}")]
    File(SettingsFileError),
    #[error("{0}")]
    Credential(CredentialStoreError),
}

/// Helper for tests and the server: build a JSON object map.
pub fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}
