//! The ProPresenter Link on the StagePilot side: which connection the saved ProPresenter host means (this computer or another),
//! the status in every state, how often the helper is asked, that nothing is asked while no host is saved, the status route, the
//! carry-over of `propresenter.link` through a save, and the automatic update of the helper (a new kind of helper next to the
//! Library Link and the Lightkey helper). The helper is a stand-in on a loopback port that checks the HMAC like the real one;
//! no ProPresenter, no screen, no rig.

use async_trait::async_trait;
use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, Method, Request, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use http_body_util::BodyExt;
use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use stagepilot_core::config::{PlaybackLibrarySettings, TimerOutput};
use stagepilot_core::helper_update::{HelperUpdateMode, UpdateBundle};
use stagepilot_core::settings::{PersistentSettings, SettingsService};
use stagepilot_lights::lightkey::LightkeyStatus;
use stagepilot_propresenter::helper_assets::BUNDLED_HELPER_VERSION;
use stagepilot_propresenter::link::fake::FakeHost;
use stagepilot_propresenter::link::helper::sign;
use stagepilot_propresenter::link::status::{LinkMode, LinkState};
use stagepilot_propresenter::link::{
    HostInfo, IndexEntry, LibraryFile, LinkError, ProPresenterHost, SelectOutcome, PLAYBACK_LIBRARY,
};
use stagepilot_server::helper_update::{HelperUpdater, Outcome, UpdateEnvironment, UpdaterConfig};
use stagepilot_server::playback_library::PlaybackLibrary;
use stagepilot_server::propresenter_link::{
    host_part, is_own_address, memory_settings, names_this_computer, Choice, ProPresenterLink,
};
use stagepilot_server::{build_app, AppOptions, OutputBackends, Runtime};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::ServiceExt;

const KEY: &str = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-_AbCdE";
const TEST_SEED: [u8; 32] = [7; 32];

fn own_private_address() -> Option<Ipv4Addr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if ip.is_private() => Some(ip),
        _ => None,
    }
}

/// Saves a ProPresenter host with the ProPresenter output chosen (a host is "saved" when ProPresenter is in use).
fn save_host(settings: &SettingsService, host: &str, enabled: bool) {
    let mut next = settings.snapshot();
    next.integration_modes.timer_output = if enabled {
        TimerOutput::Propresenter
    } else {
        TimerOutput::Simulated
    };
    next.propresenter.host = host.to_string();
    settings.save(next).unwrap();
}

fn keypair() -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&TEST_SEED).unwrap()
}

fn public() -> [u8; 32] {
    <[u8; 32]>::try_from(keypair().public_key().as_ref()).unwrap()
}

fn leak(bytes: Vec<u8>) -> &'static [u8] {
    Box::leak(bytes.into_boxed_slice())
}

fn source(version: &str) -> Vec<u8> {
    format!("\"\"\"stand-in\"\"\"\n__version__ = \"{version}\"\n").into_bytes()
}

/// A bundle for the ProPresenter Link, signed by the test key (`signed`) or not at all.
fn bundle(version: &str, signed: bool) -> UpdateBundle {
    let file = leak(source(version));
    let sums = leak(
        format!(
            "{}  install.sh\n{}  propresenter_link.py\n",
            hex::encode(Sha256::digest(b"#!/bin/sh\n")),
            hex::encode(Sha256::digest(file))
        )
        .into_bytes(),
    );
    let signature = if signed {
        STANDARD.encode(keypair().sign(sums).as_ref()).into_bytes()
    } else {
        vec![]
    };
    UpdateBundle {
        version: version.to_string(),
        files: vec![("propresenter_link.py", file)],
        sums,
        signature: leak(signature),
    }
}

// ---- a stand-in for the helper --------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Says {
    /// Answers to our key.
    Fine,
    /// Wants another key: 401 to everything.
    Refuses,
    /// Does not know our key: the bare health, 401 to the rest.
    BareHealth,
    /// 503 to everything (it is there but cannot answer).
    Down,
}

struct Helper {
    says: Says,
    version: String,
    updater: Option<u32>,
    delay: Duration,
    gets: u32,
    posts: u32,
    post_signed: Vec<bool>,
    hosts: Vec<String>,
}

#[derive(Clone)]
struct Fake(Arc<Mutex<Helper>>);

impl Fake {
    fn new(version: &str) -> Self {
        Self(Arc::new(Mutex::new(Helper {
            says: Says::Fine,
            version: version.to_string(),
            updater: Some(1),
            delay: Duration::ZERO,
            gets: 0,
            posts: 0,
            post_signed: vec![],
            hosts: vec![],
        })))
    }

    fn set(&self, f: impl FnOnce(&mut Helper)) {
        f(&mut self.0.lock().unwrap());
    }

    fn gets(&self) -> u32 {
        self.0.lock().unwrap().gets
    }

    async fn serve(&self) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = Router::new().fallback(answer).with_state(self.clone());
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        port
    }
}

fn json_response(status: u16, value: Value) -> Response {
    (
        StatusCode::from_u16(status).unwrap(),
        [(header::CONTENT_TYPE, "application/json")],
        value.to_string(),
    )
        .into_response()
}

