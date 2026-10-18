//! Port of backend/tests/test_remote_server.py, test_remote_transport.py, test_remote_runtime.py
//! and test_remote_lifecycle.py. The Python tests drive the real FastAPI app; here the ingress is
//! a small axum router (the real `RemoteIngress` lives in the server crate, owned by 6b-A), so
//! these tests cover the listener, HTTPS-only check, reconcile loop and lifecycle with it.

mod common;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::routing::get;
use axum::Router;
use common::*;
use serde_json::json;
use stagepilot_remote::control::RemoteControl;
use stagepilot_remote::error::{RemoteError, Res};
use stagepilot_remote::files::{atomic_write, read_desired, DesiredRemote};
use stagepilot_remote::platform::{InMemoryLocker, StopFlag};
use stagepilot_remote::provider::CloudflareProvider;
use stagepilot_remote::runtime::{reconcile, GenerationSink, IngressFactory, ManagedRemote};
use stagepilot_remote::server::{remote_listener, require_https, ListenerAttachment};

fn unused_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn app() -> Router {
    Router::new().route(
        "/api/v1/access",
        get(|| async { axum::Json(json!({"mode": "remote"})) }),
    )
}

fn http_client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap()
}

async fn get_status(port: u16, headers: &[(&str, &str)]) -> Option<u16> {
    let client = http_client();
    let mut request = client.get(format!("http://127.0.0.1:{port}/api/v1/access"));
    for (k, v) in headers {
        request = request.header(*k, *v);
    }
    request.send().await.ok().map(|r| r.status().as_u16())
}

