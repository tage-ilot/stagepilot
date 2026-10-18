//! Port of backend/tests/test_remote_feature.py (the parts that belong to this crate), plus
//! test_remote_health_alert.py, remote_health and test_remote_checkin.py /
//! test_remote_admin_actions.py (control-plane side).

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::*;
use serde_json::{json, Value};
use stagepilot_remote::beta_control::{ActionHandler, BetaRemoteControl};
use stagepilot_remote::bootstrap::Pacing;
use stagepilot_remote::error::RemoteError;
use stagepilot_remote::feature::RemoteFeature;
use stagepilot_remote::files::{
    atomic_write, read_desired, safe_status, BetaControlConfig, DesiredRemote,
};
use stagepilot_remote::health::{check_status, render};
use stagepilot_remote::health_alert::{
    build_alert_payload, check_and_alert, send_alert, DEFAULT_SOURCE,
};
use stagepilot_remote::http::{HttpClient, HttpRequest, HttpResponse, HttpTransport};
use stagepilot_remote::platform::{Clock, InMemoryLocker, ManualClock};
use stagepilot_remote::status_map;

const ORIGIN_URL: &str = "https://remote.stagepilot.test";

fn feature_at(dir: &std::path::Path, clock: Arc<ManualClock>) -> RemoteFeature {
    RemoteFeature::new(
        &dir.join("remote.json"),
        8766,
        Arc::new(InMemoryLocker::default()),
        clock,
    )
}

#[test]
fn atomic_write_replaces_contents_and_leaves_no_temp_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub/remote.json");
    atomic_write(&path, "durable file contents").unwrap();
    atomic_write(&path, "second").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
    let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, vec![std::ffi::OsString::from("remote.json")]);
}

#[test]
fn intent_is_idempotent_and_generation_changes_on_toggle() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::new(1000.0));
    let feature = feature_at(dir.path(), clock);
    assert!(!feature.intent().enabled);
    feature.set_enabled(true).unwrap();
    let generation = feature.intent().generation;
    feature.set_enabled(true).unwrap();
    assert_eq!(feature.intent().generation, generation);
    feature.set_enabled(false).unwrap();
    assert!(!feature.intent().enabled);
    assert_ne!(feature.intent().generation, generation);
    assert!(!read_desired(&feature.control).enabled);
}

#[test]
fn status_connected_then_stale() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::new(1_000_000.0));
    let feature = feature_at(dir.path(), clock.clone());
    feature.set_enabled(true).unwrap();
    let generation = feature.intent().generation;
    safe_status(
        &feature.status_path,
        status_map!("available" => true, "state" => "connected", "url" => ORIGIN_URL,
                    "generation" => generation, "checked_at" => clock.wall()),
    )
    .unwrap();
    let status = feature.status();
    assert_eq!(status["state"], "connected");
    assert_eq!(status["url"], ORIGIN_URL);
    assert_eq!(status["temporary_url"], true);
    assert_eq!(status["available"], true);
    // A heartbeat 100 s old is stale: no URL is exposed.
    clock.advance(100.0);
    assert!(feature.status()["url"].is_null());
}

#[test]
fn stale_status_and_foreign_route() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::new(1_000_000.0));
    let feature = feature_at(dir.path(), clock.clone());
    feature.set_enabled(true).unwrap();
    safe_status(
        &feature.status_path,
        status_map!("available" => true, "state" => "connected", "url" => ORIGIN_URL,
                    "generation" => feature.intent().generation, "checked_at" => clock.wall() - 100.0),
    )
    .unwrap();
    assert!(feature.status()["url"].is_null());
    feature.set_enabled(false).unwrap();
    atomic_write(
        &feature.control,
        &DesiredRemote {
            enabled: true,
            generation: uuid::Uuid::new_v4().to_string(),
            public_origin: ORIGIN_URL.into(),
            port: 8766,
        }
        .to_json(),
    )
    .unwrap();
    assert!(matches!(
        feature.set_enabled(true),
        Err(RemoteError::Value(_))
    ));
}

