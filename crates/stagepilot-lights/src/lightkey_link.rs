//! Lightkey link: connect to the `lightkey-api` agent and record fixture changes against a song.
//!
//! Owned by `LightsPlugin` (started and stopped with it) so it adds no new plugin or setting page.
//! Fixture changes arrive from the agent's OSC relay (`crate::lightkey_relay`), timestamped from
//! Lightkey's own DMX output, converted to this machine's clock with a measured offset and then to
//! song time. The agent's HTTP `/log` is still polled, for link status only: cue changes it
//! reports never reach a take or a replay. The pure rules live in `crate::lightkey` and
//! `crate::lightkey_fixtures`; this file is the thin async shell around them. The clock and the
//! HTTP client are injected (`Clock`, `LightkeyHttp`), so everything is testable offline.
//!
//! Replay is not done here: the song-lighting timeline in `crate::plugin` is the one replay
//! engine. This link keeps the recorder, the relay client and the status the Lighting panel shows.

use crate::audio::{AudioHub, AudioInput, SharedRecorder};
use crate::lightkey::{
    clean_project, effective_api_url, fixtures_word, helper_is_outdated,
    recording_is_worth_keeping, LightkeyAuto, LightkeyCaptured, LightkeyHelper,
    LightkeyLink as Link, LightkeyReadFixtures, LightkeySaving, LightkeyStatus,
};
use crate::lightkey_auto::{
    decide, judge, Action, Availability, Coverage, Decision, PassVerdict, Reason, SongCapture,
};
use crate::lightkey_fit::{fit_within_caps, trim_to_caps, Caps, Fitted, Stage};
use crate::lightkey_fixtures::{FixtureCommand, FixtureState};
use crate::lightkey_looks::LookRecorder;
use crate::lightkey_recording::{self, RawRecordingView};
use crate::lightkey_relay::{relay_target, RelayClient};
use crate::lightkey_smooth::smooth_rows;
use crate::lightkey_start;
use crate::loudness::LoudnessRecorder;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use stagepilot_core::bus::{EventBus, Subscription};
use stagepilot_core::config::{
    CaptureSimplified, CaptureStatus, LightingEffect, LightkeyMode, LightkeySettings,
    OscLightingCue, SongLoudness, DEFAULT_LIGHTKEY_RELAY_PORT,
};
use stagepilot_core::events::{EventPayload, EventType, StagePilotEvent, TimerPayload};
use stagepilot_core::state::Song;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

/// Closest spacing of two OSC packets: 5 ms = at most 200 packets per second.
/// A stop within this many seconds of the song's end is the song ending, not a pause.
/// How long a song-start handler waits for the audio input to open before it carries on.
const AUDIO_OPEN_WAIT: Duration = Duration::from_millis(1500);
const SONG_END_TOLERANCE_SECONDS: f64 = 2.0;
/// A recording finishes by itself this long after the song's scheduled length (the timer keeps
/// counting into overrun, so there is no stop). The grace keeps a fade-out after the last cue.
const SONG_END_GRACE_SECONDS: f64 = 5.0;
/// How long a shutdown waits for an unfinished recording to be saved before it gives up.
const SHUTDOWN_SAVE_WAIT: Duration = Duration::from_secs(3);
pub const MIN_SEND_GAP: Duration = Duration::from_millis(5);
pub const POLL_SECONDS: f64 = 0.25;
/// How often the helper's `GET /health` is read (and at once on every connect).
pub const HEALTH_SECONDS: f64 = 30.0;
/// `POST /fixtures/refresh` re-parses a Lightkey project, which can take a moment.
pub const REFRESH_TIMEOUT_SECONDS: f64 = 20.0;
pub const RETRY_SECONDS: f64 = 2.0;
pub const OFFSET_SAMPLES: usize = 7;
pub const OFFSET_REFRESH_SECONDS: f64 = 120.0;
pub const HTTP_TIMEOUT_SECONDS: f64 = 2.0;
/// Seconds either side of a resume that still counts as "the same place in the song".
pub const RESUME_TOLERANCE_SECONDS: f64 = 2.0;

// ---------------------------------------------------------------------------------- injection

/// The clock the link reads. `wall` is Unix seconds (what `time.time()` is), `mono` a monotonic
/// reading in seconds (`time.monotonic()`).
pub trait Clock: Send + Sync {
    fn wall(&self) -> f64;
    fn mono(&self) -> f64;
}

#[derive(Debug, Clone, Copy)]
pub struct SystemClock {
    started: std::time::Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self {
            started: std::time::Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn wall(&self) -> f64 {
        let now = Utc::now();
        now.timestamp() as f64 + f64::from(now.timestamp_subsec_micros()) / 1e6
    }
    fn mono(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }
}

fn datetime_seconds(value: DateTime<Utc>) -> f64 {
    value.timestamp() as f64 + f64::from(value.timestamp_subsec_micros()) / 1e6
}

/// Why a GET failed (the three outcomes the link distinguishes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpFailure {
    /// The agent answered with this non-success status.
    Status(u16),
    Timeout,
    /// Connect error, bad body, anything else.
    Other(String),
}

/// A GET against the Lightkey agent returning the parsed JSON body of a 2xx answer.
#[async_trait]
pub trait LightkeyHttp: Send + Sync {
    async fn get_json(&self, url: &str, query: &[(&str, String)]) -> Result<Value, HttpFailure>;

    /// POST with an empty body. Unlike `get_json` a non-2xx answer is not an error: the helper
    /// explains itself in the body (`POST /fixtures/refresh` answers 403, 409 and 422 with a
    /// code). Only a connection failure or a timeout is `Err`. The default is for fakes that
    /// have no POST.
    async fn post_json(&self, _url: &str) -> Result<PostAnswer, HttpFailure> {
        Err(HttpFailure::Other("POST is not supported here".into()))
    }
}

/// The answer to a POST: the HTTP status and the parsed JSON body, whatever the status.
pub type PostAnswer = (u16, Value);

pub type HttpFactory = Arc<dyn Fn() -> Arc<dyn LightkeyHttp> + Send + Sync>;

/// `reqwest` implementation: plain HTTP on the local network, 2 s connect and read timeouts
/// (httpx's per-phase timeout), no redirects, no proxy.
pub struct ReqwestLightkeyHttp {
    http: reqwest::Client,
}

impl ReqwestLightkeyHttp {
    pub fn new() -> Self {
        // Cargo unifies reqwest features across the workspace: a TLS provider must be installed
        // before any Client is built (see the ProPresenter client).
        let _ = rustls::crypto::ring::default_provider().install_default();
        let timeout = Duration::from_secs_f64(HTTP_TIMEOUT_SECONDS);
        let http = reqwest::Client::builder()
            .connect_timeout(timeout)
            .read_timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .unwrap_or_default();
        Self { http }
    }
}

impl Default for ReqwestLightkeyHttp {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LightkeyHttp for ReqwestLightkeyHttp {
    async fn get_json(&self, url: &str, query: &[(&str, String)]) -> Result<Value, HttpFailure> {
        // Query values are numbers here, so no percent-encoding is needed.
        let mut full = url.to_string();
        for (i, (key, value)) in query.iter().enumerate() {
            full.push(if i == 0 { '?' } else { '&' });
            full.push_str(&format!("{key}={value}"));
        }
        let response = self.http.get(&full).send().await.map_err(|e| {
            if e.is_timeout() {
                HttpFailure::Timeout
            } else {
                HttpFailure::Other(e.to_string())
            }
        })?;
        let status = response.status();
        if !status.is_success() {
            return Err(HttpFailure::Status(status.as_u16()));
        }
        let bytes = response.bytes().await.map_err(|e| {
            if e.is_timeout() {
                HttpFailure::Timeout
            } else {
                HttpFailure::Other(e.to_string())
            }
        })?;
        serde_json::from_slice(&bytes).map_err(|e| HttpFailure::Other(e.to_string()))
    }

    async fn post_json(&self, url: &str) -> Result<PostAnswer, HttpFailure> {
        let failed = |e: reqwest::Error| {
            if e.is_timeout() {
                HttpFailure::Timeout
            } else {
                HttpFailure::Other(e.to_string())
            }
        };
        let response = self
            .http
            .post(url)
            .timeout(Duration::from_secs_f64(REFRESH_TIMEOUT_SECONDS))
            .send()
            .await
            .map_err(failed)?;
        let status = response.status().as_u16();
        let bytes = response.bytes().await.map_err(failed)?;
        // A body that is not JSON (an old helper's plain 404 page) is just "no details".
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Ok((status, body))
    }
}

/// `(lighting enabled, method is OSC, OSC host, Lightkey settings)`.
pub type LinkSettings = (bool, bool, String, LightkeySettings);
pub type SettingsProvider = Arc<dyn Fn() -> LinkSettings + Send + Sync>;

// ---------------------------------------------------------------------------- Record switch

/// Remembers whether the Record switch was on when the app last ran.
pub trait ArmedStore: Send + Sync {
    /// Unreadable means off.
    fn load_armed(&self) -> bool {
        false
    }
    fn save_armed(&self, _armed: bool) -> Result<(), std::io::Error> {
        Ok(())
    }
}

#[derive(Default)]
pub struct MemoryArmedStore {
    armed: Mutex<bool>,
}

impl MemoryArmedStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ArmedStore for MemoryArmedStore {
    fn load_armed(&self) -> bool {
        *self.armed.lock().unwrap_or_else(|p| p.into_inner())
    }
    fn save_armed(&self, armed: bool) -> Result<(), std::io::Error> {
        *self.armed.lock().unwrap_or_else(|p| p.into_inner()) = armed;
        Ok(())
    }
}

/// The Record switch in a one-word file next to the settings file.
pub struct FileArmedStore {
    pub path: PathBuf,
}

impl FileArmedStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl ArmedStore for FileArmedStore {
    fn load_armed(&self) -> bool {
        std::fs::read_to_string(&self.path).is_ok_and(|text| text.trim() == "armed")
    }

