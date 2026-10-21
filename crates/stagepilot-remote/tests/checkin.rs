//! Port of backend/tests/test_remote_checkin.py: the always-on lightweight check-in.

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use serde_json::json;
use stagepilot_remote::bootstrap::{sanitize_device_name, Pacing};
use stagepilot_remote::desktop::{DesktopOptions, DesktopRemoteManager};
use stagepilot_remote::http::{HttpRequest, HttpResponse, HttpTransport, TransportError};
use stagepilot_remote::platform::StopFlag;
use uuid::Uuid;

/// Understands enroll and GET status only; any other route fails the test.
#[derive(Default)]
struct CheckinState {
    installation_id: String,
    hostname: String,
    credential: String,
    enroll_calls: u32,
    status_calls: u32,
    last_enroll_device_name: Option<String>,
    last_status_device_name: Option<String>,
}

#[derive(Clone, Default)]
struct CheckinOnly(Arc<Mutex<CheckinState>>);

impl HttpTransport for CheckinOnly {
    fn send(&self, r: &HttpRequest) -> Result<HttpResponse, TransportError> {
        let mut s = self.0.lock().unwrap();
        let path = r.path();
        if path.ends_with("/v1/installations/enroll") {
            s.enroll_calls += 1;
            s.last_enroll_device_name = r
                .json
                .as_ref()
                .and_then(|j| j["deviceName"].as_str())
                .map(str::to_owned);
            if s.installation_id.is_empty() {
                s.installation_id = Uuid::new_v4().simple().to_string();
                s.hostname = format!("sp-{}.remote.example.com", s.installation_id);
                s.credential = format!("spi_{}.{}", s.installation_id, "s".repeat(43));
            }
            return Ok(HttpResponse::json_response(
                201,
                &json!({"installationId": s.installation_id, "hostname": s.hostname,
                        "installationCredential": s.credential}),
            ));
        }
        if path.ends_with("/status") && r.method == "GET" {
            s.status_calls += 1;
            s.last_status_device_name = r
                .query
                .iter()
                .find(|(k, _)| k == "deviceName")
                .map(|(_, v)| v.clone());
            return Ok(HttpResponse::json_response(
                200,
                &json!({"installationId": s.installation_id, "hostname": s.hostname,
                        "phase": "disabled", "pendingActions": []}),
            ));
        }
        panic!(
            "Unexpected control-plane route hit by the lightweight check-in: {} {path}",
            r.method
        );
    }
}

fn build_manager(
    dir: &std::path::Path,
    fake: &CheckinOnly,
    device_name: Option<&str>,
    real_clock: bool,
) -> DesktopRemoteManager {
    let binary = dir.join("resources/cloudflared");
    std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
    std::fs::write(&binary, b"test binary").unwrap();
    let root = dir.join("remote");
    let store = verified_store(&root.join("bootstrap.json"), memory_credentials());
    let platform = if real_clock {
        real_clock_platform(Arc::new(fake.clone())).0
    } else {
        test_platform(Arc::new(fake.clone())).platform
    };
    DesktopRemoteManager::new(
        &root,
        &binary,
        platform,
        DesktopOptions {
            bootstrap_store: Some(store),
            transport: Some(Arc::new(fake.clone())),
            control_plane_origin: ORIGIN.into(),
            pacing: Pacing::instant(),
            device_name: device_name.map(str::to_owned),
            ..DesktopOptions::default()
        },
    )
    .unwrap()
}

#[test]
fn lightweight_checkin_enrolls_without_remote_access_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let fake = CheckinOnly::default();
    let manager = build_manager(dir.path(), &fake, Some("stage-pc"), false);
    assert!(!manager.feature.intent().enabled);
    manager.lightweight_checkin();
    let s = fake.0.lock().unwrap();
    assert_eq!((s.enroll_calls, s.status_calls), (1, 1));
    assert!(manager.bootstrap.state().unwrap().active.is_some());
    assert!(!manager.feature.intent().enabled);
}

#[test]
fn lightweight_checkin_sends_sanitized_device_name() {
    let dir = tempfile::tempdir().unwrap();
    let fake = CheckinOnly::default();
    let manager = build_manager(dir.path(), &fake, Some("  My-Desktop\u{7}.local  "), false);
    manager.lightweight_checkin();
    let s = fake.0.lock().unwrap();
    assert_eq!(
        s.last_enroll_device_name.as_deref(),
        Some("My-Desktop.local")
    );
    assert_eq!(
        s.last_status_device_name.as_deref(),
        Some("My-Desktop.local")
    );
}

#[test]
fn lightweight_checkin_omits_invalid_device_name() {
    let dir = tempfile::tempdir().unwrap();
    let fake = CheckinOnly::default();
    let manager = build_manager(dir.path(), &fake, Some("   "), false);
    manager.lightweight_checkin();
    let s = fake.0.lock().unwrap();
    assert_eq!(s.last_enroll_device_name, None);
    assert_eq!(s.last_status_device_name, None);
}

#[test]
fn lightweight_checkin_repeated_runs_never_call_provisioning_routes() {
    let dir = tempfile::tempdir().unwrap();
    let fake = CheckinOnly::default();
    let manager = build_manager(dir.path(), &fake, Some("stage-pc"), false);
    for _ in 0..3 {
        manager.lightweight_checkin();
    }
    // ensure_enrolled is idempotent once an active identity exists: only the first call enrolls.
    let s = fake.0.lock().unwrap();
    assert_eq!((s.enroll_calls, s.status_calls), (1, 3));
}

#[tokio::test(flavor = "multi_thread")]
async fn run_loop_checks_in_even_when_remote_access_never_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let fake = CheckinOnly::default();
    let manager = Arc::new(build_manager(dir.path(), &fake, Some("stage-pc"), true));
    let stop = StopFlag::new();
    let task = tokio::spawn(manager.clone().run(stop.clone()));
    for _ in 0..100 {
        {
            let s = fake.0.lock().unwrap();
            if s.enroll_calls >= 1 && s.status_calls >= 1 {
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    {
        let s = fake.0.lock().unwrap();
        assert!(s.enroll_calls >= 1 && s.status_calls >= 1);
    }
    assert!(!manager.feature.intent().enabled);
    stop.set();
    task.await.unwrap();
}

#[test]
fn sanitize_device_name_table() {
    let long = "a".repeat(300);
    let cases: [(Option<&str>, Option<String>); 7] = [
        (Some("my-laptop.local"), Some("my-laptop.local".into())),
        (Some("  padded  "), Some("padded".into())),
        (Some(&long), Some("a".repeat(253))),
        (Some("bad\u{7}name\u{0}here"), Some("badnamehere".into())),
        (Some(""), None),
        (Some("   "), None),
        (None, None),
    ];
    for (raw, expected) in cases {
        assert_eq!(sanitize_device_name(raw), expected, "{raw:?}");
    }
}
