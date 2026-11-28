//! The helpers update themselves when StagePilot starts: against fake helpers (axum servers on a loopback port) that check what the
//! real ones check, with a throwaway signing key made here (the project's private key is never in a test). StagePilot must send an
//! update exactly when every condition holds and not otherwise, never to a computer that is not on the local network, and a bundle
//! that is not signed by the key it trusts must never leave. No real helper, no rig.

use async_trait::async_trait;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use stagepilot_core::config::PlaybackLibrarySettings;
use stagepilot_core::helper_update::{
    public_key_bytes, HelperUpdateMode, UpdateBundle, HELPER_SIGNING_PUBLIC_KEY,
};
use stagepilot_lights::lightkey::LightkeyStatus;
use stagepilot_playback::library::HelperState;
use stagepilot_server::helper_update::{
    lan_target, signature_valid, HelperUpdater, Outcome, UpdateEnvironment, UpdaterConfig,
};
use stagepilot_server::playback_library::{sign, sign_post, PlaybackLibrary};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const KEY: &str = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-_AbCdE";
const TEST_SEED: [u8; 32] = [7; 32];
const OTHER_SEED: [u8; 32] = [9; 32];

fn keypair(seed: [u8; 32]) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&seed).unwrap()
}

fn public_of(seed: [u8; 32]) -> [u8; 32] {
    <[u8; 32]>::try_from(keypair(seed).public_key().as_ref()).unwrap()
}

fn leak(bytes: Vec<u8>) -> &'static [u8] {
    Box::leak(bytes.into_boxed_slice())
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn source(version: &str) -> Vec<u8> {
    format!("\"\"\"stand-in\"\"\"\n__version__ = \"{version}\"\n").into_bytes()
}

/// A bundle of these files, whose `SHA256SUMS` also lists `install.sh` (a file that is listed and not sent), signed by `seed`
/// (`None`: not signed at all).
fn bundle(version: &str, names: &[&'static str], seed: Option<[u8; 32]>) -> UpdateBundle {
    let files: Vec<(&'static str, &'static [u8])> = names
        .iter()
        .map(|name| (*name, leak(source(version))))
        .collect();
    let mut lines: Vec<String> = files
        .iter()
        .map(|(name, bytes)| format!("{}  {name}\n", sha256_hex(bytes)))
        .collect();
    lines.push(format!("{}  install.sh\n", sha256_hex(b"#!/bin/sh\n")));
    lines.sort();
    let sums = leak(lines.concat().into_bytes());
    let signature = seed.map_or(vec![], |seed| {
        STANDARD
            .encode(keypair(seed).sign(sums).as_ref())
            .into_bytes()
    });
    UpdateBundle {
        version: version.to_string(),
        files,
        sums,
        signature: leak(signature),
    }
}

fn library_bundle(version: &str, seed: Option<[u8; 32]>) -> UpdateBundle {
    bundle(version, &["playback_library.py"], seed)
}

fn lightkey_bundle(version: &str, seed: Option<[u8; 32]>) -> UpdateBundle {
    bundle(version, &["lightkey_api.py", "lightkey_cues.py"], seed)
}

fn version_in(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix("__version__ = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .map(str::to_string)
}

fn newer(candidate: &str, running: &str) -> bool {
    stagepilot_core::helper_update::parse_version(candidate)
        > stagepilot_core::helper_update::parse_version(running)
}

/// What a real helper checks in `POST /update` after authentication: the signature, the checksums, the names, the version.
fn check_update(
    body: &[u8],
    trusted: &[u8; 32],
    names: &[&str],
    running: &str,
) -> Result<String, String> {
    let request: Value = serde_json::from_slice(body).map_err(|_| "not JSON".to_string())?;
    let files = request["files"].as_object().ok_or("no files")?;
    let sums = request["sums"].as_str().ok_or("no sums")?;
    let signature = STANDARD
        .decode(request["sig"].as_str().ok_or("no sig")?)
        .map_err(|_| "bad base64")?;
    UnparsedPublicKey::new(&ED25519, trusted)
        .verify(sums.as_bytes(), &signature)
        .map_err(|_| "The update is not signed by the StagePilot project.".to_string())?;
    let mut version = None;
    for (name, encoded) in files {
        let bytes = STANDARD
            .decode(encoded.as_str().ok_or("bad file")?)
            .map_err(|_| "bad base64")?;
        let listed = sums
            .lines()
            .find_map(|line| line.strip_suffix(&format!("  {name}")))
            .ok_or("A file in the update is not in the signed list.")?;
        if listed != sha256_hex(&bytes) {
            return Err("A file in the update does not match the signed checksums.".into());
        }
        if name.ends_with("api.py") || name == "playback_library.py" {
            version = version_in(&String::from_utf8_lossy(&bytes));
        }
    }
    let mut got: Vec<&str> = files.keys().map(String::as_str).collect();
    got.sort_unstable();
    let mut want = names.to_vec();
    want.sort_unstable();
    if got != want {
        return Err("The update does not hold exactly the files this helper takes.".into());
    }
    let version = version.ok_or("no version")?;
    if !newer(&version, running) {
        return Err("The update is not newer than the version that runs.".into());
    }
    Ok(version)
}

/// What a fake helper does with an update it accepted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Then {
    /// Restart on the new version after this long.
    Apply(Duration),
    /// Answer 500 and change nothing.
    Error500,
    /// Say it is updating and never come back with the new version.
    NeverComesBack,
    /// Come back on the old version and say why.
    RollsBack,
}

struct Helper {
    version: String,
    updater: Option<u32>,
    trusted: [u8; 32],
    then: Then,
    posts: u32,
    gets: u32,
    applied_at: Option<(Instant, String)>,
    last_update_error: Option<String>,
    recording: bool,
    /// Every `Host` header the helper saw.
    hosts: Vec<String>,
    /// The update request's HMAC was valid (Playback helper only).
    signed_ok: Vec<bool>,
}

#[derive(Clone)]
struct Fake {
    inner: Arc<Mutex<Helper>>,
    /// The Playback helper checks the key; the Lightkey helper has none.
    keyed: bool,
    names: &'static [&'static str],
}

