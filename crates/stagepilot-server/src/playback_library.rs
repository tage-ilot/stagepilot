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
//! Rules (all kept by `Library`):
//! * Off means off: with the setting off nothing is asked, ever, and nothing runs in the background.
//! * Nothing is polled. A window asking (`GET /status`, `GET /plan`) is the only thing that causes a
//!   request, and the helper is asked at most once per 30 s however many windows ask; the Check
//!   button (`?check=1`) may ask again after 2 s. Failures are remembered for the same time, so a
//!   computer that does not answer is not asked again and again.
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
//! * Every failure is one short plain sentence.
//! * Library data is held in memory only.

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
use serde_json::{json, Value};
use stagepilot_core::config::{
    clean_playback_library_address, PlaybackLibrarySettings, DEFAULT_PLAYBACK_LIBRARY_PORT,
};
use stagepilot_core::state::ServicePlan;
use stagepilot_playback::library::{
    apply_detail, health_words, match_plan, parse_health, parse_setlist, LibraryArtwork,
    LibraryPlan, LibrarySection, LibrarySetlist, LibraryState, LibraryStatus, ParsedSetlist,
    PlanMatch, PlanTitle, MAX_ARTWORK_BYTES,
};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
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

const DID_NOT_ANSWER: &str = "The Playback computer did not answer.";
const NOT_UNDERSTOOD: &str =
    "The Playback library answered, but not in a way StagePilot understands.";
const NO_ADDRESS: &str = "Enter the address of the Playback computer.";
const SWITCHED_OFF: &str = "The Playback library is off.";
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

/// What one ask of `/setlists/<date>` gave.
#[derive(Debug, Clone)]
enum Fetched {
    Setlist(ParsedSetlist),
    /// Playback has no setlist for the date; `newest` is the one it has most recently.
    NoSetlist {
        newest: Option<LibrarySetlist>,
    },
    /// The helper is up and Playback's library cannot be read yet.
    Waiting,
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
    setlist_date: String,
    setlist: Slot<Fetched>,
    /// When the setlist and its songs were last read in full, and the `library_revision` the helper
    /// had then (none when it sends none).
    read: Option<ReadMark>,
    /// The loaded plan as the pairing sees it (date and titles): a change means a full read.
    plan_key: String,
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

pub struct PlaybackLibrary {
    http: reqwest::Client,
    inner: Mutex<Inner>,
    /// Cover pictures, in memory only.
    art: std::sync::Mutex<ArtCache>,
    art_gate: tokio::sync::Semaphore,
    /// Added to the clock (tests move it to pass minutes at once; it is 0 in the product).
    skew_ms: AtomicU64,
}

impl Default for PlaybackLibrary {
    fn default() -> Self {
        Self::new()
    }
}

impl PlaybackLibrary {
    pub fn new() -> Self {
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
            skew_ms: AtomicU64::new(0),
        }
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

    /// The setting is off: forget everything held (so switching it on starts from nothing).
    fn forget(&self) {
        if let Ok(mut inner) = self.inner.try_lock() {
            if !inner.base.is_empty() {
                *inner = Inner::default();
            }
        }
        if let Ok(mut art) = self.art.lock() {
            *art = ArtCache::default();
        }
    }

