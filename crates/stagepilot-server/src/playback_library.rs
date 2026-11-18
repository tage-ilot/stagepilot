//! The optional Playback library connection: the HTTP side. Read-only.
//!
//! A small helper on the Playback computer (`playback-library`, port 8780) answers `GET` with what
//! Playback knows about a setlist: key, BPM, length, arrangement. This module asks it, on demand,
//! and pairs the answer with the loaded plan (`stagepilot_playback::library`).
//!
//! Why it is its own module here and not in the Playback plugin: it must never share a lock, a task
//! or a settings comparison with the Playback connection. Nothing in this file touches the plugin,
//! the song order, the scan result, a timer, the lights, ProPresenter or Planning Center.
//!
//! The helper is the "Library Link" on the Playback computer. It answers on any network path, but only to
//! a caller that signs each request with the key StagePilot made (`X-PBL-Time` and `X-PBL-Sig`, an HMAC of
//! `GET\n<path and query>\n<time>`). The key lives in the credential store, is made when the first setup
//! session is opened, and is never returned, logged or saved in `settings.json`.
//!
//! Rules (all kept by `Library`):
//! * There is no on/off switch and nothing to forget: while a Playback host is saved the helper is asked, at
//!   most once per 30 s (see below), and a helper that is not there is not a warning anywhere
//!   (`HelperState::NotInstalled`). The key is made once and kept; running the setup command again reuses it.
//! * The helper is asked on a few occasions only. A window asking (`GET /status`, `GET /plan`). The look the
//!   runtime makes by itself when it starts (health, then the open setlist and its songs, read fresh), when
//!   a key is paired: if the helper is silent it is tried again after 2 s, 5 s, 15 s and then every 30 s,
//!   until it has answered once. The watcher's look every few seconds once the Library Link has been in use
//!   (`playback_order`). And the "Scan Playback Songs" button when the Library Link works (a fresh read).
//!   A computer that never had a Library Link set up (no key) is never asked in the background. Apart from
//!   the fresh reads, the helper is asked at most once per 30 s however many ask; the Check button
//!   (`?check=1`) may ask again after 2 s. Failures are remembered for the same time, so a computer that
//!   does not answer is not asked again and again.
//! * One request is 3 s at most (connect, answer and body together), a redirect is never followed,
//!   and an answer larger than its cap (16 KiB health, 256 KiB setlist or song) is refused. The
//!   waveform is never asked for.
//! * The last good answer is kept for 60 s and shown, marked as old, while the helper does not answer.
//! * A check is cheap: every 30 s only `/health` is asked, and the setlist and the songs are read again
//!   only when its `library_revision` is new, the plan or the setting changed, 10 minutes have passed,
//!   or a paired song lacks its data. A helper that sends no revision is read in full each time.
//! * Cover pictures come through here and nowhere else (the window never calls the helper): only for a
//!   song of the current paired plan, at most 2 MB, JPEG or PNG by type and by first bytes, held in
//!   memory only (64 pictures, 8 MB, oldest out first), never written to disk, a log or the plan.
//! * A song's waveform comes through here too, and only when the window shows that song's lighting
//!   timeline: a song of the current paired plan, at most 20,000 numbers in 0..=1, 6 s and 600,000
//!   bytes at most, cut to whole numbers 0..=255 (nothing else of the answer is passed on), held in
//!   memory only (16 songs, by `library_revision`; 10 minutes for a helper that sends none).
//! * Every failure is one short plain sentence.
//! * Library data is held in memory only.

use crate::playback_key_watch::KeyWatchStore;
use crate::runtime::Runtime;
use crate::validation::{http_error, ok, unprocessable, Json};
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::Router;
use chrono::{DateTime, SecondsFormat, Utc};
use futures_util::StreamExt;
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use stagepilot_core::config::{
    clean_playback_library_address, PlaybackLibrarySettings, DEFAULT_PLAYBACK_LIBRARY_PORT,
};
use stagepilot_core::credential_guard::GuardedCredentialStore;
use stagepilot_core::settings::{CredentialStore, MemoryCredentialStore, SettingsService};
use stagepilot_core::state::ServicePlan;
use stagepilot_playback::helper_assets::BUNDLED_HELPER_VERSION;
use stagepilot_playback::library::{
    apply_detail, health_words, helper_state_for, helper_words, match_plan, match_setlist,
    parse_health, parse_setlist, parse_setlist_list, HelperState, HelperStatus, LibraryArtwork,
    LibraryPlan, LibrarySection, LibrarySetlist, LibraryState, LibraryStatus, ParsedSetlist,
    PlanMatch, PlanTitle, MAX_ARTWORK_BYTES, MAX_SETLISTS,
};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// One request, connect to last byte.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
/// The least time between two asks of the helper from windows that only look.
pub const MIN_REFRESH: Duration = Duration::from_secs(30);
/// The least time between two asks when the operator presses Check.
pub const MIN_CHECK: Duration = Duration::from_secs(2);
/// How long the last good answer may stand in for a helper that does not answer.
pub const KEEP_GOOD: Duration = Duration::from_secs(60);
const HEALTH_CAP: usize = 16 * 1024;
/// One update request: the whole bundle (a few hundred KB at most) goes to the helper, which checks it before it answers.
const UPDATE_TIMEOUT: Duration = Duration::from_secs(20);
const BODY_CAP: usize = 256 * 1024;
/// Songs whose own answer (section count) is read for one setlist, and how many are read at once.
const DETAIL_SONGS: usize = 30;
const DETAIL_AT_ONCE: usize = 4;
/// The whole of one refresh of a setlist and its songs.
const REFRESH_BUDGET: Duration = Duration::from_secs(8);
const DETAIL_BUDGET: Duration = Duration::from_secs(5);
/// Even when the helper's `library_revision` has not changed, the setlist and songs are read again
/// after this long (a safety net for a token that does not move when it should).
pub const SAFETY_NET: Duration = Duration::from_secs(10 * 60);
/// A picture the helper described with the same `ETag` is served from memory for this long before
/// the helper is asked (`If-None-Match`) whether it is still the same.
const ARTWORK_RECHECK: Duration = Duration::from_secs(10 * 60);
const ARTWORK_CAP: usize = MAX_ARTWORK_BYTES as usize;
const ARTWORK_MAX_PICTURES: usize = 64;
const ARTWORK_MAX_TOTAL: usize = 8 * 1024 * 1024;
/// Pictures fetched from the helper at the same time.
const ARTWORK_AT_ONCE: usize = 4;
/// The helper decodes a song's picture on first use (under a second): a waveform gets longer than a check.
const WAVEFORM_TIMEOUT: Duration = Duration::from_secs(6);
const WAVEFORM_CAP: usize = 600_000;
const WAVEFORM_MAX_POINTS: usize = 20_000;
const WAVEFORM_MAX_STEP_S: f64 = 10.0;
const WAVEFORM_MAX_SONGS: usize = 16;
/// A helper that sends no `library_revision` cannot say when a waveform is out of date.
const WAVEFORM_NO_REVISION_TTL: Duration = Duration::from_secs(10 * 60);
const WAVEFORM_AT_ONCE: usize = 2;
/// Points per second asked of the helper (a 7-minute song is about 3,400 numbers).
const WAVEFORM_POINTS_PER_SECOND: u32 = 8;

/// A helper that refused our signature is asked again no sooner than this, however many windows or Check
/// presses ask: ten bad signatures in a minute make it refuse this address for a minute.
pub const MIN_RETRY_AFTER_REFUSAL: Duration = Duration::from_secs(10);
/// The shortest key the helper accepts; ours is 43 characters.
const MIN_KEY_LEN: usize = 32;

const DID_NOT_ANSWER: &str = "The Playback computer did not answer.";
const NOT_UNDERSTOOD: &str =
    "The Playback library answered, but not in a way StagePilot understands.";
const NO_ADDRESS: &str = "Choose the Playback computer first.";
const LOOKED_OLD: &str = "Playback did not answer just now; this is what it said a moment ago.";
const BLOCKED: &str = "The Playback library refused this computer. Its list of allowed computers may not include this one.";
const NOT_READY: &str = "The Playback library is running but cannot read Playback yet. If the Playback computer just restarted, click Allow there, or wait a moment.";

/// Why one ask failed. Every case has one sentence (`words`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// No connection, no answer in time, or the connection broke.
    NoAnswer,
    /// An HTTP status other than 200.
    Status(u16),
    /// The answer was too big or not JSON.
    Unreadable,
}

impl Failure {
    pub fn words(&self) -> String {
        match self {
            Failure::NoAnswer => DID_NOT_ANSWER.to_string(),
            Failure::Status(403) => BLOCKED.to_string(),
            Failure::Status(503) => NOT_READY.to_string(),
            Failure::Status(_) | Failure::Unreadable => NOT_UNDERSTOOD.to_string(),
        }
    }
}

/// The base URL of a saved address: `http://host:8780` unless the address names its own port.
pub fn base_url(address: &str) -> Option<String> {
    let clean = clean_playback_library_address(address).ok()?;
    let rest = clean.split_once("://").map_or(clean.as_str(), |(_, r)| r);
    if rest.is_empty() {
        return None;
    }
    let has_port = rest
        .rsplit_once(':')
        .is_some_and(|(host, port)| !host.contains(':') && port.parse::<u16>().is_ok());
    Some(if has_port {
        clean
    } else {
        format!("{clean}:{DEFAULT_PLAYBACK_LIBRARY_PORT}")
    })
}

