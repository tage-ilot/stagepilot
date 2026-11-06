//! A keychain that never answers, refuses, errors or answers late must never hold up StagePilot.
//!
//! rc.3 on the macOS iMac sat in `SecKeychainFindGenericPassword` during startup, waiting on an
//! approval dialog nobody could answer, and the backend never opened its port. These tests build
//! the real runtime and the real HTTP server around a fake credential store that behaves like that
//! dialog (blocks until released, refuses, errors, answers after 3 s, holds nothing) and check:
//!
//! * the server answers `/api/v1/health` over a real socket within a generous bound;
//! * only Planning Center says what is wrong ("Waiting for keychain access" / "refused");
//! * when the store starts answering, Planning Center connects by itself, with no restart.
//!
//! No real keychain and no rig are involved.

use axum::body::Body;
use axum::extract::Request as AxumRequest;
use axum::response::Response;
use serde_json::{json, Value};
use stagepilot_core::config::{ConnectionMethod, IntegrationModes, ServiceSource};
use stagepilot_core::credential_guard::{KEYCHAIN_REFUSED, KEYCHAIN_WAITING};
use stagepilot_core::settings::{
    CredentialStore, CredentialStoreError, MemorySettingsStore, PersistentPlanningCenterSettings,
    PersistentSettings, SettingsService,
};
use stagepilot_core::state::ConnectionStatus;
use stagepilot_pco::runtime::PcoOptions;
use stagepilot_server::launch::serve_until;
use stagepilot_server::Runtime;
use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::Url;

// ---- the fake keychain --------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    /// Waits on a dialog nobody answers, until `release`.
    Block,
    Error,
    /// Answers after 3 s.
    Slow,
    /// Answers at once: no entry stored.
    Empty,
    /// Answers at once with the stored token.
    Answer,
}

struct Inner {
    mode: Mutex<Mode>,
    released: Mutex<bool>,
    cv: Condvar,
    value: Mutex<Option<String>>,
    slow_done: Mutex<bool>,
}

#[derive(Clone)]
struct FakeKeychain(Arc<Inner>);

impl FakeKeychain {
    fn new(mode: Mode, value: Option<String>) -> Self {
        Self(Arc::new(Inner {
            mode: Mutex::new(mode),
            released: Mutex::new(false),
            cv: Condvar::new(),
            value: Mutex::new(value),
            slow_done: Mutex::new(false),
        }))
    }
    fn set_mode(&self, mode: Mode) {
        *self.0.mode.lock().unwrap() = mode;
    }
    /// The dialog is answered: blocked calls return.
    fn release(&self) {
        *self.0.released.lock().unwrap() = true;
        self.0.cv.notify_all();
    }
    fn act(&self) -> Result<(), CredentialStoreError> {
        let mode = *self.0.mode.lock().unwrap();
        match mode {
            Mode::Block => {
                let mut released = self.0.released.lock().unwrap();
                while !*released {
                    released = self.0.cv.wait(released).unwrap();
                }
                Ok(())
            }
            Mode::Slow => {
                // Only the first calls wait (the dialog is answered after 3 s).
                if !*self.0.slow_done.lock().unwrap() {
                    std::thread::sleep(Duration::from_secs(3));
                    *self.0.slow_done.lock().unwrap() = true;
                }
                Ok(())
            }
            Mode::Error => Err(CredentialStoreError("user denied".into())),
            Mode::Empty | Mode::Answer => Ok(()),
        }
    }
}

impl CredentialStore for FakeKeychain {
    fn get_secret(&self) -> Result<Option<String>, CredentialStoreError> {
        let mode = *self.0.mode.lock().unwrap();
        self.act()?;
        if mode == Mode::Empty {
            return Ok(None);
        }
        Ok(self.0.value.lock().unwrap().clone())
    }
    fn set_secret(&self, secret: &str) -> Result<(), CredentialStoreError> {
        self.act()?;
        *self.0.value.lock().unwrap() = Some(secret.to_string());
        Ok(())
    }
    fn remove_secret(&self) -> Result<(), CredentialStoreError> {
        self.act()?;
        *self.0.value.lock().unwrap() = None;
        Ok(())
    }
}

// ---- a fake Planning Center ----------------------------------------------------------------------

