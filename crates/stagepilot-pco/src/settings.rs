//! Effective Planning Center settings: the Rust counterpart of `PersistentSettings.to_runtime`
//! plus the `STAGEPILOT_PCO_*` environment overrides (`core/settings.py::_environment_overrides`).

use crate::models::{PcoSettings, Secret};
use stagepilot_core::config::ConnectionMethod;
use stagepilot_core::settings::{PersistentPlanningCenterSettings, SettingsService};
use std::collections::BTreeMap;

fn optional(environ: &BTreeMap<String, String>, name: &str) -> Option<Option<String>> {
    environ
        .get(name)
        .map(|v| Some(v.trim().to_string()).filter(|s| !s.is_empty()))
}

/// Overlay the `STAGEPILOT_PCO_*` environment on persisted settings. A value that does not parse
/// (a non-numeric lookahead, a timeout outside 1..=60) is ignored; Python would refuse to start.
pub fn apply_environment(
    environ: &BTreeMap<String, String>,
    persistent: &PersistentPlanningCenterSettings,
    secret: Option<String>,
    access_token: Option<String>,
) -> PcoSettings {
    let mut settings = PcoSettings {
        app_id: persistent
            .app_id
            .clone()
            .filter(|s| !s.is_empty())
            .map(Secret::new),
        secret: secret.filter(|s| !s.is_empty()).map(Secret::new),
        connection_method: persistent.connection_method,
        access_token: access_token.filter(|s| !s.is_empty()).map(Secret::new),
        service_type_id: persistent.service_type_id.clone(),
        plan_title_preference: persistent.plan_title_preference.clone(),
        preferred_service_time: persistent.preferred_service_time.clone(),
        upcoming_lookahead_days: persistent.upcoming_lookahead_days,
        request_timeout_seconds: persistent.request_timeout_seconds,
        user_agent: crate::models::default_user_agent(),
    };
    if let Some(v) = optional(environ, "STAGEPILOT_PCO_APP_ID") {
        settings.app_id = v.map(Secret::new);
    }
    if let Some(v) = optional(environ, "STAGEPILOT_PCO_SECRET") {
        settings.secret = v.map(Secret::new);
    }
    if let Some(v) = optional(environ, "STAGEPILOT_PCO_SERVICE_TYPE_ID") {
        settings.service_type_id = v;
    }
    if let Some(v) = optional(environ, "STAGEPILOT_PCO_PLAN_TITLE") {
        settings.plan_title_preference = v;
    }
    if let Some(v) = optional(environ, "STAGEPILOT_PCO_SERVICE_TIME") {
        settings.preferred_service_time = v;
    }
    if let Some(days) = environ
        .get("STAGEPILOT_PCO_LOOKAHEAD_DAYS")
        .and_then(|v| v.trim().parse::<u16>().ok())
        .filter(|d| *d <= 365)
    {
        settings.upcoming_lookahead_days = days;
    }
    if let Some(timeout) = environ
        .get("STAGEPILOT_PCO_TIMEOUT_SECONDS")
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|t| (1.0..=60.0).contains(t))
    {
        settings.request_timeout_seconds = timeout;
    }
    if let Some(agent) = environ
        .get("STAGEPILOT_PCO_USER_AGENT")
        .filter(|a| !a.is_empty() && a.chars().count() <= 256)
    {
        settings.user_agent = agent.clone();
    }
    settings
}

/// The effective settings from the settings service. `access_token` is only used for OAuth.
pub fn effective(service: &SettingsService, access_token: Option<String>) -> PcoSettings {
    let persistent = service.snapshot().planning_center;
    let token = (persistent.connection_method == ConnectionMethod::Oauth)
        .then_some(access_token)
        .flatten();
    apply_environment(
        service.environ(),
        &persistent,
        service.planning_center_secret(),
        token,
    )
}