async fn wait_listener(port: u16, enabled: bool) {
    let https = [("x-forwarded-proto", "https")];
    for _ in 0..160 {
        let status = get_status(port, &https).await;
        if enabled && status == Some(200) {
            return;
        }
        if !enabled && status.is_none() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("Listener did not reach requested state (enabled={enabled})");
}

#[tokio::test(flavor = "multi_thread")]
async fn transport_https_is_required_and_exact() {
    let port = unused_port();
    let listener = remote_listener(require_https(app()), port)
        .await
        .expect("bound");
    for headers in [
        vec![],
        vec![("x-forwarded-proto", "http")],
        vec![("x-forwarded-proto", "https,http")],
    ] {
        assert_eq!(get_status(port, &headers).await, Some(426), "{headers:?}");
    }
    // Two identical headers are rejected too.
    let client = http_client();
    let response = client
        .get(format!("http://127.0.0.1:{port}/api/v1/access"))
        .header("x-forwarded-proto", "https")
        .header("x-forwarded-proto", "https")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 426);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(
        get_status(port, &[("x-forwarded-proto", "https")]).await,
        Some(200)
    );
    listener.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn remote_port_conflict_keeps_lan_alive_and_socket_is_released_after_stop() {
    let occupied = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let busy = occupied.local_addr().unwrap().port();
    assert!(remote_listener(app(), busy).await.is_none());
    drop(occupied);
    let listener = remote_listener(app(), busy)
        .await
        .expect("bound after release");
    assert_eq!(get_status(busy, &[]).await, Some(200));
    listener.stop().await;
    // Remote socket is released after graceful shutdown.
    std::net::TcpListener::bind(("127.0.0.1", busy)).unwrap();
}

#[test]
fn invalid_listener_configuration_and_single_attachment() {
    let mut a = ListenerAttachment::new();
    for (origin, port) in [
        ("http://remote.test", 8766),
        ("https://remote.test/path", 8766),
        ("https://remote.test", 0),
    ] {
        assert!(
            matches!(a.attach_fixed(origin, port), Err(RemoteError::Value(_))),
            "{origin}:{port}"
        );
        assert!(!a.is_attached());
    }
    a.attach_fixed("https://remote.stagepilot.test", unused_port())
        .unwrap();
    assert!(a.is_attached());
    let err = a
        .attach_fixed("https://remote.stagepilot.test", unused_port())
        .unwrap_err();
    assert!(err.to_string().contains("already attached"));
}

// ------------------------------------------------------------------ runtime

#[derive(Default)]
struct Sink(Mutex<Vec<String>>);

impl GenerationSink for Sink {
    fn installation_generation(&self, generation: &str) -> Res<()> {
        self.0.lock().unwrap().push(generation.to_owned());
        Ok(())
    }
}

fn factory() -> IngressFactory {
    Arc::new(|_origin: &str| app())
}

const ORIGIN: &str = "https://managed.stagepilot.test";

#[tokio::test(flavor = "multi_thread")]
async fn live_enable_disable_reenable_and_restart_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let control = dir.path().join("remote.json");
    let port = unused_port();
    let mut desired = DesiredRemote {
        enabled: true,
        generation: uuid::Uuid::new_v4().to_string(),
        public_origin: ORIGIN.into(),
        port,
    };
    let sink = Arc::new(Sink::default());
    let mut attachment = ListenerAttachment::new();
    let managed =
        ManagedRemote::attach(&control, 8765, factory(), sink.clone(), &mut attachment).unwrap();
    wait_listener(port, false).await;
    atomic_write(&control, &desired.to_json()).unwrap();
    wait_listener(port, true).await;
    assert_eq!(sink.0.lock().unwrap().last(), Some(&desired.generation));
    // The HTTPS-only check wraps the ingress.
    assert_eq!(get_status(port, &[]).await, Some(426));
    // Disable closes the listener but not anything else.
    atomic_write(&control, &DesiredRemote::default().to_json()).unwrap();
    wait_listener(port, false).await;
    // A new generation re-enables it.
    desired.generation = uuid::Uuid::new_v4().to_string();
    atomic_write(&control, &desired.to_json()).unwrap();
    wait_listener(port, true).await;
    // Corrupt desired state closes Remote; recovery is automatic.
    atomic_write(&control, "not-json").unwrap();
    wait_listener(port, false).await;
    atomic_write(&control, &desired.to_json()).unwrap();
    wait_listener(port, true).await;
    managed.finish().await;
    std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
    // A fresh process-equivalent recovers the persisted desired state.
    let mut again = ListenerAttachment::new();
    let restarted = ManagedRemote::attach(
        &control,
        8765,
        factory(),
        Arc::new(Sink::default()),
        &mut again,
    )
    .unwrap();
    wait_listener(port, true).await;
    restarted.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn bind_conflict_retry_and_invalid_policy_leave_lan_alive() {
    let dir = tempfile::tempdir().unwrap();
    let control = dir.path().join("remote.json");
    let occupied = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = occupied.local_addr().unwrap().port();
    let mut desired = DesiredRemote {
        enabled: true,
        generation: uuid::Uuid::new_v4().to_string(),
        public_origin: ORIGIN.into(),
        port,
    };
    atomic_write(&control, &desired.to_json()).unwrap();
    // "LAN" stand-in: an independent listener that must stay up throughout.
    let lan_port = unused_port();
    let lan = remote_listener(app(), lan_port).await.unwrap();
    let mut attachment = ListenerAttachment::new();
    let managed = ManagedRemote::attach(
        &control,
        8765,
        factory(),
        Arc::new(Sink::default()),
        &mut attachment,
    )
    .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(get_status(lan_port, &[]).await, Some(200));
    drop(occupied);
    wait_listener(port, true).await;
    // A Remote port equal to the LAN port is refused.
    desired.port = 8765;
    atomic_write(&control, &desired.to_json()).unwrap();
    wait_listener(port, false).await;
    assert_eq!(get_status(lan_port, &[]).await, Some(200));
    managed.finish().await;
    lan.stop().await;
}

#[test]
fn managed_attachment_validation() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let mut a = ListenerAttachment::new();
        let sink: Arc<dyn GenerationSink> = Arc::new(Sink::default());
        assert!(ManagedRemote::attach(
            std::path::Path::new("relative.json"),
            8765,
            factory(),
            sink.clone(),
            &mut a
        )
        .is_err());
        let dir = tempfile::tempdir().unwrap();
        let managed = ManagedRemote::attach(
            &dir.path().join("r.json"),
            8765,
            factory(),
            sink.clone(),
            &mut a,
        )
        .unwrap();
        let err = ManagedRemote::attach(&dir.path().join("r.json"), 8765, factory(), sink, &mut a)
            .err()
            .unwrap();
        assert!(err.to_string().contains("already attached"));
        managed.finish().await;
    });
}