impl Fake {
    fn library(version: &str, then: Then) -> Self {
        Self::make(version, then, true, &["playback_library.py"])
    }

    fn lightkey(version: &str, then: Then) -> Self {
        Self::make(
            version,
            then,
            false,
            &["lightkey_api.py", "lightkey_cues.py"],
        )
    }

    fn make(version: &str, then: Then, keyed: bool, names: &'static [&'static str]) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Helper {
                version: version.to_string(),
                updater: Some(1),
                trusted: public_of(TEST_SEED),
                then,
                posts: 0,
                gets: 0,
                applied_at: None,
                last_update_error: None,
                recording: false,
                hosts: vec![],
                signed_ok: vec![],
            })),
            keyed,
            names,
        }
    }

    fn set(&self, f: impl FnOnce(&mut Helper)) {
        f(&mut self.inner.lock().unwrap());
    }

    fn posts(&self) -> u32 {
        self.inner.lock().unwrap().posts
    }

    fn gets(&self) -> u32 {
        self.inner.lock().unwrap().gets
    }

    fn running(&self) -> String {
        let mut h = self.inner.lock().unwrap();
        if let Some((at, version)) = h.applied_at.clone() {
            if Instant::now() >= at {
                h.version = version;
                h.applied_at = None;
            }
        }
        h.version.clone()
    }

    async fn serve_on(&self, port: u16) -> String {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = Router::new().fallback(answer).with_state(self.clone());
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("127.0.0.1:{port}")
    }

    async fn serve(&self) -> String {
        self.serve_on(0).await
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
    let running = fake.running();
    let mut h = fake.inner.lock().unwrap();
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
    // The Playback helper: a request is good when its HMAC is (the body's hash included for a POST).
    let signed = match (text("x-pbl-time"), text("x-pbl-sig")) {
        (Some(time), Some(sig)) => time.parse::<u64>().is_ok_and(|time| {
            let want = if method == Method::POST {
                sign_post(KEY, &target, time, &body)
            } else {
                sign(KEY, &target, time)
            };
            want == sig
        }),
        _ => false,
    };
    if method == Method::GET && uri.path() == "/health" {
        h.gets += 1;
        if fake.keyed && !signed {
            return json_response(
                200,
                json!({"ok": true, "name": "playback-library", "version": running, "auth": "required"}),
            );
        }
        let mut health =
            json!({"ok": true, "version": running, "database": "ok", "songs": 1, "setlists": 1});
        if fake.keyed {
            health["auth"] = json!("key");
        }
        if let Some(level) = h.updater {
            health["updater"] = json!(level);
        }
        if let Some(error) = &h.last_update_error {
            health["last_update_error"] = json!(error);
        }
        return json_response(200, health);
    }
    if method == Method::GET && uri.path() == "/record" {
        return json_response(200, json!({"recording": h.recording}));
    }
    if method == Method::POST && uri.path() == "/update" {
        h.posts += 1;
        h.signed_ok.push(signed);
        if fake.keyed && !signed {
            return json_response(401, json!({"error": "key required"}));
        }
        let version = match check_update(&body, &h.trusted, fake.names, &running) {
            Ok(version) => version,
            Err(error) => return json_response(422, json!({"error": error})),
        };
        return match h.then {
            Then::Error500 => {
                json_response(500, json!({"error": "the update could not be written"}))
            }
            Then::Apply(after) => {
                h.applied_at = Some((Instant::now() + after, version.clone()));
                json_response(
                    200,
                    json!({"updating": true, "from": running, "to": version}),
                )
            }
            Then::NeverComesBack => json_response(
                200,
                json!({"updating": true, "from": running, "to": version}),
            ),
            Then::RollsBack => {
                h.last_update_error = Some(format!(
                    "Version {version} would not start, so version {running} was put back."
                ));
                json_response(
                    200,
                    json!({"updating": true, "from": running, "to": version}),
                )
            }
        };
    }
    json_response(404, json!({"error": "not found"}))
}

/// What StagePilot knows about the world, set by hand.
struct Env {
    auto: AtomicBool,
    playing: AtomicBool,
    timer: AtomicBool,
    address: Mutex<String>,
    lightkey: Mutex<Option<LightkeyStatus>>,
}

impl Env {
    fn new(address: &str) -> Arc<Self> {
        Arc::new(Self {
            auto: AtomicBool::new(true),
            playing: AtomicBool::new(false),
            timer: AtomicBool::new(false),
            address: Mutex::new(address.to_string()),
            lightkey: Mutex::new(None),
        })
    }
}