// ---- the key ---------------------------------------------------------------------------------------

/// Lower-case hex HMAC-SHA256 of `message` under `key`.
pub fn hmac_sha256_hex(key: &[u8], message: &[u8]) -> String {
    // HMAC takes a key of any length, so `new_from_slice` cannot fail.
    let mut mac = Hmac::<Sha256>::new_from_slice(key).unwrap_or_else(|_| std::process::abort());
    mac.update(message);
    hex::encode(mac.finalize().into_bytes())
}

/// The signature of one request: `hex(HMAC-SHA256(key, "GET\n" + path-with-query + "\n" + time))`, the same
/// bytes the helper checks.
pub fn sign(key: &str, target: &str, time: u64) -> String {
    hmac_sha256_hex(key.as_bytes(), format!("GET\n{target}\n{time}").as_bytes())
}

/// The signature of a request with a body (`POST /update`): `hex(HMAC-SHA256(key, "POST\n" + target + "\n" + time + "\n" +
/// hex(SHA-256(body))))`, so the body cannot be swapped under a good signature. The same bytes the helper checks.
pub fn sign_post(key: &str, target: &str, time: u64, body: &[u8]) -> String {
    let digest = hex::encode(Sha256::digest(body));
    hmac_sha256_hex(
        key.as_bytes(),
        format!("POST\n{target}\n{time}\n{digest}").as_bytes(),
    )
}

/// Bytes as base64url without padding (`A-Za-z0-9-_`).
fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        let symbols = chunk.len() + 1;
        for i in 0..symbols {
            out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
        }
    }
    out
}

/// A new key: 32 random bytes from the system, as 43 base64url characters.
fn new_key() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| "The system could not make a random key.".to_string())?;
    Ok(base64url(&bytes))
}

/// Seconds since the epoch.
type WallClock = Box<dyn Fn() -> u64 + Send + Sync>;

fn system_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// A stored key is usable when it is one line of plain characters the helper would accept.
fn usable_key(key: &str) -> bool {
    key.len() >= MIN_KEY_LEN && key.bytes().all(|b| (33..=126).contains(&b))
}

/// What one ask of `/setlists/<date>` gave.
#[derive(Debug, Clone)]
enum Fetched {
    /// The setlist, and Playback's recent setlists for the chooser (read only when one was chosen).
    Setlist(ParsedSetlist, Vec<LibrarySetlist>),
    /// Playback has no setlist for the date (or the one chosen is gone); `newest` is the one it has most recently
    /// and `others` its recent setlists, for the chooser.
    NoSetlist {
        newest: Option<LibrarySetlist>,
        others: Vec<LibrarySetlist>,
    },
    /// The helper is up and Playback's library cannot be read yet.
    Waiting,
}

/// Which setlist is asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SetlistAsk {
    /// The one Playback has for a plan's date (`YYYY-MM-DD`).
    Date(String),
    /// One the operator chose, by Playback's own number.
    Id(i64),
}

impl SetlistAsk {
    fn path(&self) -> String {
        match self {
            Self::Date(date) => format!("/setlists/{date}"),
            Self::Id(id) => format!("/setlists/{id}"),
        }
    }

    /// What the held answer belongs to; another one means everything is read again.
    fn key(&self) -> String {
        self.path()
    }
}

/// The loaded plan's songs as the pairing reads them. The one place a plan becomes `PlanTitle`s, so the
/// window's matchup and the song order are decided from exactly the same input.
pub fn plan_titles(plan: &ServicePlan) -> Vec<PlanTitle> {
    plan.songs
        .iter()
        .map(|song| PlanTitle {
            key: song
                .source_song_id
                .clone()
                .unwrap_or_else(|| song.id.clone()),
            title: song.title.clone(),
            seconds: song.duration_seconds,
            key_name: song.key_name.clone(),
        })
        .collect()
}

/// What the Library Link says about the setlist Playback has open, for the song order.
#[derive(Debug, Clone)]
pub struct OpenSetlist {
    /// The helper is ready or outdated and can read Playback: it is part of the song order at all.
    pub in_use: bool,
    /// The answer is a fresh one, not the last good copy shown while the helper is silent.
    pub current: bool,
    /// Playback's open setlist; `None` when it cannot be read or Playback has none open.
    pub setlist: Option<ParsedSetlist>,
}

/// One kind of answer with its throttle and its last good copy.
struct Slot<T> {
    asked: Option<Instant>,
    last: Option<Result<T, Failure>>,
    good: Option<(Instant, DateTime<Utc>, T)>,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self {
            asked: None,
            last: None,
            good: None,
        }
    }
}

impl<T: Clone> Slot<T> {
    /// Whether a new ask is allowed `gap` after the last one.
    fn due(&self, gap: Duration, now: Instant) -> bool {
        self.asked
            .is_none_or(|at| now.saturating_duration_since(at) >= gap)
    }

    fn record(&mut self, result: Result<T, Failure>, now: Instant) {
        self.asked = Some(now);
        if let Ok(value) = &result {
            self.good = Some((now, Utc::now(), value.clone()));
        }
        self.last = Some(result);
    }

    /// The helper confirmed (by an unchanged revision) that the last good answer is still right.
    fn touch(&mut self, now: Instant) {
        self.asked = Some(now);
        if let Some(Ok(value)) = &self.last {
            self.good = Some((now, Utc::now(), value.clone()));
        }
    }

    /// The answer to show: the last ask's, or the last good one (marked old, under `KEEP_GOOD`).
    fn shown(&self, now: Instant) -> Result<(T, bool, Option<DateTime<Utc>>), Failure> {
        match &self.last {
            Some(Ok(value)) => Ok((value.clone(), false, self.good.as_ref().map(|g| g.1))),
            Some(Err(failure)) => match &self.good {
                Some((at, when, value)) if now.saturating_duration_since(*at) < KEEP_GOOD => {
                    Ok((value.clone(), true, Some(*when)))
                }
                _ => Err(failure.clone()),
            },
            None => Err(Failure::NoAnswer),
        }
    }
}

type HealthSlot = Slot<stagepilot_playback::library::Health>;

#[derive(Default)]
struct Inner {
    /// Whom the slots belong to; they are dropped when the address changes.
    base: String,
    health: HealthSlot,
    /// The setlist for one plan date.
    setlist_ask: String,
    setlist: Slot<Fetched>,
    /// When the setlist and its songs were last read in full, and the `library_revision` the helper
    /// had then (none when it sends none).
    read: Option<ReadMark>,
    /// The loaded plan as the pairing sees it (date and titles): a change means a full read.
    plan_key: String,
    /// The setlist Playback has open (`/setlists/current`), read for the song order and nothing else
    /// (`None` inside: Playback has no open setlist), and when it was last read in full.
    open: Slot<Option<ParsedSetlist>>,
    open_read: Option<ReadMark>,
}

struct ReadMark {
    at: Instant,
    revision: Option<String>,
}

#[derive(Clone)]
struct Detail {
    section_count: Option<u32>,
    sections: Vec<LibrarySection>,
    artwork: Option<LibraryArtwork>,
    arrangement: Option<String>,
}

/// A cover picture held in memory, as the helper sent it.
#[derive(Clone)]
pub struct Picture {
    /// `image/jpeg` or `image/png`.
    pub content_type: &'static str,
    /// The helper's `ETag`, quotes included.
    pub etag: String,
    pub bytes: Bytes,
}

struct ArtEntry {
    picture: Picture,
    /// Insertion order: the lowest goes first when the cache is full.
    seq: u64,
    checked: Instant,
}

#[derive(Default)]
struct ArtCache {
    pictures: HashMap<(String, u64), ArtEntry>,
    total: usize,
    next: u64,
}

impl ArtCache {
    fn remove(&mut self, key: &(String, u64)) {
        if let Some(old) = self.pictures.remove(key) {
            self.total -= old.picture.bytes.len();
        }
    }

    fn insert(&mut self, key: (String, u64), picture: Picture, now: Instant) {
        self.remove(&key);
        self.total += picture.bytes.len();
        self.next += 1;
        let seq = self.next;
        self.pictures.insert(
            key,
            ArtEntry {
                picture,
                seq,
                checked: now,
            },
        );
        while self.pictures.len() > ARTWORK_MAX_PICTURES || self.total > ARTWORK_MAX_TOTAL {
            let Some(oldest) = self
                .pictures
                .iter()
                .min_by_key(|(_, entry)| entry.seq)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            self.remove(&oldest);
        }
    }
}

/// Why a cover could not be given to the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtworkFailure {
    /// The song has no picture, or what the helper sent is not a JPEG or PNG within the cap.
    NoPicture,
    /// The Playback computer did not answer (or answered with an error).
    NoAnswer,
}

/// A song's waveform as the window gets it: `{"step_s": 0.125, "peak": [0..255, ...]}` and nothing else.
#[derive(Clone)]
pub struct Waveform {
    /// A quoted tag made from the bytes below.
    pub etag: String,
    /// The JSON answer.
    pub body: Bytes,
}

struct WaveEntry {
    wave: Waveform,
    /// The helper's `library_revision` at the last good read when this was fetched (none: the helper sends none).
    revision: Option<String>,
    fetched: Instant,
    /// Insertion order: the lowest goes first when the cache is full.
    seq: u64,
}

#[derive(Default)]
struct WaveCache {
    songs: HashMap<(String, u64), WaveEntry>,
    next: u64,
}