async fn fake_pco(request: AxumRequest) -> Response {
    let path = request.uri().path().to_string();
    let body = if path.ends_with("/service_types") {
        json!({"data": [{"type": "ServiceType", "id": "42",
            "attributes": {"name": "Weekend", "sequence": 1}}], "meta": {"total_count": null}})
    } else {
        json!({"data": [], "meta": {"total_count": null}})
    };
    Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn start_fake_pco() -> Url {
    let app = axum::Router::new().fallback(fake_pco);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap()
}

// ---- the application ------------------------------------------------------------------------------

fn token_blob() -> String {
    let expires = chrono::Utc::now().timestamp() as f64 + 7200.0;
    json!({"access_token": "access", "refresh_token": "refresh", "expires_at": expires,
        "scope": "services", "needs_reconnect": false})
    .to_string()
}

struct App {
    port: u16,
    runtime: Arc<Runtime>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    server: Option<tokio::task::JoinHandle<()>>,
    /// How long building and starting the runtime took.
    startup: Duration,
}

async fn free_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

/// The runtime and the real HTTP server around `keychain`, as `run_production_until` builds them:
/// the same fake behind the Planning Center secret and behind the OAuth tokens.
async fn app(keychain: &FakeKeychain, method: ConnectionMethod) -> App {
    let api = start_fake_pco().await;
    let persistent = PersistentSettings {
        integration_modes: IntegrationModes {
            service_source: ServiceSource::PlanningCenter,
            ..IntegrationModes::default()
        },
        timezone: "UTC".into(),
        planning_center: PersistentPlanningCenterSettings {
            app_id: Some("test-app".into()),
            service_type_id: Some("42".into()),
            connection_method: method,
            ..PersistentPlanningCenterSettings::default()
        },
        ..PersistentSettings::default()
    };
    let begun = Instant::now();
    let service = SettingsService::new(
        Box::new(MemorySettingsStore::new(Some(persistent))),
        Box::new(keychain.clone()),
        BTreeMap::new(),
    );
    service.load();
    let options = PcoOptions {
        memory_tokens: true,
        api_base: Some(api),
        oauth_credentials: Some(Arc::new(keychain.clone())),
        ..PcoOptions::default()
    };
    let runtime = Runtime::with_pco(Arc::new(service), false, options);
    runtime.set_keychain_retry_interval(Duration::from_millis(200));
    let port = free_port().await;
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let serving = runtime.clone();
    let server = tokio::spawn(async move {
        let address = format!("127.0.0.1:{port}").parse().unwrap();
        let _ = serve_until(serving, address, None, async move {
            let _ = stopped.await;
        })
        .await;
    });
    let app = App {
        port,
        runtime,
        stop: Some(stop),
        server: Some(server),
        startup: Duration::ZERO,
    };
    app.wait_for_health(Duration::from_secs(10)).await;
    App {
        startup: begun.elapsed(),
        ..app
    }
}

impl App {
    async fn get(&self, path: &str) -> Option<(u16, Value)> {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", self.port))
            .await
            .ok()?;
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).await.ok()?;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.ok()?;
        let text = String::from_utf8_lossy(&raw).to_string();
        let status = text.split_whitespace().nth(1)?.parse().ok()?;
        let body = text.split("\r\n\r\n").nth(1).unwrap_or_default();
        Some((status, serde_json::from_str(body).unwrap_or(Value::Null)))
    }

    async fn wait_for_health(&self, within: Duration) {
        let end = Instant::now() + within;
        while Instant::now() < end {
            if let Some((200, _)) = self.get("/api/v1/health").await {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("/api/v1/health did not answer within {within:?}");
    }

    async fn pco_detail(&self) -> Option<String> {
        let (_, body) = self.get("/api/v1/planning-center/status").await?;
        body["detail"].as_str().map(str::to_string)
    }

    async fn until(&self, what: &str, check: impl Fn(&App) -> bool) {
        let end = Instant::now() + Duration::from_secs(15);
        while Instant::now() < end {
            if check(self) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("never reached: {what}");
    }

    async fn shut_down(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(server) = self.server.take() {
            let _ = tokio::time::timeout(Duration::from_secs(10), server).await;
        }
    }
}

fn pco_connected(app: &App) -> bool {
    app.runtime.store.snapshot().planning_center_status == ConnectionStatus::Connected
}

// ---- tests ---------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_keychain_that_never_answers_does_not_stop_the_server_and_recovers() {
    let keychain = FakeKeychain::new(Mode::Block, Some(token_blob()));
    let app = app(&keychain, ConnectionMethod::Oauth).await;
    assert!(
        app.startup < Duration::from_secs(5),
        "startup took {:?} behind a keychain that never answers",
        app.startup
    );
    // The whole app answers; Planning Center alone says why it is not connected.
    for path in [
        "/api/v1/health",
        "/api/v1/state",
        "/api/v1/playback-api/status",
        "/api/v1/planning-center/status",
    ] {
        let began = Instant::now();
        let (code, _) = app.get(path).await.unwrap_or((0, Value::Null));
        assert_eq!(code, 200, "{path}");
        assert!(began.elapsed() < Duration::from_secs(2), "{path} was slow");
    }
    assert_eq!(app.pco_detail().await.as_deref(), Some(KEYCHAIN_WAITING));
    assert!(!pco_connected(&app));
    app.until("the plugin reports the keychain wait", |a| {
        a.runtime
            .store
            .snapshot()
            .service_load
            .message
            .as_deref()
            .is_some_and(|m| m.contains("Waiting for keychain access"))
    })
    .await;
    // Nobody answered for a while; the app is still fine.
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert!(app.get("/api/v1/health").await.is_some());
    // The dialog is answered later: Planning Center connects by itself.
    keychain.set_mode(Mode::Answer);
    keychain.release();
    app.until(
        "Planning Center connects after the keychain answers",
        pco_connected,
    )
    .await;
    let detail = app.pco_detail().await.unwrap_or_default();
    assert!(!detail.to_lowercase().contains("keychain"), "{detail}");
    app.shut_down().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refused_keychain_says_so_and_recovers_when_it_answers() {
    let keychain = FakeKeychain::new(Mode::Error, Some(token_blob()));
    let app = app(&keychain, ConnectionMethod::Oauth).await;
    assert!(app.startup < Duration::from_secs(5), "{:?}", app.startup);
    assert_eq!(app.pco_detail().await.as_deref(), Some(KEYCHAIN_REFUSED));
    assert!(!pco_connected(&app));
    keychain.set_mode(Mode::Answer);
    app.until(
        "Planning Center connects after the keychain answers",
        pco_connected,
    )
    .await;
    app.shut_down().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_keychain_that_answers_after_three_seconds_connects_without_a_restart() {
    let keychain = FakeKeychain::new(Mode::Slow, Some(token_blob()));
    let app = app(&keychain, ConnectionMethod::Oauth).await;
    assert!(
        app.startup < Duration::from_secs(2),
        "startup waited for a slow keychain: {:?}",
        app.startup
    );
    assert!(!pco_connected(&app));
    app.until(
        "Planning Center connects once the slow read finishes",
        pco_connected,
    )
    .await;
    app.shut_down().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_empty_keychain_is_not_an_error() {
    let keychain = FakeKeychain::new(Mode::Empty, None);
    let app = app(&keychain, ConnectionMethod::Oauth).await;
    assert!(app.startup < Duration::from_secs(2), "{:?}", app.startup);
    let detail = app.pco_detail().await;
    assert!(
        !detail
            .as_deref()
            .is_some_and(|d| d.contains("keychain") || d.contains("Keychain")),
        "{detail:?}"
    );
    assert!(!app.runtime.keychain_degraded());
    app.shut_down().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_manual_secret_behind_a_blocked_keychain_does_not_stop_startup_either() {
    let keychain = FakeKeychain::new(Mode::Block, Some("pat-secret".into()));
    let app = app(&keychain, ConnectionMethod::Manual).await;
    assert!(app.startup < Duration::from_secs(5), "{:?}", app.startup);
    assert_eq!(app.pco_detail().await.as_deref(), Some(KEYCHAIN_WAITING));
    // The stored secret was not thrown away: once the dialog is answered it is used.
    keychain.set_mode(Mode::Answer);
    keychain.release();
    app.until(
        "Planning Center connects with the stored secret",
        pco_connected,
    )
    .await;
    app.shut_down().await;
}
