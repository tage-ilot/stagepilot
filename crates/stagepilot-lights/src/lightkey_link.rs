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

use crate::lightkey::{
    effective_api_url, recording_is_worth_keeping, LightkeyCaptured, LightkeyLink as Link,
    LightkeyStatus,
};
use crate::lightkey_fixtures::FixtureCommand;
use crate::lightkey_looks::{look_rows, LookRecorder};
use crate::audio::{AudioHub, AudioInput, SharedRecorder};
use crate::lightkey_relay::{relay_target, RelayClient};
use crate::loudness::LoudnessRecorder;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use stagepilot_core::bus::{EventBus, Subscription};
use stagepilot_core::config::{
    LightkeySettings, OscLightingCue, SongLoudness, DEFAULT_LIGHTKEY_RELAY_PORT,
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
pub const MIN_SEND_GAP: Duration = Duration::from_millis(5);
pub const POLL_SECONDS: f64 = 0.25;
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
}

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
}

/// Why the looks could not be saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LooksError {
    /// The song's timeline would pass the row or message caps: the link tries a coarser Detail.
    TooLarge,
    Failed(String),
}

pub type LooksFuture = Pin<Box<dyn Future<Output = Result<(), LooksError>> + Send>>;

/// Saves a song's captured rows into its cue map through the normal, serialized save path
/// (replacing only the captured rows of that song, keeping manual ones).
pub type LooksSink = Arc<dyn Fn(SongLooks) -> LooksFuture + Send + Sync>;

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
    /// Furthest song position reached.
    last_position: f64,
    running: bool,
    /// Fixture changes from the `lightkey-api` relay, when it is connected.
    fixtures: Option<LookRecorder>,
    /// Loudness windows from the audio input, on the same song clock.
    loudness: Option<SharedRecorder>,
}

impl Session {
    fn new(song: Song, started_at: f64) -> Self {
        let key = song_key(&song);
        Self {
            song,
            key,
            started_at,
            last_position: 0.0,
            running: true,
            fixtures: None,
            loudness: None,
        }
    }
}

fn song_key(song: &Song) -> String {
    song.source_song_id
        .clone()
        .unwrap_or_else(|| song.id.clone())
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
}

struct Inner {
    bus: EventBus,
    settings: SettingsProvider,
    armed_store: Arc<dyn ArmedStore>,
    looks_sink: Option<LooksSink>,
    loudness_sink: Option<LoudnessSink>,
    audio: Option<Arc<AudioHub>>,
    http_factory: HttpFactory,
    clock: Arc<dyn Clock>,
    relay_port: u16,
    relay: RelayClient,
    state: Mutex<State>,
    wake: Notify,
}

/// Everything `LightkeyLink::new` needs besides the bus and the settings.
pub struct LinkOptions {
    /// Remembers the Record switch.
    pub armed_store: Arc<dyn ArmedStore>,
    /// Where a finished recording goes (the song's timeline). `None` saves nothing.
    pub looks_sink: Option<LooksSink>,
    /// Where a recording's loudness goes. `None` saves none.
    pub loudness_sink: Option<LoudnessSink>,
    /// The audio input that measures loudness. `None` records no loudness (headless builds).
    pub audio: Option<Arc<dyn AudioInput>>,
    pub http_factory: HttpFactory,
    pub clock: Arc<dyn Clock>,
    /// UDP port of the `lightkey-api` relay (21700 unless the agent was configured otherwise).
    pub relay_port: u16,
}

