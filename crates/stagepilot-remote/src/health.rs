//! Fail closed when the private Remote connector heartbeat is stale (`remote_health.py`).

use std::path::Path;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct StatusCheck {
    pub healthy: bool,
    pub state: String,
    pub age_seconds: Option<f64>,
    pub detail: String,
}

fn check(healthy: bool, state: &str, age: Option<f64>, detail: &str) -> StatusCheck {
    StatusCheck {
        healthy,
        state: state.to_owned(),
        age_seconds: age,
        detail: detail.to_owned(),
    }
}

pub fn check_status(path: &Path, max_age_seconds: f64, now: f64) -> StatusCheck {
    let invalid = || {
        check(
            false,
            "unknown",
            None,
            "connector status heartbeat is invalid",
        )
    };
    let Ok(link) = std::fs::symlink_metadata(path) else {
        return invalid();
    };
    if link.file_type().is_symlink() || !link.is_file() || link.len() > 4096 {
        return invalid();
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return invalid();
    };
    let Ok(payload) = serde_json::from_str::<Value>(&text) else {
        return invalid();
    };
    let Some(state) = payload.get("state").and_then(Value::as_str) else {
        return invalid();
    };
    let Some(checked_at) = number(payload.get("checked_at")) else {
        return invalid();
    };
    if !matches!(state, "disabled" | "reconnecting" | "connected" | "stopped") {
        return invalid();
    }
    let age = now - checked_at;
    if !(0.0..=max_age_seconds).contains(&age) {
        return check(
            false,
            state,
            Some(age),
            "connector status heartbeat is stale",
        );
    }
    if state == "stopped" {
        return check(false, state, Some(age), "connector supervisor is stopped");
    }
    check(
        true,
        state,
        Some(age),
        "connector status heartbeat is fresh",
    )
}

/// `float(value)`: numbers and numeric strings.
pub fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// JSON line printed by the health CLI.
pub fn render(result: &StatusCheck) -> String {
    serde_json::json!({
        "healthy": result.healthy,
        "state": result.state,
        "age_seconds": result.age_seconds.map(|a| (a * 1000.0).round() / 1000.0),
        "detail": result.detail,
    })
    .to_string()
}
