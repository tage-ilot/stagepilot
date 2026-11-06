//! Port of backend/tests/test_remote_checkin.py: the always-on lightweight check-in.

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use serde_json::json;
use stagepilot_remote::bootstrap::{sanitize_device_name, sanitize_location, Pacing};
use stagepilot_remote::desktop::{DesktopOptions, DesktopRemoteManager};
use stagepilot_remote::http::{HttpRequest, HttpResponse, HttpTransport, TransportError};
use stagepilot_remote::platform::{ManualClock, StopFlag};
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
    last_status_location: Option<String>,
    status_had_location_param: bool,
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
            s.status_had_location_param = r.query.iter().any(|(k, _)| k == "location");
            s.last_status_location = r
                .query
                .iter()
                .find(|(k, _)| k == "location")
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

/// Stands in for db-ip: counts calls, optionally fails or stalls. Never touches the network.
struct FakeLookup {
    calls: Mutex<u32>,
    body: Mutex<Result<(u16, String), String>>,
    delay: std::time::Duration,
}

impl FakeLookup {
    fn ok(body: &str) -> Arc<Self> {
        Self::with(Ok((200, body.to_owned())), std::time::Duration::ZERO)
    }
    fn with(body: Result<(u16, String), String>, delay: std::time::Duration) -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(0),
            body: Mutex::new(body),
            delay,
        })
    }
    fn calls(&self) -> u32 {
        *self.calls.lock().unwrap()
    }
}

impl HttpTransport for FakeLookup {
    fn send(&self, r: &HttpRequest) -> Result<HttpResponse, TransportError> {
        assert_eq!(r.url, "https://api.db-ip.com/v2/free/self");
        assert!(r.timeout_secs <= 10.0);
        *self.calls.lock().unwrap() += 1;
        std::thread::sleep(self.delay);
        match &*self.body.lock().unwrap() {
            Ok((status, body)) => Ok(HttpResponse::new(*status, body.clone())),
            Err(e) => Err(TransportError(e.clone())),
        }
    }
}

const RANCHO: &str = r#"{"ipAddress":"203.0.113.9","countryCode":"US","countryName":"United States","stateProv":"California","city":"Rancho Mirage"}"#;

fn build_with_lookup(
    dir: &std::path::Path,
    fake: &CheckinOnly,
    lookup: Option<Arc<FakeLookup>>,
    disabled: bool,
) -> (DesktopRemoteManager, Arc<ManualClock>) {
    let binary = dir.join("resources/cloudflared");
    std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
    std::fs::write(&binary, b"test binary").unwrap();
    let root = dir.join("remote");
    let store = verified_store(&root.join("bootstrap.json"), memory_credentials());
    let tp = test_platform(Arc::new(fake.clone()));
    let manager = DesktopRemoteManager::new(
        &root,
        &binary,
        tp.platform,
        DesktopOptions {
            bootstrap_store: Some(store),
            transport: Some(Arc::new(fake.clone())),
            control_plane_origin: ORIGIN.into(),
            pacing: Pacing::instant(),
            device_name: Some("stage-pc".into()),
            ip_lookup: lookup.map(|l| l as Arc<dyn HttpTransport>),
            ip_lookup_disabled: Some(disabled),
            ..DesktopOptions::default()
        },
    )
    .unwrap();
    (manager, tp.clock)
}

fn location_file(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("remote").join("location.json")
}

#[test]
fn checkin_omits_location_until_one_is_known_then_sends_it() {
    let dir = tempfile::tempdir().unwrap();
    let fake = CheckinOnly::default();
    let lookup = FakeLookup::ok(RANCHO);
    let (manager, _) = build_with_lookup(dir.path(), &fake, Some(lookup.clone()), false);
    manager.lightweight_checkin();
    assert!(!fake.0.lock().unwrap().status_had_location_param);

    assert!(manager.refresh_location_if_due());
    assert_eq!(lookup.calls(), 1);
    manager.lightweight_checkin();
    assert_eq!(
        fake.0.lock().unwrap().last_status_location.as_deref(),
        Some("Rancho Mirage, CA")
    );
    // Only the city is on disk: no address anywhere in the file.
    let stored = std::fs::read_to_string(location_file(dir.path())).unwrap();
    assert!(stored.contains("Rancho Mirage, CA") && !stored.contains("203.0.113.9"));
}

#[test]
fn lookup_runs_weekly_not_per_checkin() {
    let dir = tempfile::tempdir().unwrap();
    let fake = CheckinOnly::default();
    let lookup = FakeLookup::ok(RANCHO);
    let (manager, clock) = build_with_lookup(dir.path(), &fake, Some(lookup.clone()), false);
    assert!(manager.refresh_location_if_due());
    for _ in 0..5 {
        manager.lightweight_checkin();
        assert!(!manager.refresh_location_if_due());
    }
    clock.advance(6.0 * 86_400.0);
    assert!(!manager.refresh_location_if_due());
    assert_eq!(lookup.calls(), 1);
    clock.advance(2.0 * 86_400.0);
    // Stale: looked up again (same city, so "unchanged").
    assert!(!manager.refresh_location_if_due());
    assert_eq!(lookup.calls(), 2);
}