impl Default for LinkOptions {
    fn default() -> Self {
        Self {
            armed_store: Arc::new(MemoryArmedStore::new()),
            looks_sink: None,
            loudness_sink: None,
            audio: None,
            http_factory: Arc::new(|| Arc::new(ReqwestLightkeyHttp::new())),
            clock: Arc::new(SystemClock::default()),
            relay_port: DEFAULT_LIGHTKEY_RELAY_PORT,
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
                loudness_sink: options.loudness_sink,
                audio: options
                    .audio
                    .map(|input| Arc::new(AudioHub::new(input, options.clock.clone()))),
                http_factory: options.http_factory,
                clock: options.clock,
                relay_port: options.relay_port,
                relay: RelayClient::new(),
                state: Mutex::new(State {
                    captured: None,
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
        }
        self.inner.relay.reset();
        self.inner.wake.notify_one();
        self.inner.sync_audio();
    }

    /// The audio input hub (picker, meter), when this build can listen to an input.
    pub fn audio(&self) -> Option<Arc<AudioHub>> {
        self.inner.audio.clone()
    }

    /// Whether the Record switch is on (replay never runs while it is).
    pub fn armed(&self) -> bool {
        lock(&self.inner.state).armed
    }

    /// The song the timeline is replaying fixture looks for (shown in the status), if any.
    pub fn set_replaying(&self, song: Option<String>) {
        lock(&self.inner.state).replaying = song;
    }

    // ------------------------------------------------------------ status
    pub fn status(&self) -> LightkeyStatus {
        self.inner.status()
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

    fn replay_blocked_reason(&self, armed: bool) -> Option<String> {
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
        if !settings.replay {
            return Some("Replay is switched off.".into());
        }
        None
    }

    fn status(&self) -> LightkeyStatus {
        let (enabled, _osc, osc_host, settings) = (self.settings)();
        let mut st = self.st();
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
            replay_blocked: self.replay_blocked_reason(st.armed),
            replaying_song: st.replaying.clone(),
            message,
            captured: st.captured.clone(),
            fixtures: self.relay.status(
                st.session
                    .as_ref()
                    .is_some_and(|session| session.fixtures.is_some()),
            ),
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
        self.sync_audio();
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
        let mut st = self.st();
        if st.link != link {
            tracing::info!(link = ?link, "lightkey_link_changed");
        }
        st.link = link;
        st.detail = detail.map(str::to_string);
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
            }
            tokio::select! {
                () = self.wake.notified() => {}
                () = tokio::time::sleep(Duration::from_secs_f64(delay)) => {}
            }
        }
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
                Arc::new(move |local: f64, commands: Vec<FixtureCommand>| {
                    this.apply_fixture_change(local, &commands);
                })
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
                )
                .await;
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
    fn apply_fixture_change(&self, local_time: f64, commands: &[FixtureCommand]) {
        let mut st = self.st();
        let stopped = st.stopped;
        if let Some(session) = st.session.as_mut() {
            if session.running && !stopped {
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
        let body = http
            .get_json(&format!("{base}/log"), &[("since", since.to_string())])
            .await?;
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

    /// Clock offset from the lowest-round-trip of several samples (NTP style).
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
            if best.is_none_or(|(best_rtt, _)| rtt < best_rtt) {
                best = Some((rtt, remote - (sent + received) / 2.0));
            }
        }
        if let Some((_, offset)) = best {
            let mut st = self.st();
            st.offset = offset;
            st.offset_at = self.clock.mono();
        }
        Ok(())
    }

    // ------------------------------------------------------------ timer events
    async fn on_song(self: &Arc<Self>, event: &StagePilotEvent) {
        if let EventPayload::Song { song, .. } = &event.payload {
            self.st().pending_song = Some(song.clone());
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
        let previous = self.st().session.take();
        if let Some(previous) = previous {
            self.finish(previous, "A new song started.").await;
        }
        let started_at = self.clock_from(payload, 0.0);
        let armed = self.st().armed;
        if armed {
            let recording = {
                let mut st = self.st();
                if st.link == Link::Connected {
                    let mut session = Session::new(song.clone(), started_at);
                    if self.relay.is_live() {
                        session.fixtures = Some(LookRecorder::new(&self.relay.state()));
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
            let audio_problem = self.sync_audio();
            let with_fixtures = self
                .st()
                .session
                .as_ref()
                .is_some_and(|session| session.fixtures.is_some());
            let mut text = if with_fixtures || self.relay.status(false).is_none() {
                format!("Recording {}.", song.title)
            } else {
                format!(
                    "Recording {}. Fixture levels are not recorded: the Lightkey API relay is not connected.",
                    song.title
                )
            };
            if let Some(problem) = audio_problem {
                text.push_str(&format!(" Loudness is not recorded: {problem}"));
            }
            self.say(text);
        }
    }

    async fn on_repositioned(self: &Arc<Self>, event: &StagePilotEvent) {
        let EventPayload::Timer(payload) = &event.payload else {
            return;
        };
        let position = payload.position_seconds.unwrap_or(0.0).max(0.0);
        let had_session = {
            let mut st = self.st();
            if let Some(session) = st.session.as_mut() {
                let same_place =
                    (position - session.last_position).abs() <= RESUME_TOLERANCE_SECONDS;
                if event.event_type == EventType::TimerResumed && payload.running && same_place {
                    // Paused and resumed in place: carry on, and catch up on anything changed
                    // meanwhile.
                    session.started_at = payload
                        .started_at
                        .map_or_else(|| self.clock.wall(), datetime_seconds)
                        - position;
                    session.running = true;
                    if let Some(recorder) = session.fixtures.as_mut() {
                        recorder.observe(position, &self.relay.state().commands());
                    }
                    if let Some(loudness) = &session.loudness {
                        let mut loudness = loudness.lock().unwrap_or_else(|p| p.into_inner());
                        loudness.set_started_at(session.started_at);
                        loudness.set_running(true);
                    }
                    Some(true)
                } else {
                    st.session = None;
                    Some(false)
                }
            } else {
                None
            }
        };
        if had_session == Some(false) {
            self.say("Recording discarded: the song was moved.");
        }
        self.sync_audio();
    }

    /// Pause, stop, song end and a failed timer: a recording session pauses or is kept.
    async fn on_stopped(self: &Arc<Self>, event: &StagePilotEvent) {
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
                    session.last_position = session.last_position.max(position);
                    session.running = false;
                    if let Some(loudness) = &session.loudness {
                        loudness
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .set_running(false);
                    }
                    let duration = f64::from(session.song.duration_seconds.unwrap_or(0));
                    if recording_is_worth_keeping(session.last_position, duration) {
                        match st.session.take() {
                            Some(session) => Next::Finish(Box::new(session)),
                            None => Next::Nothing,
                        }
                    } else {
                        Next::Paused(session.song.title.clone())
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

    /// Reset discards a recording in progress.
    async fn on_reset(self: &Arc<Self>, _event: &StagePilotEvent) {
        let had = self.st().session.take().is_some();
        if had {
            self.say("Recording discarded: the countdown was reset.");
        }
        self.sync_audio();
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
    fn sync_audio(&self) -> Option<String> {
        let hub = self.audio.as_ref()?;
        let recorder = self.st().session.as_ref().and_then(|s| s.loudness.clone());
        hub.set_device((self.settings)().3.audio_device.clone());
        hub.set_recorder(recorder).err()
    }

    /// A finished recording's loudness goes to the song, under the same keep-it rule as the
    /// looks (at least half the song). The line is added to the status; never an error.
    async fn save_loudness(self: &Arc<Self>, session: &mut Session) -> Option<String> {
        let recorder = session.loudness.take()?;
        let problem = self.audio.as_ref().and_then(|hub| hub.info().reason);
        self.sync_audio();
        let duration = f64::from(session.song.duration_seconds.unwrap_or(0));
        let mut covered = session.last_position;
        if session.running {
            covered = covered.max(self.clock.wall() - session.started_at);
        }
        let covered = if duration > 0.0 { covered.min(duration) } else { covered };
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

    async fn finish_looks(self: &Arc<Self>, mut session: Session, reason: &str) -> String {
        let duration = f64::from(session.song.duration_seconds.unwrap_or(0));
        if session.running {
            session.last_position = session
                .last_position
                .max(self.clock.wall() - session.started_at);
        }
        let covered = if duration > 0.0 {
            session.last_position.min(duration)
        } else {
            session.last_position
        };
        if !recording_is_worth_keeping(covered, duration) {
            return self.say(format!(
                "Not saved: only {covered:.0} of {duration:.0} seconds recorded."
            ));
        }
        let title = session.song.title.clone();
        let Some(recorder) = session.fixtures.take() else {
            return self.say(format!(
                "Not saved: no fixture changes were recorded for {title}."
            ));
        };
        if recorder.is_empty() {
            return self.say(format!(
                "Not saved: no fixture changes were recorded for {title}."
            ));
        }
        let Some(sink) = self.looks_sink.clone() else {
            return self.say("Could not save the looks to the timeline.");
        };
        let recorder = Arc::new(recorder);
        let mut interval = (self.settings)().3.detail.interval_seconds();
        // A song that would pass the timeline caps is saved coarser, never dropped.
        for _ in 0..MAX_COARSER_TRIES {
            let thin = recorder.clone();
            let rows =
                tokio::task::spawn_blocking(move || look_rows(&thin.looks(duration, interval)))
                    .await
                    .unwrap_or_default();
            if rows.is_empty() {
                return self.say(format!(
                    "Not saved: no fixture changes were recorded for {title}."
                ));
            }
            let count = rows.len();
            let saved = sink(SongLooks {
                song_key: session.key.clone(),
                song_title: title.clone(),
                rows,
            })
            .await;
            match saved {
                Ok(()) => {
                    tracing::info!(song = %session.key, rows = count, reason, "lightkey_looks_saved");
                    self.st().captured = Some(LightkeyCaptured {
                        song_title: title.clone(),
                        looks: u32::try_from(count).unwrap_or(u32::MAX),
                    });
                    return self.say(format!(
                        "Saved {count} look{} to {title}'s timeline.",
                        if count == 1 { "" } else { "s" }
                    ));
                }
                Err(LooksError::TooLarge) => interval *= 2.0,
                Err(LooksError::Failed(error)) => {
                    tracing::error!(song = %session.key, error = %error, "lightkey_looks_save_failed");
                    return self.say("Could not save the looks to the timeline.");
                }
            }
        }
        self.say("Could not save the looks to the timeline: the song has too many rows.")
    }
}

/// How many times a recording too large for the timeline is retried at twice the interval.
const MAX_COARSER_TRIES: usize = 6;

// ------------------------------------------------------- Python-flavoured JSON coercions
// The agent's JSON is read the way the Python link reads it (`int()`, `float()`, `bool()`,
// `str()`); anything Python would raise on is an `HttpFailure::Other`.

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