#[async_trait]
impl UpdateEnvironment for Env {
    fn auto_update(&self) -> bool {
        self.auto.load(Ordering::SeqCst)
    }

    fn playback_settings(&self) -> PlaybackLibrarySettings {
        PlaybackLibrarySettings {
            enabled: None,
            address: self.address.lock().unwrap().clone(),
        }
    }

    fn song_playing(&self) -> bool {
        self.playing.load(Ordering::SeqCst)
    }

    fn timer_running(&self) -> bool {
        self.timer.load(Ordering::SeqCst)
    }

    async fn lightkey(&self) -> Option<LightkeyStatus> {
        self.lightkey.lock().unwrap().clone()
    }
}

/// The Lightkey strip's status for a helper at `api_url`, as the link would report it.
fn lightkey_status(api_url: &str, version: &str, updater: bool) -> LightkeyStatus {
    serde_json::from_value(json!({
        "api_url": api_url, "link": "connected", "detail": null, "active_count": null,
        "recording": false, "recording_song": null, "replay": true, "mode": "auto",
        "replay_blocked": null, "replaying_song": null, "message": null,
        "helper": {"version": version, "outdated": false, "updater": updater},
    }))
    .unwrap()
}

fn quick() -> UpdaterConfig {
    UpdaterConfig {
        start_delay: Duration::ZERO,
        tick: Duration::from_millis(5),
        retry: Duration::from_millis(30),
        window: Duration::from_millis(400),
        wait_for_new_version: Duration::from_millis(600),
        poll: Duration::from_millis(10),
        backoff: vec![Duration::from_millis(40), Duration::from_millis(80)],
        max_attempts: 3,
        updated_shown: Duration::from_secs(5),
    }
}

fn library() -> Arc<PlaybackLibrary> {
    let library = Arc::new(PlaybackLibrary::new());
    library.set_key_for_test(KEY);
    library
}

struct Rig {
    updater: HelperUpdater,
    env: Arc<Env>,
    library_helper: Fake,
    lightkey_helper: Fake,
    library_address: String,
    lightkey_address: String,
}

/// StagePilot with a Library Link at 0.3.0 and a Lightkey helper at 0.5.0, both able to update themselves, bundles 0.4.0 and 0.6.0
/// signed by the test key.
async fn standard_rig() -> Rig {
    rig_with(
        library_bundle("0.4.0", Some(TEST_SEED)),
        lightkey_bundle("0.6.0", Some(TEST_SEED)),
        Then::Apply(Duration::from_millis(30)),
    )
    .await
}

async fn rig_with(library_b: UpdateBundle, lightkey_b: UpdateBundle, then: Then) -> Rig {
    let library_helper = Fake::library("0.3.0", then);
    let lightkey_helper = Fake::lightkey("0.5.0", then);
    let library_address = library_helper.serve().await;
    let lightkey_address = lightkey_helper.serve().await;
    let env = Env::new(&library_address);
    *env.lightkey.lock().unwrap() = Some(lightkey_status(
        &format!("http://{lightkey_address}"),
        "0.5.0",
        true,
    ));
    let updater = HelperUpdater::with_parts(
        env.clone(),
        library(),
        library_b,
        lightkey_b,
        &public_of(TEST_SEED),
        quick(),
    );
    Rig {
        updater,
        env,
        library_helper,
        lightkey_helper,
        library_address,
        lightkey_address,
    }
}

// ---------------------------------------------------------------- the Library Link

#[tokio::test]
async fn an_older_library_link_that_can_update_itself_gets_the_signed_bundle_once() {
    let rig = standard_rig().await;
    let outcome = rig.updater.evaluate_playback().await;
    assert_eq!(outcome, Outcome::Updated("0.4.0".into()));
    assert_eq!(rig.library_helper.posts(), 1);
    assert_eq!(rig.library_helper.running(), "0.4.0");
    assert_eq!(rig.library_helper.inner.lock().unwrap().signed_ok, [true]);
    // A second look finds nothing to do: it is current, and nothing more is sent.
    assert_eq!(rig.updater.evaluate_playback().await, Outcome::Current);
    assert_eq!(rig.library_helper.posts(), 1);
}