#[test]
fn named_marker_survives_ui_disable_and_reports_stable_url() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::new(1_000_000.0));
    let feature = feature_at(dir.path(), clock.clone());
    atomic_write(
        &feature.control,
        &DesiredRemote::marker(ORIGIN_URL, 18766).to_json(),
    )
    .unwrap();
    feature.set_enabled(true).unwrap();
    let generation = feature.intent().generation;
    safe_status(
        &feature.status_path,
        status_map!("available" => true, "state" => "connected", "url" => ORIGIN_URL, "generation" => generation,
                    "temporary_url" => false, "checked_at" => clock.wall()),
    )
    .unwrap();
    assert_eq!(feature.status()["url"], ORIGIN_URL);
    assert_eq!(feature.status()["temporary_url"], false);
    feature.set_enabled(false).unwrap();
    let disabled = read_desired(&feature.control);
    assert!(!disabled.enabled);
    assert_eq!(disabled.public_origin, ORIGIN_URL);
    assert_eq!(disabled.port, 18766);
}

#[test]
fn status_states_and_messages() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::new(1_000_000.0));
    let feature = feature_at(dir.path(), clock.clone());
    // Off and unavailable.
    let status = feature.status();
    assert_eq!(status["state"], "off");
    assert!(status["message"]
        .as_str()
        .unwrap()
        .contains("unavailable on this installation"));
    // Enabling with no fresh heartbeat: "enabling", then "error" after 15 s.
    feature.set_enabled(true).unwrap();
    assert_eq!(feature.status()["state"], "enabling");
    clock.advance(16.0);
    let status = feature.status();
    assert_eq!(status["state"], "error");
    assert!(status["message"].as_str().unwrap().contains("unavailable"));
    // Bad URLs downgrade "connected" to "error"; reconnecting carries its message.
    for url in [
        "http://x.test",
        "https://x.test/p",
        "https://u@x.test",
        "https://x.test:8443",
        "https://x.test?q=1",
    ] {
        safe_status(
            &feature.status_path,
            status_map!("available" => true, "state" => "connected", "url" => url,
                        "generation" => feature.intent().generation, "checked_at" => clock.wall()),
        )
        .unwrap();
        let status = feature.status();
        assert_eq!(status["state"], "error", "{url}");
        assert!(status["url"].is_null());
    }
    safe_status(
        &feature.status_path,
        status_map!("available" => true, "state" => "reconnecting", "generation" => feature.intent().generation,
                    "checked_at" => clock.wall()),
    )
    .unwrap();
    assert!(feature.status()["message"]
        .as_str()
        .unwrap()
        .contains("Reconnecting"));
    safe_status(
        &feature.status_path,
        status_map!("available" => true, "state" => "bogus", "generation" => feature.intent().generation,
                    "checked_at" => clock.wall()),
    )
    .unwrap();
    assert_eq!(feature.status()["state"], "error");
}

#[test]
fn set_managed_enabled_requires_a_uuid_and_keeps_marker() {
    let dir = tempfile::tempdir().unwrap();
    let feature = feature_at(dir.path(), Arc::new(ManualClock::new(5.0)));
    assert!(feature.set_managed_enabled(true, "nope").is_err());
    atomic_write(
        &feature.control,
        &DesiredRemote::marker(ORIGIN_URL, 18766).to_json(),
    )
    .unwrap();
    let generation = uuid::Uuid::new_v4().to_string();
    feature.set_managed_enabled(true, &generation).unwrap();
    assert_eq!(feature.intent().generation, generation);
    assert_eq!(feature.intent().port, 18766);
    feature.set_managed_enabled(false, "").unwrap();
    assert!(!feature.intent().enabled);
    assert_eq!(read_desired(&feature.control).public_origin, ORIGIN_URL);
}