async fn answer(
    State(fake): State<Fake>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let (says, delay) = {
        let h = fake.0.lock().unwrap();
        (h.says, h.delay)
    };
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    let mut h = fake.0.lock().unwrap();
    if let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) {
        h.hosts.push(host.to_string());
    }
    let target = uri
        .path_and_query()
        .map_or(uri.path().to_string(), |t| t.as_str().to_string());
    let text = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let covered = matches!(method, Method::PUT | Method::POST).then_some(body.as_ref());
    let signed = says != Says::Refuses
        && match (text("x-ppl-time"), text("x-ppl-sig")) {
            (Some(time), Some(sig)) => time.parse::<u64>().is_ok_and(|t| {
                t.abs_diff(now) <= 120 && sign(KEY, method.as_str(), &target, t, covered) == sig
            }),
            _ => false,
        };
    if says == Says::Down {
        return json_response(503, json!({"error": "not ready"}));
    }
    if method == Method::GET && uri.path() == "/health" {
        h.gets += 1;
        if !signed {
            return if says == Says::Refuses {
                json_response(401, json!({"error": "key required"}))
            } else {
                json_response(
                    200,
                    json!({"ok": true, "name": "propresenter-link", "version": h.version, "auth": "required"}),
                )
            };
        }
        let mut health = json!({
            "ok": true, "name": "propresenter-link", "version": h.version, "auth": "key", "os": "macOS",
            "data_folder": "/Users/x/Documents/ProPresenter", "data_folder_exists": true,
            "libraries": ["Playback", "Songs"], "playback_library_exists": true,
            "can_import": false, "import_blocker": "Allow the ProPresenter Link in System Settings > Privacy & Security > Accessibility, then try again.",
        });
        if let Some(level) = h.updater {
            health["updater"] = json!(level);
        }
        return json_response(200, health);
    }
    if method == Method::POST && uri.path() == "/update" {
        h.posts += 1;
        h.post_signed.push(signed);
        if !signed {
            return json_response(401, json!({"error": "key required"}));
        }
        let request: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        let (Some(sums), Some(sig)) = (request["sums"].as_str(), request["sig"].as_str()) else {
            return json_response(400, json!({"error": "not in the expected shape"}));
        };
        let valid = STANDARD.decode(sig).is_ok_and(|sig| {
            UnparsedPublicKey::new(&ED25519, public())
                .verify(sums.as_bytes(), &sig)
                .is_ok()
        });
        if !valid {
            return json_response(
                422,
                json!({"error": "The update is not signed by the StagePilot project."}),
            );
        }
        let version = request["files"]["propresenter_link.py"]
            .as_str()
            .and_then(|b| STANDARD.decode(b).ok())
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|t| {
                t.lines().find_map(|l| {
                    l.strip_prefix("__version__ = \"")?
                        .strip_suffix('"')
                        .map(str::to_string)
                })
            })
            .unwrap_or_default();
        let running = h.version.clone();
        h.version = version.clone();
        return json_response(
            200,
            json!({"updating": true, "from": running, "to": version}),
        );
    }
    json_response(404, json!({"error": "not found"}))
}

/// A link on its own settings, with the ProPresenter output on and the stand-in helper as "another computer".
async fn rig(fake: &Fake) -> (Arc<SettingsService>, Arc<ProPresenterLink>) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let settings = memory_settings();
    save_host(&settings, "127.0.0.1", true);
    let link = Arc::new(ProPresenterLink::new(settings.clone()));
    link.set_own_address_check(Arc::new(|_| false));
    link.set_helper_port(fake.serve().await);
    link.set_key_for_test(KEY);
    (settings, link)
}

// ---- macOS asks for permission --------------------------------------------------------------------------------------------

/// This computer while macOS asks the person for permission: `info()` never comes back while `stuck` is set.
struct AskingMac {
    inner: FakeHost,
    stuck: AtomicBool,
    looks: std::sync::atomic::AtomicUsize,
}

impl AskingMac {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: FakeHost::new()
                .with_library("Songs")
                .with_library("Playback"),
            stuck: AtomicBool::new(true),
            looks: std::sync::atomic::AtomicUsize::new(0),
        })
    }
}

#[async_trait]
impl ProPresenterHost for AskingMac {
    async fn info(&self) -> Result<HostInfo, LinkError> {
        self.looks.fetch_add(1, Ordering::SeqCst);
        if self.stuck.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        self.inner.info().await
    }
    async fn list_library(&self, name: &str) -> Result<Vec<LibraryFile>, LinkError> {
        self.inner.list_library(name).await
    }
    async fn read_file(&self, library: &str, file: &str) -> Result<Vec<u8>, LinkError> {
        self.inner.read_file(library, file).await
    }
    async fn ensure_library(&self, name: &str) -> Result<bool, LinkError> {
        self.inner.ensure_library(name).await
    }
    async fn create_file(
        &self,
        library: &str,
        file: &str,
        bytes: &[u8],
        sha256: &str,
    ) -> Result<(), LinkError> {
        self.inner.create_file(library, file, bytes, sha256).await
    }
    async fn song_index(&self) -> Result<Vec<IndexEntry>, LinkError> {
        self.inner.song_index().await
    }
    async fn search_import(
        &self,
        multitracks_id: &str,
        name: &str,
        playlist: Option<String>,
    ) -> Result<String, LinkError> {
        self.inner
            .search_import(multitracks_id, name, playlist)
            .await
    }
    async fn select_library(&self, name: &str) -> Result<SelectOutcome, LinkError> {
        self.inner.select_library(name).await
    }
}