#[tokio::test]
async fn the_window_says_updating_then_updated_once_and_never_for_a_current_helper() {
    let rig = standard_rig().await;
    rig.library_helper
        .set(|h| h.then = Then::Apply(Duration::from_millis(250)));
    let updater = Arc::new(rig.updater);
    let working = {
        let updater = updater.clone();
        tokio::spawn(async move { updater.evaluate_playback().await })
    };
    let helper_status = |version: &str| stagepilot_playback::library::HelperStatus {
        state: HelperState::Ready,
        version: Some(version.to_string()),
        bundled_version: "0.4.0".into(),
        message: String::new(),
        paired: true,
        updater: true,
        last_update_error: None,
        playback_remote: None,
        update: None,
    };
    // Before anything: an older helper that will be updated by itself.
    let before = updater.playback_view(&helper_status("0.3.0")).unwrap();
    assert_eq!(before.mode, HelperUpdateMode::Automatic);
    let mut saw_updating = false;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        if let Some(view) = updater.playback_view(&helper_status("0.3.0")) {
            if view.mode == HelperUpdateMode::Updating {
                assert_eq!(
                    view.message.as_deref(),
                    Some("Updating the Library Link to 0.4.0…")
                );
                saw_updating = true;
                break;
            }
        }
    }
    assert!(
        saw_updating,
        "the status line says it is updating while it does"
    );
    assert_eq!(working.await.unwrap(), Outcome::Updated("0.4.0".into()));
    let done = updater.playback_view(&helper_status("0.4.0")).unwrap();
    assert_eq!(done.mode, HelperUpdateMode::Updated);
    assert_eq!(
        done.message.as_deref(),
        Some("Updated to 0.4.0 automatically.")
    );
    // A helper that was never behind has nothing to say.
    let other = HelperUpdater::with_parts(
        Env::new("127.0.0.1:9"),
        library(),
        library_bundle("0.4.0", Some(TEST_SEED)),
        lightkey_bundle("0.6.0", Some(TEST_SEED)),
        &public_of(TEST_SEED),
        quick(),
    );
    assert!(other.playback_view(&helper_status("0.4.0")).is_none());
    assert!(other.playback_view(&helper_status("0.5.0")).is_none());
}

#[tokio::test]
async fn a_helper_that_cannot_update_itself_is_left_alone_and_keeps_the_command_box() {
    let rig = standard_rig().await;
    rig.library_helper.set(|h| h.updater = None);
    assert_eq!(rig.updater.evaluate_playback().await, Outcome::NotCapable);
    assert_eq!(rig.library_helper.posts(), 0);
    let helper = stagepilot_playback::library::HelperStatus {
        state: HelperState::Outdated,
        version: Some("0.3.0".into()),
        bundled_version: "0.4.0".into(),
        message: String::new(),
        paired: true,
        updater: false,
        last_update_error: None,
        playback_remote: None,
        update: None,
    };
    assert_eq!(
        rig.updater.playback_view(&helper).unwrap().mode,
        HelperUpdateMode::Manual
    );
}

#[tokio::test]
async fn a_helper_that_is_not_older_is_left_alone() {
    for running in ["0.4.0", "0.4.1", "0.10.0", "1.0.0"] {
        let rig = standard_rig().await;
        rig.library_helper.set(|h| h.version = running.to_string());
        assert_eq!(
            rig.updater.evaluate_playback().await,
            Outcome::Current,
            "{running}"
        );
        assert_eq!(rig.library_helper.posts(), 0, "{running}");
    }
}

#[tokio::test]
async fn a_bundle_that_is_not_signed_by_the_trusted_key_never_leaves() {
    let wrong_signer = library_bundle("0.4.0", Some(OTHER_SEED));
    let unsigned = library_bundle("0.4.0", None);
    let mut tampered = library_bundle("0.4.0", Some(TEST_SEED));
    tampered.sums = leak([tampered.sums, b"0000  extra\n".as_slice()].concat());
    let mut garbage = library_bundle("0.4.0", Some(TEST_SEED));
    garbage.signature = b"not base64 at all!";
    let mut short = library_bundle("0.4.0", Some(TEST_SEED));
    short.signature = leak(STANDARD.encode([1u8; 63]).into_bytes());
    for (what, bad) in [
        ("signed by another key", wrong_signer),
        ("not signed", unsigned),
        ("sums changed after signing", tampered),
        ("signature is not base64", garbage),
        ("signature is 63 bytes", short),
    ] {
        let rig = rig_with(
            bad,
            lightkey_bundle("0.6.0", Some(TEST_SEED)),
            Then::Apply(Duration::from_millis(10)),
        )
        .await;
        assert_eq!(
            rig.updater.evaluate_playback().await,
            Outcome::Unsigned,
            "{what}"
        );
        assert_eq!(rig.library_helper.posts(), 0, "{what}");
        assert_eq!(rig.library_helper.gets(), 0, "{what}: not even asked");
        let helper = stagepilot_playback::library::HelperStatus {
            state: HelperState::Ready,
            version: Some("0.3.0".into()),
            bundled_version: "0.4.0".into(),
            message: String::new(),
            paired: true,
            updater: true,
            last_update_error: None,
            playback_remote: None,
            update: None,
        };
        assert_eq!(
            rig.updater.playback_view(&helper).unwrap().mode,
            HelperUpdateMode::Manual,
            "{what}: the command box stays"
        );
    }
}

#[tokio::test]
async fn nothing_is_sent_while_a_song_plays_or_the_timer_runs_and_it_is_sent_when_quiet() {
    let rig = standard_rig().await;
    rig.env.playing.store(true, Ordering::SeqCst);
    assert_eq!(
        rig.updater.evaluate_playback().await,
        Outcome::NotQuiet("a song is playing")
    );
    rig.env.playing.store(false, Ordering::SeqCst);
    rig.env.timer.store(true, Ordering::SeqCst);
    assert_eq!(
        rig.updater.evaluate_playback().await,
        Outcome::NotQuiet("the song timer is running")
    );
    assert_eq!(rig.library_helper.posts(), 0);
    rig.env.timer.store(false, Ordering::SeqCst);
    assert_eq!(
        rig.updater.evaluate_playback().await,
        Outcome::Updated("0.4.0".into())
    );
    assert_eq!(rig.library_helper.posts(), 1);
}