    /// `GET base+path`, JSON, at most `cap` bytes, 3 s in all. Only a 200 is an answer.
    async fn get(&self, base: &str, path: &str, cap: usize) -> Result<(u16, Value), Failure> {
        let url = format!("{base}{path}");
        let mut response = self
            .http
            .get(&url)
            .header("accept", "application/json")
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

    fn off(settings: &PlaybackLibrarySettings) -> LibraryStatus {
        LibraryStatus {
            enabled: false,
            address: settings.address.clone(),
            state: LibraryState::Off,
            message: SWITCHED_OFF.to_string(),
            version: None,
            songs: None,
            setlists: None,
            stale: false,
            checked_at: None,
        }
    }

    fn problem(settings: &PlaybackLibrarySettings, message: &str) -> LibraryStatus {
        LibraryStatus {
            enabled: true,
            state: LibraryState::Problem,
            message: message.to_string(),
            ..Self::off(settings)
        }
    }

    /// Where the helper stands, from `/health`. With the setting off nothing is asked. `check` is
    /// the Check button: it may ask again 2 s after the last ask instead of 30 s.
    pub async fn status(&self, settings: &PlaybackLibrarySettings, check: bool) -> LibraryStatus {
        if !settings.enabled {
            self.forget();
            return Self::off(settings);
        }
        let Some(base) = base_url(&settings.address) else {
            return Self::problem(settings, NO_ADDRESS);
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
        let gap = if check { MIN_CHECK } else { MIN_REFRESH };
        if inner.health.due(gap, self.now()) {
            let answer = self.get_ok(base, "/health", HEALTH_CAP).await;
            // A helper that answers 503 to /health is told apart from one that is silent.
            inner
                .health
                .record(answer.map(|value| parse_health(&value)), self.now());
        }
        match inner.health.shown(self.now()) {
            Ok((health, stale, when)) => {
                let (state, mut message) = health_words(&health);
                if stale {
                    message = format!("{LOOKED_OLD} {message}");
                }
                LibraryStatus {
                    enabled: true,
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
            Err(failure) => Self::problem(settings, &failure.words()),
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

    /// Reads the setlist for `date`, then each song's own answer for its section count.
    async fn fetch_setlist(&self, base: &str, date: &str) -> Result<Fetched, Failure> {
        let (status, value) = self
            .get(base, &format!("/setlists/{date}"), BODY_CAP)
            .await?;
        match status {
            200 => {
                let mut parsed = parse_setlist(&value).ok_or(Failure::Unreadable)?;
                if parsed.date.as_deref().is_some_and(|d| d != date) {
                    // Whatever the helper meant, it is not the setlist of this day.
                    return Ok(Fetched::NoSetlist {
                        newest: Some(parsed.summary()),
                    });
                }
                self.read_song_details(base, &mut parsed).await;
                Ok(Fetched::Setlist(parsed))
            }
            404 => {
                let newest = match self.get(base, "/setlists/latest", BODY_CAP).await {
                    Ok((200, value)) => parse_setlist(&value).map(|p| p.summary()),
                    _ => None,
                };
                Ok(Fetched::NoSetlist { newest })
            }
            503 => Ok(Fetched::Waiting),
            other => Err(Failure::Status(other)),
        }
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
            Fetched::Setlist(parsed) => {
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

    /// The loaded plan paired with Playback's setlist for the plan's date.
    pub async fn plan(
        &self,
        settings: &PlaybackLibrarySettings,
        plan: Option<&ServicePlan>,
        check: bool,
    ) -> LibraryPlan {
        let date = plan.map(|p| p.date.format("%Y-%m-%d").to_string());
        let empty = |state: LibraryState, message: Option<String>| LibraryPlan {
            enabled: settings.enabled,
            state,
            message,
            plan_date: date.clone(),
            setlist: None,
            newest: None,
            songs: vec![],
            stale: false,
            checked_at: None,
        };
        if !settings.enabled {
            self.forget();
            return empty(LibraryState::Off, Some(SWITCHED_OFF.to_string()));
        }
        let Some(base) = base_url(&settings.address) else {
            return empty(LibraryState::Problem, Some(NO_ADDRESS.to_string()));
        };
        let (Some(plan), Some(date)) = (plan, date.clone()) else {
            return empty(LibraryState::Ready, None);
        };
        if plan.songs.is_empty() {
            return empty(LibraryState::Ready, None);
        }
        let titles: Vec<PlanTitle> = plan
            .songs
            .iter()
            .map(|song| PlanTitle {
                key: song
                    .source_song_id
                    .clone()
                    .unwrap_or_else(|| song.id.clone()),
                title: song.title.clone(),
                seconds: song.duration_seconds,
            })
            .collect();
        // One ask at a time: a second window waits here and then reads what the first one learned.
        let mut inner = self.inner.lock().await;
        let status = self.status_locked(&mut inner, settings, &base, check).await;
        if status.state != LibraryState::Ready {
            return empty(status.state, Some(status.message));
        }
        if inner.setlist_date != date {
            inner.setlist_date = date.clone();
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
                        tokio::time::timeout(REFRESH_BUDGET, self.fetch_setlist(&base, &date))
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
        let finish = |state, message, setlist, newest, songs| LibraryPlan {
            enabled: true,
            state,
            message,
            plan_date: Some(date.clone()),
            setlist,
            newest,
            songs,
            stale,
            checked_at: checked_at.clone(),
        };
        match fetched {
            Fetched::Setlist(parsed) => finish(
                LibraryState::Ready,
                stale_note,
                Some(parsed.summary()),
                None,
                match_plan(&titles, &parsed.songs),
            ),
            Fetched::NoSetlist { newest } => {
                let message = match &newest {
                    Some(n) => format!(
                        "Playback has no setlist for {date}; its newest is {} from {}.",
                        n.title,
                        n.date.as_deref().unwrap_or("an unknown date")
                    ),
                    None => format!("Playback has no setlist for {date}."),
                };
                finish(LibraryState::NoSetlist, Some(message), None, newest, vec![])
            }
            Fetched::Waiting => finish(
                LibraryState::Waiting,
                Some(NOT_READY.to_string()),
                None,
                None,
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
        let mut request = self
            .http
            .get(&url)
            .header("accept", "image/jpeg, image/png");
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
        if !settings.enabled {
            return Err(ArtworkFailure::NoPicture);
        }
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
        .route("/api/v1/playback-library/settings", put(settings))
}

fn to_json<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

async fn status(State(rt): State<Rt>, Query(ask): Query<Ask>) -> Json {
    let settings = rt.settings_service.snapshot().playback_library;
    ok(to_json(
        &rt.playback_library.status(&settings, ask.check()).await,
    ))
}

async fn plan(State(rt): State<Rt>, Query(ask): Query<Ask>) -> Json {
    let settings = rt.settings_service.snapshot().playback_library;
    let loaded = rt.store.snapshot().plan;
    ok(to_json(
        &rt.playback_library
            .plan(&settings, loaded.as_ref(), ask.check())
            .await,
    ))
}

/// `PUT /api/v1/playback-library/settings` `{enabled, address}`: the one setting. It is saved on
/// its own; nothing else in the settings, and nothing running, is touched.
async fn settings(State(rt): State<Rt>, body: Bytes) -> Json {
    let value: Option<Value> = serde_json::from_slice(&body).ok();
    let Some(Value::Object(map)) = value else {
        return http_error(
            422,
            "Send {\"enabled\": true or false, \"address\": \"host or host:port\"}.",
        );
    };
    let bad = |field: &str, message: &str, input: &Value| {
        unprocessable(vec![json!({
            "type": "value_error", "loc": ["body", field], "msg": message, "input": input
        })])
    };
    let current = rt.settings_service.snapshot().playback_library;
    let enabled = match map.get("enabled") {
        None => current.enabled,
        Some(Value::Bool(b)) => *b,
        Some(other) => return bad("enabled", "Input should be true or false", other),
    };
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
    let _serialised = rt.reconcile.lock().await;
    let mut next = rt.settings_service.snapshot();
    next.playback_library = PlaybackLibrarySettings { enabled, address };
    if let Err(error) = rt.settings_service.save(next.clone()) {
        return http_error(503, &error.0);
    }
    ok(to_json(
        &rt.playback_library
            .status(&next.playback_library, false)
            .await,
    ))
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
    let settings = rt.settings_service.snapshot().playback_library;
    if !settings.enabled {
        return no_picture();
    }
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