fn local_link(host: &Arc<AskingMac>, limit: Duration) -> Arc<ProPresenterLink> {
    let settings = memory_settings();
    save_host(&settings, "127.0.0.1", true);
    let link = Arc::new(ProPresenterLink::new(settings));
    link.set_local_host_for_test(host.clone());
    link.set_local_look_limit(limit);
    link
}

#[tokio::test]
async fn while_macos_asks_for_permission_the_status_answers_waiting_within_the_limit_and_the_next_call_looks_again(
) {
    let mac = AskingMac::new();
    let link = local_link(&mac, Duration::from_millis(300));
    let started = std::time::Instant::now();
    let waiting = link.status(false).await;
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "answered after {:?}, the probe never returns",
        started.elapsed()
    );
    assert_eq!(waiting.mode, Some(LinkMode::Local));
    assert_eq!(waiting.state, LinkState::Waiting);
    assert_eq!(
        waiting.message,
        "macOS may be asking for permission on this computer. Answer the prompt, then press Connect again."
    );
    assert!(waiting.import_blocker.is_none() && !waiting.can_import && !waiting.stale);
    assert!(waiting.libraries.is_empty() && waiting.data_folder.is_none());
    assert_eq!(
        serde_json::to_value(&waiting).unwrap()["state"],
        json!("waiting")
    );
    // The same ask again, at once (no 30 s wait): it looks again, and does not wait on the first look's fate.
    let again = link.status(false).await;
    assert_eq!(again.state, LinkState::Waiting);
    assert_eq!(mac.looks.load(Ordering::SeqCst), 2);
    // The person answered the prompt: the next call gets the real answer, and nothing was left held.
    mac.stuck.store(false, Ordering::SeqCst);
    let ready = link.status(false).await;
    assert_eq!(ready.state, LinkState::Ready);
    assert_eq!(ready.libraries, ["Playback", "Songs"]);
    assert_eq!(mac.looks.load(Ordering::SeqCst), 3);
    // A good answer is then kept for the usual 30 s.
    link.status(false).await;
    assert_eq!(mac.looks.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn waiting_for_macos_is_said_even_when_an_older_good_answer_is_held() {
    let mac = AskingMac::new();
    mac.stuck.store(false, Ordering::SeqCst);
    let link = local_link(&mac, Duration::from_millis(300));
    assert_eq!(link.status(false).await.state, LinkState::Ready);
    mac.stuck.store(true, Ordering::SeqCst);
    link.advance_clock(Duration::from_secs(31));
    let status = link.status(false).await;
    assert_eq!((status.state, status.stale), (LinkState::Waiting, false));
}

#[tokio::test]
async fn two_windows_asking_while_macos_asks_are_not_held_up_by_each_other() {
    let mac = AskingMac::new();
    let link = local_link(&mac, Duration::from_millis(400));
    let first = {
        let link = link.clone();
        tokio::spawn(async move { link.status(false).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    // The second window does not wait for the first look's limit: it reads what is held.
    let second = tokio::time::timeout(Duration::from_millis(250), link.status(false))
        .await
        .expect("the second ask must not wait for the first");
    assert_eq!(second.state, LinkState::Checking);
    assert_eq!(first.await.unwrap().state, LinkState::Waiting);
    assert_eq!(mac.looks.load(Ordering::SeqCst), 1);
}

// ---- which computer ---------------------------------------------------------------------------------------------------

#[tokio::test]
async fn the_saved_host_decides_between_this_computer_and_another() {
    // Loopback is this computer, however it is written.
    for host in [
        "127.0.0.1",
        "127.0.0.2",
        "localhost",
        "LocalHost",
        "[::1]",
        "::1",
    ] {
        assert!(names_this_computer(host, &is_own_address).await, "{host}");
    }
    // One of this computer's own addresses is too.
    if let Some(ip) = own_private_address() {
        assert!(
            names_this_computer(&ip.to_string(), &is_own_address).await,
            "{ip}"
        );
    }
    // Another computer is another computer: a private address nobody here holds, a public one, a name that leads nowhere.
    for host in [
        "192.0.2.77",
        "203.0.113.9",
        "10.254.254.254",
        "propresenter-mac.invalid",
        "0.0.0.0",
        "224.0.0.1",
    ] {
        assert!(!names_this_computer(host, &is_own_address).await, "{host}");
    }
    // The question itself can be answered by hand (the product asks the system).
    assert!(
        names_this_computer("10.1.2.3", &|ip| ip
            == IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3)))
        .await
    );
    assert!(
        !names_this_computer("10.1.2.4", &|ip| ip
            == IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3)))
        .await
    );
    assert_eq!(
        host_part("http://10.0.0.5:1025/").as_deref(),
        Some("10.0.0.5")
    );
}