#[test]
fn oversized_or_corrupt_intent_reads_as_default() {
    let dir = tempfile::tempdir().unwrap();
    let feature = feature_at(dir.path(), Arc::new(ManualClock::new(5.0)));
    std::fs::write(&feature.intent_path, "x".repeat(5000)).unwrap();
    assert!(!feature.intent().enabled);
    std::fs::write(&feature.intent_path, r#"{"enabled":"yes"}"#).unwrap();
    assert!(!feature.intent().enabled);
}

// ------------------------------------------------------------------ remote_health

fn write_status(path: &std::path::Path, state: &str, checked_at: f64) {
    std::fs::write(
        path,
        json!({"state": state, "checked_at": checked_at}).to_string(),
    )
    .unwrap();
}

#[test]
fn health_check_states() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("connector-status.json");
    write_status(&path, "connected", 100.0);
    let fresh = check_status(&path, 10.0, 103.0);
    assert!(fresh.healthy);
    assert_eq!(fresh.detail, "connector status heartbeat is fresh");
    let stale = check_status(&path, 10.0, 160.0);
    assert!(!stale.healthy);
    assert_eq!(stale.detail, "connector status heartbeat is stale");
    assert!(!check_status(&path, 10.0, 90.0).healthy); // clock skew into the future
    write_status(&path, "stopped", 100.0);
    assert_eq!(
        check_status(&path, 10.0, 101.0).detail,
        "connector supervisor is stopped"
    );
    write_status(&path, "weird", 100.0);
    let invalid = check_status(&path, 10.0, 101.0);
    assert!(!invalid.healthy && invalid.state == "unknown" && invalid.age_seconds.is_none());
    assert_eq!(invalid.detail, "connector status heartbeat is invalid");
    assert!(!check_status(&dir.path().join("missing.json"), 10.0, 1.0).healthy);
    std::fs::write(&path, "x".repeat(5000)).unwrap();
    assert!(!check_status(&path, 10.0, 1.0).healthy);
    std::fs::write(&path, r#"{"state":"connected"}"#).unwrap();
    assert!(!check_status(&path, 10.0, 1.0).healthy);
    #[cfg(unix)]
    {
        write_status(&dir.path().join("real.json"), "connected", 100.0);
        std::os::unix::fs::symlink(dir.path().join("real.json"), dir.path().join("link.json"))
            .unwrap();
        assert!(!check_status(&dir.path().join("link.json"), 10.0, 101.0).healthy);
    }
    write_status(&path, "connected", 100.0);
    let printed: Value =
        serde_json::from_str(&render(&check_status(&path, 10.0, 100.12345))).unwrap();
    assert_eq!(printed["age_seconds"], json!(0.123));
    assert_eq!(printed["healthy"], json!(true));
}

// ------------------------------------------------------------------ remote_health_alert

#[test]
fn stale_heartbeat_triggers_alert() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("connector-status.json");
    let now = 1_000_000.0;
    write_status(&path, "connected", now - 60.0);
    let calls = Mutex::new(Vec::<(String, Value)>::new());
    let sender = |url: &str, payload: &Value| {
        calls
            .lock()
            .unwrap()
            .push((url.to_owned(), payload.clone()));
        Ok(())
    };
    let result = check_and_alert(
        &path,
        10.0,
        Some("http://127.0.0.1:9/hermes/whatsapp-relay"),
        now,
        &sender,
    )
    .unwrap();
    assert!(!result.healthy);
    assert_eq!(result.detail, "connector status heartbeat is stale");
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "http://127.0.0.1:9/hermes/whatsapp-relay");
    let payload = &calls[0].1;
    assert_eq!(payload["event"], "connector_heartbeat_stale");
    assert_eq!(payload["state"], "connected");
    assert!((payload["age_seconds"].as_f64().unwrap() - 60.0).abs() < 0.5);
    assert!(payload.get("webhook_url").is_none() && payload.get("url").is_none());
}

#[test]
fn fresh_heartbeat_does_not_trigger_alert() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("connector-status.json");
    write_status(&path, "connected", 998.0);
    let calls = Mutex::new(0);
    let sender = |_: &str, _: &Value| {
        *calls.lock().unwrap() += 1;
        Ok(())
    };
    let result =
        check_and_alert(&path, 10.0, Some("http://127.0.0.1:9/x"), 1000.0, &sender).unwrap();
    assert!(result.healthy);
    assert_eq!(*calls.lock().unwrap(), 0);
}

