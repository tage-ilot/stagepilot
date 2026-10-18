//! Port of backend/tests/test_remote_bootstrap.py.

mod common;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use common::*;
use serde_json::json;
use stagepilot_remote::bootstrap::*;
use stagepilot_remote::desktop::{DesktopOptions, DesktopRemoteManager};
use stagepilot_remote::error::{RemoteError, Res};
use stagepilot_remote::files::read_desired;
use stagepilot_remote::http::{HttpRequest, HttpResponse, HttpTransport, TransportError};
use stagepilot_remote::platform::StopFlag;

struct Fixture {
    manager: DesktopRemoteManager,
    credentials: Arc<MemoryCredentials>,
    fake: FakeControlPlane,
    payload: serde_json::Value,
    tp: TestPlatform,
    _dir: tempfile::TempDir,
}

fn options(store: Arc<DesktopBootstrapStore>, fake: &FakeControlPlane) -> DesktopOptions {
    DesktopOptions {
        bootstrap_store: Some(store),
        transport: Some(Arc::new(fake.clone())),
        control_plane_origin: ORIGIN.to_owned(),
        pacing: Pacing::instant(),
        device_name: Some("test-host".into()),
        ..DesktopOptions::default()
    }
}

fn binary(dir: &std::path::Path) -> std::path::PathBuf {
    let binary = dir.join("resources/cloudflared");
    std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
    std::fs::write(&binary, b"test binary").unwrap();
    binary
}

fn manager_for(dir: &std::path::Path, id: &str) -> Fixture {
    let payload = bundle_payload(id);
    let credentials = memory_credentials();
    let root = dir.join("remote");
    let store = verified_store(&root.join("bootstrap.json"), credentials.clone());
    let fake = FakeControlPlane::new(payload.clone());
    let tp = test_platform(Arc::new(fake.clone()));
    let manager = DesktopRemoteManager::new(
        &root,
        &binary(dir),
        tp.platform.clone(),
        options(store, &fake),
    )
    .unwrap();
    Fixture {
        manager,
        credentials,
        fake,
        payload,
        tp,
        _dir: tempfile::tempdir().unwrap(),
    }
}

/// Mirrors `manager_fixture`: the store is already enrolled.
fn enrolled_fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let mut f = manager_for(dir.path(), &"a".repeat(32));
    let transport: Arc<dyn HttpTransport> = Arc::new(f.fake.clone());
    f.manager
        .bootstrap
        .ensure_enrolled(
            ORIGIN,
            DEFAULT_REMOTE_PORT,
            Some(&transport),
            &Pacing::instant(),
            None,
        )
        .unwrap();
    // Keep the temp dir alive for the fixture's lifetime.
    f._dir = dir;
    f
}

fn restart(f: &Fixture) -> DesktopRemoteManager {
    DesktopRemoteManager::new(
        &f.manager.root,
        &f.manager.cloudflared_binary,
        f.tp.platform.clone(),
        options(f.manager.bootstrap.clone(), &f.fake),
    )
    .unwrap()
}

fn id_of(f: &Fixture) -> String {
    f.payload["installationId"].as_str().unwrap().to_owned()
}

#[test]
fn first_enable_transparently_enrolls_and_keeps_credential_native() {
    let dir = tempfile::tempdir().unwrap();
    let f = manager_for(dir.path(), &"a".repeat(32));
    let enabled = f.manager.enable().unwrap();
    assert_eq!(enabled["provisioned"], json!(true));
    let store = &f.manager.bootstrap;
    let active = store.state().unwrap().active.unwrap();
    assert_eq!(active.installation_id, id_of(&f));
    assert_eq!(
        f.credentials.get(&active.installation_id).unwrap().unwrap(),
        f.payload["installationCredential"].as_str().unwrap()
    );
    let serialized = std::fs::read_to_string(store.path()).unwrap();
    assert!(!serialized.contains(f.payload["installationCredential"].as_str().unwrap()));
    assert_eq!(
        store.state().unwrap().enrollment_nonce.unwrap(),
        f.fake.state.lock().unwrap().enrollment_nonce
    );
    assert_eq!(active.bundle_id, active.installation_id);
}

