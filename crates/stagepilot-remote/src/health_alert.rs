//! Notify an operator when the private Remote connector heartbeat goes stale
//! (`remote_health_alert.py`): one condition, one delivery path (a webhook POST).

use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::error::{RemoteError, Res};
use crate::health::{check_status, StatusCheck};
use crate::http::{HttpRequest, HttpTransport};

pub type AlertSender<'a> = &'a dyn Fn(&str, &Value) -> Res<()>;

/// POST the payload to the operator-configured webhook; non-2xx is an error.
pub fn send_alert(
    transport: &Arc<dyn HttpTransport>,
    webhook_url: &str,
    payload: &Value,
) -> Res<()> {
    let request = HttpRequest {
        method: "POST".into(),
        url: webhook_url.to_owned(),
        json: Some(payload.clone()),
        timeout_secs: 10.0,
        ..HttpRequest::default()
    };
    let response = transport
        .send(&request)
        .map_err(|e| RemoteError::io(e.to_string()))?;
    if !response.is_success() {
        return Err(RemoteError::io(format!(
            "webhook returned HTTP {}",
            response.status
        )));
    }
    Ok(())
}

pub fn build_alert_payload(result: &StatusCheck, source: &str) -> Value {
    json!({
        "source": source,
        "event": "connector_heartbeat_stale",
        "state": result.state,
        "age_seconds": result.age_seconds.map(|a| (a * 1000.0).round() / 1000.0),
        "detail": result.detail,
    })
}

pub const DEFAULT_SOURCE: &str = "stagepilot-remote-connector";

/// Evaluate the freshness check and alert when stale. No webhook configured is a no-op.
pub fn check_and_alert(
    status_path: &Path,
    max_age_seconds: f64,
    webhook_url: Option<&str>,
    now: f64,
    sender: AlertSender<'_>,
) -> Res<StatusCheck> {
    let result = check_status(status_path, max_age_seconds, now);
    if !result.healthy {
        if let Some(url) = webhook_url.filter(|u| !u.is_empty()) {
            sender(url, &build_alert_payload(&result, DEFAULT_SOURCE))?;
        }
    }
    Ok(result)
}