#[tokio::test]
async fn with_the_switch_off_in_the_settings_file_nothing_is_asked_or_sent() {
    let rig = standard_rig().await;
    rig.env.auto.store(false, Ordering::SeqCst);
    assert_eq!(rig.updater.evaluate_playback().await, Outcome::Off);
    assert_eq!(rig.updater.evaluate_lightkey().await, Outcome::Off);
    assert_eq!(rig.library_helper.gets() + rig.library_helper.posts(), 0);
    assert_eq!(rig.lightkey_helper.gets() + rig.lightkey_helper.posts(), 0);
    let helper = stagepilot_playback::library::HelperStatus {
        state: HelperState::Ready,
        version: Some("0.3.0".into()),
        bundled_version: "0.4.0".into(),
        message: String::new(),
        paired: true,
        updater: true,
        last_update_error: None,
        playback_remote: None,
        update: None,
    };
    assert_eq!(
        rig.updater.playback_view(&helper).unwrap().mode,
        HelperUpdateMode::Manual
    );
}

#[tokio::test]
async fn three_failed_attempts_per_version_then_the_command_box_and_one_plain_sentence() {
    let rig = standard_rig().await;
    rig.library_helper.set(|h| h.then = Then::Error500);
    let helper = stagepilot_playback::library::HelperStatus {
        state: HelperState::Ready,
        version: Some("0.3.0".into()),
        bundled_version: "0.4.0".into(),
        message: String::new(),
        paired: true,
        updater: true,
        last_update_error: None,
        playback_remote: None,
        update: None,
    };
    for attempt in 1..=3 {
        // Each failure is followed by a wait; look again once it is over.
        loop {
            match rig.updater.evaluate_playback().await {
                Outcome::Waiting => tokio::time::sleep(Duration::from_millis(10)).await,
                Outcome::Failed(why) => {
                    assert_eq!(why, "the update could not be written");
                    break;
                }
                other => panic!("attempt {attempt}: {other:?}"),
            }
        }
        assert_eq!(rig.library_helper.posts(), attempt);
    }
    assert_eq!(rig.updater.evaluate_playback().await, Outcome::GaveUp);
    assert_eq!(
        rig.library_helper.posts(),
        3,
        "no fourth attempt in this run"
    );
    let view = rig.updater.playback_view(&helper).unwrap();
    assert_eq!(view.mode, HelperUpdateMode::Manual);
    assert_eq!(
        view.message.as_deref(),
        Some("The Library Link could not update itself. Run the setup command on the Playback computer to update it.")
    );
}

#[tokio::test]
async fn a_helper_that_never_reports_the_new_version_or_rolls_back_is_a_failed_attempt() {
    for (then, why) in [
        (Then::NeverComesBack, "did not come back"),
        (Then::RollsBack, "put back"),
    ] {
        let rig = rig_with(
            library_bundle("0.4.0", Some(TEST_SEED)),
            lightkey_bundle("0.6.0", Some(TEST_SEED)),
            then,
        )
        .await;
        let Outcome::Failed(message) = rig.updater.evaluate_playback().await else {
            panic!("{then:?}");
        };
        assert!(message.contains(why), "{then:?}: {message}");
        assert_eq!(rig.library_helper.posts(), 1);
    }
}

#[tokio::test]
async fn a_failure_then_a_success_counts_and_ends_well() {
    let rig = standard_rig().await;
    rig.library_helper.set(|h| h.then = Then::Error500);
    assert!(matches!(
        rig.updater.evaluate_playback().await,
        Outcome::Failed(_)
    ));
    rig.library_helper
        .set(|h| h.then = Then::Apply(Duration::from_millis(20)));
    // The wait after a failure is respected.
    assert_eq!(rig.updater.evaluate_playback().await, Outcome::Waiting);
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert_eq!(
        rig.updater.evaluate_playback().await,
        Outcome::Updated("0.4.0".into())
    );
    assert_eq!(rig.library_helper.posts(), 2);
}

#[tokio::test]
async fn a_helper_that_does_not_know_our_key_is_not_touched() {
    let rig = standard_rig().await;
    // Another key than the helper's: it answers the bare minimum.
    let other = HelperUpdater::with_parts(
        rig.env.clone(),
        {
            let library = Arc::new(PlaybackLibrary::new());
            library.set_key_for_test("ZyXwVuTsRqPoNmLkJiHgFeDcBa9876543210_-ZyXwV");
            library
        },
        library_bundle("0.4.0", Some(TEST_SEED)),
        lightkey_bundle("0.6.0", Some(TEST_SEED)),
        &public_of(TEST_SEED),
        quick(),
    );
    assert_eq!(other.evaluate_playback().await, Outcome::Unreachable);
    assert_eq!(rig.library_helper.posts(), 0);
}

// ---------------------------------------------------------------- the Lightkey helper