/// Why a waveform could not be given to the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveformFailure {
    /// The song has none, or what the helper sent is not a waveform within the limits.
    NoWaveform,
    /// The Playback computer did not answer (or answered with an error).
    NoAnswer,
}

pub struct PlaybackLibrary {
    http: reqwest::Client,
    inner: Mutex<Inner>,
    /// Cover pictures, in memory only.
    art: std::sync::Mutex<ArtCache>,
    art_gate: tokio::sync::Semaphore,
    /// Waveforms, in memory only.
    waves: std::sync::Mutex<WaveCache>,
    wave_gate: tokio::sync::Semaphore,
    /// Added to the clock (tests move it to pass minutes at once; it is 0 in the product).
    skew_ms: AtomicU64,
    /// The key StagePilot made for the helper. Guarded: a keychain that waits on a dialog never holds a
    /// request up.
    keys: Arc<dyn CredentialStore>,
    /// Two setup sessions opened at once must not make two keys.
    key_gate: std::sync::Mutex<()>,
    /// Seconds since the epoch, for the `X-PBL-Time` header (tests replace it).
    wall: std::sync::Mutex<WallClock>,
    /// The setlist the operator chose for one plan (the plan's id, Playback's setlist number). In memory only: it
    /// ends with the program, and it belongs to that plan alone (another plan, or none, ignores it).
    choice: std::sync::Mutex<Option<(String, i64)>>,
    /// The keys last seen for Playback's setlist songs, to tell when one was changed (`playback-keys.json`).
    keys_seen: std::sync::Mutex<KeyWatchStore>,
}

impl Default for PlaybackLibrary {
    fn default() -> Self {
        Self::new()
    }
}

impl PlaybackLibrary {
    /// A library whose key is held in memory only (tests, demo): the product uses the credential store.
    pub fn new() -> Self {
        Self::with_credentials(Arc::new(MemoryCredentialStore::default()))
    }

    /// A library whose key lives in `credentials` (the OS keychain in the product).
    pub fn with_credentials(credentials: Arc<dyn CredentialStore>) -> Self {
        // Cargo unifies reqwest features across the workspace: a TLS provider must be installed
        // before any Client is built (the same line as every other HTTP client here).
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .connect_timeout(REQUEST_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .unwrap_or_default();
        Self {
            http,
            inner: Mutex::new(Inner::default()),
            art: std::sync::Mutex::new(ArtCache::default()),
            art_gate: tokio::sync::Semaphore::new(ARTWORK_AT_ONCE),
            waves: std::sync::Mutex::new(WaveCache::default()),
            wave_gate: tokio::sync::Semaphore::new(WAVEFORM_AT_ONCE),
            skew_ms: AtomicU64::new(0),
            keys: Arc::new(GuardedCredentialStore::new(credentials)),
            key_gate: std::sync::Mutex::new(()),
            wall: std::sync::Mutex::new(Box::new(system_seconds)),
            choice: std::sync::Mutex::new(None),
            keys_seen: std::sync::Mutex::new(KeyWatchStore::in_memory()),
        }
    }

    /// Keeps the keys it has seen in `path`, so a restart does not forget a change (the product; tests and the demo
    /// hold them in memory only).
    pub fn with_key_memory_file(self, path: impl Into<std::path::PathBuf>) -> Self {
        *self.keys_seen.lock().unwrap_or_else(|p| p.into_inner()) = KeyWatchStore::file(path);
        self
    }

    /// The key, when there is one that is fit to use. Never logged, never returned by a route.
    fn key(&self) -> Option<String> {
        self.keys
            .get_secret()
            .ok()
            .flatten()
            .filter(|key| usable_key(key))
    }

    /// A key exists: StagePilot made one (a setup session was opened).
    pub fn is_paired(&self) -> bool {
        self.key().is_some()
    }

    /// Makes the key when there is none (the first setup session does this). `true` when it made one.
    pub fn ensure_key(&self) -> Result<bool, String> {
        let _one_at_a_time = self.key_gate.lock().unwrap_or_else(|p| p.into_inner());
        if self.key().is_some() {
            return Ok(false);
        }
        let key = new_key()?;
        self.keys.set_secret(&key).map_err(|_| {
            "StagePilot could not save the key in the secure credential store.".to_string()
        })?;
        Ok(true)
    }

    /// The key for the installer StagePilot serves to the Playback computer, and for nothing else.
    pub(crate) fn key_for_installer(&self) -> Option<String> {
        self.key()
    }

    /// Replaces the key (tests only).
    #[doc(hidden)]
    pub fn set_key_for_test(&self, key: &str) {
        let _ = self.keys.set_secret(key);
    }

    /// Replaces the clock that stamps requests, seconds since the epoch (tests only).
    #[doc(hidden)]
    pub fn set_wall_clock(&self, clock: WallClock) {
        *self.wall.lock().unwrap_or_else(|p| p.into_inner()) = clock;
    }

    fn wall_seconds(&self) -> u64 {
        (self.wall.lock().unwrap_or_else(|p| p.into_inner()))()
    }

    /// Adds `X-PBL-Time` and `X-PBL-Sig` to a request for `target` (the path and query exactly as sent)
    /// when there is a key. Without a key nothing is added: the helper then answers the bare minimum
    /// (a keyed one) or everything (one that has no key).
    fn signed(&self, request: reqwest::RequestBuilder, target: &str) -> reqwest::RequestBuilder {
        match self.key() {
            Some(key) => {
                let time = self.wall_seconds();
                request
                    .header("X-PBL-Time", time.to_string())
                    .header("X-PBL-Sig", sign(&key, target, time))
            }
            None => request,
        }
    }

    /// Drops every answer held, so the next ask starts from nothing (a new key, or another address).
    pub async fn reset(&self) {
        self.clear_held();
        *self.inner.lock().await = Inner::default();
        *self.choice.lock().unwrap_or_else(|p| p.into_inner()) = None;
        // Another computer (or none) is another Playback: its song numbers mean other songs.
        self.keys_seen
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }

    /// The setlist the operator chose for this plan, if any.
    pub fn chosen_for(&self, plan_id: &str) -> Option<i64> {
        self.choice
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .filter(|(id, _)| id == plan_id)
            .map(|(_, setlist)| *setlist)
    }

    /// Chooses Playback's setlist `setlist_id` for the plan (or, with `None`, goes back to the setlist on the plan's
    /// date). Kept in memory for this plan only; one choice at a time.
    pub fn choose(&self, plan_id: &str, setlist_id: Option<i64>) {
        *self.choice.lock().unwrap_or_else(|p| p.into_inner()) =
            setlist_id.map(|setlist| (plan_id.to_string(), setlist));
    }

    fn now(&self) -> Instant {
        Instant::now() + Duration::from_millis(self.skew_ms.load(Ordering::Relaxed))
    }

    /// Moves this module's clock forward (tests only: the 10-minute safety net without waiting).
    #[doc(hidden)]
    pub fn advance_clock(&self, by: Duration) {
        let ms = u64::try_from(by.as_millis()).unwrap_or(u64::MAX);
        self.skew_ms.fetch_add(ms, Ordering::Relaxed);
    }

    /// How many pictures and how many bytes are held (tests only).
    #[doc(hidden)]
    pub fn artwork_held(&self) -> (usize, usize) {
        self.art
            .lock()
            .map_or((0, 0), |art| (art.pictures.len(), art.total))
    }

    /// How many waveforms are held (tests only).
    #[doc(hidden)]
    pub fn waveforms_held(&self) -> usize {
        self.waves.lock().map_or(0, |waves| waves.songs.len())
    }

    /// Lets go of everything held in memory (answers, pictures, waveforms).
    fn clear_held(&self) {
        if let Ok(mut waves) = self.waves.lock() {
            *waves = WaveCache::default();
        }
        if let Ok(mut inner) = self.inner.try_lock() {
            if !inner.base.is_empty() {
                *inner = Inner::default();
            }
        }
        if let Ok(mut art) = self.art.lock() {
            *art = ArtCache::default();
        }
    }

    /// Whether the last ask of `/health` was answered: `Some(true)` it was (the helper answered, with or without our key), `Some(false)`
    /// it was not (nothing there), `None` nothing was asked yet or the answer is being read right now. Asks nothing; the automatic
    /// update uses it to notice that a helper that was silent is there again.
    pub fn last_health_reachable(&self) -> Option<bool> {
        let inner = self.inner.try_lock().ok()?;
        match &inner.health.last {
            Some(Ok(_)) => Some(true),
            Some(Err(Failure::NoAnswer)) => Some(false),
            Some(Err(_)) => Some(true),
            None => None,
        }
    }

    /// One signed `GET /health` now, whatever the throttle says (the automatic update asks at its own pace, never more than once a
    /// second): the helper's answer parsed, or why there was none.
    pub(crate) async fn fresh_health(
        &self,
        connect_base: &str,
        host: &str,
    ) -> Result<stagepilot_playback::library::Health, Failure> {
        let mut response = self
            .signed(
                self.http
                    .get(format!("{connect_base}/health"))
                    .header(header::HOST, host)
                    .header("accept", "application/json"),
                "/health",
            )
            .send()
            .await
            .map_err(|_| Failure::NoAnswer)?;
        let status = response.status().as_u16();
        if status != 200 {
            return Err(Failure::Status(status));
        }
        let mut body: Vec<u8> = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Failure::NoAnswer)? {
            if body.len() + chunk.len() > HEALTH_CAP {
                return Err(Failure::Unreadable);
            }
            body.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&body).map_err(|_| Failure::Unreadable)?;
        Ok(parse_health(&value))
    }