    fn save_armed(&self, armed: bool) -> Result<(), std::io::Error> {
        if let Some(parent) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, if armed { "armed" } else { "off" })
    }
}

// ------------------------------------------------------------------------- saving the looks

/// The captured look rows of one song, ready for its timeline.
#[derive(Debug, Clone)]
pub struct SongLooks {
    pub song_key: String,
    pub song_title: String,
    pub rows: Vec<OscLightingCue>,
    /// The repeating patterns found in the pass (empty when none).
    pub effects: Vec<LightingEffect>,
    /// How much of the song the pass covered and whether it was a complete capture.
    pub status: CaptureStatus,
}

/// Why the looks could not be saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LooksError {
    /// The song's timeline (with the rows already in it) would pass the row, message, effect or
    /// step caps: the link trims the capture further and saves again.
    TooLarge,
    /// The song already has a capture that is at least as good (a complete capture is never
    /// replaced by a partial one); nothing was changed.
    Kept,
    Failed(String),
}

pub type LooksFuture = Pin<Box<dyn Future<Output = Result<(), LooksError>> + Send>>;

/// Saves a song's captured rows into its cue map through the normal, serialized save path
/// (replacing only the captured rows of that song, keeping manual ones).
pub type LooksSink = Arc<dyn Fn(SongLooks) -> LooksFuture + Send + Sync>;

/// What the link knows about a song's stored capture (read from the saved cue maps).
pub type CaptureLookup = Arc<dyn Fn(&str) -> SongCapture + Send + Sync>;

/// A song's loudness from one recording, to be averaged into what the song already has.
#[derive(Debug, Clone)]
pub struct SongLoudnessTake {
    pub song_key: String,
    pub song_title: String,
    pub loudness: SongLoudness,
}

/// Saves a recording's loudness into the song's cue map (averaging with earlier recordings, see
/// [`crate::loudness::merge_loudness`]) through the same serialized save path as the looks.
pub type LoudnessSink = Arc<dyn Fn(SongLoudnessTake) -> LooksFuture + Send + Sync>;

// ------------------------------------------------------------------------------------- state

/// One song being recorded (`_Session`).
struct Session {
    song: Song,
    key: String,
    /// Wall-clock seconds when song time 0 happened.
    started_at: f64,
    /// Where the song stands when it is not running (the position it was paused or moved to).
    position: f64,
    running: bool,
    /// Fixture changes from the `lightkey-api` relay, when it is connected.
    fixtures: Option<LookRecorder>,
    /// Loudness windows from the audio input, on the same song clock.
    loudness: Option<SharedRecorder>,
    /// The stretches of the song really recorded: the timer ran and the lighting connection was
    /// live. Their union is the coverage that judges the pass.
    covered: Coverage,
    /// Where the open recorded stretch began (`None`: nothing is being recorded now).
    recorded_from: Option<f64>,
    /// The stretches the timer ran, whatever the link did (judges the loudness).
    played: Coverage,
    /// Where the timer's current run began.
    played_from: Option<f64>,
    /// The Lightkey project (base name) open when this recording began, as the relay hello
    /// said; `None` when it had not said. Saved with the capture.
    project: Option<String>,
    /// Set when the project or its fixture map changed mid-recording: the recording is thrown
    /// away, with this sentence as the reason.
    discarded: Option<String>,
}

impl Session {
    fn new(song: Song, started_at: f64) -> Self {
        let key = song_key(&song);
        Self {
            song,
            key,
            started_at,
            position: 0.0,
            running: true,
            fixtures: None,
            loudness: None,
            covered: Coverage::new(),
            recorded_from: None,
            played: Coverage::new(),
            played_from: Some(0.0),
            project: None,
            discarded: None,
        }
    }

    /// Where the song is at wall-clock second `wall`.
    fn position_at(&self, wall: f64) -> f64 {
        if self.running {
            (wall - self.started_at).max(0.0)
        } else {
            self.position
        }
    }

    /// A recorded stretch begins at `position`, with `state` as the value of every property.
    fn open_stretch(&mut self, position: f64, state: &FixtureState) {
        if self.recorded_from.is_some() {
            return;
        }
        if let Some(recorder) = self.fixtures.as_mut() {
            recorder.begin_stretch(position, state);
            self.recorded_from = Some(position);
        }
    }

    /// The open recorded stretch ends at `position`.
    fn close_stretch(&mut self, position: f64) {
        if let Some(from) = self.recorded_from.take() {
            self.covered.add(from, position);
            if let Some(recorder) = self.fixtures.as_mut() {
                recorder.end_stretch(position);
            }
        }
    }

    /// The timer stops (or the song is moved) at `position`: ends the open stretches.
    fn close_run(&mut self, position: f64) {
        self.close_stretch(position);
        if let Some(from) = self.played_from.take() {
            self.played.add(from, position);
        }
    }

    /// Seconds the timer ran, including the run in progress at `wall`.
    fn played_total(&self, wall: f64, duration: f64) -> f64 {
        let mut played = self.played.clone();
        if let Some(from) = self.played_from {
            played.add(from, self.position_at(wall));
        }
        played.total(duration)
    }
}

/// The capture/replay decision for one timer start of one song.
#[derive(Clone)]
struct SongDecision {
    key: String,
    /// Identifies the timer start (its `started_at`), so a restart decides again.
    started: Option<i64>,
    decision: Decision,
}

fn song_key(song: &Song) -> String {
    song.source_song_id
        .clone()
        .unwrap_or_else(|| song.id.clone())
}

/// Marks a recording as "being saved" for as long as it lives. Dropping it (every way out of the
/// save, including an early `return` and a panic) clears the mark, so the Lighting card can never
/// be left saying "Saving..." for good.
struct SavingGuard {
    inner: Arc<Inner>,
    mark: LightkeySaving,
}

impl Drop for SavingGuard {
    fn drop(&mut self) {
        let mut st = lock(&self.inner.state);
        // Another song may have started its own save since: only this one's mark is removed.
        if st.saving.as_ref() == Some(&self.mark) {
            st.saving = None;
        }
    }
}

/// How long a status message stays up if nothing changes.
pub const MESSAGE_SECONDS: f64 = 10.0;

/// A status message with the moment and the state it was said in. It is shown only while that
/// state still holds and for `MESSAGE_SECONDS`, so it can never go stale on screen.
struct Message {
    text: String,
    at: f64,
    link: Link,
    armed: bool,
    replay: bool,
    replaying: bool,
}

struct State {
    captured: Option<LightkeyCaptured>,
    /// The recording being fitted and saved (see `SavingGuard`).
    saving: Option<LightkeySaving>,
    subscriptions: Vec<Subscription>,
    poll_task: Option<JoinHandle<()>>,
    relay_task: Option<JoinHandle<()>>,
    stopped: bool,
    link: Link,
    detail: Option<String>,
    active: Option<std::collections::BTreeSet<String>>,
    seq: i64,
    /// `lightkey_clock - local_clock`, seconds.
    offset: f64,
    offset_at: f64,
    armed: bool,
    session: Option<Session>,
    message: Option<Message>,
    replaying: Option<String>,
    pending_song: Option<Song>,
    availability: Availability,
    decision: Option<SongDecision>,
    /// The project and fixture map the relay hello reported last, kept across relay sessions so
    /// a change while the link was down is still noticed.
    hello: Option<KnownHello>,
    /// What the helper's `/health` said at `health_at` (monotonic seconds).
    health: Option<LightkeyHelper>,
    health_at: Option<f64>,
}

/// The parts of a `/relay/hello` that say which fixtures the helper decodes with.
#[derive(Debug, Clone, PartialEq, Eq)]
struct KnownHello {
    project: Option<String>,
    fixtures: i64,
    map: i64,
}

impl KnownHello {
    /// "Show.lightkeyproj", or the project with its fixture count when only the fixtures changed.
    fn label(&self, with_count: bool) -> String {
        let name = self.project.as_deref().unwrap_or("an unnamed project");
        if with_count {
            let count = u32::try_from(self.fixtures.max(0)).unwrap_or(u32::MAX);
            format!("{name}, {}", fixtures_word(count))
        } else {
            name.to_string()
        }
    }

    /// A project name that changed, or a fixture map that was replaced under the same name.
    fn differs_from(&self, earlier: &Self) -> bool {
        self.map != earlier.map
            || matches!((&self.project, &earlier.project), (Some(a), Some(b)) if a != b)
    }
}

struct Inner {
    bus: EventBus,
    settings: SettingsProvider,
    armed_store: Arc<dyn ArmedStore>,
    looks_sink: Option<LooksSink>,
    fixture_tap: Option<FixtureTap>,
    link_changed: Option<LinkChanged>,
    loudness_sink: Option<LoudnessSink>,
    captures: Option<CaptureLookup>,
    audio: Option<Arc<AudioHub>>,
    http_factory: HttpFactory,
    clock: Arc<dyn Clock>,
    relay_port: u16,
    caps: Caps,
    recordings_dir: Option<PathBuf>,
    relay: RelayClient,
    state: Mutex<State>,
    wake: Notify,
}

/// What the fixture stream tells the lights plugin (operator override).
pub enum FixtureEvent<'a> {
    /// Fixture properties changed on the Lightkey Mac. `keyframe`: found by a keyframe.
    Changed {
        commands: &'a [FixtureCommand],
        keyframe: bool,
    },
    /// The relay session (re)started: the state is unknown until it is learned again.
    StreamReset,
}