#[test]
fn installation_ids_of_8_16_and_32_chars_validate() {
    for width in [32_usize, 16, 8] {
        let dir = tempfile::tempdir().unwrap();
        let id = "c".repeat(width);
        let f = manager_for(dir.path(), &id);
        let enabled = f.manager.enable().unwrap();
        assert_eq!(enabled["provisioned"], json!(true));
        let active = f.manager.bootstrap.state().unwrap().active.unwrap();
        assert_eq!(active.installation_id, id);
        assert_eq!(active.hostname, format!("sp-{id}.remote.example.com"));
    }
}

#[test]
fn enrollment_honors_retry_after_with_backoff_and_jitter() {
    let dir = tempfile::tempdir().unwrap();
    let payload = bundle_payload(&"a".repeat(32));
    let store = verified_store(
        &dir.path().join("remote/bootstrap.json"),
        memory_credentials(),
    );
    let attempts = Arc::new(AtomicU32::new(0));
    let transport: Arc<dyn HttpTransport> = Arc::new(FnTransport({
        let (attempts, payload) = (attempts.clone(), payload.clone());
        move |_: &HttpRequest| {
            if attempts.fetch_add(1, Ordering::SeqCst) + 1 < 3 {
                let mut r = HttpResponse::json_response(429, &json!({"error": "limited"}));
                r.headers.insert("retry-after".into(), "2".into());
                return Ok(r);
            }
            Ok(HttpResponse::json_response(201, &payload))
        }
    }));
    let sleeps = Arc::new(Mutex::new(Vec::<f64>::new()));
    let pacing = Pacing {
        sleep: {
            let s = sleeps.clone();
            Arc::new(move |x| s.lock().unwrap().push(x))
        },
        random: Arc::new(|| 0.5),
    };
    let metadata = store
        .ensure_enrolled(ORIGIN, DEFAULT_REMOTE_PORT, Some(&transport), &pacing, None)
        .unwrap();
    assert_eq!(
        metadata.installation_id,
        payload["installationId"].as_str().unwrap()
    );
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
    assert_eq!(*sleeps.lock().unwrap(), vec![2.0, 2.0]);
}

#[test]
fn untrusted_origin_is_refused_and_enroll_failure_modes() {
    let dir = tempfile::tempdir().unwrap();
    let store = verified_store(&dir.path().join("b.json"), memory_credentials());
    let transport: Arc<dyn HttpTransport> = Arc::new(FnTransport(|_: &HttpRequest| {
        Err(TransportError("x".into()))
    }));
    let err = store
        .ensure_enrolled(
            "https://evil.example.com",
            18766,
            Some(&transport),
            &Pacing::instant(),
            None,
        )
        .unwrap_err();
    assert_eq!(
        err,
        RemoteError::provider("The enrollment service is not trusted")
    );
    let err = store
        .ensure_enrolled(ORIGIN, 18766, Some(&transport), &Pacing::instant(), None)
        .unwrap_err();
    assert!(
        matches!(&err, RemoteError::Provider(m) if m.contains("Could not reach")),
        "{err:?}"
    );
    // Unbound / invalid responses.
    for body in [
        json!({"installationId": "a".repeat(32), "hostname": "sp-bbbbbbbb.remote.example.com",
               "installationCredential": format!("spi_{}.{}", "a".repeat(32), "s".repeat(43))}),
        json!({"installationId": "a".repeat(32), "hostname": format!("sp-{}.remote.example.com", "a".repeat(32)),
               "installationCredential": "spi_short"}),
    ] {
        let store = verified_store(&dir.path().join("c.json"), memory_credentials());
        let t: Arc<dyn HttpTransport> = Arc::new(FnTransport(move |_: &HttpRequest| {
            Ok(HttpResponse::json_response(201, &body))
        }));
        let err = store
            .ensure_enrolled(ORIGIN, 18766, Some(&t), &Pacing::instant(), None)
            .unwrap_err();
        assert!(
            matches!(&err, RemoteError::Provider(m) if m.contains("not installation-bound")),
            "{err:?}"
        );
        let _ = std::fs::remove_file(dir.path().join("c.json"));
    }
}