    /// `POST /update` to a helper that can update itself: the body is signed with the key like every request, plus its SHA-256, and
    /// goes to `connect_base` (an `http://<address>:<port>` the caller has checked is on the local network) with `host` as its `Host`.
    /// Without a usable key nothing is sent. Returns the helper's status and its JSON answer (at most 16 KiB).
    pub(crate) async fn post_update(
        &self,
        connect_base: &str,
        host: &str,
        body: Vec<u8>,
    ) -> Result<(u16, Value), Failure> {
        let key = self.key().ok_or(Failure::NoAnswer)?;
        let time = self.wall_seconds();
        let mut response = self
            .http
            .post(format!("{connect_base}/update"))
            .header(header::HOST, host)
            .header(header::CONTENT_TYPE, "application/json")
            .header("X-PBL-Time", time.to_string())
            .header("X-PBL-Sig", sign_post(&key, "/update", time, &body))
            .timeout(UPDATE_TIMEOUT)
            .body(body)
            .send()
            .await
            .map_err(|_| Failure::NoAnswer)?;
        let status = response.status().as_u16();
        let mut answer: Vec<u8> = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Failure::NoAnswer)? {
            if answer.len() + chunk.len() > HEALTH_CAP {
                return Err(Failure::Unreadable);
            }
            answer.extend_from_slice(&chunk);
        }
        Ok((
            status,
            serde_json::from_slice(&answer).unwrap_or(Value::Null),
        ))
    }

    /// `GET base+path`, JSON, at most `cap` bytes, 3 s in all. Only a 200 is an answer.
    async fn get(&self, base: &str, path: &str, cap: usize) -> Result<(u16, Value), Failure> {
        let url = format!("{base}{path}");
        let mut response = self
            .signed(
                self.http.get(&url).header("accept", "application/json"),
                path,
            )
            .send()
            .await
            .map_err(|_| Failure::NoAnswer)?;
        let status = response.status().as_u16();
        if status != 200 {
            return Ok((status, Value::Null));
        }
        if response.content_length().is_some_and(|n| n > cap as u64) {
            return Err(Failure::Unreadable);
        }
        let mut body: Vec<u8> = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Failure::NoAnswer)? {
            if body.len() + chunk.len() > cap {
                return Err(Failure::Unreadable);
            }
            body.extend_from_slice(&chunk);
        }
        let value = serde_json::from_slice(&body).map_err(|_| Failure::Unreadable)?;
        Ok((200, value))
    }

    /// `get`, with any status other than 200 a failure.
    async fn get_ok(&self, base: &str, path: &str, cap: usize) -> Result<Value, Failure> {
        match self.get(base, path, cap).await? {
            (200, value) => Ok(value),
            (status, _) => Err(Failure::Status(status)),
        }
    }

    /// What the window is told when the library cannot feed anything: the Library Link's state and its
    /// plain sentence, and no library data. Only `unreachable` is a problem (a helper that was in use and is
    /// silent now); every other state is quiet.
    fn inactive(
        &self,
        settings: &PlaybackLibrarySettings,
        state: HelperState,
        version: Option<String>,
        message: Option<&str>,
    ) -> LibraryStatus {
        let words = message.map_or_else(|| helper_words(state, version.as_deref()), str::to_string);
        let unreachable = state == HelperState::Unreachable;
        LibraryStatus {
            enabled: unreachable,
            active: false,
            helper: HelperStatus {
                state,
                version: version.clone(),
                bundled_version: BUNDLED_HELPER_VERSION.to_string(),
                message: words.clone(),
                paired: self.key().is_some(),
                updater: false,
                last_update_error: None,
                update: None,
            },
            address: settings.address.clone(),
            state: if unreachable {
                LibraryState::Problem
            } else {
                LibraryState::Off
            },
            message: words,
            version,
            songs: None,
            setlists: None,
            stale: false,
            checked_at: None,
        }
    }

    /// Where the helper stands, from `/health`. `check` is the Check button: it may ask again 2 s after
    /// the last ask instead of 30 s.
    pub async fn status(&self, settings: &PlaybackLibrarySettings, check: bool) -> LibraryStatus {
        let Some(base) = base_url(&settings.address) else {
            return self.inactive(settings, HelperState::NotInstalled, None, Some(NO_ADDRESS));
        };
        let mut inner = self.inner.lock().await;
        self.status_locked(&mut inner, settings, &base, check).await
    }

    async fn status_locked(
        &self,
        inner: &mut Inner,
        settings: &PlaybackLibrarySettings,
        base: &str,
        check: bool,
    ) -> LibraryStatus {
        if inner.base != base {
            *inner = Inner {
                base: base.to_string(),
                ..Inner::default()
            };
        }
        let mut gap = if check { MIN_CHECK } else { MIN_REFRESH };
        // A helper that refused our signature is not pressed: ten bad ones in a minute lock this address out.
        if matches!(
            inner.health.last,
            Some(Err(Failure::Status(401 | 403 | 429)))
        ) {
            gap = gap.max(MIN_RETRY_AFTER_REFUSAL);
        }
        if inner.health.due(gap, self.now()) {
            let answer = self.get_ok(base, "/health", HEALTH_CAP).await;
            // A helper that answers 503 to /health is told apart from one that is silent.
            inner
                .health
                .record(answer.map(|value| parse_health(&value)), self.now());
        }
        match inner.health.shown(self.now()) {
            Ok((health, stale, when)) => {
                let helper_state = helper_state_for(&health);
                if !helper_state.is_active() {
                    return self.inactive(settings, helper_state, health.version.clone(), None);
                }
                let (state, mut message) = health_words(&health);
                if stale {
                    message = format!("{LOOKED_OLD} {message}");
                }
                LibraryStatus {
                    enabled: true,
                    active: true,
                    helper: HelperStatus {
                        state: helper_state,
                        version: health.version.clone(),
                        bundled_version: BUNDLED_HELPER_VERSION.to_string(),
                        message: helper_words(helper_state, health.version.as_deref()),
                        paired: self.key().is_some(),
                        updater: health.updater.is_some_and(|level| level >= 1),
                        last_update_error: health.last_update_error.clone(),
                        update: None,
                    },
                    address: settings.address.clone(),
                    state,
                    message,
                    version: health.version.clone(),
                    songs: health.songs,
                    setlists: health.setlists,
                    stale,
                    checked_at: when.map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true)),
                }
            }
            // The helper refused: it is there, and it does not know this StagePilot's key (or limits this address).
            Err(Failure::Status(401 | 403 | 429)) => {
                self.inactive(settings, HelperState::NeedsKey, None, None)
            }
            // Silence. Never in use: nothing was ever installed here, and that is not a warning anywhere. In
            // use before: it has gone quiet.
            Err(failure) => {
                if settings.enabled == Some(true) {
                    let mut status = self.inactive(settings, HelperState::Unreachable, None, None);
                    // The helper row says it did not answer; the sentence for the header says why, as it always did.
                    status.message = failure.words();
                    status
                } else {
                    self.inactive(settings, HelperState::NotInstalled, None, None)
                }
            }
        }
    }

    /// One song's own answer: its sections and arrangement name. Never cached here: the revision
    /// check decides when songs are read again, and a copy served from a cache would outlive it.
    async fn song_detail(&self, base: &str, id: i64) -> Option<Detail> {
        let value = self
            .get_ok(base, &format!("/songs/{id}"), BODY_CAP)
            .await
            .ok()?;
        let mut scratch = stagepilot_playback::library::LibrarySong {
            setlist_song_id: Some(id),
            position: 1,
            title: String::new(),
            key: None,
            key_original: None,
            key_was: None,
            key_changed_at: None,
            bpm: None,
            scan_length_s: None,
            countdown_length_s: None,
            length_source: None,
            arrangement: None,
            section_count: None,
            sections: Vec::new(),
            artwork: None,
            rental_expires_utc: None,
        };
        apply_detail(&mut scratch, &value);
        Some(Detail {
            section_count: scratch.section_count,
            sections: scratch.sections,
            artwork: scratch.artwork,
            arrangement: scratch.arrangement,
        })
    }

    /// Playback's recent setlists for the chooser; none when the helper cannot say.
    async fn recent_setlists(&self, base: &str) -> Vec<LibrarySetlist> {
        match self
            .get(base, &format!("/setlists?limit={MAX_SETLISTS}"), BODY_CAP)
            .await
        {
            Ok((200, value)) => parse_setlist_list(&value),
            _ => Vec::new(),
        }
    }

    /// Reads the setlist `ask` names (the one on a date, or one the operator chose by number), then each song's own
    /// answer for its section count.
    async fn fetch_setlist(&self, base: &str, ask: &SetlistAsk) -> Result<Fetched, Failure> {
        let (status, value) = self.get(base, &ask.path(), BODY_CAP).await?;
        match status {
            200 => {
                let mut parsed = parse_setlist(&value).ok_or(Failure::Unreadable)?;
                if let SetlistAsk::Date(date) = ask {
                    if parsed.date.as_deref().is_some_and(|d| d != date) {
                        // Whatever the helper meant, it is not the setlist of this day.
                        return Ok(Fetched::NoSetlist {
                            newest: Some(parsed.summary()),
                            others: self.recent_setlists(base).await,
                        });
                    }
                }
                if let SetlistAsk::Id(id) = ask {
                    if parsed.id.is_some_and(|n| n != *id) {
                        return Ok(Fetched::NoSetlist {
                            newest: None,
                            others: self.recent_setlists(base).await,
                        });
                    }
                }
                self.read_song_details(base, &mut parsed).await;
                // The setlist was read in full just now: the one place the keys are compared with the last time.
                self.note_keys(&mut parsed);
                let others = match ask {
                    SetlistAsk::Id(_) => self.recent_setlists(base).await,
                    SetlistAsk::Date(_) => Vec::new(),
                };
                Ok(Fetched::Setlist(parsed, others))
            }
            404 => {
                let newest = match self.get(base, "/setlists/latest", BODY_CAP).await {
                    Ok((200, value)) => parse_setlist(&value).map(|p| p.summary()),
                    _ => None,
                };
                let others = self.recent_setlists(base).await;
                Ok(Fetched::NoSetlist { newest, others })
            }
            503 => Ok(Fetched::Waiting),
            other => Err(Failure::Status(other)),
        }
    }

    /// Compares each song's key with the one seen the time before (no request: the setlist is already read) and puts
    /// `key_was` and `key_changed_at` on the songs whose key was changed.
    fn note_keys(&self, setlist: &mut ParsedSetlist) {
        let now = Utc::now()
            + chrono::Duration::milliseconds(
                i64::try_from(self.skew_ms.load(Ordering::Relaxed)).unwrap_or(0),
            );
        self.keys_seen
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .observe(&mut setlist.songs, now);
    }

    async fn read_song_details(&self, base: &str, setlist: &mut ParsedSetlist) {
        let jobs: Vec<(usize, i64)> = setlist
            .songs
            .iter()
            .enumerate()
            .take(DETAIL_SONGS)
            .filter_map(|(i, s)| s.setlist_song_id.map(|id| (i, id)))
            .collect();
        let mut answers = futures_util::stream::iter(jobs)
            .map(|(i, id)| async move { (i, self.song_detail(base, id).await) })
            .buffer_unordered(DETAIL_AT_ONCE);
        let deadline = tokio::time::Instant::now() + DETAIL_BUDGET;
        while let Ok(Some((i, detail))) = tokio::time::timeout_at(deadline, answers.next()).await {
            if let (Some(detail), Some(song)) = (detail, setlist.songs.get_mut(i)) {
                song.section_count = detail.section_count;
                song.sections = detail.sections;
                if song.artwork.is_none() {
                    song.artwork = detail.artwork;
                }
                if song.arrangement.is_none() {
                    song.arrangement = detail.arrangement;
                }
            }
        }
    }

    /// Whether the setlist and the songs must be read again, not just `/health`. Yes when: nothing
    /// good was read yet (the plan, the date or the setting changed, or the last read failed); the
    /// helper sends no `library_revision`; the token differs from the one at the last good read; that
    /// read is 10 minutes old; or a paired song lacks its own data.
    fn needs_full_read(
        &self,
        inner: &Inner,
        revision: Option<&String>,
        titles: &[PlanTitle],
    ) -> bool {
        let Some(Ok(fetched)) = &inner.setlist.last else {
            return true;
        };
        let (Some(mark), Some(revision)) = (&inner.read, revision) else {
            return true;
        };
        if mark.revision.as_ref() != Some(revision)
            || self.now().saturating_duration_since(mark.at) >= SAFETY_NET
        {
            return true;
        }
        match fetched {
            Fetched::Waiting => true,
            Fetched::NoSetlist { .. } => false,
            Fetched::Setlist(parsed, _) => {
                let paired: HashSet<i64> = match_plan(titles, &parsed.songs)
                    .iter()
                    .filter(|song| song.status == PlanMatch::Matched)
                    .filter_map(|song| song.library.as_ref()?.setlist_song_id)
                    .collect();
                parsed.songs.iter().take(DETAIL_SONGS).any(|song| {
                    song.section_count.is_none()
                        && song.setlist_song_id.is_some_and(|id| paired.contains(&id))
                })
            }
        }
    }

    /// The setlist Playback opened last (`/setlists/current`), which is the one the Playback API plays. It is
    /// read on its own, apart from the setlist the window matches with the plan (that one may be the
    /// setlist of another date, chosen by a person): the song order must come from the open one.
    ///
    /// The helper is asked no more often than the rest: `/health` at most every 30 s, the setlist only
    /// when the helper's `library_revision` is new or 10 minutes have passed (a helper that sends no
    /// revision is read each time). Nothing is asked when no Playback computer is saved.
    pub async fn open_setlist(&self, settings: &PlaybackLibrarySettings) -> OpenSetlist {
        self.open_setlist_inner(settings, false).await
    }

    /// The same, read fresh: the health, the open setlist and its songs are asked now, whatever the
    /// 30 s gap or an unchanged `library_revision` would have said. This is what a person's press on
    /// "Scan Playback Songs" and the connection made at start-up cause. The next window that asks
    /// for the plan reads it in full too.
    pub async fn open_setlist_fresh(&self, settings: &PlaybackLibrarySettings) -> OpenSetlist {
        self.open_setlist_inner(settings, true).await
    }

    async fn open_setlist_inner(
        &self,
        settings: &PlaybackLibrarySettings,
        fresh: bool,
    ) -> OpenSetlist {
        let none = OpenSetlist {
            in_use: false,
            current: false,
            setlist: None,
        };
        let Some(base) = base_url(&settings.address) else {
            return none;
        };
        let mut inner = self.inner.lock().await;
        if fresh {
            // Forget when anything was last asked: every answer below is read again.
            inner.health.asked = None;
            inner.open.asked = None;
            inner.open_read = None;
            inner.setlist.asked = None;
            inner.read = None;
        }
        let status = self.status_locked(&mut inner, settings, &base, false).await;
        if !status.active {
            return none;
        }
        if status.state != LibraryState::Ready {
            return OpenSetlist {
                in_use: true,
                current: !status.stale,
                setlist: None,
            };
        }
        if inner.open.due(MIN_REFRESH, self.now()) {
            let revision = match &inner.health.last {
                Some(Ok(health)) => Some(health.library_revision.clone()),
                _ => None,
            };
            if let Some(revision) = revision {
                let unchanged = matches!(&inner.open.last, Some(Ok(_)))
                    && matches!((&inner.open_read, &revision), (Some(mark), Some(now))
                        if mark.revision.as_ref() == Some(now)
                            && self.now().saturating_duration_since(mark.at) < SAFETY_NET);
                if unchanged {
                    inner.open.touch(self.now());
                } else {
                    let answer = tokio::time::timeout(REFRESH_BUDGET, self.fetch_open(&base))
                        .await
                        .unwrap_or(Err(Failure::NoAnswer));
                    let good = answer.is_ok();
                    inner.open.record(answer, self.now());
                    inner.open_read = good.then(|| ReadMark {
                        at: self.now(),
                        revision,
                    });
                }
            }
        }
        match inner.open.shown(self.now()) {
            Ok((setlist, stale, _)) => OpenSetlist {
                in_use: true,
                current: !stale && !status.stale,
                setlist,
            },
            Err(_) => OpenSetlist {
                in_use: true,
                current: true,
                setlist: None,
            },
        }
    }

    async fn fetch_open(&self, base: &str) -> Result<Option<ParsedSetlist>, Failure> {
        match self.get(base, "/setlists/current", BODY_CAP).await? {
            (200, value) => parse_setlist(&value).map(Some).ok_or(Failure::Unreadable),
            // Nothing is open, or the library cannot be read yet: there is no setlist to use.
            (404 | 503, _) => Ok(None),
            (other, _) => Err(Failure::Status(other)),
        }
    }

    /// The loaded plan paired with Playback's setlist for the plan's date.
    pub async fn plan(
        &self,
        settings: &PlaybackLibrarySettings,
        plan: Option<&ServicePlan>,
        check: bool,
    ) -> LibraryPlan {
        let date = plan.map(|p| p.date.format("%Y-%m-%d").to_string());
        // `enabled` says the library is in use. Where nothing is asked (no plan, no address) it is what
        // the saved setting says; once the helper has been asked it is what the helper's state says.
        let in_use = AtomicBool::new(settings.enabled == Some(true));
        let empty = |state: LibraryState, message: Option<String>| LibraryPlan {
            enabled: in_use.load(Ordering::Relaxed),
            state,
            message,
            plan_date: date.clone(),
            setlist: None,
            newest: None,
            songs: vec![],
            playback_only: vec![],
            chosen: false,
            setlists: vec![],
            stale: false,
            checked_at: None,
        };
        let Some(base) = base_url(&settings.address) else {
            in_use.store(false, Ordering::Relaxed);
            return empty(LibraryState::Off, Some(NO_ADDRESS.to_string()));
        };
        let (Some(plan), Some(date)) = (plan, date.clone()) else {
            return empty(LibraryState::Ready, None);
        };
        if plan.songs.is_empty() {
            return empty(LibraryState::Ready, None);
        }
        let titles = plan_titles(plan);
        // One ask at a time: a second window waits here and then reads what the first one learned.
        let mut inner = self.inner.lock().await;
        let status = self.status_locked(&mut inner, settings, &base, check).await;
        in_use.store(status.enabled, Ordering::Relaxed);
        if !status.active || status.state != LibraryState::Ready {
            return empty(status.state, Some(status.message));
        }
        let chosen = self.chosen_for(&plan.id);
        let ask = match chosen {
            Some(id) => SetlistAsk::Id(id),
            None => SetlistAsk::Date(date.clone()),
        };
        if inner.setlist_ask != ask.key() {
            inner.setlist_ask = ask.key();
            inner.setlist = Slot::default();
            inner.read = None;
        }
        let plan_key = titles
            .iter()
            .map(|t| t.title.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if inner.plan_key != plan_key {
            inner.plan_key = plan_key;
            inner.read = None;
        }
        let gap = if check { MIN_CHECK } else { MIN_REFRESH };
        if inner.setlist.due(gap, self.now()) {
            // Only a helper that answered `/health` just now is read further; one that did not is
            // not asked for more (what is held stands in, marked old, as before).
            let revision = match &inner.health.last {
                Some(Ok(health)) => Some(health.library_revision.clone()),
                _ => None,
            };
            if let Some(revision) = revision {
                if self.needs_full_read(&inner, revision.as_ref(), &titles) {
                    let answer =
                        tokio::time::timeout(REFRESH_BUDGET, self.fetch_setlist(&base, &ask))
                            .await
                            .unwrap_or(Err(Failure::NoAnswer));
                    let good = answer.is_ok();
                    inner.setlist.record(answer, self.now());
                    inner.read = good.then(|| ReadMark {
                        at: self.now(),
                        revision,
                    });
                } else {
                    inner.setlist.touch(self.now());
                }
            }
        }
        let (fetched, stale, when) = match inner.setlist.shown(self.now()) {
            Ok(shown) => shown,
            Err(failure) => return empty(LibraryState::Problem, Some(failure.words())),
        };
        drop(inner);
        let stale = stale || status.stale;
        let checked_at = when.map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true));
        let stale_note = stale.then(|| LOOKED_OLD.to_string());
        let finish =
            |state, message, setlist, newest, songs, playback_only, chosen, setlists| LibraryPlan {
                enabled: true,
                state,
                message,
                plan_date: Some(date.clone()),
                setlist,
                newest,
                songs,
                playback_only,
                chosen,
                setlists,
                stale,
                checked_at: checked_at.clone(),
            };
        match fetched {
            Fetched::Setlist(parsed, others) => {
                let matched = match_setlist(&titles, &parsed.songs);
                finish(
                    LibraryState::Ready,
                    stale_note,
                    Some(parsed.summary()),
                    None,
                    matched.songs,
                    matched.playback_only,
                    chosen.is_some(),
                    others,
                )
            }
            Fetched::NoSetlist { newest, others } => {
                // A chosen setlist that Playback no longer has is forgotten: the plan goes back to its date.
                if chosen.is_some() {
                    self.choose(&plan.id, None);
                }
                let message = match &newest {
                    Some(n) => format!(
                        "Playback has no setlist for {date}; its newest is {} from {}.",
                        n.title,
                        n.date.as_deref().unwrap_or("an unknown date")
                    ),
                    None => format!("Playback has no setlist for {date}."),
                };
                finish(
                    LibraryState::NoSetlist,
                    Some(message),
                    None,
                    newest,
                    vec![],
                    vec![],
                    false,
                    others,
                )
            }
            Fetched::Waiting => finish(
                LibraryState::Waiting,
                Some(NOT_READY.to_string()),
                None,
                None,
                vec![],
                vec![],
                false,
                vec![],
            ),
        }
    }
}