#[test]
fn stale_heartbeat_without_webhook_configured_does_not_call_sender() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("connector-status.json");
    write_status(&path, "reconnecting", 940.0);
    let sender =
        |_: &str, _: &Value| -> stagepilot_remote::Res<()> { panic!("sender must not be called") };
    for url in [None, Some("")] {
        let result = check_and_alert(&path, 10.0, url, 1000.0, &sender).unwrap();
        assert!(!result.healthy);
    }
}

#[test]
fn send_alert_posts_expected_payload_and_raises_on_http_error() {
    let seen = Arc::new(Mutex::new(Vec::<HttpRequest>::new()));
    let status = Arc::new(Mutex::new(200_u16));
    let transport: Arc<dyn HttpTransport> = Arc::new(FnTransport({
        let (seen, status) = (seen.clone(), status.clone());
        move |r: &HttpRequest| {
            seen.lock().unwrap().push(r.clone());
            Ok(HttpResponse::json_response(
                *status.lock().unwrap(),
                &json!({"ok": true}),
            ))
        }
    }));
    let payload =
        json!({"event": "connector_heartbeat_stale", "state": "connected", "age_seconds": 42.0});
    send_alert(&transport, "http://127.0.0.1:9/relay", &payload).unwrap();
    {
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].url, "http://127.0.0.1:9/relay");
        assert_eq!(seen[0].method, "POST");
        assert_eq!(seen[0].json.as_ref().unwrap(), &payload);
    }
    *status.lock().unwrap() = 500;
    assert!(send_alert(
        &transport,
        "http://127.0.0.1:9/relay",
        &json!({"event": "x"})
    )
    .is_err());
}

#[test]
fn build_alert_payload_has_no_secrets() {
    let result = stagepilot_remote::health::StatusCheck {
        healthy: false,
        state: "connected".into(),
        age_seconds: Some(55.5),
        detail: "connector status heartbeat is stale".into(),
    };
    assert_eq!(
        build_alert_payload(&result, DEFAULT_SOURCE),
        json!({
            "source": "stagepilot-remote-connector",
            "event": "connector_heartbeat_stale",
            "state": "connected",
            "age_seconds": 55.5,
            "detail": "connector status heartbeat is stale",
        })
    );
}

// ------------------------------------------------------------------ test_remote_admin_actions.py

const INSTALLATION: &str = "aaaaaaaaaaaaaaaa";

fn credential() -> String {
    format!("spi_{INSTALLATION}.signature-not-a-real-secret")
}

#[derive(Default)]
struct PlaneState {
    pending: Vec<Value>,
    acked: Vec<String>,
    approvals: Vec<Value>,
    authorizations: Vec<Option<String>>,
}

#[derive(Clone, Default)]
struct Plane(Arc<Mutex<PlaneState>>);

impl HttpTransport for Plane {
    fn send(
        &self,
        r: &HttpRequest,
    ) -> Result<HttpResponse, stagepilot_remote::http::TransportError> {
        let mut s = self.0.lock().unwrap();
        let path = r.path();
        s.authorizations
            .push(r.header("authorization").map(str::to_owned));
        let payload = r.json.clone().unwrap_or_else(|| json!({}));
        if path.ends_with("/status") {
            return Ok(HttpResponse::json_response(
                200,
                &json!({"installationId": INSTALLATION, "phase": "provisioned", "pendingActions": s.pending}),
            ));
        }
        if path.ends_with("/ack") {
            let id = path.rsplit('/').nth(1).unwrap().to_owned();
            s.acked.push(id.clone());
            return Ok(HttpResponse::json_response(
                200,
                &json!({"acknowledged": id}),
            ));
        }
        if path.ends_with("/approval-requests") {
            if r.method == "POST" {
                let record = json!({"requestId": "req-1", "installationId": INSTALLATION,
                                    "reason": payload["reason"], "status": "pending"});
                s.approvals.push(record.clone());
                return Ok(HttpResponse::json_response(201, &record));
            }
            return Ok(HttpResponse::json_response(
                200,
                &json!({"requests": s.approvals}),
            ));
        }
        panic!("Unexpected control-plane route: {path}");
    }
}