#[test]
fn desktop_disable_then_enable_reprovisions_same_hostname_new_generation() {
    let f = enrolled_fixture();
    let manager = &f.manager;
    let enabled = manager.enable().unwrap();
    let desired = read_desired(&manager.desired_path);
    let active = manager.bootstrap.state().unwrap().active.unwrap();
    assert_eq!(enabled["provisioned"], json!(true));
    assert!(desired.enabled && desired.generation == manager.feature.intent().generation);
    assert_eq!(
        manager.bootstrap.credential(&active).unwrap(),
        f.payload["installationCredential"].as_str().unwrap()
    );
    let original_id = active.installation_id.clone();
    let original_hostname = active.hostname.clone();
    let original_credential = manager.bootstrap.credential(&active).unwrap();

    let restarted = restart(&f);
    restarted.reconcile_control().unwrap();
    assert!(restarted.feature.intent().enabled);
    assert!(read_desired(&restarted.desired_path).enabled);

    let disabled = restarted.disable().unwrap();
    assert!(f.fake.state.lock().unwrap().revoked);
    assert_eq!(disabled["provisioned"], json!(false));
    assert!(!read_desired(&restarted.desired_path).enabled);
    assert!(!restarted.installation_dir.join("connector.token").exists());
    assert!(f.credentials.get(&original_id).unwrap().is_some());
    let nonce = f.fake.state.lock().unwrap().enrollment_nonce.clone();
    let state = restarted.bootstrap.state().unwrap();
    assert_eq!(state.enrollment_nonce.unwrap(), nonce);
    assert!(state.active.is_none());
    assert_eq!(state.retired.unwrap().installation_id, original_id);

    let reenabled = restarted.enable().unwrap();
    assert_eq!(reenabled["provisioned"], json!(true));
    let active_after = restarted.bootstrap.state().unwrap().active.unwrap();
    assert_eq!(active_after.installation_id, original_id);
    assert_eq!(active_after.hostname, original_hostname);
    let new_credential = restarted.bootstrap.credential(&active_after).unwrap();
    assert_ne!(new_credential, original_credential);
    assert_eq!(
        f.credentials.get(&original_id).unwrap().unwrap(),
        new_credential
    );

    // A different owner replaying a different nonce never collides with this installation.
    let other_dir = tempfile::tempdir().unwrap();
    let other = manager_for(other_dir.path(), &"f".repeat(32));
    assert_eq!(other.manager.enable().unwrap()["provisioned"], json!(true));
    assert_ne!(
        other
            .manager
            .bootstrap
            .state()
            .unwrap()
            .active
            .unwrap()
            .installation_id,
        original_id
    );
}