// ---- cover pictures -------------------------------------------------------------------------------

/// `image/jpeg` or `image/png` as the picture's type, whatever the case or parameters.
fn picture_type(value: &str) -> Option<&'static str> {
    let kind = value.split(';').next()?.trim().to_ascii_lowercase();
    match kind.as_str() {
        "image/jpeg" => Some("image/jpeg"),
        "image/png" => Some("image/png"),
        _ => None,
    }
}

/// Whether the first bytes are those of a JPEG or a PNG, and the one the type says.
fn picture_starts_right(content_type: &str, bytes: &[u8]) -> bool {
    match content_type {
        "image/jpeg" => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        "image/png" => bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]),
        _ => false,
    }
}

/// An `ETag` that is safe to pass on: one visible ASCII token, 80 characters at most, no commas.
fn plain_etag(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|b| (0x21..=0x7e).contains(&b) && b != b',')
}

/// What one ask for a picture gave.
enum Raw {
    NotModified,
    Missing,
    Picture(Picture),
}

impl PlaybackLibrary {
    /// `GET base/songs/<id>/artwork`, at most `ARTWORK_CAP` bytes, 3 s in all, same client as every
    /// other ask (no redirect, no proxy). `held` is the `ETag` of the copy held, sent as `If-None-Match`.
    async fn get_picture(
        &self,
        base: &str,
        id: u64,
        held: Option<&str>,
        expected_etag: &str,
    ) -> Result<Raw, ArtworkFailure> {
        let url = format!("{base}/songs/{id}/artwork");
        let mut request = self.signed(
            self.http
                .get(&url)
                .header("accept", "image/jpeg, image/png"),
            &format!("/songs/{id}/artwork"),
        );
        if let Some(etag) = held.filter(|e| plain_etag(e)) {
            request = request.header("if-none-match", etag);
        }
        let mut response = request.send().await.map_err(|_| ArtworkFailure::NoAnswer)?;
        match response.status().as_u16() {
            304 => return Ok(Raw::NotModified),
            404 => return Ok(Raw::Missing),
            200 => {}
            _ => return Err(ArtworkFailure::NoAnswer),
        }
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(picture_type)
            .ok_or(ArtworkFailure::NoPicture)?;
        let etag = response
            .headers()
            .get(header::ETAG)
            .and_then(|v| v.to_str().ok())
            .filter(|e| plain_etag(e))
            .map_or_else(|| expected_etag.to_string(), str::to_string);
        if response
            .content_length()
            .is_some_and(|n| n > ARTWORK_CAP as u64)
        {
            return Err(ArtworkFailure::NoPicture);
        }
        let mut body: Vec<u8> = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| ArtworkFailure::NoAnswer)?
        {
            if body.len() + chunk.len() > ARTWORK_CAP {
                return Err(ArtworkFailure::NoPicture);
            }
            body.extend_from_slice(&chunk);
        }
        if !picture_starts_right(content_type, &body) {
            return Err(ArtworkFailure::NoPicture);
        }
        Ok(Raw::Picture(Picture {
            content_type,
            etag,
            bytes: Bytes::from(body),
        }))
    }

    /// The cover of one song of the current paired plan. `expected` is what the plan says about it
    /// (its `ETag`). A held copy with that `ETag` is given at once for ten minutes; after that, and
    /// when the plan names another `ETag`, the helper is asked (`If-None-Match` with the held one:
    /// a `304` keeps the held bytes). Held in memory only.
    pub async fn picture(
        &self,
        settings: &PlaybackLibrarySettings,
        id: u64,
        expected: &LibraryArtwork,
    ) -> Result<Picture, ArtworkFailure> {
        let base = base_url(&settings.address).ok_or(ArtworkFailure::NoPicture)?;
        let key = (base.clone(), id);
        let held = {
            let art = self.art.lock().map_err(|_| ArtworkFailure::NoAnswer)?;
            art.pictures
                .get(&key)
                .map(|entry| (entry.picture.clone(), entry.checked))
        };
        if let Some((picture, checked)) = &held {
            if picture.etag == expected.etag
                && self.now().saturating_duration_since(*checked) < ARTWORK_RECHECK
            {
                return Ok(picture.clone());
            }
        }
        let _slot = self
            .art_gate
            .acquire()
            .await
            .map_err(|_| ArtworkFailure::NoAnswer)?;
        let held_etag = held.as_ref().map(|(picture, _)| picture.etag.as_str());
        let answer = self.get_picture(&base, id, held_etag, &expected.etag).await;
        let now = self.now();
        let mut art = self.art.lock().map_err(|_| ArtworkFailure::NoAnswer)?;
        match answer? {
            Raw::NotModified => {
                // The helper says the held copy is still the picture: keep its bytes.
                let entry = art
                    .pictures
                    .get_mut(&key)
                    .ok_or(ArtworkFailure::NoPicture)?;
                entry.checked = now;
                Ok(entry.picture.clone())
            }
            Raw::Missing => {
                art.remove(&key);
                Err(ArtworkFailure::NoPicture)
            }
            Raw::Picture(picture) => {
                art.insert(key, picture.clone(), now);
                Ok(picture)
            }
        }
    }
}