#[tokio::test]
async fn the_choice_follows_the_saved_host_and_nothing_is_chosen_while_none_is_saved() {
    let settings = memory_settings();
    let link = ProPresenterLink::new(settings.clone());
    assert_eq!(
        link.choose().await,
        Choice::NoHost,
        "the ProPresenter output is not in use"
    );
    assert!(link.connect().await.is_none());
    save_host(&settings, "127.0.0.1", false);
    assert_eq!(
        link.choose().await,
        Choice::NoHost,
        "a host is only saved while ProPresenter is in use"
    );
    save_host(&settings, "127.0.0.1", true);
    assert_eq!(link.choose().await, Choice::Local);
    assert_eq!(link.connect().await.unwrap().0, LinkMode::Local);
    save_host(&settings, "203.0.113.9", true);
    assert_eq!(link.choose().await, Choice::Helper("203.0.113.9".into()));
    assert_eq!(link.connect().await.unwrap().0, LinkMode::Helper);
    save_host(&settings, " 203.0.113.9:1025 ", true);
    assert_eq!(
        link.choose().await,
        Choice::Helper("203.0.113.9".into()),
        "the ProPresenter API's port is not the helper's"
    );
    save_host(&settings, "   ", true);
    assert_eq!(link.choose().await, Choice::NoHost);
}

// ---- nothing asked ------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn nothing_is_asked_while_no_host_is_saved_not_even_by_the_status_or_the_check_button() {
    let fake = Fake::new(BUNDLED_HELPER_VERSION);
    let (settings, link) = rig(&fake).await;
    link.ensure_key().unwrap();
    save_host(&settings, "127.0.0.1", false);
    for check in [false, true, true] {
        let status = link.status(check).await;
        assert_eq!(status.mode, None);
        assert_eq!(status.state, LinkState::NotInstalled);
        assert_eq!(status.message, "Choose the ProPresenter computer first.");
        assert!(
            !status.can_import && status.libraries.is_empty() && status.computer_address.is_none()
        );
    }
    assert_eq!(fake.gets(), 0);
}

// ---- Local ------------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn on_the_propresenter_computer_the_status_is_ready_without_a_helper_or_a_key() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ProPresenter");
    std::fs::create_dir_all(root.join("Libraries/Songs")).unwrap();
    std::fs::create_dir_all(root.join("Libraries/Playback")).unwrap();
    let settings = memory_settings();
    save_host(&settings, "127.0.0.1", true);
    let mut next = settings.snapshot();
    next.propresenter.link.data_folder = Some(root.display().to_string());
    settings.save(next).unwrap();
    let link = ProPresenterLink::new(settings.clone());
    let status = link.status(false).await;
    assert_eq!(status.mode, Some(LinkMode::Local));
    assert_eq!(status.state, LinkState::Ready);
    assert_eq!(
        status.message,
        "ProPresenter is on this computer, so no setup is needed."
    );
    assert_eq!(status.libraries, ["Playback", "Songs"]);
    assert!(status.playback_library_exists);
    assert_eq!(status.data_folder.as_deref(), Some(root.to_str().unwrap()));
    assert!(status.computer_address.is_none() && !status.paired);
    assert_eq!(status.bundled_version, BUNDLED_HELPER_VERSION);
    if !cfg!(target_os = "macos") {
        assert!(!status.can_import);
        assert!(status.import_blocker.unwrap().contains("Mac"));
    }
    assert!(!link.is_paired(), "a local connection never makes a key");
    // The connection reads and writes the real folder (and only Playback).
    let (mode, host) = link.connect().await.unwrap();
    assert_eq!(mode, LinkMode::Local);
    let bytes = b"synthetic".to_vec();
    host.ensure_library(PLAYBACK_LIBRARY).await.unwrap();
    host.create_file(
        PLAYBACK_LIBRARY,
        "A (1).pro",
        &bytes,
        &hex::encode(Sha256::digest(&bytes)),
    )
    .await
    .unwrap();
    assert!(root.join("Libraries/Playback/A (1).pro").is_file());
}

#[tokio::test]
async fn a_folder_that_is_not_there_is_one_plain_sentence() {
    let settings = memory_settings();
    save_host(&settings, "127.0.0.1", true);
    let mut next = settings.snapshot();
    next.propresenter.link.data_folder = Some("/nonexistent/ProPresenter".into());
    settings.save(next).unwrap();
    let status = ProPresenterLink::new(settings).status(false).await;
    assert_eq!(status.mode, Some(LinkMode::Local));
    assert_eq!(status.state, LinkState::NotAnswering);
    assert!(
        status
            .message
            .contains("folder was not found on this computer"),
        "{}",
        status.message
    );
    assert!(status.libraries.is_empty());
}

