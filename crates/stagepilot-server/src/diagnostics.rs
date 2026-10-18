//! `POST /api/v1/diagnostics/send`: the Rust counterpart of `api/diagnostics.py`.
//!
//! Local proxy for "Send logs to developer". The frontend never talks to the control plane: this
//! route reuses the installation's enrolled identity (the bootstrap store behind the Remote
//! manager) to forward the bundle to `POST /v1/installations/:id/diagnostics`. A machine without
//! an active enrolment cannot send logs this way and is told so with a 503.

use crate::remote_types::{ActiveEnrollment, DiagnosticsBootstrap};
use crate::runtime::Runtime;
use crate::validation::{http_error, invalid_body, ok, unprocessable, Json};
use axum::body::Bytes;
use axum::extract::State;
use serde_json::{json, Value};
use std::sync::{Arc, Once};
use std::time::Duration;

/// Mirrors the 2 MB client/server caps: oversized bundles are refused locally.
pub const MAX_BUNDLE_BYTES: usize = 2 * 1024 * 1024;
const NOT_SET_UP: &str =
    "Sending logs to the developer requires Remote Access to be set up on this installation.";
const UNAVAILABLE: &str = "Sending logs to the developer is unavailable right now.";

/// `json.dumps(value, default=str)` for the playback section: default separators, ASCII output.
/// (Object keys are written sorted: this build's `serde_json` has no `preserve_order`.)
fn dumps(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            let mut out = String::from("\"");
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    '\u{8}' => out.push_str("\\b"),
                    '\u{c}' => out.push_str("\\f"),
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
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(dumps).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(map) => format!(
            "{{{}}}",
            map.iter()
                .map(|(k, v)| format!("{}: {}", dumps(&Value::String(k.clone())), dumps(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// `with_playback_section`: add the last scan result and the recent (already redacted)
/// playback log lines, never pushing an in-limit bundle over the control-plane cap.
pub fn with_playback_section(bundle: &str, rt: &Runtime) -> String {
    let section = json!({
        "last_scan": rt.playback.scan_snapshot().last_scan,
        "recent_log": stagepilot_playback::plugin::recent_log(),
    });
    let combined = match serde_json::from_str::<Value>(bundle) {
        Ok(Value::Object(mut map)) => {
            map.insert("playback".into(), section);
            dumps(&Value::Object(map))
        }
        _ => format!("{bundle}\n--- playback ---\n{}", dumps(&section)),
    };
    if combined.len() <= MAX_BUNDLE_BYTES {
        combined
    } else {
        bundle.to_string()
    }
}

/// `DiagnosticsSendRequest`: `{"bundle": str}`; other keys are ignored.
fn parse(raw: &[u8]) -> Result<String, Vec<Value>> {
    let parsed: Option<Value> = serde_json::from_slice(raw).ok();
    let Some(Value::Object(map)) = &parsed else {
        return Err(invalid_body(raw, parsed.as_ref()));
    };
    match map.get("bundle") {
        Some(Value::String(bundle)) => Ok(bundle.clone()),
        Some(other) => Err(vec![json!({
            "type": "string_type", "loc": ["body", "bundle"],
            "msg": "Input should be a valid string", "input": other
        })]),
        None => Err(vec![json!({
            "type": "missing", "loc": ["body", "bundle"],
            "msg": "Field required", "input": Value::Object(map.clone())
        })]),
    }
}

fn install_crypto_provider() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // An error means a provider is already installed process-wide, which is fine.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

pub async fn send(State(rt): State<Arc<Runtime>>, body: Bytes) -> Json {
    let bundle = match parse(&body) {
        Ok(bundle) => bundle,
        Err(detail) => return unprocessable(detail),
    };
    if bundle.len() > MAX_BUNDLE_BYTES {
        return http_error(413, "Diagnostic bundle is too large to send.");
    }
    let Some(bootstrap) = rt
        .remote_wiring()
        .manager
        .and_then(|manager| manager.bootstrap())
    else {
        return http_error(503, NOT_SET_UP);
    };
    let active = match bootstrap.active() {
        Err(_) => return http_error(503, UNAVAILABLE),
        Ok(None) => return http_error(503, NOT_SET_UP),
        Ok(Some(active)) => active,
    };
    if !bootstrap
        .trusted_origins()
        .contains(&active.control_plane_origin)
    {
        return http_error(503, UNAVAILABLE);
    }
    let payload = with_playback_section(&bundle, &rt);
    match forward(bootstrap, active, payload).await {
        Forwarded::Sent => ok(json!({"ok": true, "message": "Logs sent to the developer."})),
        Forwarded::TooMany => http_error(
            429,
            "Too many diagnostic uploads recently; try again later.",
        ),
        Forwarded::Failed => http_error(
            502,
            "Logs could not be sent to the developer. Try again shortly.",
        ),
    }
}

enum Forwarded {
    Sent,
    TooMany,
    Failed,
}

async fn forward(
    bootstrap: Arc<dyn DiagnosticsBootstrap>,
    active: ActiveEnrollment,
    payload: String,
) -> Forwarded {
    let Ok(credential) = bootstrap.credential(&active) else {
        tracing::warn!("diagnostics credential unavailable");
        return Forwarded::Failed;
    };
    install_crypto_provider();
    // `trust_env=False, follow_redirects=False`, 30 s timeout.
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
    else {
        return Forwarded::Failed;
    };
    let url = format!(
        "{}/v1/installations/{}/diagnostics",
        active.control_plane_origin.trim_end_matches('/'),
        active.installation_id
    );
    let response = client
        .post(url)
        .header("authorization", format!("Bearer {credential}"))
        .header("content-type", "application/octet-stream")
        .body(payload.into_bytes())
        .send()
        .await;
    match response {
        Ok(r) if r.status().as_u16() == 429 => Forwarded::TooMany,
        Ok(r) if r.status().is_success() => Forwarded::Sent,
        Ok(_) | Err(_) => {
            tracing::warn!("diagnostics upload could not be forwarded to the control plane");
            Forwarded::Failed
        }
    }
}