#[test]
fn revoke_failure_closes_local_access_and_retries_after_restart() {
    let f = enrolled_fixture();
    f.manager.enable().unwrap();
    f.fake.state.lock().unwrap().fail_revoke = true;
    assert!(f.manager.disable().unwrap_err().is_provider());
    assert!(!read_desired(&f.manager.desired_path).enabled);
    assert!(f.credentials.get(&id_of(&f)).unwrap().is_some());
    f.fake.state.lock().unwrap().fail_revoke = false;
    let restarted = restart(&f);
    restarted.reconcile_control().unwrap();
    assert!(f.fake.state.lock().unwrap().revoked);
    assert!(restarted.bootstrap.state().unwrap().active.is_none());
    assert!(f.credentials.get(&id_of(&f)).unwrap().is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn startup_reconciliation_retries_after_transient_outage() {
    let f = enrolled_fixture();
    f.manager.enable().unwrap();
    // The supervisor loop uses the real clock for its retry delays.
    let (platform, _spawner) = real_clock_platform(Arc::new(f.fake.clone()));
    let restarted = Arc::new(
        DesktopRemoteManager::new(
            &f.manager.root,
            &f.manager.cloudflared_binary,
            platform,
            options(f.manager.bootstrap.clone(), &f.fake),
        )
        .unwrap(),
    );
    f.fake.state.lock().unwrap().offline = true;
    let stop = StopFlag::new();
    let task = tokio::spawn(restarted.clone().run(stop.clone()));
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(restarted.connector_token().is_none());
    f.fake.state.lock().unwrap().offline = false;
    for _ in 0..100 {
        if restarted.connector_token().is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(
        restarted.connector_token().as_deref(),
        Some("desktop-installation-cloudflared-token")
    );
    stop.set();
    task.await.unwrap();
}

#[test]
fn revoked_or_expired_installation_is_retired_during_reconcile() {
    let mut f = enrolled_fixture();
    let revoked = Arc::new(AtomicU32::new(0));
    let counter = revoked.clone();
    f.manager.revoke_sessions = Some(Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    }));
    f.manager.enable().unwrap();
    f.fake.state.lock().unwrap().reject_credential = true;
    f.manager.reconcile_control().unwrap();
    assert!(f.manager.bootstrap.state().unwrap().active.is_none());
    assert!(f.credentials.get(&id_of(&f)).unwrap().is_none());
    assert!(!read_desired(&f.manager.desired_path).enabled);
    assert_eq!(revoked.load(Ordering::SeqCst), 1);
    assert!(f.manager.connector_token().is_none());
}

#[test]
fn missing_native_credential_fails_closed_without_reenrollment() {
    let f = enrolled_fixture();
    f.credentials.delete(&id_of(&f)).unwrap();
    // An external wipe is only detected on the next process start: fresh, uncached store.
    let fresh = verified_store(f.manager.bootstrap.path(), f.credentials.clone());
    let restarted = DesktopRemoteManager::new(
        &f.manager.root,
        &f.manager.cloudflared_binary,
        f.tp.platform.clone(),
        options(fresh, &f.fake),
    )
    .unwrap();
    let status = restarted.status();
    assert_eq!(status["provisioned"], json!(true));
    assert_eq!(status["credential_available"], json!(false));
    let err = restarted.enable().unwrap_err();
    assert!(
        matches!(&err, RemoteError::Provider(m) if m.contains("credential is unavailable")),
        "{err:?}"
    );
    assert!(restarted.bootstrap.state().unwrap().active.is_some());
}

#[test]
fn regenerate_revokes_old_identity_and_provisions_a_new_hostname() {
    let f = enrolled_fixture();
    f.manager.enable().unwrap();
    let original_id = id_of(&f);
    let original_hostname = f
        .manager
        .bootstrap
        .state()
        .unwrap()
        .active
        .unwrap()
        .hostname;
    let result = f.manager.regenerate().unwrap();
    assert!(f.fake.state.lock().unwrap().reenrolled);
    assert!(f.credentials.get(&original_id).unwrap().is_none());
    let new_active = f.manager.bootstrap.state().unwrap().active.unwrap();
    assert_ne!(new_active.installation_id, original_id);
    assert_ne!(new_active.hostname, original_hostname);
    assert_eq!(result["provisioned"], json!(true));
    assert_eq!(result["enabled"], json!(true));
    f.manager.regenerate().unwrap();
    let third = f.manager.bootstrap.state().unwrap().active.unwrap();
    assert_ne!(third.installation_id, new_active.installation_id);
    assert_ne!(third.hostname, new_active.hostname);
    assert!(f
        .credentials
        .get(&new_active.installation_id)
        .unwrap()
        .is_none());
}

#[test]
fn regenerate_failure_keeps_old_identity() {
    let f = enrolled_fixture();
    f.manager.enable().unwrap();
    f.fake.state.lock().unwrap().fail_reenroll = true;
    assert!(f.manager.regenerate().unwrap_err().is_provider());
    assert_eq!(
        f.manager
            .bootstrap
            .state()
            .unwrap()
            .active
            .unwrap()
            .installation_id,
        id_of(&f)
    );
    assert!(f.credentials.get(&id_of(&f)).unwrap().is_some());
    assert!(f.manager.feature.intent().enabled);
}

#[test]
fn regenerate_cancel_path_makes_no_changes() {
    let f = enrolled_fixture();
    f.manager.enable().unwrap();
    let before = f.manager.status();
    let before_active = f.manager.bootstrap.state().unwrap().active;
    for _ in 0..3 {
        f.manager.status();
    }
    assert_eq!(before, f.manager.status());
    assert_eq!(before_active, f.manager.bootstrap.state().unwrap().active);
    assert!(!f.fake.state.lock().unwrap().revoked);
    assert!(f.credentials.get(&id_of(&f)).unwrap().is_some());
}

#[test]
fn regenerate_repeatedly_never_hits_anonymous_enrollment_endpoint() {
    let f = enrolled_fixture();
    f.manager.enable().unwrap();
    let before = f.fake.count("/v1/installations/enroll");
    let mut seen = std::collections::BTreeSet::from([f
        .manager
        .bootstrap
        .state()
        .unwrap()
        .active
        .unwrap()
        .installation_id]);
    for _ in 0..5 {
        let result = f.manager.regenerate().unwrap();
        assert_eq!(result["enabled"], json!(true));
        let active = f.manager.bootstrap.state().unwrap().active.unwrap();
        assert!(seen.insert(active.installation_id));
    }
    assert_eq!(f.fake.count("/v1/installations/enroll"), before);
}

#[test]
fn enable_after_disable_reactivates_via_authenticated_path_not_anonymous_enroll() {
    let mut f = enrolled_fixture();
    f.manager.enable().unwrap();
    let original_id = id_of(&f);
    f.manager.disable().unwrap();
    let state = f.manager.bootstrap.state().unwrap();
    assert!(state.active.is_none() && state.retired.is_some());
    let enroll = Arc::new(AtomicU32::new(0));
    f.manager.transport = Some(Arc::new(Counting {
        inner: Arc::new(f.fake.clone()),
        enroll: enroll.clone(),
    }));
    let reenabled = f.manager.enable().unwrap();
    assert!(f.fake.state.lock().unwrap().reactivated);
    assert_eq!(enroll.load(Ordering::SeqCst), 0);
    assert_eq!(reenabled["provisioned"], json!(true));
    let active = f.manager.bootstrap.state().unwrap().active.unwrap();
    assert_eq!(active.installation_id, original_id);
    assert!(f.manager.bootstrap.state().unwrap().retired.is_none());
    assert!(f.credentials.get(&original_id).unwrap().is_some());
}

#[test]
fn genuinely_first_time_installation_still_uses_anonymous_enroll() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = manager_for(dir.path(), &"a".repeat(32));
    let enroll = Arc::new(AtomicU32::new(0));
    f.manager.transport = Some(Arc::new(Counting {
        inner: Arc::new(f.fake.clone()),
        enroll: enroll.clone(),
    }));
    assert_eq!(f.manager.enable().unwrap()["provisioned"], json!(true));
    assert_eq!(enroll.load(Ordering::SeqCst), 1);
    assert!(!f.fake.state.lock().unwrap().reactivated);
}