fn build_control(
    dir: &std::path::Path,
    plane: &Plane,
    handlers: BTreeMap<String, ActionHandler>,
) -> BetaRemoteControl {
    let config = BetaControlConfig {
        control_plane_url: "https://control.example.com".into(),
        installation_id: INSTALLATION.into(),
        hostname: format!("sp-{INSTALLATION}.remote.example.com"),
        credential_file: None,
        state_dir: dir.join("state"),
        installation_dir: dir.join("installation"),
        remote_port: 8766,
        lan_port: 8765,
    };
    let client = HttpClient::new(&config.control_plane_url, 20.0, Arc::new(plane.clone()));
    BetaRemoteControl::new(
        config,
        client,
        Arc::new(InMemoryLocker::default()),
        Some(Arc::new(|| Ok(credential()))),
        Pacing::instant(),
        None,
    )
    .with_action_handlers(handlers)
}

fn action(kind: &str, installation: &str) -> Value {
    json!({"actionId": format!("{installation}:{kind}:0001"), "installationId": installation,
           "kind": kind, "requestedAt": "2026-01-01T00:00:00.000Z"})
}

fn counting_handler(counter: &Arc<Mutex<Vec<&'static str>>>) -> BTreeMap<String, ActionHandler> {
    let counter = counter.clone();
    BTreeMap::from([(
        "rate_limit_reset".to_owned(),
        Box::new(move || {
            counter.lock().unwrap().push("reset");
            Ok(())
        }) as ActionHandler,
    )])
}

#[test]
fn no_pending_actions_applies_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let plane = Plane::default();
    let applied = Arc::new(Mutex::new(vec![]));
    let mut control = build_control(dir.path(), &plane, counting_handler(&applied));
    assert!(control.apply_pending_actions().unwrap().is_empty());
    assert!(applied.lock().unwrap().is_empty() && plane.0.lock().unwrap().acked.is_empty());
}

#[test]
fn queued_reset_is_applied_then_acknowledged() {
    let dir = tempfile::tempdir().unwrap();
    let plane = Plane::default();
    plane.0.lock().unwrap().pending = vec![action("rate_limit_reset", INSTALLATION)];
    let applied = Arc::new(Mutex::new(vec![]));
    let mut control = build_control(dir.path(), &plane, counting_handler(&applied));
    assert_eq!(
        control.apply_pending_actions().unwrap(),
        vec!["rate_limit_reset"]
    );
    assert_eq!(*applied.lock().unwrap(), vec!["reset"]);
    let state = plane.0.lock().unwrap();
    assert_eq!(
        state.acked,
        vec![format!("{INSTALLATION}:rate_limit_reset:0001")]
    );
    let expected = Some(format!("Bearer {}", credential()));
    assert!(state.authorizations.iter().all(|a| *a == expected));
}

#[test]
fn action_attributed_to_another_installation_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let plane = Plane::default();
    plane.0.lock().unwrap().pending = vec![action("rate_limit_reset", &"b".repeat(16))];
    let applied = Arc::new(Mutex::new(vec![]));
    let mut control = build_control(dir.path(), &plane, counting_handler(&applied));
    assert!(control.apply_pending_actions().unwrap().is_empty());
    assert!(applied.lock().unwrap().is_empty() && plane.0.lock().unwrap().acked.is_empty());
}

#[test]
fn unknown_action_kind_is_ignored_and_not_acknowledged() {
    let dir = tempfile::tempdir().unwrap();
    let plane = Plane::default();
    plane.0.lock().unwrap().pending = vec![
        action("something_new", INSTALLATION),
        json!("junk"),
        json!({"kind": 3}),
    ];
    let applied = Arc::new(Mutex::new(vec![]));
    let mut control = build_control(dir.path(), &plane, counting_handler(&applied));
    assert!(control.apply_pending_actions().unwrap().is_empty());
    assert!(plane.0.lock().unwrap().acked.is_empty());
}

