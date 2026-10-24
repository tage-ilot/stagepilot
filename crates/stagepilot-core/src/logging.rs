//! Which log level a process starts with (m1).
//!
//! rc.4 read only `STAGEPILOT_LOG_LEVEL`, so the `log_level` saved in the settings file (DEBUG on
//! the live Mac) was ignored. The subscriber has to exist before the settings load (so the load's
//! own WARN/ERROR lines are not lost), hence the saved level is read with a side-effect-free peek
//! at the file instead of through the settings service.

use serde_json::Value;
use std::path::Path;

/// Every StagePilot crate's tracing target prefix. `DEBUG` in the settings turns on our own debug
/// output; dependencies (hyper, reqwest, ...) stay at INFO so a debug run stays readable.
const OWN_TARGETS: [&str; 11] = [
    "stagepilot_core",
    "stagepilot_server",
    "stagepilot_headless",
    "stagepilot_pco",
    "stagepilot_playback",
    "stagepilot_midi",
    "stagepilot_osc",
    "stagepilot_lights",
    "stagepilot_propresenter",
    "stagepilot_remote",
    "stagepilot_lib",
];

/// The `log_level` string in a settings file, if the file is readable JSON with a valid level.
/// Never writes, never logs, never fails.
pub fn saved_log_level(settings_path: &Path) -> Option<String> {
    let bytes = std::fs::read(settings_path).ok()?;
    let body = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    let value: Value = serde_json::from_slice(body).ok()?;
    let level = value.get("log_level")?.as_str()?.to_string();
    filter_for_level(&level).map(|_| level)
}

/// The tracing filter for a settings level name (`DEBUG`, `WARNING`, ...), `None` if unknown.
pub fn filter_for_level(level: &str) -> Option<String> {
    match level.trim().to_ascii_uppercase().as_str() {
        "DEBUG" => {
            let own: Vec<String> = OWN_TARGETS.iter().map(|t| format!("{t}=debug")).collect();
            Some(format!("info,{}", own.join(",")))
        }
        "INFO" => Some("info".into()),
        "WARNING" | "WARN" => Some("warn".into()),
        "ERROR" | "CRITICAL" => Some("error".into()),
        _ => None,
    }
}

/// The filter to start with: the environment variable wins (any filter the operator wrote),
/// then the saved settings level, then INFO.
pub fn startup_filter(env_level: Option<&str>, saved_level: Option<&str>) -> String {
    if let Some(env) = env_level.map(str::trim).filter(|v| !v.is_empty()) {
        return filter_for_level(env).unwrap_or_else(|| env.to_lowercase());
    }
    saved_level
        .and_then(filter_for_level)
        .unwrap_or_else(|| "info".into())
}

/// [`startup_filter`] for this process: `STAGEPILOT_LOG_LEVEL` and the default settings file.
pub fn process_startup_filter() -> String {
    let environ = crate::settings::process_environment();
    let path = crate::settings::default_settings_path(&environ);
    startup_filter(
        environ.get("STAGEPILOT_LOG_LEVEL").map(String::as_str),
        saved_log_level(&path).as_deref(),
    )
}
