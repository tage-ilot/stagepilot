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
//! * Every failure is one short plain sentence.
//! * Library data is held in memory only.

use crate::runtime::Runtime;
use crate::validation::{http_error, ok, unprocessable, Json};
use axum::body::Bytes;
use axum::extract::{Query, State};
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
    apply_detail, health_words, match_plan, parse_health, parse_setlist, LibraryPlan,
    LibrarySetlist, LibraryState, LibraryStatus, ParsedSetlist, PlanTitle,
};
use std::collections::HashMap;
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
    fn due(&self, gap: Duration) -> bool {
        self.asked.is_none_or(|at| at.elapsed() >= gap)
    }

    fn record(&mut self, result: Result<T, Failure>) {
        self.asked = Some(Instant::now());
        if let Ok(value) = &result {
            self.good = Some((Instant::now(), Utc::now(), value.clone()));
        }
        self.last = Some(result);
    }

    /// The answer to show: the last ask's, or the last good one (marked old, under `KEEP_GOOD`).
    fn shown(&self) -> Result<(T, bool, Option<DateTime<Utc>>), Failure> {
        match &self.last {
            Some(Ok(value)) => Ok((value.clone(), false, self.good.as_ref().map(|g| g.1))),
            Some(Err(failure)) => match &self.good {
                Some((at, when, value)) if at.elapsed() < KEEP_GOOD => {
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
}

#[derive(Clone)]
struct Detail {
    sections: Option<u32>,
    arrangement: Option<String>,
}

pub struct PlaybackLibrary {
    http: reqwest::Client,
    inner: Mutex<Inner>,
    /// What each song's own answer gave, for `KEEP_GOOD` (key: base URL and Playback's song id).
    details: std::sync::Mutex<HashMap<(String, i64), (Instant, Detail)>>,
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
            details: std::sync::Mutex::new(HashMap::new()),
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
        if inner.health.due(gap) {
            let answer = self.get_ok(base, "/health", HEALTH_CAP).await;
            // A helper that answers 503 to /health is told apart from one that is silent.
            inner
                .health
                .record(answer.map(|value| parse_health(&value)));
        }
        match inner.health.shown() {
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

    /// One song's section count and arrangement name (cached for `KEEP_GOOD`).
    async fn song_detail(&self, base: &str, id: i64) -> Option<Detail> {
        let key = (base.to_string(), id);
        if let Ok(cache) = self.details.lock() {
            if let Some((at, detail)) = cache.get(&key) {
                if at.elapsed() < KEEP_GOOD {
                    return Some(detail.clone());
                }
            }
        }
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
            sections: None,
            rental_expires_utc: None,
        };
        apply_detail(&mut scratch, &value);
        let detail = Detail {
            sections: scratch.sections,
            arrangement: scratch.arrangement,
        };
        if let Ok(mut cache) = self.details.lock() {
            if cache.len() > 500 {
                cache.clear();
            }
            cache.insert(key, (Instant::now(), detail.clone()));
        }
        Some(detail)
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
                song.sections = detail.sections;
                if song.arrangement.is_none() {
                    song.arrangement = detail.arrangement;
                }
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
        // One ask at a time: a second window waits here and then reads what the first one learned.
        let mut inner = self.inner.lock().await;
        let status = self.status_locked(&mut inner, settings, &base, check).await;
        if status.state != LibraryState::Ready {
            return empty(status.state, Some(status.message));
        }
        if inner.setlist_date != date {
            inner.setlist_date = date.clone();
            inner.setlist = Slot::default();
        }
        let gap = if check { MIN_CHECK } else { MIN_REFRESH };
        if inner.setlist.due(gap) {
            let answer = tokio::time::timeout(REFRESH_BUDGET, self.fetch_setlist(&base, &date))
                .await
                .unwrap_or(Err(Failure::NoAnswer));
            inner.setlist.record(answer);
        }
        let (fetched, stale, when) = match inner.setlist.shown() {
            Ok(shown) => shown,
            Err(failure) => return empty(LibraryState::Problem, Some(failure.words())),
        };
        drop(inner);
        let checked_at = when.map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true));
        let stale_note = stale.then(|| LOOKED_OLD.to_string());
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
