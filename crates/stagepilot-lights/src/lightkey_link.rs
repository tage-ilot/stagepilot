//! Lightkey link: connect to `lightkey-api`, record live cue changes against a song, replay them.
//! Port of `plugins/lights/lightkey.py`.
//!
//! Owned by `LightsPlugin` (started and stopped with it) so it adds no new plugin or setting page.
//! It polls `GET {api}/log` on the Lightkey Mac, which reports every change of the active-cue set
//! with a timestamp taken from Lightkey's own DMX output. Those timestamps are converted to this
//! machine's clock with a measured offset and then to song time. The pure rules live in
//! `crate::lightkey`; this file is the thin async shell around them. The clock and the HTTP client
//! are injected (`Clock`, `LightkeyHttp`), so everything is testable offline.

use crate::lightkey::{
    build_take_edges, diff, effective_api_url, osc_address, replay_plan, summarize,
    take_is_worth_keeping, truncate_micros, LightkeyLink as Link, LightkeyStatus, LightkeyTake,
    LightkeyTakeSummary, Observation, OperatorHold, ReplayCommand, REPLAY_LEAD_SECONDS,
};
use crate::lightkey_fixtures::{
    clear_address, fixture_replay_plan, FixtureCommand, FixtureRecorder, FixtureReplay,
};
use crate::lightkey_relay::{relay_target, RelayClient};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use stagepilot_core::bus::{EventBus, Subscription};
use stagepilot_core::config::{LightkeySettings, OscArgument, DEFAULT_LIGHTKEY_RELAY_PORT};
use stagepilot_core::events::{EventPayload, EventType, StagePilotEvent, TimerPayload};
use stagepilot_core::state::Song;
use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

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

fn utc_from_wall(wall: f64) -> DateTime<Utc> {
    truncate_micros(
        DateTime::from_timestamp_micros((wall * 1e6).round() as i64).unwrap_or_default(),
    )
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

/// Send one OSC message (address only: replay never sends an argument) through the saved output.
pub type SendOsc =
    Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>> + Send + Sync>;

/// Sends one OSC message with arguments through the saved Lighting output (fixture replay).
pub type SendCommand = Arc<
    dyn Fn(String, Vec<OscArgument>) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>>
        + Send
        + Sync,
>;

/// `(lighting enabled, method is OSC, OSC host, Lightkey settings)`.
pub type LinkSettings = (bool, bool, String, LightkeySettings);
pub type SettingsProvider = Arc<dyn Fn() -> LinkSettings + Send + Sync>;

// -------------------------------------------------------------------------------- take stores

pub trait TakeStore: Send + Sync {
    fn load(&self) -> Result<BTreeMap<String, LightkeyTake>, std::io::Error>;
    fn save(&self, takes: &BTreeMap<String, LightkeyTake>) -> Result<(), std::io::Error>;
}

#[derive(Default)]
pub struct MemoryTakeStore {
    takes: Mutex<BTreeMap<String, LightkeyTake>>,
}

impl MemoryTakeStore {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn takes(&self) -> BTreeMap<String, LightkeyTake> {
        self.takes.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
    pub fn set(&self, takes: BTreeMap<String, LightkeyTake>) {
        *self.takes.lock().unwrap_or_else(|p| p.into_inner()) = takes;
    }
}

impl TakeStore for MemoryTakeStore {
    fn load(&self) -> Result<BTreeMap<String, LightkeyTake>, std::io::Error> {
        Ok(self.takes())
    }
    fn save(&self, takes: &BTreeMap<String, LightkeyTake>) -> Result<(), std::io::Error> {
        self.set(takes.clone());
        Ok(())
    }
}

/// `_TakeFile`.
#[derive(Serialize, Deserialize)]
struct TakeFile {
    #[serde(default = "one")]
    version: i64,
    #[serde(default)]
    takes: Vec<LightkeyTake>,
}

fn one() -> i64 {
    1
}

/// All captures in one atomically written JSON file next to the settings file.
pub struct FileTakeStore {
    pub path: PathBuf,
}

impl FileTakeStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn bad_path(&self) -> PathBuf {
        let mut name = self.path.file_name().unwrap_or_default().to_os_string();
        name.push(".bad");
        self.path.with_file_name(name)
    }

    fn parse(text: &str) -> Option<BTreeMap<String, LightkeyTake>> {
        let mut file: TakeFile = serde_json::from_str(text).ok()?;
        let mut takes = BTreeMap::new();
        for take in &mut file.takes {
            take.normalise_and_validate().ok()?;
        }
        for take in file.takes {
            takes.insert(take.song_key.clone(), take);
        }
        Some(takes)
    }
}

/// `model_dump_json(indent=1)`: one-space indent.
fn to_indented_json(value: &impl Serialize) -> Result<String, std::io::Error> {
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    value
        .serialize(&mut serializer)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    String::from_utf8(out).map_err(|e| std::io::Error::other(e.to_string()))
}

impl TakeStore for FileTakeStore {
    fn load(&self) -> Result<BTreeMap<String, LightkeyTake>, std::io::Error> {
        if !self.path.exists() {
            return Ok(BTreeMap::new());
        }
        let parsed = std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| Self::parse(&text));
        match parsed {
            Some(takes) => Ok(takes),
            None => {
                // Never block startup on a bad capture file: keep it for inspection, start empty.
                let _ = std::fs::rename(&self.path, self.bad_path());
                Ok(BTreeMap::new())
            }
        }
    }

    fn save(&self, takes: &BTreeMap<String, LightkeyTake>) -> Result<(), std::io::Error> {
        let mut sorted: Vec<LightkeyTake> = takes.values().cloned().collect();
        sorted.sort_by(|a, b| a.song_key.cmp(&b.song_key));
        let text = to_indented_json(&TakeFile {
            version: 1,
            takes: sorted,
        })?;
        let name = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let temporary = self
            .path
            .with_file_name(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
        let result = (|| {
            if let Some(parent) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            {
                use std::io::Write;
                let mut handle = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temporary)?;
                handle.write_all(text.as_bytes())?;
                handle.flush()?;
                handle.sync_all()?;
            }
            std::fs::rename(&temporary, &self.path)
        })();
        let _ = remove_if_exists(&temporary);
        result
    }
}