#[tokio::test]
async fn an_older_lightkey_helper_gets_the_signed_bundle_and_the_window_says_so_once() {
    let rig = standard_rig().await;
    assert_eq!(
        rig.updater.evaluate_lightkey().await,
        Outcome::Updated("0.6.0".into())
    );
    assert_eq!(rig.lightkey_helper.posts(), 1);
    assert_eq!(rig.lightkey_helper.running(), "0.6.0");
    // The window learns the new version from the helper's next health answer; until then and after it says so once.
    let mut status = lightkey_status(&format!("http://{}", rig.lightkey_address), "0.6.0", true);
    rig.updater.annotate_lightkey(&mut status);
    let update = status.helper.unwrap().update.unwrap();
    assert_eq!(update.mode, HelperUpdateMode::Updated);
    assert_eq!(
        update.message.as_deref(),
        Some("Updated to 0.6.0 automatically.")
    );
    assert_eq!(rig.updater.evaluate_lightkey().await, Outcome::Current);
    assert_eq!(rig.lightkey_helper.posts(), 1);
}

#[tokio::test]
async fn the_lightkey_helper_is_left_alone_when_it_cannot_update_is_not_older_or_is_busy() {
    // No updater (0.5.0, installed by hand): the command box stays.
    let rig = standard_rig().await;
    *rig.env.lightkey.lock().unwrap() = Some(lightkey_status(
        &format!("http://{}", rig.lightkey_address),
        "0.5.0",
        false,
    ));
    assert_eq!(rig.updater.evaluate_lightkey().await, Outcome::NotCapable);
    let mut status = lightkey_status(&format!("http://{}", rig.lightkey_address), "0.5.0", false);
    rig.updater.annotate_lightkey(&mut status);
    assert_eq!(
        status.helper.unwrap().update.unwrap().mode,
        HelperUpdateMode::Manual
    );
    // Not older.
    *rig.env.lightkey.lock().unwrap() = Some(lightkey_status(
        &format!("http://{}", rig.lightkey_address),
        "0.6.0",
        true,
    ));
    assert_eq!(rig.updater.evaluate_lightkey().await, Outcome::Current);
    // Playing, timer, StagePilot's own recording, the helper's own take.
    *rig.env.lightkey.lock().unwrap() = Some(lightkey_status(
        &format!("http://{}", rig.lightkey_address),
        "0.5.0",
        true,
    ));
    rig.env.playing.store(true, Ordering::SeqCst);
    assert!(matches!(
        rig.updater.evaluate_lightkey().await,
        Outcome::NotQuiet(_)
    ));
    rig.env.playing.store(false, Ordering::SeqCst);
    rig.env.timer.store(true, Ordering::SeqCst);
    assert!(matches!(
        rig.updater.evaluate_lightkey().await,
        Outcome::NotQuiet(_)
    ));
    rig.env.timer.store(false, Ordering::SeqCst);
    let mut recording = lightkey_status(&format!("http://{}", rig.lightkey_address), "0.5.0", true);
    recording.recording = true;
    *rig.env.lightkey.lock().unwrap() = Some(recording);
    assert!(matches!(
        rig.updater.evaluate_lightkey().await,
        Outcome::NotQuiet(_)
    ));
    *rig.env.lightkey.lock().unwrap() = Some(lightkey_status(
        &format!("http://{}", rig.lightkey_address),
        "0.5.0",
        true,
    ));
    rig.lightkey_helper.set(|h| h.recording = true);
    assert_eq!(
        rig.updater.evaluate_lightkey().await,
        Outcome::NotQuiet("a take is being recorded")
    );
    assert_eq!(
        rig.lightkey_helper.posts(),
        0,
        "nothing was sent in any of these"
    );
    rig.lightkey_helper.set(|h| h.recording = false);
    assert_eq!(
        rig.updater.evaluate_lightkey().await,
        Outcome::Updated("0.6.0".into())
    );
}

#[tokio::test]
async fn an_unsigned_lightkey_bundle_never_leaves_and_three_failures_end_in_the_command_box() {
    let rig = rig_with(
        library_bundle("0.4.0", Some(TEST_SEED)),
        lightkey_bundle("0.6.0", Some(OTHER_SEED)),
        Then::Apply(Duration::from_millis(10)),
    )
    .await;
    assert_eq!(rig.updater.evaluate_lightkey().await, Outcome::Unsigned);
    assert_eq!(rig.lightkey_helper.gets() + rig.lightkey_helper.posts(), 0);

    let rig = standard_rig().await;
    rig.lightkey_helper.set(|h| h.then = Then::Error500);
    for _ in 0..3 {
        loop {
            match rig.updater.evaluate_lightkey().await {
                Outcome::Waiting => tokio::time::sleep(Duration::from_millis(10)).await,
                Outcome::Failed(_) => break,
                other => panic!("{other:?}"),
            }
        }
    }
    assert_eq!(rig.updater.evaluate_lightkey().await, Outcome::GaveUp);
    assert_eq!(rig.lightkey_helper.posts(), 3);
    let mut status = lightkey_status(&format!("http://{}", rig.lightkey_address), "0.5.0", true);
    rig.updater.annotate_lightkey(&mut status);
    let update = status.helper.unwrap().update.unwrap();
    assert_eq!(update.mode, HelperUpdateMode::Manual);
    assert_eq!(
        update.message.as_deref(),
        Some("The helper could not update itself. Run the setup command on the Lighting computer to update it.")
    );
}

// ---------------------------------------------------------------- the local network only