// ---- the helper ---------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn every_state_of_the_helper_has_its_status_and_its_one_line() {
    // Not installed: nothing answers and no setup was ever started.
    let silent = {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        probe.local_addr().unwrap().port()
    };
    let fake = Fake::new(BUNDLED_HELPER_VERSION);
    let (_settings, link) = rig(&fake).await;
    link.clear_key_for_test();
    link.set_helper_port(silent);
    let status = link.status(true).await;
    assert_eq!(
        (status.mode, status.state),
        (Some(LinkMode::Helper), LinkState::NotInstalled)
    );
    assert_eq!(
        status.message,
        "The Presenter Link is not installed on the ProPresenter computer."
    );
    assert_eq!(status.computer_address.as_deref(), Some("127.0.0.1"));
    assert!(!status.paired);
    // Not answering: a setup made a key, and now nothing answers.
    link.ensure_key().unwrap();
    link.advance_clock(Duration::from_secs(5));
    let status = link.status(true).await;
    assert_eq!(status.state, LinkState::NotAnswering);
    assert_eq!(status.message, "The Presenter Link did not answer.");
    assert!(status.paired);

    // A helper that wants another key.
    let port = fake.serve().await;
    link.set_helper_port(port);
    link.set_key_for_test(&"Z".repeat(43));
    link.advance_clock(Duration::from_secs(60));
    let status = link.status(true).await;
    assert_eq!(status.state, LinkState::NeedsCommand);
    assert_eq!(
        status.message,
        "The Presenter Link does not know this StagePilot yet. Run the setup command again."
    );
    assert!(status.libraries.is_empty() && !status.can_import);
    // A helper that answers the bare health only (no valid signature from us).
    fake.set(|h| h.says = Says::BareHealth);
    link.advance_clock(Duration::from_secs(60));
    assert_eq!(link.status(true).await.state, LinkState::NeedsCommand);

    // Ready: our key, the bundled version.
    fake.set(|h| h.says = Says::Fine);
    link.set_key_for_test(KEY);
    link.advance_clock(Duration::from_secs(60));
    let status = link.status(true).await;
    assert_eq!(status.state, LinkState::Ready);
    assert_eq!(status.version.as_deref(), Some(BUNDLED_HELPER_VERSION));
    assert_eq!(
        status.message,
        format!("The Presenter Link (version {BUNDLED_HELPER_VERSION}) is ready.")
    );
    assert_eq!(status.libraries, ["Playback", "Songs"]);
    assert!(status.playback_library_exists && status.updater);
    assert!(!status.can_import);
    assert!(status
        .import_blocker
        .as_deref()
        .unwrap()
        .contains("Accessibility"));
    assert_eq!(
        status.data_folder.as_deref(),
        Some("/Users/x/Documents/ProPresenter")
    );

    // Outdated: it works and is older than the one bundled here.
    fake.set(|h| h.version = "0.0.1".into());
    link.advance_clock(Duration::from_secs(60));
    let status = link.status(true).await;
    assert_eq!(status.state, LinkState::Outdated);
    assert_eq!(status.message, "The Presenter Link (version 0.0.1) can be updated. It still works; run the setup command to update it.");
    assert_eq!(status.libraries, ["Playback", "Songs"], "it is still used");
    // A newer one is fine.
    fake.set(|h| h.version = "9.9.9".into());
    link.advance_clock(Duration::from_secs(60));
    assert_eq!(link.status(true).await.state, LinkState::Ready);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_look_in_flight_is_checking_until_it_has_an_answer() {
    let fake = Fake::new(BUNDLED_HELPER_VERSION);
    let (_settings, link) = rig(&fake).await;
    fake.set(|h| h.delay = Duration::from_millis(600));
    let slow = {
        let link = link.clone();
        tokio::spawn(async move { link.status(false).await })
    };
    tokio::time::sleep(Duration::from_millis(150)).await;
    let meanwhile = link.status(false).await;
    assert_eq!(meanwhile.state, LinkState::Checking);
    assert_eq!(meanwhile.message, "Checking the Presenter Link…");
    assert_eq!(slow.await.unwrap().state, LinkState::Ready);
    assert_eq!(fake.gets(), 1, "two windows, one ask");
}

#[tokio::test]
async fn the_helper_is_asked_at_most_every_thirty_seconds_and_the_check_button_after_two() {
    let fake = Fake::new(BUNDLED_HELPER_VERSION);
    let (_settings, link) = rig(&fake).await;
    for _ in 0..5 {
        assert_eq!(link.status(false).await.state, LinkState::Ready);
    }
    assert_eq!(fake.gets(), 1, "five windows, one ask");
    link.advance_clock(Duration::from_secs(1));
    link.status(true).await;
    assert_eq!(fake.gets(), 1, "the Check button waits two seconds");
    link.advance_clock(Duration::from_secs(2));
    link.status(true).await;
    assert_eq!(fake.gets(), 2);
    link.advance_clock(Duration::from_secs(29));
    link.status(false).await;
    assert_eq!(fake.gets(), 2);
    link.advance_clock(Duration::from_secs(2));
    link.status(false).await;
    assert_eq!(fake.gets(), 3, "a window asks again after 30 seconds");
}