fn remove_if_exists(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

// ------------------------------------------------------------------------------------- state

/// One song being recorded (`_Session`).
struct Session {
    song: Song,
    key: String,
    /// Wall-clock seconds when song time 0 happened.
    started_at: f64,
    baseline: Vec<String>,
    active: std::collections::BTreeSet<String>,
    observed: Vec<Observation>,
    /// Furthest song position reached.
    last_position: f64,
    running: bool,
    /// Fixture changes from the `lightkey-api` relay, when it is connected.
    fixtures: Option<FixtureRecorder>,
}

impl Session {
    fn new(song: Song, started_at: f64, baseline: &std::collections::BTreeSet<String>) -> Self {
        let key = song_key(&song);
        Self {
            song,
            key,
            started_at,
            baseline: baseline.iter().cloned().collect(),
            active: baseline.clone(),
            observed: Vec::new(),
            last_position: 0.0,
            running: true,
            fixtures: None,
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
    takes: BTreeMap<String, LightkeyTake>,
    subscriptions: Vec<Subscription>,
    poll_task: Option<JoinHandle<()>>,
    replay_task: Option<JoinHandle<()>>,
    fixture_task: Option<JoinHandle<()>>,
    relay_task: Option<JoinHandle<()>>,
    /// Fixtures the running fixture replay holds with overrides; cleared when it ends.
    held_fixtures: std::collections::BTreeSet<String>,
    replay_generation: u64,
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
    hold: OperatorHold,
    pending_song: Option<Song>,
}

struct Inner {
    bus: EventBus,
    settings: SettingsProvider,
    send_osc: SendOsc,
    store: Arc<dyn TakeStore>,
    http_factory: HttpFactory,
    clock: Arc<dyn Clock>,
    send_command: Option<SendCommand>,
    relay_port: u16,
    relay: RelayClient,
    state: Mutex<State>,
    wake: Notify,
}

/// Everything `LightkeyLink::new` needs besides the bus, the settings and the OSC sender.
pub struct LinkOptions {
    pub store: Arc<dyn TakeStore>,
    pub http_factory: HttpFactory,
    pub clock: Arc<dyn Clock>,
    /// Sends fixture OSC (with arguments). `None` leaves fixture replay off.
    pub send_command: Option<SendCommand>,
    /// UDP port of the `lightkey-api` relay (21700 unless the agent was configured otherwise).
    pub relay_port: u16,
}

impl Default for LinkOptions {
    fn default() -> Self {
        Self {
            store: Arc::new(MemoryTakeStore::new()),
            http_factory: Arc::new(|| Arc::new(ReqwestLightkeyHttp::new())),
            clock: Arc::new(SystemClock::default()),
            send_command: None,
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
    pub fn new(
        bus: EventBus,
        settings: SettingsProvider,
        send_osc: SendOsc,
        options: LinkOptions,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                bus,
                settings,
                send_osc,
                store: options.store,
                http_factory: options.http_factory,
                clock: options.clock,
                send_command: options.send_command,
                relay_port: options.relay_port,
                relay: RelayClient::new(),
                state: Mutex::new(State {
                    takes: BTreeMap::new(),
                    subscriptions: Vec::new(),
                    poll_task: None,
                    replay_task: None,
                    fixture_task: None,
                    relay_task: None,
                    held_fixtures: Default::default(),
                    replay_generation: 0,
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
                    hold: OperatorHold::new(),
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
    }

    /// Stop a running replay if the saved settings no longer allow it (replay switched off, or
    /// Lighting turned off). Gives the fixtures back to the cues.
    pub async fn refresh_replay(&self) {
        let armed = self.inner.st().armed;
        if self.inner.replay_blocked_reason(armed).is_some() {
            self.inner.cancel_replay().await;
        }
    }

    // ------------------------------------------------------------ status
    pub fn status(&self) -> LightkeyStatus {
        self.inner.status()
    }

    pub fn take_summaries(&self) -> Vec<LightkeyTakeSummary> {
        lock(&self.inner.state)
            .takes
            .values()
            .map(summarize)
            .collect()
    }

    pub fn has_take(&self, song_key: &str) -> bool {
        lock(&self.inner.state).takes.contains_key(song_key)
    }

    // ------------------------------------------------------------ actions
    pub async fn set_recording(&self, on: bool) -> String {
        self.inner.clone().set_recording(on).await
    }

    pub async fn clear_take(&self, song_key: &str) -> bool {
        self.inner.clone().clear_take(song_key).await
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
        let mut takes: Vec<LightkeyTakeSummary> = st.takes.values().map(summarize).collect();
        takes.sort_by_key(|item| item.song_title.to_lowercase());
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
            takes,
            fixtures: self.relay.status(
                st.session
                    .as_ref()
                    .is_some_and(|session| session.fixtures.is_some()),
            ),
        }
    }

    async fn start(self: Arc<Self>) {
        self.st().stopped = false;
        let store = self.store.clone();
        let loaded = tokio::task::spawn_blocking(move || store.load())
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default();
        self.st().takes = loaded;
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
        self.cancel_replay().await;
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
        let mut st = self.st();
        st.session = None;
        st.armed = false;
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
            self.cancel_replay().await;
            return self.say("Recording starts with the next song.");
        }
        let session = {
            let mut st = self.st();
            st.armed = false;
            st.session.take()
        };
        match session {
            Some(session) => self.finish(session, "Recording turned off.").await,
            None => self.say("Recording is off."),
        }
    }

    async fn clear_take(self: Arc<Self>, song_key: &str) -> bool {
        let mut takes = {
            let st = self.st();
            if !st.takes.contains_key(song_key) {
                return false;
            }
            st.takes.clone()
        };
        takes.remove(song_key);
        let (store, to_save) = (self.store.clone(), takes.clone());
        let saved = tokio::task::spawn_blocking(move || store.save(&to_save)).await;
        if !matches!(saved, Ok(Ok(()))) {
            // Python lets the OSError escape the route (a 500); keep the take and say so.
            self.say("Could not save the capture to disk.");
            return false;
        }
        self.st().takes = takes;
        self.say("Capture cleared.");
        true
    }

    // ------------------------------------------------------------ polling
    fn set_link(&self, link: Link, detail: Option<&str>) {
        let mut st = self.st();
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
            let when = match event.get("effective_ms") {
                Some(v) => py_float(v)?,
                None => match state.get("server_time") {
                    Some(v) => py_float(v)?,
                    None => 0.0,
                },
            };
            self.apply_change(names("added")?, names("removed")?, when / 1000.0);
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

    fn apply_change(&self, added: Vec<String>, removed: Vec<String>, lightkey_time: f64) {
        let now = self.clock.mono();
        let mut st = self.st();
        let local_time = lightkey_time - st.offset;
        for cue in added.iter().chain(removed.iter()) {
            if st.hold.note_change(cue, now) {
                tracing::info!(cue = %cue, "lightkey_operator_change");
            }
        }
        let stopped = st.stopped;
        if let Some(session) = st.session.as_mut() {
            if session.running && !stopped {
                let position = local_time - session.started_at;
                for cue in &removed {
                    session.active.remove(cue);
                }
                session.active.extend(added.iter().cloned());
                session.observed.push((position, added, removed));
            }
        }
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
        self.cancel_replay().await;
        let previous = self.st().session.take();
        if let Some(previous) = previous {
            self.finish(previous, "A new song started.").await;
        }
        let started_at = self.clock_from(payload, 0.0);
        let armed = self.st().armed;
        if armed {
            let recording = {
                let mut st = self.st();
                match (&st.active, st.link) {
                    (Some(active), Link::Connected) => {
                        let mut session = Session::new(song.clone(), started_at, active);
                        if self.relay.is_live() {
                            session.fixtures = Some(FixtureRecorder::new(&self.relay.state()));
                        }
                        st.session = Some(session);
                        true
                    }
                    _ => false,
                }
            };
            if !recording {
                self.say("Not recording this song: Lightkey is not connected.");
                return;
            }
            let with_fixtures = self
                .st()
                .session
                .as_ref()
                .is_some_and(|session| session.fixtures.is_some());
            if with_fixtures || self.relay.status(false).is_none() {
                self.say(format!("Recording \"{}\".", song.title));
            } else {
                self.say(format!(
                    "Recording \"{}\". Fixture levels are not recorded: the Lightkey API relay is not connected.",
                    song.title
                ));
            }
            return;
        }
        self.start_replay(&song, payload, 0.0).await;
    }

    async fn on_repositioned(self: &Arc<Self>, event: &StagePilotEvent) {
        let EventPayload::Timer(payload) = &event.payload else {
            return;
        };
        let song = {
            let st = self.st();
            payload.song.clone().or_else(|| st.pending_song.clone())
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
                    if let Some(session) = st.session.as_mut() {
                        if let Some(recorder) = session.fixtures.as_mut() {
                            recorder.observe(position, &self.relay.state().commands());
                        }
                    }
                    if let Some(active) = st.active.clone() {
                        let session = st.session.as_mut();
                        if let Some(session) = session {
                            let (added, removed) = diff(session.active.iter(), active.iter());
                            if !added.is_empty() || !removed.is_empty() {
                                session.observed.push((position, added, removed));
                                session.active = active;
                            }
                        }
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
        match had_session {
            Some(true) => return,
            Some(false) => {
                self.say("Recording discarded: the song was moved.");
                return;
            }
            None => {}
        }
        self.cancel_replay().await;
        if !payload.running {
            return;
        }
        let Some(song) = song else {
            return;
        };
        self.start_replay(&song, payload, position).await;
    }

    async fn on_stopped(self: &Arc<Self>, event: &StagePilotEvent) {
        self.cancel_replay().await;
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
                    let duration = f64::from(session.song.duration_seconds.unwrap_or(0));
                    if take_is_worth_keeping(session.last_position, duration) {
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

    async fn on_reset(self: &Arc<Self>, _event: &StagePilotEvent) {
        self.cancel_replay().await;
        let had = self.st().session.take().is_some();
        if had {
            self.say("Recording discarded: the countdown was reset.");
        }
    }

    async fn finish(self: &Arc<Self>, mut session: Session, reason: &str) -> String {
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
        if !take_is_worth_keeping(covered, duration) {
            return self.say(format!(
                "Not saved: only {covered:.0} of {duration:.0} seconds of \"{}\" were recorded.",
                session.song.title
            ));
        }
        let built = build_take_edges(session.baseline.iter(), &session.observed, duration)
            .and_then(|(initial, edges)| {
                let count = edges.len();
                LightkeyTake::new(
                    session.key.clone(),
                    session.song.title.clone(),
                    utc_from_wall(self.clock.wall()),
                    duration,
                    initial,
                    edges,
                )
                .map(|take| (take, count))
            });
        let Ok((mut take, count)) = built else {
            tracing::error!(song = %session.key, "lightkey_take_invalid");
            return self.say("Could not save the capture to disk.");
        };
        let mut fixture_changes = 0usize;
        if let Some(recorder) = session.fixtures.take() {
            match recorder.finish(duration, false) {
                Ok(Some(track)) => {
                    fixture_changes = track.events.len();
                    take.fixtures = Some(track);
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::error!(song = %session.key, error = %error, "lightkey_fixture_track_invalid");
                }
            }
        }
        let mut takes = self.st().takes.clone();
        takes.insert(session.key.clone(), take);
        let (store, to_save) = (self.store.clone(), takes.clone());
        let saved = tokio::task::spawn_blocking(move || store.save(&to_save)).await;
        if !matches!(saved, Ok(Ok(()))) {
            tracing::error!(song = %session.key, "lightkey_take_save_failed");
            return self.say("Could not save the capture to disk.");
        }
        self.st().takes = takes;
        let fixtures_text = if fixture_changes > 0 {
            format!(" and {fixture_changes} fixture changes")
        } else {
            String::new()
        };
        self.say(format!(
            "Saved \"{}\": {count} changes{fixtures_text}. {reason}",
            session.song.title
        ))
    }

    // ------------------------------------------------------------ replay
    async fn start_replay(self: &Arc<Self>, song: &Song, payload: &TimerPayload, position: f64) {
        let armed = self.st().armed;
        if self.replay_blocked_reason(armed).is_some() {
            return;
        }
        let Some(take) = self.st().takes.get(&song_key(song)).cloned() else {
            return;
        };
        let (sync_now, later) = replay_plan(&take, position);
        let wall = self.clock.wall();
        let wall_started = payload.started_at.map_or(wall, datetime_seconds);
        let elapsed = (wall - wall_started).max(0.0);
        let clock_zero = self.clock.mono() - elapsed - position;
        let mut st = self.st();
        st.replay_generation += 1;
        let generation = st.replay_generation;
        st.replaying = Some(song.title.clone());
        let task = tokio::spawn(self.clone().run_replay(
            generation,
            sync_now,
            later,
            clock_zero,
            take.duration_seconds,
        ));
        st.replay_task = Some(task);
        if let (Some(track), Some(_)) = (take.fixtures.as_ref(), self.send_command.as_ref()) {
            let plan = fixture_replay_plan(track, position);
            st.held_fixtures.extend(plan.fixtures.iter().cloned());
            let task = tokio::spawn(self.clone().run_fixture_replay(
                generation,
                plan,
                track.lead_seconds,
                clock_zero,
                take.duration_seconds,
            ));
            st.fixture_task = Some(task);
        }
    }

    /// Send the fixture track: the state at the position at once, then each change `lead`
    /// seconds before its time. Every fixture it touched is given back to the cues at the end.
    async fn run_fixture_replay(
        self: Arc<Self>,
        generation: u64,
        plan: FixtureReplay,
        lead: f64,
        clock_zero: f64,
        song_seconds: f64,
    ) {
        let outcome: Result<(), String> = async {
            for command in &plan.sync_now {
                self.send_fixture(command).await?;
            }
            for event in &plan.later {
                let delay = clock_zero + event.at_seconds - lead - self.clock.mono();
                if delay > 0.0 {
                    tokio::time::sleep(Duration::from_secs_f64(delay)).await;
                }
                for command in &event.osc {
                    self.send_fixture(command).await?;
                }
            }
            let rest = (clock_zero + song_seconds - self.clock.mono()).max(0.0);
            tokio::time::sleep(Duration::from_secs_f64(rest)).await;
            Ok(())
        }
        .await;
        if let Err(error) = outcome {
            tracing::error!(error = %error, "lightkey_fixture_replay_failed");
            self.say("A fixture command could not be sent.");
        }
        if self.st().replay_generation == generation {
            self.release_fixtures().await;
        }
    }

    async fn send_fixture(&self, command: &FixtureCommand) -> Result<(), String> {
        match &self.send_command {
            Some(send) => send(command.a.clone(), command.osc_args()).await,
            None => Ok(()),
        }
    }

    /// Give every fixture replay held back to the cues. One clear per fixture, repeated once:
    /// a single wildcard clear was seen to leave fixtures held on a real rig.
    async fn release_fixtures(&self) {
        let held: Vec<String> = std::mem::take(&mut self.st().held_fixtures)
            .into_iter()
            .collect();
        let Some(send) = self.send_command.clone() else {
            return;
        };
        for round in 0..2 {
            for name in &held {
                if let Err(error) = send(clear_address(name), Vec::new()).await {
                    tracing::warn!(fixture = %name, error = %error, "lightkey_fixture_clear_failed");
                }
            }
            if round == 0 && !held.is_empty() {
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
        }
    }

    async fn run_replay(
        self: Arc<Self>,
        generation: u64,
        sync_now: Vec<ReplayCommand>,
        later: Vec<ReplayCommand>,
        clock_zero: f64,
        song_seconds: f64,
    ) {
        let outcome: Result<(), String> = async {
            for command in &sync_now {
                self.send(command).await?;
            }
            for command in &later {
                let delay =
                    clock_zero + command.at_seconds - REPLAY_LEAD_SECONDS - self.clock.mono();
                if delay > 0.0 {
                    tokio::time::sleep(Duration::from_secs_f64(delay)).await;
                }
                self.send(command).await?;
            }
            // Stay "replaying" until the song's end (or a stop), so the strip does not go quiet
            // while the song is still playing.
            let rest = (clock_zero + song_seconds - self.clock.mono()).max(0.0);
            tokio::time::sleep(Duration::from_secs_f64(rest)).await;
            Ok(())
        }
        .await;
        if let Err(error) = outcome {
            tracing::error!(error = %error, "lightkey_replay_failed");
            self.say("A replay command could not be sent.");
        }
        let mut st = self.st();
        if st.replay_generation == generation {
            st.replaying = None;
        }
    }

    async fn send(&self, command: &ReplayCommand) -> Result<(), String> {
        let now = self.clock.mono();
        {
            let mut st = self.st();
            if st.hold.is_held(&command.cue, now) {
                return Ok(()); // the operator touched this cue a moment ago: leave it with them
            }
            st.hold.note_sent(&command.cue, now);
        }
        (self.send_osc)(osc_address(&command.cue, command.verb)).await
    }

    async fn cancel_replay(&self) {
        let task = {
            let mut st = self.st();
            st.replay_generation += 1;
            st.replaying = None;
            st.replay_task.take()
        };
        if let Some(task) = task {
            task.abort();
            let _ = task.await;
        }
        let fixture_task = self.st().fixture_task.take();
        if let Some(task) = fixture_task {
            task.abort();
            let _ = task.await;
        }
        self.release_fixtures().await;
    }
}

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