// ---- waveforms ------------------------------------------------------------------------------------

/// The helper's waveform answer, checked: `peak` is 1 to 20,000 finite numbers in 0..=1 and `step_s`
/// a finite number above 0 and at most 10. Returns the step and each peak as a whole number 0..=255
/// (the share times 255, rounded). Anything else, and every other field (`mean`, ...), is dropped.
pub fn parse_waveform(value: &Value) -> Option<(f64, Vec<u8>)> {
    let step = value.get("step_s")?.as_f64()?;
    if !step.is_finite() || step <= 0.0 || step > WAVEFORM_MAX_STEP_S {
        return None;
    }
    let peaks = value.get("peak")?.as_array()?;
    if peaks.is_empty() || peaks.len() > WAVEFORM_MAX_POINTS {
        return None;
    }
    let mut peak = Vec::with_capacity(peaks.len());
    for number in peaks {
        let share = number.as_f64()?;
        if !share.is_finite() || !(0.0..=1.0).contains(&share) {
            return None;
        }
        // 0.0..=1.0 times 255, rounded, always fits.
        peak.push((share * 255.0).round() as u8);
    }
    Some((step, peak))
}

impl PlaybackLibrary {
    /// `GET base/songs/<id>/waveform?points_per_second=8&clock=performed`, at most `WAVEFORM_CAP`
    /// bytes, 6 s in all, same client as every other ask (no redirect, no proxy).
    async fn get_waveform(&self, base: &str, id: u64) -> Result<(f64, Vec<u8>), WaveformFailure> {
        let target = format!(
            "/songs/{id}/waveform?points_per_second={WAVEFORM_POINTS_PER_SECOND}&clock=performed"
        );
        let url = format!("{base}{target}");
        let mut response = self
            .signed(
                self.http
                    .get(&url)
                    .header("accept", "application/json")
                    .timeout(WAVEFORM_TIMEOUT),
                &target,
            )
            .send()
            .await
            .map_err(|_| WaveformFailure::NoAnswer)?;
        match response.status().as_u16() {
            200 => {}
            404 => return Err(WaveformFailure::NoWaveform),
            _ => return Err(WaveformFailure::NoAnswer),
        }
        if response
            .content_length()
            .is_some_and(|n| n > WAVEFORM_CAP as u64)
        {
            return Err(WaveformFailure::NoWaveform);
        }
        let mut body: Vec<u8> = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| WaveformFailure::NoAnswer)?
        {
            if body.len() + chunk.len() > WAVEFORM_CAP {
                return Err(WaveformFailure::NoWaveform);
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice::<Value>(&body)
            .ok()
            .and_then(|value| parse_waveform(&value))
            .ok_or(WaveformFailure::NoWaveform)
    }

    /// The `library_revision` of the helper's last good full read (none: it sends none).
    async fn read_revision(&self) -> Option<String> {
        let inner = self.inner.lock().await;
        inner.read.as_ref().and_then(|mark| mark.revision.clone())
    }

    fn held_waveform(&self, key: &(String, u64), revision: Option<&String>) -> Option<Waveform> {
        let waves = self.waves.lock().ok()?;
        let entry = waves.songs.get(key)?;
        let fresh = match (revision, entry.revision.as_ref()) {
            // The helper's database has not changed since this was fetched.
            (Some(now), Some(then)) => now == then,
            // No token to compare: it stands for ten minutes.
            (None, None) => {
                self.now().saturating_duration_since(entry.fetched) < WAVEFORM_NO_REVISION_TTL
            }
            _ => false,
        };
        fresh.then(|| entry.wave.clone())
    }

    /// The waveform of one song of the current paired plan (the route has checked that). Held in
    /// memory by song and by the helper's last good `library_revision`; asked of the helper only
    /// when nothing fresh is held. 16 songs at most, the oldest goes first.
    pub async fn waveform(
        &self,
        settings: &PlaybackLibrarySettings,
        id: u64,
    ) -> Result<Waveform, WaveformFailure> {
        let base = base_url(&settings.address).ok_or(WaveformFailure::NoWaveform)?;
        let key = (base.clone(), id);
        let revision = self.read_revision().await;
        if let Some(wave) = self.held_waveform(&key, revision.as_ref()) {
            return Ok(wave);
        }
        let _slot = self
            .wave_gate
            .acquire()
            .await
            .map_err(|_| WaveformFailure::NoAnswer)?;
        // Another window may have fetched it while this one waited for its turn.
        if let Some(wave) = self.held_waveform(&key, revision.as_ref()) {
            return Ok(wave);
        }
        let (step_s, peak) = self.get_waveform(&base, id).await?;
        let body = Bytes::from(
            serde_json::to_vec(&json!({"step_s": step_s, "peak": peak}))
                .map_err(|_| WaveformFailure::NoWaveform)?,
        );
        let digest = sha2::Sha256::digest(&body);
        let wave = Waveform {
            etag: format!("\"{}\"", hex::encode(&digest[..8])),
            body,
        };
        let fetched = self.now();
        let mut waves = self.waves.lock().map_err(|_| WaveformFailure::NoAnswer)?;
        waves.next += 1;
        let seq = waves.next;
        waves.songs.insert(
            key,
            WaveEntry {
                wave: wave.clone(),
                revision,
                fetched,
                seq,
            },
        );
        while waves.songs.len() > WAVEFORM_MAX_SONGS {
            let Some(oldest) = waves
                .songs
                .iter()
                .min_by_key(|(_, entry)| entry.seq)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            waves.songs.remove(&oldest);
        }
        Ok(wave)
    }
}

// ---- routes -----------------------------------------------------------------------------------

type Rt = Arc<Runtime>;

#[derive(serde::Deserialize, Default)]
struct Ask {
    /// The Check button: ask again after 2 s instead of 30 s.
    check: Option<String>,
}

impl Ask {
    fn check(&self) -> bool {
        matches!(self.check.as_deref(), Some("1" | "true"))
    }
}

pub fn routes() -> Router<Rt> {
    Router::new()
        .route("/api/v1/playback-library/status", get(status))
        .route("/api/v1/playback-library/plan", get(plan))
        .route(
            "/api/v1/playback-library/songs/{setlist_song_id}/artwork",
            get(artwork),
        )
        .route(
            "/api/v1/playback-library/songs/{setlist_song_id}/waveform",
            get(waveform),
        )
        .route("/api/v1/playback-library/setlist", put(choose_setlist))
        .route("/api/v1/playback-library/settings", put(settings))
}

fn to_json<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// The status as the window gets it: the helper's part also says what StagePilot is doing about an older Library Link.
fn status_json(rt: &Runtime, mut status: LibraryStatus) -> Value {
    status.helper.update = rt.helper_updater.playback_view(&status.helper);
    to_json(&status)
}

/// The settings the library works from. The helper is on the Playback computer, so unless the operator
/// typed another address under Advanced (the saved override) it is the saved Playback host, at the
/// helper's own port: when the Playback API works on a computer, so does the library.
pub fn effective_settings(rt: &Runtime) -> PlaybackLibrarySettings {
    settings_from(&rt.settings_service)
}

/// The same from the settings service alone (the automatic update reads it without a `Runtime`).
pub fn settings_from(settings_service: &SettingsService) -> PlaybackLibrarySettings {
    let saved = settings_service.snapshot();
    let mut library = saved.playback_library;
    if library.address.is_empty() {
        if let Some(host) = saved
            .playback_api
            .host
            .as_deref()
            .map(str::trim)
            .filter(|host| !host.is_empty())
        {
            if let Ok(clean) = clean_playback_library_address(host) {
                library.address = clean;
            }
        }
    }
    library
}

/// The first time the helper answers with the key (or answers at all, as an older one) the setting is saved as
/// "has been in use" (`enabled: true`): that is what lets a later silence be told apart from a computer that
/// never had the Library Link. It is the only thing a status check ever writes.
pub async fn note_in_use(rt: &Runtime, active: bool) {
    if !active || rt.settings_service.snapshot().playback_library.enabled == Some(true) {
        return;
    }
    let _serialised = rt.reconcile.lock().await;
    let mut next = rt.settings_service.snapshot();
    if next.playback_library.enabled == Some(true) {
        return;
    }
    next.playback_library.enabled = Some(true);
    if let Err(error) = rt.settings_service.save(next) {
        tracing::warn!(error = %error.0, "playback_library_in_use_not_saved");
    }
}

async fn status(State(rt): State<Rt>, Query(ask): Query<Ask>) -> Json {
    let settings = effective_settings(&rt);
    let status = rt.playback_library.status(&settings, ask.check()).await;
    note_in_use(&rt, status.active).await;
    ok(status_json(&rt, status))
}

async fn plan(State(rt): State<Rt>, Query(ask): Query<Ask>) -> Json {
    let settings = effective_settings(&rt);
    let loaded = rt.store.snapshot().plan;
    let plan = rt
        .playback_library
        .plan(&settings, loaded.as_ref(), ask.check())
        .await;
    note_in_use(&rt, plan.state == LibraryState::Ready && plan.enabled).await;
    ok(to_json(&plan))
}

/// `PUT /api/v1/playback-library/setlist` `{setlist_id}`: which Playback setlist the loaded plan is matched with, when
/// none is on its date. `null` goes back to the setlist on the plan's date. It is remembered in memory for this plan
/// only (never saved, never sent to Playback or Planning Center) and answers with the plan read again.
async fn choose_setlist(State(rt): State<Rt>, body: Bytes) -> Json {
    let value: Option<Value> = serde_json::from_slice(&body).ok();
    let setlist_id = match value.as_ref().and_then(|v| v.get("setlist_id")) {
        Some(Value::Null) => None,
        Some(Value::Number(n)) => match n.as_i64().filter(|n| *n >= 0) {
            Some(id) => Some(id),
            None => return http_error(422, "setlist_id must be a whole number."),
        },
        _ => {
            return http_error(
                422,
                "Send {\"setlist_id\": the number of a Playback setlist, or null}.",
            )
        }
    };
    let loaded = rt.store.snapshot().plan;
    let Some(plan) = loaded.as_ref() else {
        return http_error(409, "No service plan is loaded.");
    };
    rt.playback_library.choose(&plan.id, setlist_id);
    let settings = effective_settings(&rt);
    let answer = rt.playback_library.plan(&settings, Some(plan), true).await;
    note_in_use(&rt, answer.state == LibraryState::Ready && answer.enabled).await;
    ok(to_json(&answer))
}

/// `PUT /api/v1/playback-library/settings` `{address}`: the optional override of the helper's computer
/// (empty: the saved Playback host). It is saved on its own; nothing else in the settings, and nothing
/// running, is touched. The old `enabled` switch is gone; it is not read here.
async fn settings(State(rt): State<Rt>, body: Bytes) -> Json {
    let value: Option<Value> = serde_json::from_slice(&body).ok();
    let Some(Value::Object(map)) = value else {
        return http_error(422, "Send {\"address\": \"host or host:port\"}.");
    };
    let bad = |field: &str, message: &str, input: &Value| {
        unprocessable(vec![json!({
            "type": "value_error", "loc": ["body", field], "msg": message, "input": input
        })])
    };
    let current = rt.settings_service.snapshot().playback_library;
    let address = match map.get("address") {
        None => current.address.clone(),
        Some(Value::String(text)) if text.chars().count() <= 300 => {
            match clean_playback_library_address(text) {
                Ok(clean) => clean,
                Err(message) => return bad("address", message, &json!(text)),
            }
        }
        Some(other) => return bad("address", "Enter the address as text.", other),
    };
    {
        let _serialised = rt.reconcile.lock().await;
        let mut next = rt.settings_service.snapshot();
        next.playback_library = PlaybackLibrarySettings {
            enabled: next.playback_library.enabled,
            address,
        };
        if let Err(error) = rt.settings_service.save(next) {
            return http_error(503, &error.0);
        }
    }
    // Another address is another helper: nothing held for the old one stands.
    rt.playback_library.reset().await;
    let settings = effective_settings(&rt);
    let status = rt.playback_library.status(&settings, false).await;
    ok(status_json(&rt, status))
}

/// The cover of one song of the current paired plan, for the window. The window never calls the
/// helper: this is the only way a picture reaches it. 404 for everything that is not exactly that:
/// the setting off (nothing is asked at all), an id that is not a whole number, an id that is not a
/// paired song of the plan on screen, a song with no picture, a helper that sends something that is
/// not a picture.
async fn artwork(
    State(rt): State<Rt>,
    Path(raw): Path<String>,
    request_headers: HeaderMap,
) -> Response {
    let no_picture = || http_error(404, "No picture.").into_response();
    let settings = effective_settings(&rt);
    let Ok(id) = raw.parse::<u64>() else {
        return no_picture();
    };
    let loaded = rt.store.snapshot().plan;
    let current = rt
        .playback_library
        .plan(&settings, loaded.as_ref(), false)
        .await;
    let expected = (current.state == LibraryState::Ready)
        .then(|| {
            current.songs.iter().find_map(|song| {
                let library = song.library.as_ref()?;
                let same = library.setlist_song_id.and_then(|n| u64::try_from(n).ok()) == Some(id);
                (song.status == PlanMatch::Matched && same)
                    .then(|| library.artwork.clone())
                    .flatten()
            })
        })
        .flatten();
    let Some(expected) = expected else {
        return no_picture();
    };
    // The window already holds the picture the plan describes: nothing is fetched.
    if etag_matches(request_headers.get(header::IF_NONE_MATCH), &expected.etag) {
        return picture_response(None, &expected.etag, &expected.content_type);
    }
    match rt.playback_library.picture(&settings, id, &expected).await {
        Ok(picture) => {
            if etag_matches(request_headers.get(header::IF_NONE_MATCH), &picture.etag) {
                return picture_response(None, &picture.etag, picture.content_type);
            }
            picture_response(
                Some(picture.bytes.clone()),
                &picture.etag,
                picture.content_type,
            )
        }
        Err(ArtworkFailure::NoPicture) => no_picture(),
        Err(ArtworkFailure::NoAnswer) => http_error(503, DID_NOT_ANSWER).into_response(),
    }
}

/// The waveform of one song of the current paired plan, for the window (it never calls the helper).
/// 404 for everything that is not exactly that: the setting off (nothing is asked at all), an id that
/// is not a whole number, an id that is not a paired song of the plan on screen, a song with no
/// waveform, a helper that sends something that is not one. 503 when the helper does not answer.
async fn waveform(
    State(rt): State<Rt>,
    Path(raw): Path<String>,
    request_headers: HeaderMap,
) -> Response {
    let none = || http_error(404, "No waveform.").into_response();
    let settings = effective_settings(&rt);
    let Ok(id) = raw.parse::<u64>() else {
        return none();
    };
    let loaded = rt.store.snapshot().plan;
    let current = rt
        .playback_library
        .plan(&settings, loaded.as_ref(), false)
        .await;
    let paired = current.state == LibraryState::Ready
        && current.songs.iter().any(|song| {
            song.status == PlanMatch::Matched
                && song
                    .library
                    .as_ref()
                    .and_then(|library| library.setlist_song_id)
                    .and_then(|n| u64::try_from(n).ok())
                    == Some(id)
        });
    if !paired {
        return none();
    }
    match rt.playback_library.waveform(&settings, id).await {
        Ok(wave) => {
            let not_modified = etag_matches(request_headers.get(header::IF_NONE_MATCH), &wave.etag);
            let mut response = if not_modified {
                StatusCode::NOT_MODIFIED.into_response()
            } else {
                (StatusCode::OK, wave.body.clone()).into_response()
            };
            let headers = response.headers_mut();
            for (name, value) in [
                (header::ETAG, wave.etag.as_str()),
                (header::CACHE_CONTROL, "private, max-age=300"),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            ] {
                if let Ok(value) = HeaderValue::from_str(value) {
                    headers.insert(name, value);
                }
            }
            if !not_modified {
                headers.insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                );
            }
            response
        }
        Err(WaveformFailure::NoWaveform) => none(),
        Err(WaveformFailure::NoAnswer) => http_error(503, DID_NOT_ANSWER).into_response(),
    }
}