#[test]
fn reactivate_surfaces_quota_exceeded_as_specific_provider_error() {
    let f = enrolled_fixture();
    f.manager.enable().unwrap();
    f.manager.disable().unwrap();
    let limited: Arc<dyn HttpTransport> = Arc::new(FnTransport(|r: &HttpRequest| {
        assert!(
            r.path().ends_with("/reactivate"),
            "unexpected route {}",
            r.path()
        );
        Ok(HttpResponse::json_response(
            429,
            &json!({"error": "limited"}),
        ))
    }));
    let fresh = verified_store(
        f.manager.bootstrap.path(),
        f.manager.bootstrap.credentials(),
    );
    let err = fresh
        .ensure_enrolled(ORIGIN, 18766, Some(&limited), &Pacing::instant(), None)
        .unwrap_err();
    assert!(
        matches!(&err, RemoteError::Provider(m) if m.contains("enrollment limit")),
        "{err:?}"
    );
}

struct CountingCredentials {
    inner: Arc<MemoryCredentials>,
    gets: AtomicU32,
}

impl RemoteCredentialStore for CountingCredentials {
    fn get(&self, id: &str) -> Res<Option<String>> {
        self.gets.fetch_add(1, Ordering::SeqCst);
        self.inner.get(id)
    }

    fn set(&self, id: &str, credential: &str) -> Res<()> {
        self.inner.set(id, credential)
    }