pub type FixtureTap = Arc<dyn Fn(FixtureEvent<'_>) + Send + Sync>;

/// Told when the link to the helper changes (Connected, Unreachable, ...). It runs on the polling task,
/// outside every lock, and must not block.
pub type LinkChanged = Arc<dyn Fn() + Send + Sync>;

/// Everything `LightkeyLink::new` needs besides the bus and the settings.
pub struct LinkOptions {
    /// Remembers the Record switch.
    pub armed_store: Arc<dyn ArmedStore>,
    /// Where a finished recording goes (the song's timeline). `None` saves nothing.
    pub looks_sink: Option<LooksSink>,
    /// Told about every relay change and stream restart (the plugin judges operator input).
    pub fixture_tap: Option<FixtureTap>,
    /// Told when the link to the helper changes (the Lighting card shows Connected only while it is up).
    pub link_changed: Option<LinkChanged>,
    /// Where a recording's loudness goes. `None` saves none.
    pub loudness_sink: Option<LoudnessSink>,
    /// What each song's stored capture is (Auto decides from it). `None`: no song is captured.
    pub captures: Option<CaptureLookup>,
    /// The audio input that measures loudness. `None` records no loudness (headless builds).
    pub audio: Option<Arc<dyn AudioInput>>,
    pub http_factory: HttpFactory,
    pub clock: Arc<dyn Clock>,
    /// UDP port of the `lightkey-api` relay (21700 unless the agent was configured otherwise).
    pub relay_port: u16,
    /// The per-song limits a finished recording is fitted inside (the timeline's own by default;
    /// tests use tiny ones).
    pub caps: Caps,
    /// Where the raw recording of every finished song is kept (see `crate::lightkey_recording`).
    /// `None` keeps none.
    pub recordings_dir: Option<PathBuf>,
}

impl Default for LinkOptions {
    fn default() -> Self {
        Self {
            armed_store: Arc::new(MemoryArmedStore::new()),
            looks_sink: None,
            fixture_tap: None,
            link_changed: None,
            loudness_sink: None,
            captures: None,
            audio: None,
            http_factory: Arc::new(|| Arc::new(ReqwestLightkeyHttp::new())),
            clock: Arc::new(SystemClock::default()),
            relay_port: DEFAULT_LIGHTKEY_RELAY_PORT,
            caps: Caps::default(),
            recordings_dir: None,
        }
    }
}

#[derive(Clone)]
pub struct LightkeyLink {
    inner: Arc<Inner>,
}

fn lock(m: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl LightkeyLink {
    pub fn new(bus: EventBus, settings: SettingsProvider, options: LinkOptions) -> Self {
        Self {
            inner: Arc::new(Inner {
                bus,
                settings,
                armed_store: options.armed_store,
                looks_sink: options.looks_sink,
                fixture_tap: options.fixture_tap,
                link_changed: options.link_changed,
                loudness_sink: options.loudness_sink,
                captures: options.captures,
                audio: options
                    .audio
                    .map(|input| Arc::new(AudioHub::new(input, options.clock.clone()))),
                http_factory: options.http_factory,
                clock: options.clock,
                relay_port: options.relay_port,
                caps: options.caps,
                recordings_dir: options.recordings_dir,
                relay: RelayClient::new(),
                state: Mutex::new(State {
                    captured: None,
                    saving: None,
                    subscriptions: Vec::new(),
                    poll_task: None,
                    relay_task: None,
                    stopped: false,
                    link: Link::Off,
                    detail: None,
                    active: None,
                    seq: 0,
                    offset: 0.0,
                    offset_at: 0.0,
                    armed: false,
                    session: None,
                    message: None,
                    replaying: None,
                    pending_song: None,
                    availability: Availability::default(),
                    decision: None,
                    hello: None,
                    health: None,
                    health_at: None,
                }),
                wake: Notify::new(),
            }),
        }
    }

    // ------------------------------------------------------------ lifecycle
    pub async fn start(&self) {
        self.inner.clone().start().await;
    }

    pub async fn stop(&self) {
        self.inner.clone().stop().await;
    }

    /// Called after the saved Lightkey (or Lighting) settings change.
    pub fn settings_changed(&self) {
        {
            let mut st = lock(&self.inner.state);
            st.link = Link::Off;
            st.active = None;
            st.seq = 0;
            st.offset_at = 0.0;
            st.health = None;
            st.health_at = None;
        }
        self.inner.relay.reset();
        self.inner.wake.notify_one();
        let inner = self.inner.clone();
        tokio::spawn(async move {
            inner.sync_audio().await;
        });
    }

    /// The audio input hub (picker, meter), when this build can listen to an input.
    pub fn audio(&self) -> Option<Arc<AudioHub>> {
        self.inner.audio.clone()
    }

    /// Whether the Record switch is on (replay never runs while it is).
    pub fn armed(&self) -> bool {
        lock(&self.inner.state).armed
    }

    /// Decide capture or replay for the timer start of `song` (once per start; the link and the
    /// plugin both ask and get the same answer). The decision holds until the next start.
    pub fn decide_for_start(&self, song: &Song, started_at: Option<DateTime<Utc>>) -> Decision {
        self.inner.decide_for_start(song, started_at)
    }

    /// Fixture replay must not run: Record is on, or this song is being captured.
    pub fn capturing(&self) -> bool {
        self.inner.capturing()
    }

    /// The song the timeline is replaying fixture looks for (shown in the status), if any.
    pub fn set_replaying(&self, song: Option<String>) {
        lock(&self.inner.state).replaying = song;
    }

    /// The value the relay last reported for a fixture property (`/fixture/<name>/overrides/<p>`)
    /// when it is a single number: where the fixture is before the replay's first send.
    pub fn fixture_value(&self, address: &str) -> Option<f64> {
        self.inner.relay.number(address)
    }

    // ------------------------------------------------------------ status
    /// Where the link to the helper stands right now (cheap; the search reads it often).
    pub fn link_state(&self) -> Link {
        lock(&self.inner.state).link
    }

    pub fn status(&self) -> LightkeyStatus {
        self.inner.status()
    }

    /// The key of the song that is current: the one last started or selected, else the one
    /// being recorded.
    pub fn current_song_key(&self) -> Option<String> {
        let st = lock(&self.inner.state);
        st.pending_song
            .as_ref()
            .or(st.session.as_ref().map(|s| &s.song))
            .map(song_key)
    }

    /// "Read fixtures": ask the helper to re-read the open Lightkey project now.
    pub async fn read_fixtures(&self) -> LightkeyReadFixtures {
        self.inner.clone().read_fixtures().await
    }

    // ------------------------------------------------------------ actions
    pub async fn set_recording(&self, on: bool) -> String {
        self.inner.clone().set_recording(on).await
    }
}

impl Inner {
    fn st(&self) -> std::sync::MutexGuard<'_, State> {
        lock(&self.state)
    }

    fn say(&self, text: impl Into<String>) -> String {
        let text = text.into();
        let replay = (self.settings)().3.replay;
        let at = self.clock.mono();
        let mut st = self.st();
        st.message = Some(Message {
            text: text.clone(),
            at,
            link: st.link,
            armed: st.armed,
            replay,
            replaying: st.replaying.is_some(),
        });
        text
    }

    /// The current message, or `None` once it is older than `MESSAGE_SECONDS` or the link,
    /// the Record switch, the Replay switch or a running replay changed since it was said.
    fn live_message(&self, st: &mut State, replay: bool) -> Option<String> {
        let current = st.message.as_ref()?;
        let fresh = self.clock.mono() - current.at < MESSAGE_SECONDS
            && current.link == st.link
            && current.armed == st.armed
            && current.replay == replay
            && current.replaying == st.replaying.is_some();
        if fresh {
            return Some(current.text.clone());
        }
        st.message = None;
        None
    }

    /// Record pinned (the switch, or a hand-edited `mode: record`), else the saved mode.
    fn effective_mode(armed: bool, settings: &LightkeySettings) -> LightkeyMode {
        if armed || settings.mode == LightkeyMode::Record {
            LightkeyMode::Record
        } else {
            settings.mode
        }
    }

    /// The lighting connection: API linked, Lightkey running, fixture stream live, Lighting on.
    fn available_now(&self, link: Link) -> bool {
        let (enabled, osc, _host, _settings) = (self.settings)();
        enabled && osc && link == Link::Connected && self.relay.is_live()
    }

    /// Note the connection (call often: the poll loop does), and open or close the recorded
    /// stretch of the running pass to match it: a stretch of the song is recorded only while the
    /// timer runs and the lighting connection is live. A drop of any length just leaves a gap;
    /// when the connection is back, a new stretch starts with a snapshot of every property.
    fn tick(&self) {
        self.check_hello();
        let link = self.st().link;
        let available = self.available_now(link);
        let now = self.clock.mono();
        let wall = self.clock.wall();
        let wants_open = available
            && self.st().session.as_ref().is_some_and(|session| {
                session.running && session.fixtures.is_some() && session.recorded_from.is_none()
            });
        let state = wants_open.then(|| self.relay.state());
        let mut st = self.st();
        st.availability.observe(now, available);
        if let Some(session) = st.session.as_mut() {
            let position = session.position_at(wall);
            match (&state, session.running && available) {
                (Some(state), true) => session.open_stretch(position, state),
                (_, false) => session.close_stretch(position),
                _ => {}
            }
        }
    }

    /// Compare the relay's latest hello with the one before it. A replaced fixture map (a new
    /// map id) or another project name while a recording runs throws that recording away: it
    /// would mix two fixture maps. A repeated hello (same map id, same name) is nothing.
    fn check_hello(&self) {
        let Some(hello) = self.relay.hello() else {
            return;
        };
        let now = KnownHello {
            project: clean_project(Some(&hello.project)),
            fixtures: hello.fixtures,
            map: hello.map,
        };
        let notice = {
            let mut st = self.st();
            if st.hello.as_ref() == Some(&now) {
                return;
            }
            let Some(before) = st.hello.replace(now.clone()) else {
                return;
            };
            if !now.differs_from(&before) {
                return;
            }
            let counts = before.project == now.project;
            let (was, is) = (before.label(counts), now.label(counts));
            // Same name and same fixture count: only the fixtures themselves changed, so say that instead of
            // printing the same words on both sides of an arrow.
            let text = if was == is {
                format!(
                    "Not saved: the fixtures in the Lightkey project changed during the recording ({is}). It captures again next time."
                )
            } else {
                format!(
                    "Not saved: the Lightkey project or its fixtures changed during the recording ({was} -> {is}). It captures again next time."
                )
            };
            tracing::warn!(old = %before.label(true), new = %now.label(true), "lightkey_project_changed");
            match st.session.as_mut() {
                Some(session) if session.fixtures.is_some() && session.discarded.is_none() => {
                    session.fixtures = None;
                    session.recorded_from = None;
                    session.discarded = Some(text.clone());
                    Some(text)
                }
                _ => None,
            }
        };
        if let Some(text) = notice {
            self.say(text);
        }
    }

    fn decide_for_start(&self, song: &Song, started_at: Option<DateTime<Utc>>) -> Decision {
        self.tick();
        let key = song_key(song);
        let started = started_at.and_then(|at| at.timestamp_nanos_opt());
        let (_enabled, _osc, _host, settings) = (self.settings)();
        let stored = self
            .captures
            .as_ref()
            .map_or(SongCapture::NotCaptured, |lookup| lookup(&key));
        let link = self.st().link;
        let available = self.available_now(link);
        let now = self.clock.mono();
        let mut st = self.st();
        if let Some(existing) = &st.decision {
            if existing.key == key && existing.started == started {
                return existing.decision;
            }
        }
        let mode = Self::effective_mode(st.armed, &settings);
        let usable = st.availability.usable(now, available);
        let decision = decide(mode, usable, stored);
        st.decision = Some(SongDecision {
            key,
            started,
            decision,
        });
        decision
    }

    fn capturing(&self) -> bool {
        let settings = (self.settings)().3;
        let st = self.st();
        st.armed
            || settings.mode == LightkeyMode::Record
            || st
                .decision
                .as_ref()
                .is_some_and(|d| d.decision.action == Action::Capture)
    }

    /// What Auto does (or would do) with the current/next song, for the status.
    fn auto_status(&self, mode: LightkeyMode) -> Option<LightkeyAuto> {
        if mode != LightkeyMode::Auto {
            return None;
        }
        let link = self.st().link;
        let available = self.available_now(link);
        let now = self.clock.mono();
        let (decision, pending, usable) = {
            let st = self.st();
            (
                st.decision.clone(),
                st.pending_song.clone(),
                st.availability.usable(now, available),
            )
        };
        let (decision, song) = match (decision, pending) {
            (Some(d), pending) => {
                let title = pending.filter(|p| song_key(p) == d.key).map(|p| p.title);
                (d.decision, title)
            }
            (None, Some(song)) => {
                let stored = self
                    .captures
                    .as_ref()
                    .map_or(SongCapture::NotCaptured, |lookup| lookup(&song_key(&song)));
                (decide(LightkeyMode::Auto, usable, stored), Some(song.title))
            }
            (None, None) if !usable => (
                decide(LightkeyMode::Auto, false, SongCapture::NotCaptured),
                None,
            ),
            (None, None) => return None,
        };
        Some(LightkeyAuto {
            capturing: decision.action == Action::Capture,
            reason: decision.reason.text().to_string(),
            song,
        })
    }

    fn replay_blocked_reason(&self, armed: bool, auto_capture: bool) -> Option<String> {
        let (enabled, osc, _host, settings) = (self.settings)();
        if !enabled {
            return Some("Lighting is off.".into());
        }
        if !osc {
            return Some("Replay needs Lighting set to OSC.".into());
        }
        if armed {
            return Some("Paused while recording.".into());
        }
        if auto_capture {
            return Some("Paused while capturing this song.".into());
        }
        if !settings.replay {
            return Some("Replay is switched off.".into());
        }
        None
    }

    fn status(&self) -> LightkeyStatus {
        let (enabled, _osc, osc_host, settings) = (self.settings)();
        let armed_now = self.st().armed;
        let mode = Self::effective_mode(armed_now, &settings);
        let auto = self.auto_status(mode);
        let mut st = self.st();
        let auto_capture = st
            .decision
            .as_ref()
            .is_some_and(|d| d.decision.action == Action::Capture);
        let message = self.live_message(&mut st, settings.replay);
        LightkeyStatus {
            api_url: effective_api_url(&settings, &osc_host),
            link: if enabled { st.link } else { Link::Off },
            detail: st.detail.clone(),
            active_count: st
                .active
                .as_ref()
                .map(|a| u32::try_from(a.len()).unwrap_or(u32::MAX)),
            recording: st.armed,
            recording_song: st.session.as_ref().map(|s| s.song.title.clone()),
            replay: settings.replay,
            mode: Self::effective_mode(st.armed, &settings),
            auto,
            replay_blocked: self.replay_blocked_reason(st.armed, auto_capture),
            replaying_song: st.replaying.clone(),
            message,
            detail_level: settings.detail,
            captured: st.captured.clone(),
            saving: st.saving.clone(),
            fixtures: self.relay.status(
                st.session
                    .as_ref()
                    .is_some_and(|session| session.fixtures.is_some()),
            ),
            holding_fixtures: None,
            known_fixtures: None,
            yielded_fixtures: None,
            project: self
                .relay
                .hello()
                .and_then(|hello| clean_project(Some(&hello.project)))
                .or_else(|| {
                    // The helper's own answer, while the link is up and the relay is silent.
                    (st.link == Link::Connected)
                        .then(|| st.health.as_ref().and_then(|h| h.project.clone()))
                        .flatten()
                }),
            helper: (st.link == Link::Connected)
                .then(|| st.health.clone())
                .flatten(),
            project_mismatch: None,
            discovery: None,
        }
    }

    async fn start(self: Arc<Self>) {
        self.st().stopped = false;
        let store = self.armed_store.clone();
        let armed = tokio::task::spawn_blocking(move || store.load_armed())
            .await
            .unwrap_or(false);
        {
            let mut st = self.st();
            st.armed = armed;
        }
        let mut subs = Vec::new();
        macro_rules! sub {
            ($event:expr, $method:ident) => {{
                let inner = self.clone();
                subs.push(self.bus.subscribe(Some($event), move |event| {
                    let inner = inner.clone();
                    async move {
                        inner.$method(&event).await;
                        Ok(())
                    }
                }));
            }};
        }
        sub!(EventType::SongStarted, on_song);
        sub!(EventType::SongRestarted, on_song);
        sub!(EventType::TimerStarted, on_started);
        sub!(EventType::TimerResumed, on_repositioned);
        sub!(EventType::TimerSeeked, on_repositioned);
        sub!(EventType::TimerStopRequested, on_stopped);
        sub!(EventType::TimerStopped, on_stopped);
        sub!(EventType::TimerFailed, on_stopped);
        sub!(EventType::TimerResetRequested, on_reset);
        let task = tokio::spawn(self.clone().poll_loop());
        let relay_task = tokio::spawn(self.clone().relay_loop());
        let mut st = self.st();
        st.subscriptions.extend(subs);
        st.poll_task = Some(task);
        st.relay_task = Some(relay_task);
    }

    async fn stop(self: Arc<Self>) {
        self.st().stopped = true;
        self.wake.notify_one();
        // Save what a recording in progress captured (as an incomplete capture), but never hold
        // the exit up for long.
        let unfinished = self.st().session.take();
        if let Some(session) = unfinished {
            let _ = tokio::time::timeout(
                SHUTDOWN_SAVE_WAIT,
                self.finish(session, "StagePilot is shutting down."),
            )
            .await;
        }
        let task = self.st().poll_task.take();
        if let Some(task) = task {
            task.abort();
            let _ = task.await;
        }
        // Let the relay session say goodbye (it notices `stopped` within one tick), then force it.
        let relay_task = self.st().relay_task.take();
        if let Some(mut task) = relay_task {
            if tokio::time::timeout(Duration::from_millis(500), &mut task)
                .await
                .is_err()
            {
                task.abort();
                let _ = task.await;
            }
        }
        self.relay.reset();
        let subs: Vec<_> = std::mem::take(&mut self.st().subscriptions);
        for sub in subs {
            self.bus.unsubscribe(&sub);
        }
        {
            let mut st = self.st();
            st.session = None;
            st.armed = false;
        }
        self.sync_audio().await;
    }

    // ------------------------------------------------------------ actions
    async fn set_recording(self: Arc<Self>, on: bool) -> String {
        let replay = (self.settings)().3.replay;
        let (armed, message, link) = {
            let mut st = self.st();
            let message = self.live_message(&mut st, replay);
            (st.armed, message, st.link)
        };
        if on == armed {
            return message.unwrap_or_default();
        }
        if on {
            let (enabled, osc, _host, _settings) = (self.settings)();
            if !enabled {
                return self.say("Turn Lighting on first.");
            }
            if !osc {
                return self.say("Recording needs Lighting set to OSC.");
            }
            if link != Link::Connected {
                return self.say("Connect to Lightkey first.");
            }
            self.st().armed = true;
            self.persist_armed(true).await;
            return self.say("Recording starts with the next song.");
        }
        let session = {
            let mut st = self.st();
            st.armed = false;
            // A decision pinned by Record describes a mode that is now off; leaving it would
            // keep the status saying "Record" and block replay while the mode reads Auto.
            if st
                .decision
                .as_ref()
                .is_some_and(|d| d.decision.reason == Reason::PinnedRecord)
            {
                st.decision = None;
            }
            st.session.take()
        };
        self.persist_armed(false).await;
        match session {
            Some(session) => self.finish(session, "Recording turned off.").await,
            None => self.say("Recording is off."),
        }
    }

    /// Remember the Record switch for the next start. A failure is logged, never fatal.
    async fn persist_armed(&self, armed: bool) {
        let store = self.armed_store.clone();
        let saved = tokio::task::spawn_blocking(move || store.save_armed(armed)).await;
        if !matches!(saved, Ok(Ok(()))) {
            tracing::warn!(armed, "lightkey_armed_state_not_saved");
        }
    }

    // ------------------------------------------------------------ polling
    fn set_link(&self, link: Link, detail: Option<&str>) {
        let changed = {
            let mut st = self.st();
            let changed = st.link != link;
            if changed {
                tracing::info!(link = ?link, "lightkey_link_changed");
            }
            if link != Link::Connected {
                // Read the helper's health again as soon as the link is back.
                st.health_at = None;
            }
            st.link = link;
            st.detail = detail.map(str::to_string);
            changed
        };
        if changed {
            if let Some(told) = &self.link_changed {
                told();
            }
        }
    }

    fn is_stopped(&self) -> bool {
        self.st().stopped
    }

    async fn poll_loop(self: Arc<Self>) {
        let http = (self.http_factory)();
        while !self.is_stopped() {
            let (enabled, _osc, _host, _settings) = (self.settings)();
            let mut delay = POLL_SECONDS;
            if !enabled {
                self.set_link(Link::Off, None);
                delay = RETRY_SECONDS;
            } else if let Err(failure) = self.poll_once(http.as_ref()).await {
                self.on_unreachable(&failure);
                delay = RETRY_SECONDS;
            } else {
                self.poll_health(http.as_ref()).await;
            }
            self.tick();
            self.finish_if_song_over().await;
            tokio::select! {
                () = self.wake.notified() => {}
                () = tokio::time::sleep(Duration::from_secs_f64(delay)) => {}
            }
        }
    }

    /// Read the helper's `GET /health` on connect and then about every `HEALTH_SECONDS`. It
    /// never changes the link: an old helper without the new keys just reports no version.
    async fn poll_health(&self, http: &dyn LightkeyHttp) {
        let base = {
            let (_enabled, _osc, osc_host, settings) = (self.settings)();
            let st = self.st();
            let now = self.clock.mono();
            let due = st.link == Link::Connected
                && st.health_at.is_none_or(|at| now - at >= HEALTH_SECONDS);
            if !due {
                return;
            }
            effective_api_url(&settings, &osc_host)
        };
        let answer = http.get_json(&format!("{base}/health"), &[]).await;
        let mut st = self.st();
        st.health_at = Some(self.clock.mono());
        match answer {
            Ok(body) => st.health = Some(parse_health(&body)),
            // No /health page at all (every real helper has one, 0.4.x included): nothing is
            // known about the helper, so nothing is claimed about its version.
            Err(HttpFailure::Status(404 | 405)) => st.health = None,
            // Keep what was known; ask again next time.
            Err(_) => {}
        }
    }

    async fn read_fixtures(self: Arc<Self>) -> LightkeyReadFixtures {
        let (enabled, _osc, osc_host, settings) = (self.settings)();
        if !enabled {
            return refresh_failure("lighting_off", "Turn Lighting on first.");
        }
        let base = effective_api_url(&settings, &osc_host);
        let http = (self.http_factory)();
        let result = match http.post_json(&format!("{base}/fixtures/refresh")).await {
            Ok((status, body)) => interpret_refresh(status, &body),
            Err(HttpFailure::Timeout) => {
                refresh_failure("unreachable", "The Lightkey API did not answer.")
            }
            Err(_) => refresh_failure("unreachable", "Cannot reach the Lightkey API."),
        };
        if result.ok {
            // Show the new fixture map without waiting for the next health read.
            self.st().health_at = None;
            self.wake.notify_one();
        }
        result
    }

    /// Keep a session with the Lightkey API relay while Lighting is on and set to OSC.
    async fn relay_loop(self: Arc<Self>) {
        while !self.is_stopped() {
            let target = self.relay_target_now();
            let Some(target) = target else {
                self.relay.reset();
                tokio::select! {
                    () = self.wake.notified() => {}
                    () = tokio::time::sleep(Duration::from_secs(2)) => {}
                }
                continue;
            };
            let this = self.clone();
            let sink: crate::lightkey_relay::FixtureSink = {
                let this = self.clone();
                Arc::new(
                    move |local: f64, commands: Vec<FixtureCommand>, keyframe: bool| {
                        this.apply_fixture_change(local, &commands, keyframe);
                    },
                )
            };
            if let Some(tap) = &self.fixture_tap {
                tap(FixtureEvent::StreamReset);
            }
            let offset_sink: crate::lightkey_relay::OffsetSink = {
                let this = self.clone();
                Arc::new(move |shift: f64| this.redate(shift))
            };
            let clock = self.clock.clone();
            let clock_b = self.clock.clone();
            let watch = target.clone();
            self.relay
                .session(
                    &target,
                    move || clock.wall(),
                    move || clock_b.mono(),
                    move || {
                        !this.is_stopped() && this.relay_target_now().as_deref() == Some(&watch)
                    },
                    sink,
                    offset_sink,
                )
                .await;
        }
    }

    /// A better clock estimate arrived that moves the clock by more than 50 ms: the changes
    /// recorded since the previous estimate took effect were dated with the worse one. Move them
    /// by `shift` seconds (`old_offset - new_offset`) and start counting from here.
    fn redate(&self, shift: f64) {
        let mut st = self.st();
        for session in st.session.iter_mut() {
            if let Some(recorder) = session.fixtures.as_mut() {
                let moved = recorder.shift_since_mark(shift);
                if moved > 0 {
                    tracing::warn!(moved, shift, "lightkey_capture_redated");
                }
                recorder.set_mark();
            }
        }
    }

    fn relay_target_now(&self) -> Option<String> {
        let (enabled, osc, osc_host, settings) = (self.settings)();
        if !(enabled && osc) {
            return None;
        }
        relay_target(&effective_api_url(&settings, &osc_host), self.relay_port)
    }

    /// A fixture changed on the Lightkey Mac at `local_time` (this computer's clock).
    fn apply_fixture_change(&self, local_time: f64, commands: &[FixtureCommand], keyframe: bool) {
        if let Some(tap) = &self.fixture_tap {
            tap(FixtureEvent::Changed { commands, keyframe });
        }
        // A change proves the stream is live: make sure its stretch is open first.
        self.tick();
        let mut st = self.st();
        let stopped = st.stopped;
        if let Some(session) = st.session.as_mut() {
            if session.running && !stopped && session.recorded_from.is_some() {
                let position = local_time - session.started_at;
                if let Some(recorder) = session.fixtures.as_mut() {
                    recorder.observe(position, commands);
                }
            }
        }
    }

    fn on_unreachable(&self, failure: &HttpFailure) {
        let detail = match failure {
            HttpFailure::Status(403) => {
                "Lightkey API refused this address. Allow the wired interface in its config."
            }
            HttpFailure::Timeout => "Lightkey API did not answer.",
            _ => "Cannot reach the Lightkey API.",
        };
        {
            let mut st = self.st();
            if st.link == Link::Connected {
                tracing::warn!(reason = ?failure, "lightkey_link_lost");
            }
            st.active = None;
        }
        self.set_link(Link::Unreachable, Some(detail));
    }

    async fn poll_once(&self, http: &dyn LightkeyHttp) -> Result<(), HttpFailure> {
        let (_enabled, _osc, osc_host, settings) = (self.settings)();
        let base = effective_api_url(&settings, &osc_host);
        let (refresh, since) = {
            let st = self.st();
            (
                self.clock.mono() - st.offset_at > OFFSET_REFRESH_SECONDS
                    || st.link != Link::Connected,
                st.seq,
            )
        };
        if refresh {
            self.measure_offset(http, &base).await?;
        }
        let body = match http
            .get_json(&format!("{base}/log"), &[("since", since.to_string())])
            .await
        {
            Ok(body) => body,
            // `/log` is the optional cue-name feed (the helper's `lightkey_cues.py`). A helper installed without
            // it answers 404 there, and still answers `/health`, which has everything this link needs: that the
            // helper is alive and whether Lightkey is running.
            Err(HttpFailure::Status(404)) => {
                let health = http.get_json(&format!("{base}/health"), &[]).await?;
                return self.apply_health_only(&health);
            }
            Err(other) => return Err(other),
        };
        let Some(body) = body.as_object() else {
            return Err(HttpFailure::Other("the log is not an object".into()));
        };
        let empty = serde_json::Map::new();
        let state = match body.get("state") {
            None => &empty,
            Some(Value::Object(map)) => map,
            Some(_) => return Err(HttpFailure::Other("state is not an object".into())),
        };
        let server_seq = match state.get("seq") {
            None => 0,
            Some(v) => py_int(v)?,
        };
        let (fresh, current_seq) = {
            let mut st = self.st();
            if server_seq < st.seq {
                // The agent restarted: its numbering began again.
                st.seq = 0;
                st.active = None;
                drop(st);
                self.wake.notify_one();
                return Ok(());
            }
            (st.link != Link::Connected || st.active.is_none(), st.seq)
        };
        let _ = current_seq;
        if !py_truthy(state.get("online")) {
            {
                let mut st = self.st();
                st.active = None;
                st.seq = server_seq;
            }
            self.set_link(Link::LightkeyClosed, Some("Lightkey is not running."));
            return Ok(());
        }
        let active_now: std::collections::BTreeSet<String> = match state.get("active") {
            Some(Value::Array(items)) => items.iter().map(py_str).collect(),
            Some(Value::Null) | None => Default::default(),
            Some(other) if py_truthy(Some(other)) => {
                return Err(HttpFailure::Other("active is not a list".into()))
            }
            Some(_) => Default::default(),
        };
        if fresh {
            {
                let mut st = self.st();
                st.active = Some(active_now);
                st.seq = server_seq;
            }
            self.set_link(Link::Connected, None);
            return Ok(());
        }
        let events: Vec<&Value> = match body.get("events") {
            None => Vec::new(),
            Some(Value::Array(items)) => items.iter().collect(),
            Some(_) => return Err(HttpFailure::Other("events is not a list".into())),
        };
        for event in events {
            let Some(event) = event.as_object() else {
                return Err(HttpFailure::Other("an event is not an object".into()));
            };
            let seq = match event.get("seq") {
                None => 0,
                Some(v) => py_int(v)?,
            };
            {
                let mut st = self.st();
                st.seq = st.seq.max(seq);
            }
            if !event.contains_key("added") {
                continue; // an online/offline event; the state above covers it
            }
            let names = |key: &str| -> Result<Vec<String>, HttpFailure> {
                match event.get(key) {
                    None => Ok(Vec::new()),
                    Some(Value::Array(items)) => Ok(items.iter().map(py_str).collect()),
                    Some(_) => Err(HttpFailure::Other(format!("{key} is not a list"))),
                }
            };
            // The cue feed is status only: check its shape, store nothing from it.
            names("added")?;
            names("removed")?;
        }
        self.st().active = Some(active_now);
        self.set_link(Link::Connected, None);
        Ok(())
    }

    /// The link state from `GET /health` alone, for a helper without the cue-name feed.
    fn apply_health_only(&self, health: &Value) -> Result<(), HttpFailure> {
        let Some(health) = health.as_object() else {
            return Err(HttpFailure::Other(
                "the health answer is not an object".into(),
            ));
        };
        if !py_truthy(health.get("ok")) {
            return Err(HttpFailure::Other("the helper says it is not ok".into()));
        }
        {
            let mut st = self.st();
            st.seq = 0;
            st.active = if py_truthy(health.get("lightkey")) {
                Some(Default::default())
            } else {
                None
            };
        }
        if py_truthy(health.get("lightkey")) {
            self.set_link(Link::Connected, None);
        } else {
            self.set_link(Link::LightkeyClosed, Some("Lightkey is not running."));
        }
        Ok(())
    }

    /// Clock offset from the lowest-round-trip of several samples (NTP style).
    ///
    /// Captured changes are dated with the relay's own clock estimate (`lightkey_relay`); this
    /// HTTP measurement is the independent check on it. Every sample is logged
    /// (`lightkey_clock_offset`), and a result that differs from the relay's by more than 50 ms
    /// is logged as `lightkey_clock_offset_mismatch`.
    async fn measure_offset(&self, http: &dyn LightkeyHttp, base: &str) -> Result<(), HttpFailure> {
        let mut best: Option<(f64, f64)> = None;
        for _ in 0..OFFSET_SAMPLES {
            let sent = self.clock.wall();
            let answer = http.get_json(&format!("{base}/time"), &[]).await?;
            let received = self.clock.wall();
            let remote = match answer.get("t") {
                Some(v) => py_float(v)? / 1000.0,
                None => return Err(HttpFailure::Other("no t in /time".into())),
            };
            let rtt = received - sent;
            tracing::info!(
                source = "http",
                offset = remote - (sent + received) / 2.0,
                round_trip = rtt,
                "lightkey_clock_offset"
            );
            if best.is_none_or(|(best_rtt, _)| rtt < best_rtt) {
                best = Some((rtt, remote - (sent + received) / 2.0));
            }
        }
        if let Some((rtt, offset)) = best {
            let mut st = self.st();
            st.offset = offset;
            st.offset_at = self.clock.mono();
            drop(st);
            let relay = self.relay.clock_offset();
            tracing::info!(
                source = "http",
                offset,
                round_trip = rtt,
                relay_offset = relay,
                "lightkey_clock_offset_best"
            );
            if let Some(relay) = relay {
                if (relay - offset).abs() > crate::lightkey_fixtures::CLOCK_REDATE_THRESHOLD
                    && rtt < crate::lightkey_fixtures::CLOCK_POOR_ROUND_TRIP
                {
                    tracing::warn!(http = offset, relay, "lightkey_clock_offset_mismatch");
                }
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ timer events
    async fn on_song(self: &Arc<Self>, event: &StagePilotEvent) {
        if let EventPayload::Song { song, .. } = &event.payload {
            let mut st = self.st();
            st.pending_song = Some(song.clone());
            let key = song_key(song);
            if st.decision.as_ref().is_some_and(|d| d.key != key) {
                st.decision = None;
            }
        }
    }

    /// Wall-clock second at which song time 0 happened for this timer event.
    fn clock_from(&self, payload: &TimerPayload, position: f64) -> f64 {
        payload
            .started_at
            .map_or_else(|| self.clock.wall(), datetime_seconds)
            - position
    }

    async fn on_started(self: &Arc<Self>, event: &StagePilotEvent) {
        let EventPayload::Timer(payload) = &event.payload else {
            return;
        };
        let song = {
            let st = self.st();
            payload.song.clone().or_else(|| st.pending_song.clone())
        };
        let Some(song) = song else {
            return;
        };
        let decision = self.decide_for_start(&song, payload.started_at);
        let previous = self.st().session.take();
        if let Some(previous) = previous {
            self.finish(previous, "A new song started.").await;
        }
        let started_at = self.clock_from(payload, 0.0);
        if decision.action == Action::Capture {
            let pinned = decision.reason == Reason::PinnedRecord;
            let recording = {
                let mut st = self.st();
                // A pinned Record needs the API linked; Auto already counted the grace period.
                if !pinned || st.link == Link::Connected {
                    let mut session = Session::new(song.clone(), started_at);
                    session.project = st.hello.as_ref().and_then(|hello| hello.project.clone());
                    if self.relay.is_live() || !pinned {
                        let state = self.relay.state();
                        let mut recorder = LookRecorder::new(&state);
                        // Not live yet (Auto rides out a short drop): the first stretch opens
                        // when the stream is, so the start is a gap.
                        if self.available_now(st.link) {
                            session.recorded_from = Some(0.0);
                        } else {
                            recorder.end_stretch(0.0);
                        }
                        session.fixtures = Some(recorder);
                    }
                    if self.audio.is_some() {
                        session.loudness =
                            Some(Arc::new(Mutex::new(LoudnessRecorder::new(started_at))));
                    }
                    st.session = Some(session);
                    true
                } else {
                    false
                }
            };
            if !recording {
                self.say("Not recording this song: Lightkey is not connected.");
                return;
            }
            // The input opens now; a failure never stops the lights capture.
            let audio_problem = self.sync_audio().await;
            let with_fixtures = self
                .st()
                .session
                .as_ref()
                .is_some_and(|session| session.fixtures.is_some());
            let lead = if pinned {
                "Recording"
            } else {
                "Auto: capturing"
            };
            let mut text = if with_fixtures || self.relay.status(false).is_none() {
                format!("{lead} {}.", song.title)
            } else {
                format!(
                    "{lead} {}. Fixture levels are not recorded: the Lightkey API relay is not connected.",
                    song.title
                )
            };
            if let Some(problem) = audio_problem {
                text.push_str(&format!(" Loudness is not recorded: {problem}"));
            }
            self.say(text);
        }
    }

    /// Resume and seek. The pass goes on from the new position: the stretch recorded so far
    /// ends where the song was, and a new one begins at the new position (if the timer runs and
    /// the link is live). Moving forward leaves a gap; moving back (or restarting) records the
    /// same stretch again, and the later recording wins (see `LookRecorder`).
    async fn on_repositioned(self: &Arc<Self>, event: &StagePilotEvent) {
        let EventPayload::Timer(payload) = &event.payload else {
            return;
        };
        let position = payload.position_seconds.unwrap_or(0.0).max(0.0);
        let wall = self.clock.wall();
        {
            let mut st = self.st();
            if let Some(session) = st.session.as_mut() {
                let before = session.position_at(wall);
                // Only a resume in place continues the same stretch; any seek starts a new one,
                // however small.
                let same_place = event.event_type == EventType::TimerResumed
                    && (position - before).abs() <= RESUME_TOLERANCE_SECONDS;
                // The old stretch ends where the song was.
                session.close_run(before);
                session.started_at = payload.started_at.map_or(wall, datetime_seconds) - position;
                session.position = position;
                session.running = payload.running;
                if payload.running {
                    session.played_from = Some(position);
                }
                if let Some(loudness) = &session.loudness {
                    let mut loudness = loudness.lock().unwrap_or_else(|p| p.into_inner());
                    loudness.set_started_at(session.started_at);
                    if !same_place {
                        // Anything heard here before is replaced by this take.
                        loudness.next_stretch();
                    }
                    loudness.set_running(payload.running);
                }
            }
        }
        // Opens the new stretch (with a snapshot of every property, which also catches up on
        // anything changed while paused or moved) when the connection is live.
        self.tick();
        self.sync_audio().await;
    }

    /// Pause, stop, song end and a failed timer: a recording session pauses or is kept.
    async fn on_stopped(self: &Arc<Self>, event: &StagePilotEvent) {
        self.tick();
        enum Next {
            Finish(Box<Session>),
            Paused(String),
            Nothing,
        }
        let next = {
            let mut st = self.st();
            let wall = self.clock.wall();
            match st.session.as_mut() {
                None => Next::Nothing,
                Some(session) => {
                    let position = match &event.payload {
                        EventPayload::Timer(TimerPayload {
                            position_seconds: Some(p),
                            ..
                        }) => *p,
                        _ => wall - session.started_at,
                    };
                    if !session.running {
                        // Already paused (a pause is followed by the timer's own stopped
                        // event): this says nothing new and must not move the position.
                        Next::Nothing
                    } else {
                        session.close_run(position);
                        session.position = position;
                        session.running = false;
                        if let Some(loudness) = &session.loudness {
                            loudness
                                .lock()
                                .unwrap_or_else(|p| p.into_inner())
                                .set_running(false);
                        }
                        let duration = f64::from(session.song.duration_seconds.unwrap_or(0));
                        // Only the song's end (or a failed timer) closes a recording. A pause,
                        // however late in the song, keeps it: the resume carries on, and a new
                        // song or Record off saves what was captured (R-01).
                        let ended = event.event_type == EventType::TimerFailed
                            || session.position >= duration - SONG_END_TOLERANCE_SECONDS;
                        if ended {
                            match st.session.take() {
                                Some(session) => Next::Finish(Box::new(session)),
                                None => Next::Nothing,
                            }
                        } else {
                            Next::Paused(session.song.title.clone())
                        }
                    }
                }
            }
        };
        match next {
            Next::Finish(session) => {
                self.finish(*session, "Song finished.").await;
            }
            Next::Paused(title) => {
                self.say(format!(
                    "Paused \"{title}\". Recording resumes if the song does."
                ));
            }
            Next::Nothing => {}
        }
    }

    /// The song's clock passed its scheduled length plus the grace while still running: the song
    /// is over, whether or not the timer stopped (it deliberately keeps counting into overrun, and
    /// Playback's own song end does not reach the link). The pass is saved once; with the session
    /// gone, later changes cannot start or reopen one.
    async fn finish_if_song_over(self: &Arc<Self>) {
        let over = {
            let mut st = self.st();
            let wall = self.clock.wall();
            let due = st.session.as_ref().is_some_and(|session| {
                let duration = f64::from(session.song.duration_seconds.unwrap_or(0));
                session.running
                    && duration > 0.0
                    && wall - session.started_at >= duration + SONG_END_GRACE_SECONDS
            });
            if due {
                st.session.take()
            } else {
                None
            }
        };
        if let Some(session) = over {
            self.finish(session, "Song finished.").await;
        }
    }

    /// Reset discards a recording in progress.
    async fn on_reset(self: &Arc<Self>, _event: &StagePilotEvent) {
        let had = {
            let mut st = self.st();
            st.decision = None;
            st.session.take().is_some()
        };
        if had {
            self.say("Recording discarded: the countdown was reset.");
        }
        self.sync_audio().await;
    }

    /// A recording ends: below half the song it is thrown away, otherwise thinned into look
    /// rows and saved to the song's timeline. The raw changes go with `session`.
    async fn finish(self: &Arc<Self>, mut session: Session, reason: &str) -> String {
        let note = self.save_loudness(&mut session).await;
        let text = self.finish_looks(session, reason).await;
        match note {
            Some(note) => self.say(format!("{text} {note}")),
            None => text,
        }
    }

    /// Points the audio input at the running session's recorder (none: the input closes) and at
    /// the saved input choice. `Some` is why the input could not be opened.
    async fn sync_audio(&self) -> Option<String> {
        let hub = self.audio.clone()?;
        let recorder = self.st().session.as_ref().and_then(|s| s.loudness.clone());
        let device = (self.settings)().3.audio_device.clone();
        // Opening the input can wait on the microphone permission dialog, so it runs on a
        // blocking thread and the link only waits a moment for it (A-01).
        let job = tokio::task::spawn_blocking(move || {
            hub.set_device(device);
            hub.set_recorder(recorder).err()
        });
        match tokio::time::timeout(AUDIO_OPEN_WAIT, job).await {
            Ok(Ok(problem)) => problem,
            Ok(Err(_)) => Some("The audio input could not start.".into()),
            Err(_) => Some(
                "The audio input is slow to open (the system may be asking for microphone access); loudness starts when it opens."
                    .into(),
            ),
        }
    }

    /// A finished recording's loudness goes to the song, under the same keep-it rule as the
    /// looks (at least half the song). The line is added to the status; never an error.
    async fn save_loudness(self: &Arc<Self>, session: &mut Session) -> Option<String> {
        let recorder = session.loudness.take()?;
        let problem = self.audio.as_ref().and_then(|hub| hub.snapshot().reason);
        self.sync_audio().await;
        let duration = f64::from(session.song.duration_seconds.unwrap_or(0));
        // The take is kept when the timer ran for half the song, whatever the link did.
        let covered = session.played_total(self.clock.wall(), duration);
        if !recording_is_worth_keeping(covered, duration) {
            return None;
        }
        let Some(loudness) = AudioHub::finish(&recorder, duration) else {
            return Some(match problem {
                Some(problem) => format!("Loudness not saved: {problem}"),
                None => "Loudness not saved: nothing was heard.".into(),
            });
        };
        let sink = self.loudness_sink.clone()?;
        let saved = sink(SongLoudnessTake {
            song_key: session.key.clone(),
            song_title: session.song.title.clone(),
            loudness,
        })
        .await;
        Some(match saved {
            Ok(()) => "Loudness saved.".into(),
            Err(_) => "Loudness could not be saved.".into(),
        })
    }

    /// `status().saving` says this song is being saved until the returned guard is dropped.
    fn mark_saving(self: &Arc<Self>, title: &str) -> SavingGuard {
        let mark = LightkeySaving {
            song: title.to_string(),
            started_at: DateTime::<Utc>::from_timestamp(self.clock.wall() as i64, 0)
                .unwrap_or_default()
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        };
        self.st().saving = Some(mark.clone());
        SavingGuard {
            inner: self.clone(),
            mark,
        }
    }

    async fn finish_looks(self: &Arc<Self>, mut session: Session, reason: &str) -> String {
        self.tick();
        if let Some(text) = session.discarded.take() {
            tracing::info!(song = %session.key, reason, "lightkey_recording_discarded");
            return self.say(text);
        }
        let duration = f64::from(session.song.duration_seconds.unwrap_or(0));
        if session.running {
            let now = session.position_at(self.clock.wall());
            session.close_run(now);
            session.running = false;
        }
        // Where the missing seconds are does not matter, only how many.
        let covered = session.covered.total(duration);
        let title = session.song.title.clone();
        let recorder = session.fixtures.take().filter(|r| !r.is_empty());
        let verdict = judge(duration, covered, recorder.is_some());
        let complete = match verdict {
            PassVerdict::UnknownLength => {
                return self.say(format!(
                    "Not saved: the length of {title} is unknown, so the capture cannot be judged."
                ));
            }
            PassVerdict::NoChanges => {
                return self.say(format!(
                    "Not saved: no fixture changes were recorded for {title}."
                ));
            }
            PassVerdict::Complete => true,
            PassVerdict::Partial => false,
        };
        let Some(recorder) = recorder else {
            return self.say(format!(
                "Not saved: no fixture changes were recorded for {title}."
            ));
        };
        let Some(sink) = self.looks_sink.clone() else {
            return self.say("Could not save the looks to the timeline.");
        };
        // From here the recording is being fitted and saved (35-45 s for a long, busy song). The
        // guard clears the mark on every way out, after the closing message is said.
        let _saving = self.mark_saving(&title);
        let status = CaptureStatus {
            complete,
            coverage_seconds: covered,
            recorded_at: DateTime::<Utc>::from_timestamp(self.clock.wall() as i64, 0)
                .unwrap_or_default()
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            project: session.project.clone(),
            simplified: None,
            start_lights: None,
        };
        let raw = recorder.raw_with_start();
        drop(recorder);
        let (baseline, mut changes) = (raw.baseline, raw.changes);
        // The relay's state of every fixture when recording began is the song's first look:
        // fixtures that never change would otherwise leave no sample at all (the house lights).
        let start = raw
            .start
            .map(|(at, snapshot)| lightkey_start::apply(&mut changes, at, &snapshot));
        let raw_changes = changes.len();
        let (baseline, changes) = (Arc::new(baseline), Arc::new(changes));
        // The raw recording goes to disk before anything is fitted, so a song that cannot be
        // saved can still be looked at. A failure here never stops the save.
        let folder = self.recordings_dir.clone();
        let kept_in = self
            .keep_raw_recording(&session.key, duration, &baseline, &changes)
            .await;
        let caps = self.caps;
        let fitted = tokio::task::spawn_blocking(move || {
            fit_within_caps(&baseline, &changes, duration, &caps)
        })
        .await;
        let Ok(fitted) = fitted else {
            tracing::error!(song = %session.key, "lightkey_fit_failed");
            return self.say(failure_text(kept_in.as_ref().and(folder.as_ref())));
        };
        if fitted.exact_items == 0 {
            return self.say(format!(
                "Not saved: no fixture changes were recorded for {title}."
            ));
        }
        let simplified = simplified_of(&fitted);
        let status = CaptureStatus {
            simplified: simplified.clone(),
            ..status
        };
        let mut capture = fitted.capture.clone();
        if let Some(applied) = &start {
            // one "Look - N fixtures" row at the start instead of one per fixture
            lightkey_start::merge_first_look(&mut capture.rows, applied.at);
        }
        let mut trimmed = fitted.trimmed;
        let mut share = 1.0;
        for _ in 0..MAX_SINK_RETRIES {
            let rows = smooth_rows(&capture.rows);
            let (count, loops) = (rows.len(), capture.effects.len());
            // 0: the relay's state could not be read, so the capture has no starting look.
            let start_lights = start.as_ref().map_or(0, |applied| {
                lightkey_start::lights_at_start(&capture.rows, applied.at)
            });
            let simplified = if trimmed > fitted.trimmed {
                // the timeline already holds other rows: this song was trimmed further
                simplified_of(&Fitted {
                    trimmed,
                    stage: Stage::Trimmed,
                    kept_items: crate::lightkey_fit::Size::of_capture(&capture).items(),
                    ..fitted.clone()
                })
            } else {
                simplified.clone()
            };
            let saved = sink(SongLooks {
                song_key: session.key.clone(),
                song_title: title.clone(),
                rows,
                effects: capture.effects.clone(),
                status: CaptureStatus {
                    simplified: simplified.clone(),
                    start_lights: Some(u32::try_from(start_lights).unwrap_or(u32::MAX)),
                    ..status.clone()
                },
            })
            .await;
            match saved {
                Ok(()) => {
                    tracing::info!(
                        song = %session.key,
                        rows = count,
                        effects = loops,
                        complete,
                        reason,
                        stage = fitted.stage.as_str(),
                        scale = fitted.scale,
                        thin_seconds = fitted.thin_seconds,
                        trimmed,
                        raw_changes,
                        start_lights,
                        fits = fitted.fits,
                        fit_ms = u64::try_from(fitted.fit_ms).unwrap_or(u64::MAX),
                        "lightkey_looks_saved"
                    );
                    self.st().captured = Some(LightkeyCaptured {
                        song_title: title.clone(),
                        looks: u32::try_from(count + loops).unwrap_or(u32::MAX),
                    });
                    let mut text =
                        saved_message(count, loops, &title, fitted.scale, simplified.as_ref());
                    if start_lights == 0 {
                        text.push_str(" The starting look of the other lights could not be read.");
                    }
                    let missing = duration - covered;
                    return self.say(if !complete {
                        format!(
                            "{text} Incomplete capture: {covered:.0} of {duration:.0} seconds recorded; it captures again next time."
                        )
                    } else if missing >= 0.5 {
                        format!(
                            "{} ({missing:.0} s of the song were not captured).",
                            text.trim_end_matches('.')
                        )
                    } else {
                        text
                    });
                }
                Err(LooksError::Kept) => {
                    return self.say(format!(
                        "Not saved: {title} already has a capture that covers more of the song."
                    ));
                }
                Err(LooksError::TooLarge) => {
                    // Manual rows already in the song's timeline take part of the room: trim
                    // this capture further (a quarter less each time) and save again.
                    tracing::warn!(song = %session.key, rows = count, effects = loops, share, "lightkey_looks_too_large");
                    share *= 0.75;
                    let aim = caps.below(&crate::lightkey_fit::Size::of_capture(&capture), 0.75);
                    let (smaller, removed) = trim_to_caps(&capture, &aim);
                    capture = smaller;
                    trimmed += removed;
                }
                Err(LooksError::Failed(error)) => {
                    tracing::error!(song = %session.key, error = %error, "lightkey_looks_save_failed");
                    return self.say(failure_text(kept_in.as_ref().and(folder.as_ref())));
                }
            }
        }
        self.say(failure_text(kept_in.as_ref().and(folder.as_ref())))
    }

    /// Writes the raw recording next to the settings file (off the runtime's threads). `Some`
    /// is where; a failure is logged and is `None`.
    async fn keep_raw_recording(
        &self,
        key: &str,
        duration: f64,
        baseline: &Arc<Vec<FixtureCommand>>,
        changes: &Arc<Vec<(f64, FixtureCommand)>>,
    ) -> Option<PathBuf> {
        let dir = self.recordings_dir.clone()?;
        let now = DateTime::<Utc>::from_timestamp(self.clock.wall() as i64, 0).unwrap_or_default();
        let (key, shared) = (key.to_string(), (baseline.clone(), changes.clone()));
        let job = {
            let key = key.clone();
            tokio::task::spawn_blocking(move || {
                let view = RawRecordingView::new(&key, duration, &shared.0, &shared.1);
                lightkey_recording::save(&dir, &view, now)
            })
        };
        match job.await {
            Ok(Ok(path)) => Some(path),
            Ok(Err(error)) => {
                tracing::warn!(song = %key, error = %error, "lightkey_raw_recording_not_kept");
                None
            }
            Err(_) => None,
        }
    }
}

/// How a capture that did not fit as recorded was simplified (`None`: it fit).
fn simplified_of(fitted: &Fitted) -> Option<CaptureSimplified> {
    (fitted.stage != Stage::Exact).then(|| CaptureSimplified {
        stage: fitted.stage.as_str().to_string(),
        tolerance_scale: fitted.scale,
        thin_seconds: fitted.thin_seconds,
        trimmed: u32::try_from(fitted.trimmed).unwrap_or(u32::MAX),
        kept_changes: u32::try_from(fitted.kept_items).unwrap_or(u32::MAX),
        recorded_changes: u32::try_from(fitted.exact_items).unwrap_or(u32::MAX),
    })
}

/// What the operator reads when the timeline could not be written (a real storage failure).
fn failure_text(folder: Option<&PathBuf>) -> String {
    match folder {
        Some(folder) => format!(
            "Could not save the looks to the timeline. The raw recording is kept in {}.",
            folder.display()
        ),
        None => "Could not save the looks to the timeline.".to_string(),
    }
}

/// How often the capture is trimmed further when the song's timeline has no room for it.
const MAX_SINK_RETRIES: usize = 8;

/// "Saved N looks to X's timeline." (and "M loops" when effects were found). When the song was
/// too busy to keep as recorded it says how: fitted at a looser tolerance, or simplified (thinned
/// or trimmed) with how many of the changes were kept.
fn saved_message(
    count: usize,
    loops: usize,
    title: &str,
    scale: f64,
    simplified: Option<&CaptureSimplified>,
) -> String {
    let plural = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    let what = if loops == 0 {
        plural(count, "look")
    } else {
        format!("{} and {}", plural(count, "look"), plural(loops, "loop"))
    };
    match simplified {
        Some(s) if s.stage == "thinned" || s.stage == "trimmed" => format!(
            "Saved {what} to {title}'s timeline. The song was very busy, so it was simplified: kept {} of {} changes.",
            s.kept_changes, s.recorded_changes
        ),
        _ if scale > 1.0 => format!("Saved {what} to {title}'s timeline (fitted loosely: tolerance x{scale}, the song was very large)."),
        _ => format!("Saved {what} to {title}'s timeline."),
    }
}

// ------------------------------------------------------- Python-flavoured JSON coercions
// The agent's JSON is read the way the Python link reads it (`int()`, `float()`, `bool()`,
// `str()`); anything Python would raise on is an `HttpFailure::Other`.

/// The helper's `GET /health` as the strip reports it. Anything missing is just absent: a 0.4.x
/// helper sends none of the new keys, which makes it "older than 0.5.0".
fn parse_health(body: &Value) -> LightkeyHelper {
    let text = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
    };
    let version = text(body.get("version"));
    let map = body.get("fixture_map").filter(|m| m.is_object());
    let watch = body.get("watch").filter(|w| w.is_object());
    LightkeyHelper {
        outdated: helper_is_outdated(version.as_deref()),
        version,
        project: clean_project(text(map.and_then(|m| m.get("project"))).as_deref()),
        fixtures: map
            .and_then(|m| m.get("count"))
            .and_then(Value::as_u64)
            .map(|n| u32::try_from(n).unwrap_or(u32::MAX)),
        map: map.and_then(|m| m.get("map")).and_then(Value::as_i64),
        watch_state: text(watch.and_then(|w| w.get("state"))),
        watch_detail: text(watch.and_then(|w| w.get("detail"))),
    }
}

fn refresh_failure(code: &str, message: &str) -> LightkeyReadFixtures {
    LightkeyReadFixtures {
        ok: false,
        message: message.into(),
        project: None,
        count: None,
        changed: None,
        code: Some(code.into()),
    }
}

/// The helper's answer to `POST /fixtures/refresh` in words (see CONTRACT.md): 200 with
/// `changed`, or 403/409/422 with a `code` and a plain-language `error` that is passed through.
/// A helper without the endpoint answers 404 or 405.
pub fn interpret_refresh(status: u16, body: &Value) -> LightkeyReadFixtures {
    let flag = body.get("ok").and_then(Value::as_bool);
    if (200..300).contains(&status) && flag != Some(false) {
        let project = clean_project(body.get("project").and_then(Value::as_str));
        let count = body
            .get("count")
            .and_then(Value::as_u64)
            .map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX));
        let changed = body.get("changed").and_then(Value::as_bool).unwrap_or(true);
        let message = if !changed {
            format!("No change: {}", fixtures_word(count))
        } else if let Some(name) = &project {
            format!("Read {} from {name}", fixtures_word(count))
        } else {
            format!("Read {}", fixtures_word(count))
        };
        return LightkeyReadFixtures {
            ok: true,
            message,
            project,
            count: Some(count),
            changed: Some(changed),
            code: None,
        };
    }
    if status == 404 || status == 405 {
        return refresh_failure("old_helper", "Update the helper to use this.");
    }
    let code = body
        .get("code")
        .and_then(Value::as_str)
        .filter(|c| !c.is_empty())
        .unwrap_or("failed");
    let message = body
        .get("error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map_or_else(
            || format!("The Lightkey helper could not read the fixtures (HTTP {status})."),
            str::to_string,
        );
    refresh_failure(code, &message)
}

fn py_int(value: &Value) -> Result<i64, HttpFailure> {
    let bad = || HttpFailure::Other(format!("not an integer: {value}"));
    match value {
        Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f.trunc() as i64))
            .ok_or_else(bad),
        Value::String(s) => s.trim().parse::<i64>().map_err(|_| bad()),
        Value::Bool(b) => Ok(i64::from(*b)),
        _ => Err(bad()),
    }
}

fn py_float(value: &Value) -> Result<f64, HttpFailure> {
    let bad = || HttpFailure::Other(format!("not a number: {value}"));
    match value {
        Value::Number(n) => n.as_f64().ok_or_else(bad),
        Value::String(s) => s.trim().parse::<f64>().map_err(|_| bad()),
        Value::Bool(b) => Ok(f64::from(u8::from(*b))),
        _ => Err(bad()),
    }
}

fn py_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

fn py_str(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Null => "None".into(),
        other => other.to_string(),
    }
}