#[test]
fn failed_handler_leaves_action_unacknowledged() {
    let dir = tempfile::tempdir().unwrap();
    let plane = Plane::default();
    plane.0.lock().unwrap().pending = vec![action("rate_limit_reset", INSTALLATION)];
    let handlers = BTreeMap::from([(
        "rate_limit_reset".to_owned(),
        Box::new(|| Err(RemoteError::io("local reset failed"))) as ActionHandler,
    )]);
    let mut control = build_control(dir.path(), &plane, handlers);
    assert!(control.apply_pending_actions().is_err());
    assert!(plane.0.lock().unwrap().acked.is_empty());
}

#[test]
fn approval_request_and_outcome_polling() {
    let dir = tempfile::tempdir().unwrap();
    let plane = Plane::default();
    let mut control = build_control(dir.path(), &plane, BTreeMap::new());
    let created = control
        .request_approval("quota increase for a real user")
        .unwrap();
    assert_eq!(created["status"], "pending");
    assert_eq!(created["reason"], "quota increase for a real user");
    plane.0.lock().unwrap().approvals[0]["status"] = json!("approved");
    let statuses: Vec<_> = control
        .approval_requests()
        .unwrap()
        .iter()
        .map(|i| i["status"].clone())
        .collect();
    assert_eq!(statuses, vec![json!("approved")]);
}

// ------------------------------------------------------------------ beta control request rules

#[test]
fn beta_request_retry_and_error_mapping() {
    let dir = tempfile::tempdir().unwrap();
    let statuses = Arc::new(Mutex::new(vec![503_u16, 429, 200]));
    let sleeps = Arc::new(Mutex::new(Vec::<f64>::new()));
    let transport: Arc<dyn HttpTransport> = Arc::new(FnTransport({
        let statuses = statuses.clone();
        move |_: &HttpRequest| {
            let status = statuses.lock().unwrap().remove(0);
            let mut r = HttpResponse::json_response(status, &json!({"phase": "disabled"}));
            if status == 429 {
                r.headers.insert("retry-after".into(), "3".into());
            }
            Ok(r)
        }
    }));
    let config = BetaControlConfig {
        control_plane_url: "https://control.example.com".into(),
        installation_id: INSTALLATION.into(),
        hostname: format!("sp-{INSTALLATION}.remote.example.com"),
        credential_file: None,
        state_dir: dir.path().join("state"),
        installation_dir: dir.path().join("installation"),
        remote_port: 8766,
        lan_port: 8765,
    };
    let pacing = Pacing {
        sleep: {
            let s = sleeps.clone();
            Arc::new(move |x| s.lock().unwrap().push(x))
        },
        random: Arc::new(|| 0.0),
    };
    let mut control = BetaRemoteControl::new(
        config.clone(),
        HttpClient::new(&config.control_plane_url, 20.0, transport),
        Arc::new(InMemoryLocker::default()),
        Some(Arc::new(|| Ok(credential()))),
        pacing,
        None,
    );
    let body = control.lightweight_status().unwrap();
    assert_eq!(body["phase"], "disabled");
    assert_eq!(*sleeps.lock().unwrap(), vec![0.5, 3.0]);

    for (status, body, expect) in [
        (401_u16, json!({}), "revoked"),
        (403, json!({}), "revoked"),
        (
            503,
            json!({"diagnostic": "hostname ownership conflict"}),
            "ownership conflict",
        ),
        (500, json!({"diagnostic": 5}), "HTTP 500"),
    ] {
        let t: Arc<dyn HttpTransport> = Arc::new(FnTransport(move |_: &HttpRequest| {
            Ok(HttpResponse::json_response(status, &body))
        }));
        let mut c = BetaRemoteControl::new(
            config.clone(),
            HttpClient::new(&config.control_plane_url, 20.0, t),
            Arc::new(InMemoryLocker::default()),
            Some(Arc::new(|| Ok(credential()))),
            Pacing::instant(),
            None,
        );
        let err = c.lightweight_status().unwrap_err();
        assert!(err.to_string().contains(expect), "{status}: {err}");
        assert_eq!(
            matches!(err, RemoteError::InstallationRevoked(_)),
            expect == "revoked"
        );
    }
    // Non-object JSON and non-JSON bodies.
    for body in ["[1]", "<html>"] {
        let t: Arc<dyn HttpTransport> = Arc::new(FnTransport(move |_: &HttpRequest| {
            Ok(HttpResponse::new(200, body))
        }));
        let mut c = BetaRemoteControl::new(
            config.clone(),
            HttpClient::new(&config.control_plane_url, 20.0, t),
            Arc::new(InMemoryLocker::default()),
            Some(Arc::new(|| Ok(credential()))),
            Pacing::instant(),
            None,
        );
        assert!(c.lightweight_status().unwrap_err().is_provider());
    }
}