    fn delete(&self, id: &str) -> Res<()> {
        self.inner.delete(id)
    }
}

#[test]
fn credential_is_cached_in_memory_after_first_successful_read() {
    let dir = tempfile::tempdir().unwrap();
    let counting = Arc::new(CountingCredentials {
        inner: memory_credentials(),
        gets: AtomicU32::new(0),
    });
    let root = dir.path().join("remote");
    let store = verified_store(&root.join("bootstrap.json"), counting.clone());
    let fake = FakeControlPlane::new(bundle_payload(&"a".repeat(32)));
    let tp = test_platform(Arc::new(fake.clone()));
    let manager = DesktopRemoteManager::new(
        &root,
        &binary(dir.path()),
        tp.platform.clone(),
        options(store, &fake),
    )
    .unwrap();
    manager.enable().unwrap();
    let active = manager.bootstrap.state().unwrap().active.unwrap();
    manager.bootstrap.cache_clear();
    counting.gets.store(0, Ordering::SeqCst);
    for _ in 0..5 {
        manager.status();
    }
    assert_eq!(counting.gets.load(Ordering::SeqCst), 1);
    assert_eq!(
        manager.bootstrap.credential(&active).unwrap(),
        fake.state.lock().unwrap().payload["installationCredential"]
            .as_str()
            .unwrap()
    );
    assert_eq!(counting.gets.load(Ordering::SeqCst), 1);
}

#[test]
fn reenroll_invalidates_stale_cached_credential() {
    let f = enrolled_fixture();
    f.manager.enable().unwrap();
    let original = f.manager.bootstrap.state().unwrap().active.unwrap();
    let original_credential = f.manager.bootstrap.credential(&original).unwrap();
    f.manager.regenerate().unwrap();
    let new_active = f.manager.bootstrap.state().unwrap().active.unwrap();
    assert_ne!(new_active.installation_id, original.installation_id);
    let new_credential = f.manager.bootstrap.credential(&new_active).unwrap();
    assert_ne!(new_credential, original_credential);
    assert_eq!(
        f.credentials
            .get(&new_active.installation_id)
            .unwrap()
            .unwrap(),
        new_credential
    );
    assert!(!f
        .manager
        .bootstrap
        .cache_contains(&original.installation_id));
    assert!(f
        .manager
        .bootstrap
        .cache_contains(&new_active.installation_id));
}

#[test]
fn reactivate_invalidates_stale_cached_credential() {
    let f = enrolled_fixture();
    f.manager.enable().unwrap();
    let active = f.manager.bootstrap.state().unwrap().active.unwrap();
    let original = f.manager.bootstrap.credential(&active).unwrap();
    f.manager.disable().unwrap();
    assert_eq!(f.manager.enable().unwrap()["provisioned"], json!(true));
    let new_active = f.manager.bootstrap.state().unwrap().active.unwrap();
    assert_eq!(new_active.installation_id, active.installation_id);
    assert_ne!(
        f.manager.bootstrap.credential(&new_active).unwrap(),
        original
    );
}