#[tokio::test]
async fn a_helper_that_refused_us_is_not_pressed_and_a_silent_one_shows_its_last_answer_for_a_minute(
) {
    let fake = Fake::new(BUNDLED_HELPER_VERSION);
    let (_settings, link) = rig(&fake).await;
    link.set_key_for_test(&"Z".repeat(43));
    fake.set(|h| h.says = Says::Refuses);
    assert_eq!(link.status(true).await.state, LinkState::NeedsCommand);
    link.advance_clock(Duration::from_secs(3));
    link.status(true).await;
    assert_eq!(fake.gets(), 1, "ten seconds, not two, after a refusal");
    link.advance_clock(Duration::from_secs(8));
    link.status(true).await;
    assert_eq!(fake.gets(), 2);

    // Good, then silent: the last good answer is shown, marked old, for 60 seconds.
    fake.set(|h| h.says = Says::Fine);
    link.set_key_for_test(KEY);
    link.advance_clock(Duration::from_secs(60));
    assert_eq!(link.status(true).await.state, LinkState::Ready);
    fake.set(|h| h.says = Says::Down);
    link.advance_clock(Duration::from_secs(31));
    let old = link.status(false).await;
    assert_eq!((old.state, old.stale), (LinkState::Ready, true));
    assert!(
        old.message
            .starts_with("The ProPresenter computer did not answer just now"),
        "{}",
        old.message
    );
    assert_eq!(old.libraries, ["Playback", "Songs"]);
    link.advance_clock(Duration::from_secs(31));
    let gone = link.status(false).await;
    assert_eq!((gone.state, gone.stale), (LinkState::NotAnswering, false));
    assert!(gone.libraries.is_empty());
}

#[tokio::test]
async fn the_connection_to_a_helper_asks_the_host_the_operator_named() {
    let fake = Fake::new(BUNDLED_HELPER_VERSION);
    let (_settings, link) = rig(&fake).await;
    let (mode, host) = link.connect().await.unwrap();
    assert_eq!(mode, LinkMode::Helper);
    let info = host.info().await.unwrap();
    assert_eq!(info.libraries, ["Playback", "Songs"]);
    let port = link.helper_port();
    assert_eq!(fake.0.lock().unwrap().hosts, [format!("127.0.0.1:{port}")]);
}

// ---- the route ----------------------------------------------------------------------------------------------------------------

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value, String) {
    let builder = Request::builder().method(method).uri(path);
    let mut request = match body {
        Some(b) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(b.to_string())),
        None => builder.body(Body::empty()),
    }
    .unwrap();
    request.extensions_mut().insert(ConnectInfo(
        "127.0.0.1:50000".parse::<SocketAddr>().unwrap(),
    ));
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&bytes).to_string();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        text,
    )
}

#[tokio::test]
async fn the_status_route_says_the_shape_and_never_the_key() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let fake = Fake::new("0.0.1");
    let service = SettingsService::ephemeral_persistent(PersistentSettings::default());
    let rt = Runtime::with_backends(Arc::new(service), false, OutputBackends::fake_midi());
    let app = build_app(rt.clone(), AppOptions { web_root: None });
    // No host saved.
    let (code, body, _) = call(&app, "GET", "/api/v1/propresenter-link/status", None).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(body["mode"], Value::Null);
    assert_eq!(body["state"], "not_installed");
    assert_eq!(body["bundled_version"], BUNDLED_HELPER_VERSION);
    // A helper on another computer.
    save_host(&rt.settings_service, "127.0.0.1", true);
    rt.propresenter_link
        .set_own_address_check(Arc::new(|_| false));
    rt.propresenter_link.set_helper_port(fake.serve().await);
    rt.propresenter_link.set_key_for_test(KEY);
    let (_, body, text) = call(
        &app,
        "GET",
        "/api/v1/propresenter-link/status?check=1",
        None,
    )
    .await;
    assert_eq!(body["mode"], "helper");
    assert_eq!(body["state"], "outdated");
    assert_eq!(body["version"], "0.0.1");
    assert_eq!(body["computer_address"], "127.0.0.1");
    assert_eq!(body["libraries"], json!(["Playback", "Songs"]));
    assert_eq!(body["playback_library_exists"], true);
    assert_eq!(body["can_import"], false);
    assert!(body["import_blocker"]
        .as_str()
        .unwrap()
        .contains("Accessibility"));
    assert_eq!(
        body["update"]["mode"], "manual",
        "the bundled files are not signed in a working tree"
    );
    assert!(!text.contains(KEY), "the key is never part of an answer");
    for field in [
        "mode",
        "state",
        "message",
        "bundled_version",
        "can_import",
        "libraries",
        "playback_library_exists",
    ] {
        assert!(body.get(field).is_some(), "{field}");
    }
}