#[tokio::test]
async fn only_the_local_network_is_ever_a_target() {
    for ok in [
        "http://127.0.0.1:8780",
        "http://10.30.0.141:8780",
        "http://192.168.1.20:8770",
        "http://172.16.5.5:8780",
        "http://169.254.7.7:8780",
        "http://[::1]:8780",
        "http://[fd12:3456::1]:8780",
        "http://[fe80::1]:8780",
        "http://localhost:8780",
    ] {
        let target = lan_target(ok)
            .await
            .unwrap_or_else(|why| panic!("{ok}: {why}"));
        assert!(target.url.starts_with("http://"), "{ok}");
        assert!(
            !target.url.contains("localhost"),
            "the checked address is what is used: {}",
            target.url
        );
    }
    for refused in [
        "http://8.8.8.8:8780",
        "http://203.0.113.5:8780",
        "http://100.64.0.1:8780",
        "http://[2001:db8::1]:8780",
        "http://172.32.0.1:8780",
        "https://192.168.1.20:8780",
        "http://no-such-computer.invalid:8780",
        "not an address",
        "",
    ] {
        assert!(lan_target(refused).await.is_err(), "{refused}");
    }
}

#[tokio::test]
async fn nothing_leaves_for_a_helper_that_is_not_on_the_local_network() {
    // The Lightkey link says the helper is at a public address: no request of any kind is made, and the fake sees nothing.
    let rig = standard_rig().await;
    *rig.env.lightkey.lock().unwrap() =
        Some(lightkey_status("http://203.0.113.9:8770", "0.5.0", true));
    assert_eq!(
        rig.updater.evaluate_lightkey().await,
        Outcome::Refused("The computer is not on this local network.")
    );
    // And the Playback computer saved at a public address.
    *rig.env.address.lock().unwrap() = "203.0.113.5".into();
    assert_eq!(
        rig.updater.evaluate_playback().await,
        Outcome::Refused("The computer is not on this local network.")
    );
    assert_eq!(rig.library_helper.gets() + rig.library_helper.posts(), 0);
    assert_eq!(rig.lightkey_helper.gets() + rig.lightkey_helper.posts(), 0);
}

#[tokio::test]
async fn the_request_goes_to_the_checked_address_with_the_name_the_operator_gave() {
    let rig = standard_rig().await;
    *rig.env.address.lock().unwrap() = rig.library_address.replace("127.0.0.1", "localhost");
    assert_eq!(
        rig.updater.evaluate_playback().await,
        Outcome::Updated("0.4.0".into())
    );
    let hosts = rig.library_helper.inner.lock().unwrap().hosts.clone();
    assert!(!hosts.is_empty());
    assert!(
        hosts.iter().all(|h| h.starts_with("localhost:")),
        "{hosts:?}"
    );
}

// ---------------------------------------------------------------- when it looks

#[tokio::test]
async fn at_start_a_busy_helper_is_tried_every_so_often_for_the_window_and_then_left_alone() {
    let rig = standard_rig().await;
    rig.env.playing.store(true, Ordering::SeqCst);
    let start = Instant::now();
    rig.updater.open_windows(start);
    // Playing for the whole window: looked at again and again, never sent.
    let mut at = start;
    for _ in 0..30 {
        rig.updater.tick(at).await;
        at += Duration::from_millis(15);
    }
    assert_eq!(rig.library_helper.posts(), 0);
    assert!(
        rig.library_helper.gets() >= 3,
        "it kept looking: {}",
        rig.library_helper.gets()
    );
    let looked = rig.library_helper.gets();
    // The window (400 ms in this test) is over: it stops looking, even when it is quiet now.
    rig.env.playing.store(false, Ordering::SeqCst);
    let after = start + Duration::from_millis(900);
    rig.updater.tick(after).await;
    rig.updater.tick(after + Duration::from_millis(100)).await;
    assert_eq!(rig.library_helper.gets(), looked);
    assert_eq!(rig.library_helper.posts(), 0);
}

#[tokio::test]
async fn a_helper_that_is_busy_then_quiet_inside_the_window_is_updated_on_the_next_look() {
    let rig = standard_rig().await;
    rig.env.playing.store(true, Ordering::SeqCst);
    let start = Instant::now();
    rig.updater.open_windows(start);
    rig.updater.tick(start).await;
    assert_eq!(rig.library_helper.posts() + rig.lightkey_helper.posts(), 0);
    rig.env.playing.store(false, Ordering::SeqCst);
    rig.updater.tick(start + Duration::from_millis(40)).await;
    assert_eq!(rig.library_helper.posts(), 1);
    assert_eq!(rig.lightkey_helper.posts(), 1);
    assert_eq!(rig.library_helper.running(), "0.4.0");
    assert_eq!(rig.lightkey_helper.running(), "0.6.0");
}