/// A one-shot loopback HTTP server that answers GET after `delay` (the slow-broker regression).
fn slow_broker(
    delay: std::time::Duration,
    body: &'static str,
) -> (u16, std::thread::JoinHandle<()>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0_u8; 2048];
        let _ = stream.read(&mut buf);
        std::thread::sleep(delay);
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
    });
    (port, handle)
}

#[test]
fn slow_credential_broker_response_within_new_timeout_does_not_error() {
    let value = format!("spi_{}.{}", "a".repeat(8), "s".repeat(40));
    let leaked: &'static str = Box::leak(value.clone().into_boxed_str());
    let delay = std::time::Duration::from_secs(4);
    let (port, server) = slow_broker(delay, leaked);
    let store = NativeRemoteCredentialStore::new(
        Some(&format!("http://127.0.0.1:{port}")),
        Some("test-token"),
        Arc::new(stagepilot_remote::http::ReqwestTransport::new()),
    );
    let started = std::time::Instant::now();
    let got = store.get(&"a".repeat(8)).unwrap();
    assert!(started.elapsed() >= delay);
    assert_eq!(got.as_deref(), Some(value.as_str()));
    server.join().unwrap();
}

#[test]
fn native_credential_store_timeout_is_generous_not_a_few_seconds() {
    const { assert!(BROKER_TIMEOUT_SECS >= 90.0) };
}

#[test]
fn native_credential_store_rejects_non_loopback_and_missing_token() {
    let t: Arc<dyn HttpTransport> =
        Arc::new(FnTransport(|_: &HttpRequest| panic!("must not send")));
    for (origin, token) in [
        ("http://example.com:80", "t"),
        ("https://127.0.0.1:9", "t"),
        ("http://127.0.0.1", "t"),
        ("http://127.0.0.1:9/path", "t"),
        ("http://u@127.0.0.1:9", "t"),
        ("http://127.0.0.1:9", ""),
    ] {
        let store = NativeRemoteCredentialStore::new(Some(origin), Some(token), t.clone());
        // An empty token falls back to the environment, which the test run leaves unset.
        let err = store.get("aaaaaaaa").unwrap_err();
        assert_eq!(
            err,
            RemoteError::provider("The operating-system credential store is unavailable"),
            "{origin}"
        );
    }
}

#[test]
fn native_credential_store_status_mapping() {
    let status = Arc::new(AtomicU32::new(404));
    let seen = Arc::new(Mutex::new(Vec::<(String, String, Option<String>)>::new()));
    let t: Arc<dyn HttpTransport> = Arc::new(FnTransport({
        let (status, seen) = (status.clone(), seen.clone());
        move |r: &HttpRequest| {
            seen.lock().unwrap().push((
                r.method.clone(),
                r.path(),
                r.header("authorization").map(str::to_owned),
            ));
            Ok(HttpResponse::new(
                status.load(Ordering::SeqCst) as u16,
                "body",
            ))
        }
    }));
    let store = NativeRemoteCredentialStore::new(Some("http://127.0.0.1:9"), Some("tok"), t);
    assert_eq!(store.get("aaaaaaaa").unwrap(), None);
    status.store(200, Ordering::SeqCst);
    assert_eq!(store.get("aaaaaaaa").unwrap().as_deref(), Some("body"));
    status.store(500, Ordering::SeqCst);
    assert!(store.get("aaaaaaaa").is_err());
    status.store(204, Ordering::SeqCst);
    store.set("aaaaaaaa", "cred").unwrap();
    store.delete("aaaaaaaa").unwrap();
    status.store(500, Ordering::SeqCst);
    assert!(store.set("aaaaaaaa", "cred").is_err());
    assert!(store.delete("aaaaaaaa").is_err());
    let seen = seen.lock().unwrap();
    assert_eq!(
        seen[0],
        (
            "GET".into(),
            "/v1/credentials/aaaaaaaa".into(),
            Some("Bearer tok".into())
        )
    );
}