#[tokio::test(flavor = "multi_thread")]
async fn reconcile_stops_promptly_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let stop = StopFlag::new();
    let path = dir.path().join("none.json");
    let task = tokio::spawn({
        let stop = stop.clone();
        async move { reconcile(factory(), Arc::new(Sink::default()), &path, 8765, stop).await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    stop.set();
    tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .expect("stops")
        .unwrap();
}

// ------------------------------------------------------------------ lifecycle

#[tokio::test(flavor = "multi_thread")]
async fn provider_control_export_drives_real_listener() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = control_config(dir.path());
    config.remote_port = unused_port();
    let fake = FakeCloudflare::default();
    let provider = CloudflareProvider::new(&config, cloudflare_client(&fake));
    let control = Arc::new(RemoteControl::new(
        config.clone(),
        provider,
        Arc::new(InMemoryLocker::default()),
    ));
    let mut attachment = ListenerAttachment::new();
    let managed = ManagedRemote::attach(
        &control.desired_path,
        8765,
        factory(),
        Arc::new(Sink::default()),
        &mut attachment,
    )
    .unwrap();
    let port = config.remote_port;
    let apply = |action: &'static str| {
        let control = control.clone();
        async move {
            tokio::task::spawn_blocking(move || control.apply(action))
                .await
                .unwrap()
                .unwrap()
        }
    };
    let first = apply("enable").await;
    wait_listener(port, true).await;
    assert_eq!(apply("enable").await, first);
    assert_eq!(fake.state.lock().unwrap().created, 1);
    apply("disable").await;
    wait_listener(port, false).await;
    {
        let s = fake.state.lock().unwrap();
        assert!(s.tunnels.is_empty() && s.records.is_empty());
    }
    let second = apply("enable").await;
    wait_listener(port, true).await;
    assert_ne!(second["tunnel_id"], first["tunnel_id"]);
    apply("disable").await;
    managed.finish().await;
}

#[test]
fn corrupt_control_state_can_still_disable_local_access() {
    let dir = tempfile::tempdir().unwrap();
    let config = control_config(dir.path());
    let fake = FakeCloudflare::default();
    let provider = CloudflareProvider::new(&config, cloudflare_client(&fake));
    let control = RemoteControl::new(config, provider, Arc::new(InMemoryLocker::default()));
    control.apply("enable").unwrap();
    std::fs::write(
        control
            .desired_path
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("control/state.json"),
        "corrupt",
    )
    .unwrap();
    assert!(matches!(
        control.apply("disable"),
        Err(RemoteError::Value(_))
    ));
    assert!(!read_desired(&control.desired_path).enabled);
    assert!(!control.token_path.exists());
    // Cannot claim provider revocation without recoverable identity.
    assert!(!fake.state.lock().unwrap().tunnels.is_empty());
}

#[test]
fn invalid_enabled_policy_fails_validation() {
    for update in [
        r#""generation":"bad""#,
        r#""public_origin":"http://bad.test""#,
    ] {
        let text = format!(r#"{{"enabled":true,{update}}}"#);
        assert!(DesiredRemote::from_json(&text).is_err(), "{text}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_flag_wakes_async_and_blocking_waiters() {
    let stop = StopFlag::new();
    let counter = Arc::new(AtomicU32::new(0));
    let waiter = {
        let (stop, counter) = (stop.clone(), counter.clone());
        tokio::spawn(async move {
            stop.wait(30.0).await;
            counter.fetch_add(1, Ordering::SeqCst);
        })
    };
    let blocker = {
        let (stop, counter) = (stop.clone(), counter.clone());
        std::thread::spawn(move || {
            stop.wait_blocking(30.0);
            counter.fetch_add(1, Ordering::SeqCst);
        })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0);
    stop.set();
    tokio::time::timeout(Duration::from_secs(3), waiter)
        .await
        .unwrap()
        .unwrap();
    blocker.join().unwrap();
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}