#[test]
fn credential_file_rules() {
    let dir = tempfile::tempdir().unwrap();
    let credential_file = dir.path().join("cred");
    let config = BetaControlConfig {
        control_plane_url: "https://control.example.com".into(),
        installation_id: INSTALLATION.into(),
        hostname: format!("sp-{INSTALLATION}.remote.example.com"),
        credential_file: Some(credential_file.clone()),
        state_dir: dir.path().join("state"),
        installation_dir: dir.path().join("installation"),
        remote_port: 8766,
        lan_port: 8765,
    };
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let transport: Arc<dyn HttpTransport> = Arc::new(FnTransport({
        let seen = seen.clone();
        move |r: &HttpRequest| {
            seen.lock()
                .unwrap()
                .push(r.header("authorization").unwrap().to_owned());
            Ok(HttpResponse::json_response(
                200,
                &json!({"phase": "disabled"}),
            ))
        }
    }));
    let mut control = BetaRemoteControl::new(
        config.clone(),
        HttpClient::new(&config.control_plane_url, 20.0, transport),
        Arc::new(InMemoryLocker::default()),
        None,
        Pacing::instant(),
        None,
    );
    assert!(control.lightweight_status().is_err()); // missing
    std::fs::write(&credential_file, format!("{}\n", credential())).unwrap();
    make_private(&credential_file);
    control.lightweight_status().unwrap();
    assert_eq!(seen.lock().unwrap()[0], format!("Bearer {}", credential()));
    #[cfg(unix)]
    {
        make_mode(&credential_file, 0o644);
        assert!(control
            .lightweight_status()
            .unwrap_err()
            .to_string()
            .contains("private"));
        make_private(&credential_file);
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&credential_file, &link).unwrap();
        let mut c2 = config.clone();
        c2.credential_file = Some(link);
        let t: Arc<dyn HttpTransport> =
            Arc::new(FnTransport(|_: &HttpRequest| panic!("no request")));
        let mut control = BetaRemoteControl::new(
            c2.clone(),
            HttpClient::new(&c2.control_plane_url, 20.0, t),
            Arc::new(InMemoryLocker::default()),
            None,
            Pacing::instant(),
            None,
        );
        assert!(control
            .lightweight_status()
            .unwrap_err()
            .to_string()
            .contains("symlink"));
    }
    std::fs::write(&credential_file, "spi_bbbbbbbbbbbbbbbb.other").unwrap();
    make_private(&credential_file);
    assert!(control
        .lightweight_status()
        .unwrap_err()
        .to_string()
        .contains("invalid"));
}