#[test]
fn permanently_revoked_credential_surfaces_recovery_action_and_reset_unblocks() {
    let mut f = enrolled_fixture();
    f.manager.enable().unwrap();
    f.manager.disable().unwrap();
    let original_id = id_of(&f);
    f.manager.transport = Some(Arc::new(FnTransport(|r: &HttpRequest| {
        assert!(
            r.path().ends_with("/reactivate"),
            "unexpected route {}",
            r.path()
        );
        Ok(HttpResponse::json_response(
            401,
            &json!({"error": "revoked"}),
        ))
    })));
    for _ in 0..3 {
        assert!(matches!(
            f.manager.enable().unwrap_err(),
            RemoteError::PermanentlyRevoked(_)
        ));
    }
    assert_eq!(
        f.manager
            .bootstrap
            .state()
            .unwrap()
            .retired
            .unwrap()
            .installation_id,
        original_id
    );
    let status = f.manager.status();
    assert_eq!(status["permanently_revoked"], json!(true));
    assert_eq!(status["enabled"], json!(false));

    let reset = f.manager.reset_identity().unwrap();
    assert_eq!(reset["permanently_revoked"], json!(false));
    let state = f.manager.bootstrap.state().unwrap();
    assert!(state.retired.is_none() && state.active.is_none());

    f.manager.transport = Some(Arc::new(f.fake.clone()));
    f.fake.state.lock().unwrap().reject_credential = false;
    let result = f.manager.enable().unwrap();
    let new_active = f.manager.bootstrap.state().unwrap().active.unwrap();
    assert_ne!(new_active.installation_id, original_id);
    assert_eq!(result["permanently_revoked"], json!(false));
    assert_eq!(result["enabled"], json!(true));
}

#[test]
fn bootstrap_state_file_rules() {
    let dir = tempfile::tempdir().unwrap();
    let store = verified_store(&dir.path().join("b.json"), memory_credentials());
    assert_eq!(store.state().unwrap(), BootstrapState::default());
    std::fs::write(store.path(), "x".repeat(40_000)).unwrap();
    assert!(store.state().is_err());
    std::fs::write(store.path(), r#"{"unknown": 1}"#).unwrap();
    assert!(store.state().is_err());
    // The legacy field is accepted and never written back.
    std::fs::write(store.path(), r#"{"consumedBundleIds": ["x"]}"#).unwrap();
    assert_eq!(store.state().unwrap().legacy_consumed_bundle_ids, vec!["x"]);
    assert!(DesktopBootstrapStore::new("relative.json".into(), None, None).is_err());
}

#[test]
fn device_name_is_sanitised() {
    assert_eq!(sanitize_device_name(None), None);
    assert_eq!(sanitize_device_name(Some("  \u{7}  ")), None);
    assert_eq!(
        sanitize_device_name(Some("  stage\u{0}pc \n")).as_deref(),
        Some("stagepc")
    );
    assert_eq!(
        sanitize_device_name(Some(&"x".repeat(300))).unwrap().len(),
        253
    );
}

#[test]
fn retry_after_parsing_and_backoff() {
    assert_eq!(parse_retry_after(Some("2")), 2);
    assert_eq!(parse_retry_after(Some("999")), 300);
    assert_eq!(parse_retry_after(Some("-4")), 0);
    assert_eq!(parse_retry_after(Some("soon")), 0);
    assert_eq!(parse_retry_after(None), 0);
    let pacing = Pacing {
        sleep: Arc::new(|_| {}),
        random: Arc::new(|| 1.0),
    };
    assert_eq!(pacing.backoff(0, 0), 0.75);
    assert_eq!(pacing.backoff(0, 5), 5.0);
}