/// `If-None-Match` names this `ETag` (any of a list, weak or not, or `*`).
fn etag_matches(header: Option<&HeaderValue>, etag: &str) -> bool {
    let Some(text) = header.and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let bare = |tag: &str| tag.trim().trim_start_matches("W/").to_string();
    text.split(',')
        .any(|tag| tag.trim() == "*" || bare(tag) == bare(etag))
}

/// 200 with the picture, or 304 with none, both with the headers the window may cache by.
fn picture_response(bytes: Option<Bytes>, etag: &str, content_type: &str) -> Response {
    let mut response = match bytes {
        Some(bytes) => (StatusCode::OK, bytes).into_response(),
        None => StatusCode::NOT_MODIFIED.into_response(),
    };
    let headers = response.headers_mut();
    for (name, value) in [
        (header::ETAG, etag),
        (header::CONTENT_TYPE, content_type),
        (header::CACHE_CONTROL, "private, max-age=3600"),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
    ] {
        if let Ok(value) = HeaderValue::from_str(value) {
            headers.insert(name, value);
        }
    }
    response
}

#[cfg(test)]
mod key_tests {
    use super::*;

    #[test]
    fn hmac_sha256_matches_the_published_test_vector() {
        // RFC 4231, test case 2.
        assert_eq!(
            hmac_sha256_hex(b"Jefe", b"what do ya want for nothing?"),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn a_signature_is_the_hmac_of_get_path_and_time_as_the_helper_computes_it() {
        // Both values were made with Python's `hmac` on the same text (the helper's own `sign`).
        let key = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-_AbCdE";
        assert_eq!(
            sign(
                key,
                "/songs/12/waveform?points_per_second=8&clock=performed",
                1_794_945_365
            ),
            "aa6551452e98e1ebaf753d4130903855264db6eeb177acea6c8cfc22566ac4ae"
        );
        assert_eq!(
            sign(key, "/health", 1_794_945_365),
            "8fbb87bdcfd4cfd1455b54d61dd18ff37cdc4d2278f5a7c760609f132b8b4677"
        );
    }

    #[test]
    fn base64url_is_the_url_safe_alphabet_without_padding() {
        for (bytes, text) in [
            (&b""[..], ""),
            (b"f", "Zg"),
            (b"fo", "Zm8"),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg"),
            (b"fooba", "Zm9vYmE"),
            (b"foobar", "Zm9vYmFy"),
            (&[0xfb, 0xff][..], "-_8"),
        ] {
            assert_eq!(base64url(bytes), text);
        }
    }

    #[test]
    fn a_new_key_is_32_random_bytes_as_43_url_safe_characters_and_never_the_same_twice() {
        let a = new_key().unwrap();
        let b = new_key().unwrap();
        assert_ne!(a, b);
        for key in [&a, &b] {
            assert_eq!(key.len(), 43);
            assert!(key
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'));
            assert!(usable_key(key));
        }
        assert!(!usable_key("short"));
        assert!(
            !usable_key(&format!("{a} ")),
            "a space is not a key character"
        );
    }

    #[test]
    fn the_key_is_made_once_and_kept() {
        let library = PlaybackLibrary::new();
        assert!(!library.is_paired());
        assert_eq!(library.ensure_key(), Ok(true), "the first session makes it");
        let first = library.key_for_installer().unwrap();
        assert_eq!(library.ensure_key(), Ok(false), "the next one keeps it");
        assert_eq!(library.key_for_installer().unwrap(), first);
        assert_eq!(
            library.ensure_key(),
            Ok(false),
            "running the setup command again reuses the key"
        );
        assert_eq!(library.key_for_installer().unwrap(), first);
        assert!(library.is_paired());
    }

    #[test]
    fn a_stored_value_that_is_not_a_key_is_not_used_and_not_deleted() {
        let library = PlaybackLibrary::new();
        library.set_key_for_test("too short");
        assert!(!library.is_paired());
        assert_eq!(
            library.keys.get_secret().unwrap().as_deref(),
            Some("too short")
        );
    }
}