#[tokio::test]
async fn a_helper_that_was_unreachable_and_answers_again_is_looked_at_again_even_after_the_window()
{
    // Reserve a port nothing listens on, so the helper is "not there" at start.
    let port = {
        let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        probe.local_addr().unwrap().port()
    };
    let helper = Fake::library("0.3.0", Then::Apply(Duration::from_millis(20)));
    let address = format!("127.0.0.1:{port}");
    let env = Env::new(&address);
    let library = library();
    let updater = HelperUpdater::with_parts(
        env.clone(),
        library.clone(),
        library_bundle("0.4.0", Some(TEST_SEED)),
        lightkey_bundle("0.6.0", Some(TEST_SEED)),
        &public_of(TEST_SEED),
        UpdaterConfig {
            window: Duration::from_millis(5),
            retry: Duration::from_millis(10),
            ..quick()
        },
    );
    let settings = env.playback_settings();
    let start = Instant::now();
    updater.open_windows(start);
    updater.tick(start).await;
    // A window of the product asks the library for its status now and then: it sees nobody there.
    let status = library.status(&settings, true).await;
    assert_ne!(status.helper.state, HelperState::Ready);
    assert_eq!(library.last_health_reachable(), Some(false));
    updater.tick(start + Duration::from_millis(500)).await;
    assert_eq!(helper.posts(), 0);
    // The helper starts. The next status ask of a window sees it, and the scheduler opens a new window by itself.
    helper.serve_on(port).await;
    library.advance_clock(Duration::from_secs(31));
    let status = library.status(&settings, true).await;
    assert_eq!(status.helper.state, HelperState::Outdated);
    assert_eq!(library.last_health_reachable(), Some(true));
    updater.tick(start + Duration::from_millis(600)).await;
    assert_eq!(helper.posts(), 1);
    assert_eq!(helper.running(), "0.4.0");
}

#[tokio::test]
async fn the_scheduler_task_looks_after_a_start_delay_and_updates_a_quiet_helper() {
    let rig = standard_rig().await;
    let updater = Arc::new(rig.updater);
    let task = updater.spawn();
    for _ in 0..200 {
        if rig.library_helper.running() == "0.4.0" && rig.lightkey_helper.running() == "0.6.0" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    task.abort();
    assert_eq!(rig.library_helper.running(), "0.4.0");
    assert_eq!(rig.lightkey_helper.running(), "0.6.0");
    assert_eq!(rig.library_helper.posts(), 1);
    assert_eq!(rig.lightkey_helper.posts(), 1);
}

// ---------------------------------------------------------------- the key and the bundles that ship

#[test]
fn the_signature_check_agrees_with_the_rfc_8032_test_vectors() {
    // RFC 8032, section 7.1, test 2: the message is the single byte 0x72.
    let key = <[u8; 32]>::try_from(
        hex::decode("3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c").unwrap(),
    )
    .unwrap();
    let signature = hex::decode(
        "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
    )
    .unwrap();
    let make = |message: &'static [u8], signature: &[u8]| UpdateBundle {
        version: "1.0.0".into(),
        files: vec![],
        sums: message,
        signature: leak(STANDARD.encode(signature).into_bytes()),
    };
    assert!(signature_valid(&make(b"r", &signature), &key));
    assert!(!signature_valid(&make(b"s", &signature), &key));
    let mut flipped = signature.clone();
    flipped[10] ^= 1;
    assert!(!signature_valid(&make(b"r", &flipped), &key));
    assert!(!signature_valid(&make(b"r", &signature[..63]), &key));
}

#[test]
fn the_bundles_inside_this_build_are_signed_by_the_project_or_not_signed_at_all() {
    let key = public_key_bytes(HELPER_SIGNING_PUBLIC_KEY).unwrap();
    for (what, bundle) in [
        (
            "Library Link",
            stagepilot_playback::helper_assets::update_bundle(),
        ),
        (
            "Lightkey helper",
            stagepilot_lights::lightkey_helper_assets::update_bundle(),
        ),
    ] {
        // A working tree carries an empty SHA256SUMS.sig (the release machine signs); a signature that is there must be right.
        assert!(
            bundle.signature_text().is_empty() || signature_valid(&bundle, &key),
            "{what}: SHA256SUMS.sig is not a valid signature of SHA256SUMS by the project's key"
        );
    }
}

#[test]
fn the_pinned_key_is_the_same_in_the_helpers_the_scripts_and_here() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let read = |path: &str| std::fs::read_to_string(root.join(path)).unwrap();
    for (path, marker) in [
        (
            "crates/stagepilot-playback/assets/playback-helper/playback_library.py",
            "UPDATE_PUBLIC_KEY = \"",
        ),
        (
            "crates/stagepilot-lights/assets/lightkey-helper/lightkey_api.py",
            "UPDATE_PUBLIC_KEY = \"",
        ),
        ("scripts/helper_signing.mjs", "HELPER_PUBLIC_KEY = \""),
    ] {
        let text = read(path);
        let at = text.find(marker).unwrap_or_else(|| panic!("{path}"));
        let rest = &text[at + marker.len()..];
        assert_eq!(
            &rest[..rest.find('"').unwrap()],
            HELPER_SIGNING_PUBLIC_KEY,
            "{path}"
        );
    }
}

#[test]
fn the_shared_update_code_is_the_same_in_both_helpers_byte_for_byte() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let block = |path: &str| {
        let text = std::fs::read_to_string(root.join(path)).unwrap();
        let from = text.find("# ===== BEGIN helper-update core =====").unwrap();
        let to = text.find("# ===== END helper-update core =====").unwrap();
        text[from..to].to_string()
    };
    let a = block("crates/stagepilot-playback/assets/playback-helper/playback_library.py");
    let b = block("crates/stagepilot-lights/assets/lightkey-helper/lightkey_api.py");
    assert!(a.len() > 5000);
    assert_eq!(a, b);
}
