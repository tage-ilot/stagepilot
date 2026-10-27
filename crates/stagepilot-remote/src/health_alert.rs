//! Notify an operator when the private Remote connector heartbeat goes stale: one condition, one
//! delivery path (a webhook POST). The `stagepilot-remote-health-alert` binary runs [`run_cli`]
//! from a timer (see `docs/heartbeat-alerts.md`).

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

/// The environment variable that carries the webhook URL (kept out of `ps` and shell history).
pub const WEBHOOK_ENV: &str = "STAGEPILOT_HEARTBEAT_ALERT_WEBHOOK_URL";

const DEFAULT_MAX_AGE_SECONDS: f64 = 10.0;
const USAGE: &str = "usage: stagepilot-remote-health-alert --status-file <absolute path> \
[--max-age-seconds <positive number, default 10>] [--webhook-url <url>]\n\
The webhook URL is better given in STAGEPILOT_HEARTBEAT_ALERT_WEBHOOK_URL.";

/// Run the command: returns the line to print and the process exit code (0 healthy, 1 stale,
/// 2 bad arguments). `webhook_from_env` is the value of [`WEBHOOK_ENV`]; the `--webhook-url`
/// flag wins when both are given. The webhook URL is never printed.
pub fn run_cli(
    args: &[String],
    webhook_from_env: Option<&str>,
    now: f64,
    sender: AlertSender<'_>,
) -> (String, i32) {
    let usage_error = |why: &str| (format!("{why}\n{USAGE}"), 2);
    let mut status_file: Option<&str> = None;
    let mut max_age = DEFAULT_MAX_AGE_SECONDS;
    let mut webhook_flag: Option<&str> = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--status-file" => status_file = it.next().map(String::as_str),
            "--max-age-seconds" => match it.next().and_then(|v| v.parse::<f64>().ok()) {
                Some(v) => max_age = v,
                None => return usage_error("--max-age-seconds needs a number"),
            },
            "--webhook-url" => webhook_flag = it.next().map(String::as_str),
            other => return usage_error(&format!("unknown argument {other}")),
        }
    }
    let Some(status_file) = status_file else {
        return usage_error("--status-file is required");
    };
    let status_path = Path::new(status_file);
    if !status_path.is_absolute() || !(max_age.is_finite() && max_age > 0.0) {
        return usage_error("use an absolute status path and a positive maximum age");
    }
    let webhook = webhook_flag
        .or(webhook_from_env)
        .filter(|url| !url.is_empty());
    let result = match check_and_alert(status_path, max_age, webhook, now, sender) {
        Ok(result) => result,
        // The check ran, but the webhook could not be reached: report it and fail the run.
        Err(error) => {
            let result = check_status(status_path, max_age, now);
            let line = json!({
                "healthy": result.healthy,
                "state": result.state,
                "age_seconds": result.age_seconds.map(|a| (a * 1000.0).round() / 1000.0),
                "detail": result.detail,
                "alerted": false,
                "alert_error": error.to_string(),
            });
            return (line.to_string(), 1);
        }
    };
    let line = json!({
        "healthy": result.healthy,
        "state": result.state,
        "age_seconds": result.age_seconds.map(|a| (a * 1000.0).round() / 1000.0),
        "detail": result.detail,
        "alerted": webhook.is_some() && !result.healthy,
    });
    (line.to_string(), i32::from(!result.healthy))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| (*a).to_owned()).collect()
    }

    fn status_file(dir: &Path, state: &str, checked_at: f64) -> String {
        let path = dir.join("connector-status.json");
        std::fs::write(
            &path,
            json!({"state": state, "checked_at": checked_at}).to_string(),
        )
        .unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn fresh_heartbeat_is_healthy_and_sends_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let file = status_file(dir.path(), "connected", 1000.0);
        let calls = RefCell::new(Vec::new());
        let sender = |url: &str, payload: &Value| {
            calls.borrow_mut().push((url.to_owned(), payload.clone()));
            Ok(())
        };
        let (line, code) = run_cli(
            &args(&["--status-file", &file]),
            Some("http://hook.invalid/x"),
            1004.0,
            &sender,
        );
        assert_eq!(code, 0);
        assert!(calls.borrow().is_empty());
        let out: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(out["healthy"], true);
        assert_eq!(out["alerted"], false);
    }

    #[test]
    fn stale_heartbeat_posts_once_exits_1_and_never_prints_the_url() {
        let dir = tempfile::tempdir().unwrap();
        let file = status_file(dir.path(), "connected", 1000.0);
        let calls = RefCell::new(Vec::new());
        let sender = |url: &str, payload: &Value| {
            calls.borrow_mut().push((url.to_owned(), payload.clone()));
            Ok(())
        };
        let (line, code) = run_cli(
            &args(&["--status-file", &file, "--max-age-seconds", "10"]),
            Some("http://hook.invalid/secret-token"),
            1100.0,
            &sender,
        );
        assert_eq!(code, 1);
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "http://hook.invalid/secret-token");
        assert_eq!(calls[0].1["event"], "connector_heartbeat_stale");
        assert_eq!(calls[0].1["state"], "connected");
        assert!(!line.contains("secret-token"));
        let out: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(out["alerted"], true);
        assert_eq!(out["detail"], "connector status heartbeat is stale");
    }

    #[test]
    fn no_webhook_means_no_alert_but_still_exit_1() {
        let dir = tempfile::tempdir().unwrap();
        let file = status_file(dir.path(), "stopped", 1000.0);
        let sender = |_: &str, _: &Value| -> Res<()> { panic!("must not send") };
        let (line, code) = run_cli(&args(&["--status-file", &file]), None, 1001.0, &sender);
        assert_eq!(code, 1);
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap()["alerted"],
            false
        );
    }

    #[test]
    fn webhook_flag_wins_over_the_environment() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir
            .path()
            .join("missing.json")
            .to_string_lossy()
            .into_owned();
        let seen = RefCell::new(String::new());
        let sender = |url: &str, _: &Value| {
            *seen.borrow_mut() = url.to_owned();
            Ok(())
        };
        let (_, code) = run_cli(
            &args(&[
                "--status-file",
                &file,
                "--webhook-url",
                "http://flag.invalid/",
            ]),
            Some("http://env.invalid/"),
            5.0,
            &sender,
        );
        assert_eq!(code, 1);
        assert_eq!(*seen.borrow(), "http://flag.invalid/");
    }

    #[test]
    fn a_failed_delivery_is_reported_and_fails_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let file = status_file(dir.path(), "connected", 1.0);
        let sender =
            |_: &str, _: &Value| -> Res<()> { Err(RemoteError::io("webhook returned HTTP 500")) };
        let (line, code) = run_cli(
            &args(&["--status-file", &file]),
            Some("http://x.invalid/"),
            500.0,
            &sender,
        );
        assert_eq!(code, 1);
        let out: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(out["alerted"], false);
        assert!(out["alert_error"].as_str().unwrap().contains("HTTP 500"));
    }

    #[test]
    fn bad_arguments_exit_2() {
        let sender = |_: &str, _: &Value| -> Res<()> { panic!("must not send") };
        for bad in [
            args(&[]),
            args(&["--status-file", "relative/path.json"]),
            args(&["--status-file", "/tmp/x.json", "--max-age-seconds", "0"]),
            args(&["--status-file", "/tmp/x.json", "--max-age-seconds", "soon"]),
            args(&["--status-file", "/tmp/x.json", "--bogus"]),
        ] {
            assert_eq!(run_cli(&bad, None, 0.0, &sender).1, 2, "{bad:?}");
        }
    }
}