#[test]
fn legacy_location_file_is_sent_but_treated_as_stale() {
    let dir = tempfile::tempdir().unwrap();
    let fake = CheckinOnly::default();
    std::fs::create_dir_all(dir.path().join("remote")).unwrap();
    std::fs::write(location_file(dir.path()), r#"{"location":"Pasadena, CA"}"#).unwrap();
    let lookup = FakeLookup::ok(RANCHO);
    let (manager, _) = build_with_lookup(dir.path(), &fake, Some(lookup.clone()), false);
    assert_eq!(manager.location().as_deref(), Some("Pasadena, CA"));
    manager.lightweight_checkin();
    assert_eq!(
        fake.0.lock().unwrap().last_status_location.as_deref(),
        Some("Pasadena, CA")
    );
    assert!(manager.refresh_location_if_due());
    assert_eq!(manager.location().as_deref(), Some("Rancho Mirage, CA"));
    let migrated = std::fs::read_to_string(location_file(dir.path())).unwrap();
    assert!(migrated.contains("looked_up_at"));
}

#[test]
fn failed_lookup_keeps_last_value_backs_off_and_never_breaks_checkin() {
    for failure in [
        Err("connection refused".to_owned()),
        Ok((429, r#"{"errorCode":"QUOTA_EXCEEDED"}"#.to_owned())),
        Ok((200, "not json".to_owned())),
        Ok((
            200,
            r#"{"countryCode":"US","stateProv":"Texas"}"#.to_owned(),
        )),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let fake = CheckinOnly::default();
        std::fs::create_dir_all(dir.path().join("remote")).unwrap();
        std::fs::write(location_file(dir.path()), r#"{"location":"Austin, TX"}"#).unwrap();
        let lookup = FakeLookup::with(failure, std::time::Duration::ZERO);
        let (manager, clock) = build_with_lookup(dir.path(), &fake, Some(lookup.clone()), false);
        assert!(!manager.refresh_location_if_due());
        assert_eq!(lookup.calls(), 1);
        assert_eq!(manager.location().as_deref(), Some("Austin, TX"));
        manager.lightweight_checkin();
        {
            let s = fake.0.lock().unwrap();
            assert_eq!((s.enroll_calls, s.status_calls), (1, 1));
            assert_eq!(s.last_status_location.as_deref(), Some("Austin, TX"));
        }
        // Backoff: no retry inside the hour, one after it.
        clock.advance(3599.0);
        manager.refresh_location_if_due();
        assert_eq!(lookup.calls(), 1);
        clock.advance(2.0);
        manager.refresh_location_if_due();
        assert_eq!(lookup.calls(), 2);
        // Capped per day however long the process runs.
        for _ in 0..10 {
            clock.advance(3601.0);
            manager.refresh_location_if_due();
        }
        assert!(lookup.calls() <= 8, "calls: {}", lookup.calls());
    }
}

#[test]
fn opt_out_never_calls_the_lookup() {
    let dir = tempfile::tempdir().unwrap();
    let fake = CheckinOnly::default();
    let lookup = FakeLookup::ok(RANCHO);
    let (manager, _) = build_with_lookup(dir.path(), &fake, Some(lookup.clone()), true);
    assert!(!manager.refresh_location_if_due());
    assert_eq!(lookup.calls(), 0);
    // And with no transport configured at all (the default for tests) nothing happens either.
    let dir2 = tempfile::tempdir().unwrap();
    let (bare, _) = build_with_lookup(dir2.path(), &fake, None, false);
    assert!(!bare.refresh_location_if_due());
}

#[tokio::test(flavor = "multi_thread")]
async fn run_loop_checks_in_while_the_lookup_hangs() {
    let dir = tempfile::tempdir().unwrap();
    let fake = CheckinOnly::default();
    let lookup = FakeLookup::with(
        Ok((200, RANCHO.to_owned())),
        std::time::Duration::from_secs(2),
    );
    let (manager, _) = build_with_lookup(dir.path(), &fake, Some(lookup.clone()), false);
    let manager = Arc::new(manager);
    let stop = StopFlag::new();
    let started = std::time::Instant::now();
    let task = tokio::spawn(manager.clone().run(stop.clone()));
    for _ in 0..100 {
        if fake.0.lock().unwrap().status_calls >= 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(fake.0.lock().unwrap().status_calls >= 1);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "check-in waited for the slow lookup"
    );
    // The slow lookup finishes later and its city is reported right away.
    for _ in 0..200 {
        if fake.0.lock().unwrap().last_status_location.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(
        fake.0.lock().unwrap().last_status_location.as_deref(),
        Some("Rancho Mirage, CA")
    );
    stop.set();
    task.await.unwrap();
}

#[test]
fn sanitize_location_table() {
    let long = "a".repeat(81);
    let exact = "a".repeat(80);
    let cases: [(Option<&str>, Option<String>); 8] = [
        (Some("Pasadena, CA"), Some("Pasadena, CA".into())),
        (Some("  padded  "), Some("padded".into())),
        (Some("Bad\u{7}City\n"), Some("BadCity".into())),
        (Some(&exact), Some(exact.clone())),
        (Some(&long), None),
        (Some(""), None),
        (Some("\u{7}"), None),
        (None, None),
    ];
    for (raw, expected) in cases {
        assert_eq!(sanitize_location(raw), expected, "{raw:?}");
    }
}