#[tokio::test]
async fn a_save_that_does_not_send_the_link_section_keeps_it() {
    let service = SettingsService::ephemeral_persistent(PersistentSettings::default());
    let rt = Runtime::with_backends(Arc::new(service), false, OutputBackends::fake_midi());
    let app = build_app(rt.clone(), AppOptions { web_root: None });
    let mut next = rt.settings_service.snapshot();
    next.propresenter.link.data_folder = Some("/Volumes/Show/ProPresenter".into());
    rt.settings_service.save(next).unwrap();
    let (code, _, _) = call(
        &app,
        "PUT",
        "/api/v1/settings",
        Some(json!({"timezone": "America/Chicago"})),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        rt.settings_service
            .snapshot()
            .propresenter
            .link
            .data_folder
            .as_deref(),
        Some("/Volumes/Show/ProPresenter")
    );
    let (code, _, _) = call(
        &app,
        "PUT",
        "/api/v1/settings",
        Some(json!({"propresenter": {"host": "10.0.0.9"}})),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        rt.settings_service
            .snapshot()
            .propresenter
            .link
            .data_folder
            .as_deref(),
        Some("/Volumes/Show/ProPresenter"),
        "a window that sends the section without `link` does not clear it"
    );
    let (code, _, _) = call(
        &app,
        "PUT",
        "/api/v1/settings",
        Some(json!({"propresenter": {"link": {"data_folder": "/Volumes/Other/PP"}}})),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        rt.settings_service
            .snapshot()
            .propresenter
            .link
            .data_folder
            .as_deref(),
        Some("/Volumes/Other/PP")
    );
    let (code, _, _) = call(
        &app,
        "PUT",
        "/api/v1/settings",
        Some(json!({"propresenter": {"link": {"data_folder": "relative"}}})),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
}

// ---- the automatic update ---------------------------------------------------------------------------------------------------

struct Env {
    auto: AtomicBool,
    playing: AtomicBool,
    timer: AtomicBool,
}

#[async_trait]
impl UpdateEnvironment for Env {
    fn auto_update(&self) -> bool {
        self.auto.load(Ordering::SeqCst)
    }
    fn playback_settings(&self) -> PlaybackLibrarySettings {
        PlaybackLibrarySettings::default()
    }
    fn song_playing(&self) -> bool {
        self.playing.load(Ordering::SeqCst)
    }
    fn timer_running(&self) -> bool {
        self.timer.load(Ordering::SeqCst)
    }
    async fn lightkey(&self) -> Option<LightkeyStatus> {
        None
    }
}

fn updater(link: Arc<ProPresenterLink>, bundle: UpdateBundle) -> (HelperUpdater, Arc<Env>) {
    let env = Arc::new(Env {
        auto: AtomicBool::new(true),
        playing: AtomicBool::new(false),
        timer: AtomicBool::new(false),
    });
    let config = UpdaterConfig {
        poll: Duration::from_millis(20),
        wait_for_new_version: Duration::from_secs(3),
        ..UpdaterConfig::default()
    };
    let empty = bundle_for("lib", false);
    let updater = HelperUpdater::with_parts(
        env.clone(),
        Arc::new(PlaybackLibrary::new()),
        empty.clone(),
        empty,
        &public(),
        config,
    )
    .with_propresenter_parts(link, bundle, &public());
    (updater, env)
}

fn bundle_for(version: &str, signed: bool) -> UpdateBundle {
    bundle(version, signed)
}

#[tokio::test(flavor = "multi_thread")]
async fn an_older_helper_is_updated_with_a_signed_body_when_everything_is_quiet() {
    let fake = Fake::new("0.0.9");
    let (_settings, link) = rig(&fake).await;
    let (updater, env) = updater(link.clone(), bundle("0.2.0", true));
    // Not quiet: nothing is sent.
    env.playing.store(true, Ordering::SeqCst);
    assert_eq!(
        updater.evaluate_propresenter().await,
        Outcome::NotQuiet("a song is playing")
    );
    env.playing.store(false, Ordering::SeqCst);
    env.timer.store(true, Ordering::SeqCst);
    assert_eq!(
        updater.evaluate_propresenter().await,
        Outcome::NotQuiet("the song timer is running")
    );
    env.timer.store(false, Ordering::SeqCst);
    assert_eq!(fake.0.lock().unwrap().posts, 0);
    // Switched off: nothing is sent.
    env.auto.store(false, Ordering::SeqCst);
    assert_eq!(updater.evaluate_propresenter().await, Outcome::Off);
    env.auto.store(true, Ordering::SeqCst);
    // Quiet: it is sent, the helper accepts and says it runs the new version.
    let status = link.status(true).await;
    assert_eq!(status.state, LinkState::Outdated);
    assert_eq!(
        updater.propresenter_view(&status).unwrap().mode,
        HelperUpdateMode::Automatic
    );
    assert_eq!(
        updater.evaluate_propresenter().await,
        Outcome::Updated("0.2.0".into())
    );
    {
        let helper = fake.0.lock().unwrap();
        assert_eq!(
            (helper.posts, helper.post_signed.clone()),
            (1, vec![true]),
            "the body hash is part of the signature"
        );
        assert_eq!(helper.version, "0.2.0");
    }
    link.advance_clock(Duration::from_secs(60));
    let now = link.status(true).await;
    assert_eq!(now.state, LinkState::Ready);
    assert_eq!(
        updater.propresenter_view(&now).unwrap().mode,
        HelperUpdateMode::Updated
    );
    assert_eq!(updater.evaluate_propresenter().await, Outcome::Current);
    assert_eq!(fake.0.lock().unwrap().posts, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_is_sent_to_a_helper_that_cannot_take_it_or_with_a_bundle_nobody_signed() {
    let fake = Fake::new("0.1.0");
    let (_settings, link) = rig(&fake).await;
    link.set_key_for_test(KEY);
    // A helper that cannot update itself.
    fake.set(|h| h.updater = None);
    let (updater_a, _) = updater(link.clone(), bundle("0.2.0", true));
    assert_eq!(updater_a.evaluate_propresenter().await, Outcome::NotCapable);
    fake.set(|h| h.updater = Some(1));
    // A bundle with no valid signature never leaves.
    let (updater_b, _) = updater(link.clone(), bundle("0.2.0", false));
    assert_eq!(updater_b.evaluate_propresenter().await, Outcome::Unsigned);
    let status = link.status(true).await;
    assert_eq!(
        updater_b.propresenter_view(&status).unwrap().mode,
        HelperUpdateMode::Manual
    );
    // A helper that does not know our key: there is nothing this StagePilot may do to it.
    fake.set(|h| h.says = Says::Refuses);
    let (updater_c, _) = updater(link.clone(), bundle("0.2.0", true));
    assert_eq!(
        updater_c.evaluate_propresenter().await,
        Outcome::Unreachable
    );
    assert_eq!(fake.0.lock().unwrap().posts, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_address_that_is_not_on_this_network_and_a_local_connection_are_never_sent_anything() {
    let fake = Fake::new("0.1.0");
    let (settings, link) = rig(&fake).await;
    link.set_key_for_test(KEY);
    let (updater, _) = updater(link.clone(), bundle("0.2.0", true));
    save_host(&settings, "203.0.113.9", true);
    assert!(matches!(
        updater.evaluate_propresenter().await,
        Outcome::Refused(_)
    ));
    // This computer: no helper, nothing to update.
    link.set_own_address_check(Arc::new(|_| true));
    save_host(&settings, "127.0.0.1", true);
    assert_eq!(updater.evaluate_propresenter().await, Outcome::Unreachable);
    // No host saved at all.
    save_host(&settings, "127.0.0.1", false);
    assert_eq!(updater.evaluate_propresenter().await, Outcome::Unreachable);
    let helper = fake.0.lock().unwrap();
    assert_eq!(
        (helper.gets, helper.posts),
        (0, 0),
        "not even the question was asked"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn three_failed_attempts_give_up_and_say_to_run_the_setup_command() {
    let fake = Fake::new("0.1.0");
    let (_settings, link) = rig(&fake).await;
    link.set_key_for_test(KEY);
    let config = UpdaterConfig {
        poll: Duration::from_millis(10),
        wait_for_new_version: Duration::from_millis(80),
        backoff: vec![Duration::ZERO, Duration::ZERO],
        ..UpdaterConfig::default()
    };
    let env = Arc::new(Env {
        auto: AtomicBool::new(true),
        playing: AtomicBool::new(false),
        timer: AtomicBool::new(false),
    });
    let empty = bundle_for("lib", false);
    let updater = HelperUpdater::with_parts(
        env,
        Arc::new(PlaybackLibrary::new()),
        empty.clone(),
        empty,
        &public(),
        config,
    )
    .with_propresenter_parts(link.clone(), bundle("0.2.0", true), &public());
    // A helper that takes the update and never comes back with the new version: the version it runs does not change.
    // (The stand-in sets it at once, so make it answer 500 to the update instead.)
    fake.set(|h| h.updater = Some(1));
    // An older helper whose update is refused: the signature the stand-in trusts is another one.
    let other = {
        let mut b = bundle("0.2.0", true);
        b.signature = leak(
            STANDARD
                .encode(
                    Ed25519KeyPair::from_seed_unchecked(&[9; 32])
                        .unwrap()
                        .sign(b.sums)
                        .as_ref(),
                )
                .into_bytes(),
        );
        b
    };
    let updater = updater.with_propresenter_parts(
        link.clone(),
        other,
        &Ed25519KeyPair::from_seed_unchecked(&[9; 32])
            .unwrap()
            .public_key()
            .as_ref()
            .try_into()
            .unwrap(),
    );
    for attempt in 1..=3 {
        let outcome = updater.evaluate_propresenter().await;
        assert!(
            matches!(outcome, Outcome::Failed(_)),
            "attempt {attempt}: {outcome:?}"
        );
    }
    assert_eq!(updater.evaluate_propresenter().await, Outcome::GaveUp);
    assert_eq!(fake.0.lock().unwrap().posts, 3);
    let status = link.status(true).await;
    let view = updater.propresenter_view(&status).unwrap();
    assert_eq!(view.mode, HelperUpdateMode::Manual);
    assert_eq!(
        view.message.as_deref(),
        Some("The Presenter Link could not update itself. Run the setup command on the ProPresenter computer to update it.")
    );
}