#[test]
fn beta_config_validation() {
    let dir = tempfile::tempdir().unwrap();
    let ok = |url: &str| BetaControlConfig {
        control_plane_url: url.into(),
        installation_id: INSTALLATION.into(),
        hostname: "a.example.com".into(),
        credential_file: None,
        state_dir: dir.path().join("state"),
        installation_dir: dir.path().join("installation"),
        remote_port: 8766,
        lan_port: 8765,
    };
    assert!(ok("https://control.example.com").validate().is_ok());
    assert!(ok("https://control.example.com/").validate().is_ok());
    for bad in [
        "http://control.example.com",
        "https://control.example.com/x",
        "https://u:p@control.example.com",
        "control",
    ] {
        assert!(ok(bad).validate().is_err(), "{bad}");
    }
    let mut c = ok("https://control.example.com");
    c.state_dir = c.installation_dir.join("state");
    assert!(c.validate().is_err());
    let mut c = ok("https://control.example.com");
    c.credential_file = Some(c.installation_dir.join("cred"));
    assert!(c.validate().is_err());
    assert!(BetaControlConfig::from_json(r#"{"x":1}"#).is_err());
}

#[test]
fn beta_enable_disable_and_state_machine() {
    let dir = tempfile::tempdir().unwrap();
    let id = "a".repeat(32);
    let fake = FakeControlPlane::new(bundle_payload(&id));
    let config = BetaControlConfig {
        control_plane_url: ORIGIN.into(),
        installation_id: id.clone(),
        hostname: format!("sp-{id}.remote.example.com"),
        credential_file: None,
        state_dir: dir.path().join("state"),
        installation_dir: dir.path().join("installation"),
        remote_port: 8766,
        lan_port: 8765,
    };
    let cred = format!("spi_{id}.{}", "s".repeat(43));
    let mk = |fake: &FakeControlPlane| {
        BetaRemoteControl::new(
            config.clone(),
            HttpClient::new(ORIGIN, 20.0, Arc::new(fake.clone())),
            Arc::new(InMemoryLocker::default()),
            Some(Arc::new({
                let c = cred.clone();
                move || Ok(c.clone())
            })),
            Pacing::instant(),
            Some("stage-pc"),
        )
    };
    let mut control = mk(&fake);
    assert!(control.apply("explode").is_err());
    assert_eq!(control.apply("status").unwrap()["phase"], "disabled");
    let enabled = control.apply("enable").unwrap();
    assert_eq!(enabled["phase"], "enabled");
    assert!(control.connector_token.is_some());
    assert!(read_desired(&control.desired_path).enabled);
    // status keeps the token; other actions reset it.
    control.apply("status").unwrap();
    assert!(control.connector_token.is_some());
    assert_eq!(control.apply("reconcile").unwrap()["phase"], "enabled");
    // A changed binding is refused.
    let mut other_config = config.clone();
    other_config.hostname = format!("sp-{id}.other.example.com");
    let mut other = BetaRemoteControl::new(
        other_config.clone(),
        HttpClient::new(ORIGIN, 20.0, Arc::new(fake.clone())),
        Arc::new(InMemoryLocker::default()),
        Some(Arc::new({
            let c = cred.clone();
            move || Ok(c.clone())
        })),
        Pacing::instant(),
        None,
    );
    assert!(other
        .apply("status")
        .unwrap_err()
        .to_string()
        .contains("Configuration changed"));
    // Revoke.
    let revoked = control.apply("revoke").unwrap();
    assert_eq!(revoked["phase"], "disabled");
    assert!(fake.state.lock().unwrap().revoked);
    assert!(!read_desired(&control.desired_path).enabled);
    // A failed revoke keeps the revoking phase, blocks enable and retries on disable.
    let mut control = mk(&fake);
    control.apply("enable").unwrap();
    fake.state.lock().unwrap().fail_revoke = true;
    assert!(control.apply("disable").unwrap_err().is_provider());
    let state: Value =
        serde_json::from_str(&std::fs::read_to_string(&control.state_path).unwrap()).unwrap();
    assert_eq!(state["phase"], "revoking");
    assert!(!read_desired(&control.desired_path).enabled);
    assert!(control
        .apply("enable")
        .unwrap_err()
        .to_string()
        .contains("Finish retrying disable"));
    fake.state.lock().unwrap().fail_revoke = false;
    assert_eq!(control.apply("reconcile").unwrap()["phase"], "disabled");
    // A corrupt state file still closes local access on disable.
    std::fs::write(&control.state_path, "corrupt").unwrap();
    std::fs::write(&control.token_path, "old").unwrap();
    assert!(control.apply("disable").is_err());
    assert!(!control.token_path.exists());
    assert!(!read_desired(&control.desired_path).enabled);
}
