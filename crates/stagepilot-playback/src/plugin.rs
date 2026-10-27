//! The Playback input plugin: port of `plugins/playback_api/plugin.py` (`PlaybackInputPlugin`),
//! as a supervised plugin of `stagepilot-core`'s [`PluginManager`].
//!
//! It owns the connection lifecycle (client start/stop/reconfigure), normalises what Playback
//! sends into [`Observation`]s, lets the source arbiter decide whether the Playback API or MIDI
//! drives StagePilot, executes the mapped action through the arbitrated dispatcher, and publishes
//! `connection.changed` onto the event bus. Scanning, song-order discovery and the settings it
//! persists are here too; the HTTP routes that call them are a later card.
//!
//! Differences from Python that are on purpose (see `docs/archive/rust-migration/phase3b.md`):
//! * Python's `MidiPlaybackPlugin` wrapper (`midi_source = real`) is the MIDI input plugin in `stagepilot-midi`;
//!   this plugin only exposes [`PlaybackPlugin::set_midi_connected`] and
//!   [`PlaybackPlugin::midi_guard`] for it to plug into.
//! * Planning Center length writing is its own service; this plugin only exposes the
//!   `lengths_running` flag it needs ([`PlaybackPlugin::set_lengths_running`]).
//! * Mapper decisions are synchronous (state is a snapshot read), so the Python "re-check the
//!   generation after the await" steps collapse into one atomic decision.

use crate::arbiter::{
    monotonic_clock, ActionDispatcher, ActionName as PbAction, ActionOutcome as PbOutcome,
    ArbitratedDispatcher, PlaybackConnection, PlaybackSourceArbiter, Source, StateView, TimerCall,
};
use crate::client::{ConnectionOptions, PlaybackClient, PlaybackStatus};
use crate::discovery::{discover_song_order, DiscoveryError, DiscoveryOptions};
use crate::find::{
    scan_network, CancelToken, ScanCandidate, ScanHooks, ScanOutcome, ScanReport, SystemNetwork,
    DEFAULT_TIMEOUT, SCAN_BLOCK_DETAIL,
};
use crate::mapping::{Mapper, Observation, Plan};
use crate::normalizer::{Heartbeat, PlaybackEvent, SetlistId};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use stagepilot_core::bus::EventBus;
use stagepilot_core::config::{MidiSource, PlaybackApiSettings, SetlistId as CfgSetlistId};
use stagepilot_core::events::{new_event, EventPayload, EventType, Integration};
use stagepilot_core::plugin::Plugin;
use stagepilot_core::settings::{PersistentSettings, SettingsService};
use stagepilot_core::state::{ConnectionStatus, PluginHealth, PluginStatus, ServicePlan};
use stagepilot_core::state_service::StateService;
use stagepilot_core::store::StateStore;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex as AsyncMutex, Notify};
use tokio::task::JoinHandle;

pub const PLUGIN_NAME: &str = "playback_api";
pub const PLUGIN_VERSION: &str = "0.1.0";
pub const NOT_FOUND_REASON: &str = "Couldn't find Playback. Check that Playback is open, Remote Connections is on, and both computers are on the same network.";
pub const UNREACHABLE_REASON: &str =
    "Looking for Playback… If it stays offline, turn on Remote Connections.";
/// Opens the macOS Local Network privacy pane.
pub const LOCAL_NETWORK_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork";
const QUEUE_CAPACITY: usize = 64;
const HISTORY_CAPACITY: usize = 100;
const LOG_CAPACITY: usize = 200;
const TICK: Duration = Duration::from_millis(50);

/// Plain-language message and ONE concrete next step per scan failure class.
pub fn scan_message(error_class: &str) -> String {
    match error_class {
        "permission_denied" => format!(
            "{SCAN_BLOCK_DETAIL}. Quit StagePilot, open System Settings > Privacy & Security > Local Network, turn StagePilot off and on again, then reopen StagePilot and scan again."
        ),
        "no_route" => "This computer has no route to that address. Check that it is on the same Wi-Fi or network as Playback.".into(),
        "refused" => "That computer answered, but Playback isn't accepting connections. In Playback, turn on Remote Connections.".into(),
        "timed_out" => "Nothing answered in time. Check the address and that Playback is open on that computer.".into(),
        "not_playback" => "Something answered, but it didn't behave like Playback. Check the address, and that Remote Connections is on in Playback.".into(),
        "invalid_address" => "That isn't a valid address. Enter a computer name or an address like 192.0.2.10.".into(),
        _ => "Something unexpected went wrong while scanning. Copy the diagnostic details.".into(),
    }
}

static RECENT_LOG: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

/// INFO log plus a bounded in-memory copy for the diagnostics bundle.
pub fn playback_log(event: &str, fields: Value) {
    let mut line = json!({ "event": event });
    if let (Some(a), Some(b)) = (line.as_object_mut(), fields.as_object()) {
        for (k, v) in b {
            a.insert(k.clone(), v.clone());
        }
    }
    let text = line.to_string();
    tracing::info!(target: "playback", "{text}");
    let mut log = RECENT_LOG.lock().unwrap_or_else(|p| p.into_inner());
    log.push_back(text);
    while log.len() > LOG_CAPACITY {
        log.pop_front();
    }
}

pub fn recent_log() -> Vec<String> {
    RECENT_LOG
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Python-compatible keys (`json.dumps` default separators and ASCII escaping)
// ---------------------------------------------------------------------------------------------

fn py_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `json.dumps(value)` for the shapes the scan/applied keys use.
fn py_dumps(v: &Value) -> String {
    match v {
        Value::Array(a) => format!(
            "[{}]",
            a.iter().map(py_dumps).collect::<Vec<_>>().join(", ")
        ),
        Value::String(s) => py_str(s),
        other => other.to_string(),
    }
}

/// Identity of the plan the scanned lengths were measured for (not its durations).
pub fn plan_key(plan: &ServicePlan) -> String {
    py_dumps(&json!([
        plan.service_type_id,
        plan.id,
        plan.songs.iter().map(|s| s.id.clone()).collect::<Vec<_>>()
    ]))
}

fn isoformat(at: DateTime<Utc>) -> String {
    let micros = at.timestamp_subsec_micros();
    let base = at.format("%Y-%m-%dT%H:%M:%S");
    if micros == 0 {
        format!("{base}+00:00")
    } else {
        format!("{base}.{micros:06}+00:00")
    }
}

fn to_cfg(id: &SetlistId) -> CfgSetlistId {
    match id {
        SetlistId::Int(i) => CfgSetlistId::Int(*i),
        SetlistId::Str(s) => CfgSetlistId::Text(s.clone()),
    }
}

fn from_cfg(id: &CfgSetlistId) -> SetlistId {
    match id {
        CfgSetlistId::Int(i) => SetlistId::Int(*i),
        CfgSetlistId::Text(s) => SetlistId::Str(s.clone()),
    }
}

// ---------------------------------------------------------------------------------------------
// Errors, snapshots
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybackError {
    /// The operator must resolve a live-state conflict first (Python `DiscoveryConflict`).
    Conflict(String),
    /// Rejected input (Python `ValueError`).
    Invalid(String),
    /// The settings file could not be written.
    Save(String),
}

impl std::fmt::Display for PlaybackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlaybackError::Conflict(m) | PlaybackError::Invalid(m) | PlaybackError::Save(m) => {
                f.write_str(m)
            }
        }
    }
}

impl std::error::Error for PlaybackError {}

fn conflict<T>(message: &str) -> Result<T, PlaybackError> {
    Err(PlaybackError::Conflict(message.to_string()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanState {
    Idle,
    Scanning,
    Found,
    NotFound,
}

impl ScanState {
    pub fn as_str(self) -> &'static str {
        match self {
            ScanState::Idle => "idle",
            ScanState::Scanning => "scanning",
            ScanState::Found => "found",
            ScanState::NotFound => "not_found",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryState {
    Idle,
    Running,
    Failed,
    Done,
}

impl DiscoveryState {
    pub fn as_str(self) -> &'static str {
        match self {
            DiscoveryState::Idle => "idle",
            DiscoveryState::Running => "running",
            DiscoveryState::Failed => "failed",
            DiscoveryState::Done => "done",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScanSnapshot {
    pub state: ScanState,
    pub candidates: Vec<ScanCandidate>,
    pub reason: Option<String>,
    pub current: usize,
    pub total: usize,
    pub phase: Option<String>,
    pub error_class: Option<String>,
    pub typed: bool,
    pub elapsed: f64,
    pub last_scan: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct DiscoverySnapshot {
    pub state: DiscoveryState,
    pub progress: usize,
    pub song: usize,
    pub total: usize,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MonitorEntry {
    pub event: PlaybackEvent,
    pub discovery: bool,
    pub at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------------------------
// Seams for tests
// ---------------------------------------------------------------------------------------------

pub type Observer = Arc<dyn Fn(&PlaybackStatus, &[PlaybackEvent]) + Send + Sync>;
pub type ClientFactory =
    Arc<dyn Fn(ConnectionOptions, Observer) -> Result<PlaybackClient, String> + Send + Sync>;

pub fn default_client_factory() -> ClientFactory {
    Arc::new(|options, observer| {
        PlaybackClient::start(options, Some(observer)).map_err(|e| e.to_string())
    })
}

/// The network scan, replaceable in tests.
pub trait Scanner: Send + Sync {
    fn scan(
        &self,
        host: Option<String>,
        port: u16,
        hooks: ScanHooks,
    ) -> Pin<Box<dyn Future<Output = ScanReport> + Send>>;
}

pub struct SystemScanner;

impl Scanner for SystemScanner {
    fn scan(
        &self,
        host: Option<String>,
        port: u16,
        hooks: ScanHooks,
    ) -> Pin<Box<dyn Future<Output = ScanReport> + Send>> {
        Box::pin(async move {
            scan_network(
                &SystemNetwork,
                host.as_deref(),
                port,
                true,
                DEFAULT_TIMEOUT,
                &hooks,
            )
            .await
        })
    }
}

#[derive(Clone)]
pub struct PluginOptions {
    pub client_factory: ClientFactory,
    pub scanner: Arc<dyn Scanner>,
    pub step_timeout: Duration,
    pub max_steps: usize,
}

impl Default for PluginOptions {
    fn default() -> Self {
        Self {
            client_factory: default_client_factory(),
            scanner: Arc::new(SystemScanner),
            step_timeout: Duration::from_secs(5),
            max_steps: 200,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Adapters from the core state service
// ---------------------------------------------------------------------------------------------

pub struct CoreDispatcher(StateService);

impl CoreDispatcher {
    pub fn new(service: StateService) -> Self {
        Self(service)
    }
}

fn outcome(o: stagepilot_core::state_service::ActionOutcome) -> PbOutcome {
    PbOutcome::new(o.accepted, o.message)
}

impl ActionDispatcher for CoreDispatcher {
    async fn dispatch(&self, action: PbAction, source: &str) -> PbOutcome {
        match stagepilot_core::events::ActionName::parse(action.as_str()) {
            Some(a) => outcome(self.0.dispatch(a, source).await),
            None => PbOutcome::new(false, "unknown action"),
        }
    }
    async fn dispatch_song_position(&self, position: i64, source: &str) -> PbOutcome {
        outcome(self.0.dispatch_song_position(position, source).await)
    }
    async fn timer(&self, call: TimerCall, source: &str) -> PbOutcome {
        outcome(match call {
            TimerCall::Resume(s) => self.0.resume_timer_at(s, source).await,
            TimerCall::Seek(s) => self.0.seek_timer(s, source).await,
            TimerCall::Stop(s) => self.0.stop_timer_at(s, source).await,
        })
    }
}

pub struct CoreState(StateStore);

impl CoreState {
    pub fn new(store: StateStore) -> Self {
        Self(store)
    }
}

impl StateView for CoreState {
    async fn current_song_index(&self) -> Option<i64> {
        self.0.snapshot().current_song_index.map(|i| i as i64)
    }
}

// ---------------------------------------------------------------------------------------------
// The plugin
// ---------------------------------------------------------------------------------------------

enum Item {
    Connection,
    Observation(Observation),
}

struct Shared {
    midi_source: MidiSource,
    config: PlaybackApiSettings,
    mapper: Mapper,
    client: Option<Arc<PlaybackClient>>,
    queue: VecDeque<(u64, Item)>,
    history: VecDeque<MonitorEntry>,
    generation: u64,
    running: bool,
    active: bool,
    connection: bool,
    heartbeat: Option<Heartbeat>,
    seen_serial: u64,
    published: Option<(bool, String)>,
    ownership_revision: u64,
    consumer: Option<JoinHandle<()>>,
    ticker: Option<JoinHandle<()>>,
    // scan
    scan: ScanSnapshot,
    scan_started_at: Option<Instant>,
    scan_cancel: CancelToken,
    scan_task: Option<JoinHandle<()>>,
    // discovery
    discovery: DiscoveryState,
    discovery_total: usize,
    discovery_error: Option<String>,
    discovery_cancel: CancelToken,
}

struct Inner {
    bus: EventBus,
    store: StateStore,
    settings: Arc<SettingsService>,
    options: PluginOptions,
    arbiter: Arc<Mutex<PlaybackSourceArbiter>>,
    dispatcher: ArbitratedDispatcher<CoreDispatcher, CoreState>,
    shared: Arc<Mutex<Shared>>,
    wake: Notify,
    lifecycle: AsyncMutex<()>,
    dispatch_lock: AsyncMutex<()>,
    connection_lock: AsyncMutex<()>,
    discovery_run: AsyncMutex<()>,
    lengths_running: AtomicBool,
    progress: Arc<AtomicUsize>,
    song: Arc<AtomicUsize>,
}

/// The Playback input plugin. Cheap to clone; clones share one plugin.
#[derive(Clone)]
pub struct PlaybackPlugin {
    inner: Arc<Inner>,
}

fn fresh_scan() -> ScanSnapshot {
    ScanSnapshot {
        state: ScanState::Idle,
        candidates: vec![],
        reason: None,
        current: 0,
        total: 0,
        phase: None,
        error_class: None,
        typed: false,
        elapsed: 0.0,
        last_scan: None,
    }
}

impl PlaybackPlugin {
    pub fn new(
        bus: EventBus,
        store: StateStore,
        state_service: StateService,
        settings: Arc<SettingsService>,
        options: PluginOptions,
    ) -> Self {
        let effective = settings.effective_snapshot();
        let selected = effective.integration_modes.midi_source == MidiSource::PlaybackApi;
        let mut mapper = Mapper::new();
        let saved = &effective.playback_api;
        if !saved.song_order.is_empty() {
            if let Some(version) = saved.captured_version {
                let _ = mapper.install_order(
                    &saved.song_order,
                    version,
                    saved.setlist_id.as_ref().map(from_cfg),
                );
                mapper.set_skips(&saved.skipped_song_ids, &saved.skipped_plan_song_ids);
            }
        }
        let mut arbiter = PlaybackSourceArbiter::new(monotonic_clock());
        arbiter.midi_allowed = !selected;
        let arbiter = Arc::new(Mutex::new(arbiter));
        let shared = Arc::new(Mutex::new(Shared {
            midi_source: effective.integration_modes.midi_source,
            config: effective.playback_api.clone(),
            mapper,
            client: None,
            queue: VecDeque::new(),
            history: VecDeque::new(),
            generation: 0,
            running: false,
            active: false,
            connection: false,
            heartbeat: None,
            seen_serial: 0,
            published: None,
            ownership_revision: 0,
            consumer: None,
            ticker: None,
            scan: fresh_scan(),
            scan_started_at: None,
            scan_cancel: CancelToken::new(),
            scan_task: None,
            discovery: DiscoveryState::Idle,
            discovery_total: 0,
            discovery_error: None,
            discovery_cancel: CancelToken::new(),
        }));
        let mut dispatcher = ArbitratedDispatcher::new(
            arbiter.clone(),
            CoreDispatcher::new(state_service),
            CoreState::new(store.clone()),
        );
        let s = shared.clone();
        dispatcher.suppressed = Box::new(move || lock(&s).mapper.discovery);
        let s = shared.clone();
        dispatcher.api_mapping_revision = Box::new(move || lock(&s).mapper.revision() as i64);
        Self {
            inner: Arc::new(Inner {
                bus,
                store,
                settings,
                options,
                arbiter,
                dispatcher,
                shared,
                wake: Notify::new(),
                lifecycle: AsyncMutex::new(()),
                dispatch_lock: AsyncMutex::new(()),
                connection_lock: AsyncMutex::new(()),
                discovery_run: AsyncMutex::new(()),
                lengths_running: AtomicBool::new(false),
                progress: Arc::new(AtomicUsize::new(0)),
                song: Arc::new(AtomicUsize::new(0)),
            }),
        }
    }

    // ---- read side (for the routes card) ----------------------------------------------------

    pub fn selected(&self) -> bool {
        self.inner.selected()
    }

    pub fn status(&self) -> PlaybackStatus {
        self.inner.status()
    }

    pub fn connection(&self) -> PlaybackConnection {
        self.inner.connection().0
    }

    /// The one plain sentence the frontend shows (never composed client-side).
    pub fn connection_reason(&self) -> String {
        self.inner.connection().1
    }

    pub fn events(&self) -> Vec<MonitorEntry> {
        lock(&self.inner.shared).history.iter().cloned().collect()
    }

    pub fn scan_snapshot(&self) -> ScanSnapshot {
        let mut s = lock(&self.inner.shared);
        if s.scan.state == ScanState::Scanning {
            if let Some(started) = s.scan_started_at {
                s.scan.elapsed = started.elapsed().as_secs_f64();
            }
        }
        s.scan.clone()
    }

    pub fn discovery_snapshot(&self) -> DiscoverySnapshot {
        let s = lock(&self.inner.shared);
        DiscoverySnapshot {
            state: s.discovery,
            progress: self.inner.progress.load(Ordering::SeqCst),
            song: self.inner.song.load(Ordering::SeqCst),
            total: s.discovery_total,
            error: s.discovery_error.clone(),
        }
    }

    /// The saved playback settings as the running plugin sees them.
    pub fn config(&self) -> PlaybackApiSettings {
        lock(&self.inner.shared).config.clone()
    }

    pub fn mapper_stale(&self) -> bool {
        lock(&self.inner.shared).mapper.stale
    }

    /// The mapper's scanned song order and captured setlist version.
    pub fn mapper_view(&self) -> (Vec<i64>, Option<i64>) {
        let s = lock(&self.inner.shared);
        (s.mapper.song_order.clone(), s.mapper.captured_version)
    }

    /// Publish `connection.changed` if the connection state changed (the status routes do this
    /// before they read).
    pub async fn publish_connection(&self) {
        self.inner.publish_connection().await;
    }

    pub fn mapper_revision(&self) -> u64 {
        lock(&self.inner.shared).mapper.revision()
    }

    // ---- MIDI / Planning Center seams -------------------------------------------------------

    pub async fn set_midi_connected(&self, connected: bool) {
        self.inner.with_arbiter(|a| a.midi_connected = connected);
        self.inner.publish_connection().await;
    }

    /// Why a MIDI action must be ignored right now, if it must (Python `_midi_guard`).
    pub fn midi_guard(&self) -> Option<String> {
        if lock(&self.inner.shared).discovery == DiscoveryState::Running {
            return Some("ignored: song order discovery active".into());
        }
        if self.inner.selected() {
            return Some("ignored: MIDI is not the selected connection method".into());
        }
        if self.inner.with_arbiter(|a| a.snapshot().active_source) == Source::PlaybackApi {
            return Some("ignored: Playback API active".into());
        }
        None
    }

    /// The current ownership revision (Python `ownership_revision`).
    pub fn ownership_revision(&self) -> u64 {
        self.inner.with_arbiter(|a| {
            a.snapshot();
            a.revision
        })
    }

    pub fn set_lengths_running(&self, running: bool) {
        self.inner.lengths_running.store(running, Ordering::SeqCst);
    }

    // ---- plan keys --------------------------------------------------------------------------

    /// True when the loaded plan or update category differs from what was scanned.
    pub fn plan_changed(&self) -> bool {
        let saved = lock(&self.inner.shared).config.scan_plan_key.clone();
        let current = self
            .inner
            .scan_key(self.inner.store.snapshot().plan.as_ref());
        matches!((saved, current), (Some(a), Some(b)) if a != b)
    }

    pub fn scan_plan_is_current(&self) -> bool {
        !self.plan_changed()
    }

    fn applied_key(&self, plan_id: &str) -> Option<String> {
        let captured = lock(&self.inner.shared).config.captured_at?;
        Some(py_dumps(&json!([isoformat(captured), plan_id])))
    }

    /// Remember (persistently) that this scan's lengths were fully written to this plan.
    pub fn mark_lengths_applied(&self, plan_id: &str) -> Result<(), PlaybackError> {
        let Some(key) = self.applied_key(plan_id) else {
            return Ok(());
        };
        let mut config = lock(&self.inner.shared).config.clone();
        config.lengths_applied_key = Some(key);
        self.inner.save_playback(config.clone())?;
        lock(&self.inner.shared).config = config;
        Ok(())
    }

    /// Remember (persistently) which Planning Center category the length update targets.
    /// The Playback status and the scan key use it straight away (Python `choose_length_category`).
    pub fn set_update_service_type(&self, service_type_id: &str) -> Result<(), PlaybackError> {
        let mut config = lock(&self.inner.shared).config.clone();
        config.planning_center_update_service_type_id = Some(service_type_id.to_string());
        self.inner.save_playback(config.clone())?;
        lock(&self.inner.shared).config = config;
        Ok(())
    }

    pub fn lengths_applied(&self) -> bool {
        let Some(plan) = self.inner.store.snapshot().plan else {
            return false;
        };
        let saved = lock(&self.inner.shared).config.lengths_applied_key.clone();
        saved.is_some() && saved == self.applied_key(&plan.id)
    }

    // ---- settings ---------------------------------------------------------------------------

    /// Save settings the operator edited and apply them (Python `save_settings`).
    pub async fn save_settings(&self, settings: PersistentSettings) -> Result<(), PlaybackError> {
        let _life = self.inner.lifecycle.lock().await;
        self.inner.check_idle()?;
        let current = self.inner.settings.snapshot();
        let saved = &current.playback_api;
        let new = &settings.playback_api;
        if (
            &new.song_order,
            new.captured_version,
            new.captured_at,
            &new.setlist_id,
            &new.song_lengths,
            new.lengths_measured_at,
            new.discovery_duration_seconds,
        ) != (
            &saved.song_order,
            saved.captured_version,
            saved.captured_at,
            &saved.setlist_id,
            &saved.song_lengths,
            saved.lengths_measured_at,
            saved.discovery_duration_seconds,
        ) {
            return Err(PlaybackError::Invalid(
                "Song order can only be changed by Discover Song Order.".into(),
            ));
        }
        let mut settings = settings;
        settings.planning_center_length_undo = current.planning_center_length_undo.clone();
        settings.playback_api.planning_center_update_service_type_id =
            saved.planning_center_update_service_type_id.clone();
        settings.playback_api.scan_plan_key = saved.scan_plan_key.clone();
        settings.playback_api.lengths_applied_key = saved.lengths_applied_key.clone();
        options_of(&settings.playback_api, true)
            .validate()
            .map_err(invalid)?;
        self.inner
            .settings
            .save(settings)
            .map_err(|e| PlaybackError::Save(e.to_string()))?;
        self.inner.reconfigure_locked(self.inner.effective()).await
    }

    /// Save which songs are skipped. The order of either list is never touched.
    pub async fn save_arrangement(
        &self,
        skipped_song_ids: Vec<i64>,
        skipped_plan_song_ids: Vec<String>,
    ) -> Result<(), PlaybackError> {
        let _life = self.inner.lifecycle.lock().await;
        self.inner.check_idle()?;
        let current = self.inner.settings.snapshot();
        let plan = self.inner.store.snapshot().plan;
        if current.playback_api.song_order.is_empty() {
            return Err(PlaybackError::Invalid(
                "Scan songs first, then choose which songs to skip.".into(),
            ));
        }
        if plan.is_none() && !skipped_plan_song_ids.is_empty() {
            return Err(PlaybackError::Invalid(
                "Load a service plan before skipping one of its songs.".into(),
            ));
        }
        if let Some(plan) = &plan {
            if !skipped_plan_song_ids
                .iter()
                .all(|id| plan.songs.iter().any(|s| &s.id == id))
            {
                return Err(PlaybackError::Invalid(
                    "A skipped song is not in the loaded service plan.".into(),
                ));
            }
        }
        let mut updated = current.playback_api.clone();
        updated.skipped_song_ids = skipped_song_ids.clone();
        updated.skipped_plan_song_ids = skipped_plan_song_ids.clone();
        let mut errors = Vec::new();
        updated.validate(&[], &mut errors);
        if !errors.is_empty() {
            return Err(PlaybackError::Invalid("Invalid skipped songs.".into()));
        }
        // Skips change which plan song each measured length belongs to, so a previous Planning
        // Center write no longer describes this arrangement.
        updated.lengths_applied_key = None;
        self.inner.save_playback(updated.clone())?;
        let mut s = lock(&self.inner.shared);
        s.config = updated;
        s.mapper
            .set_skips(&skipped_song_ids, &skipped_plan_song_ids);
        Ok(())
    }

    /// Apply new effective settings (Python `reconfigure`).
    pub async fn reconfigure(&self) -> Result<(), PlaybackError> {
        let _life = self.inner.lifecycle.lock().await;
        if lock(&self.inner.shared).discovery == DiscoveryState::Running {
            return conflict("Wait for Discover Song Order before changing inputs.");
        }
        self.inner.reconfigure_locked(self.inner.effective()).await
    }

    // ---- scanning ---------------------------------------------------------------------------

    /// Start a scan. Turns Playback on and selects it if exactly one is found.
    pub async fn find(&self, host: Option<&str>) -> Result<PlaybackStatus, PlaybackError> {
        let host = host.map(|h| h.trim().to_string()).filter(|h| !h.is_empty());
        let port = lock(&self.inner.shared).config.port;
        if host.is_some() {
            ConnectionOptions {
                host_override: host.clone(),
                port,
                ..Default::default()
            }
            .validate()
            .map_err(invalid)?;
        }
        if lock(&self.inner.shared).discovery == DiscoveryState::Running {
            return conflict("Wait for Discover Song Order before scanning.");
        }
        // Cancel before taking the lifecycle lock: the old scan may be waiting on it.
        self.inner.cancel_scan().await;
        let _life = self.inner.lifecycle.lock().await;
        let cancel = CancelToken::new();
        {
            let mut s = lock(&self.inner.shared);
            s.scan_cancel = cancel.clone();
            s.scan.state = ScanState::Scanning;
            s.scan.candidates = vec![];
            s.scan.reason = None;
            s.scan.current = 0;
            s.scan.total = 0;
            s.scan.error_class = None;
            s.scan.typed = host.is_some();
            s.scan.phase = Some(match &host {
                Some(h) => format!("Connecting to {h}…"),
                None => "This computer".into(),
            });
            s.scan_started_at = Some(Instant::now());
            s.scan.elapsed = 0.0;
        }
        let inner = self.inner.clone();
        let task = tokio::spawn(async move { inner.run_scan(host, cancel).await });
        lock(&self.inner.shared).scan_task = Some(task);
        Ok(self.inner.status())
    }

    pub async fn cancel_scan(&self) {
        self.inner.cancel_scan().await;
        let mut s = lock(&self.inner.shared);
        if s.scan.state == ScanState::Scanning {
            s.scan.state = ScanState::Idle;
            s.scan.reason = Some("Scan cancelled.".into());
        }
    }

    // ---- song-order discovery ---------------------------------------------------------------

    /// Walk the whole setlist and remember its order and lengths (Python `discover_song_order`).
    /// Pre-condition failures are `Err(Conflict)`; failures during the walk are recorded in
    /// [`discovery_snapshot`](Self::discovery_snapshot) and return `Ok`.
    pub async fn discover_song_order(&self) -> Result<(), PlaybackError> {
        let inner = &self.inner;
        let (client, cancel, original_version) = {
            let _life = inner.lifecycle.lock().await;
            if lock(&inner.shared).discovery == DiscoveryState::Running
                || inner.lengths_running.load(Ordering::SeqCst)
            {
                return conflict(
                    "Song order discovery or Planning Center update is already running.",
                );
            }
            let heartbeat = inner.check_discovery()?;
            let _dispatch = inner.dispatch_lock.lock().await;
            let cancel = CancelToken::new();
            let client = {
                let mut s = lock(&inner.shared);
                s.mapper.set_discovery(true);
                s.discovery = DiscoveryState::Running;
                s.discovery_total = if s.mapper.stale {
                    0
                } else {
                    s.mapper.song_order.len()
                };
                s.discovery_error = None;
                s.discovery_cancel = cancel.clone();
                s.client.clone()
            };
            inner.progress.store(0, Ordering::SeqCst);
            inner.song.store(0, Ordering::SeqCst);
            (client, cancel, heartbeat.setlist_version)
        };
        let _run = inner.discovery_run.lock().await;
        let started = Instant::now();
        let result = match client {
            Some(client) => {
                let opts = DiscoveryOptions {
                    step_timeout: inner.options.step_timeout,
                    max_steps: inner.options.max_steps,
                    measure: true,
                    cancel,
                    progress: Some(inner.progress.clone()),
                    song: Some(inner.song.clone()),
                };
                discover_song_order(&*client, &opts).await
            }
            None => Err(DiscoveryError::Conflict(
                "Playback is disconnected. Enable remote connections and connect first.".into(),
            )),
        };
        let _ = original_version;
        let outcome = match result {
            Ok(done) => inner.finish_discovery(done, started),
            Err(DiscoveryError::Cancelled) => Err(
                "Song order discovery cancelled; no result saved. Check Playback selection.".into(),
            ),
            Err(DiscoveryError::Conflict(m)) => {
                Err(format!("{m} No result saved; check Playback selection."))
            }
        };
        let mut s = lock(&inner.shared);
        match outcome {
            Ok(()) => s.discovery = DiscoveryState::Done,
            Err(message) => {
                s.discovery = DiscoveryState::Failed;
                s.discovery_error = Some(message);
            }
        }
        s.mapper.set_discovery(false);
        Ok(())
    }

    /// Abort a running discovery (it restores the original selection on the way out).
    pub fn cancel_discovery(&self) {
        lock(&self.inner.shared).discovery_cancel.cancel();
    }
}

fn invalid(e: crate::client::ClientError) -> PlaybackError {
    PlaybackError::Invalid(e.to_string())
}

fn options_of(config: &PlaybackApiSettings, selected: bool) -> ConnectionOptions {
    ConnectionOptions {
        enabled: config.enabled && selected,
        host_override: config.host.clone(),
        port: config.port,
        fast_transport: config.fast_transport,
        auto_scan: config.auto_scan,
        ..Default::default()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Inner {
    fn selected(&self) -> bool {
        lock(&self.shared).midi_source == MidiSource::PlaybackApi
    }

    fn with_arbiter<R>(&self, f: impl FnOnce(&mut PlaybackSourceArbiter) -> R) -> R {
        f(&mut lock(&self.arbiter))
    }

    fn effective(&self) -> (MidiSource, PlaybackApiSettings) {
        let s = self.settings.effective_snapshot();
        (s.integration_modes.midi_source, s.playback_api)
    }

    fn status(&self) -> PlaybackStatus {
        let s = lock(&self.shared);
        match &s.client {
            Some(c) => c.status(),
            None => PlaybackStatus {
                port: s.config.port,
                ..Default::default()
            },
        }
    }

    fn check_idle(&self) -> Result<(), PlaybackError> {
        if lock(&self.shared).discovery == DiscoveryState::Running
            || self.lengths_running.load(Ordering::SeqCst)
        {
            return conflict("Wait for song discovery or Planning Center changes to finish.");
        }
        Ok(())
    }

    /// Persist `playback_api` on top of the saved settings (every Python `_service.save` site).
    fn save_playback(&self, playback: PlaybackApiSettings) -> Result<(), PlaybackError> {
        let mut saved = self.settings.snapshot();
        saved.playback_api = playback;
        self.settings
            .save(saved)
            .map_err(|e| PlaybackError::Save(e.to_string()))
    }

    fn scan_key(&self, plan: Option<&ServicePlan>) -> Option<String> {
        let plan = plan?;
        let category = lock(&self.shared)
            .config
            .planning_center_update_service_type_id
            .clone()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                self.settings
                    .effective_snapshot()
                    .planning_center
                    .service_type_id
                    .filter(|s| !s.is_empty())
            });
        Some(py_dumps(&json!([category, plan_key(plan)])))
    }

    // ---- connection -------------------------------------------------------------------------

    /// The arbiter's view with the one plain sentence the frontend shows.
    fn connection(&self) -> (PlaybackConnection, String) {
        let result = self.with_arbiter(|a| a.snapshot());
        let status = self.status();
        let s = lock(&self.shared);
        let selected = s.midi_source == MidiSource::PlaybackApi;
        let reason = if s.scan.state == ScanState::Scanning {
            "Looking for Playback…".to_string()
        } else if result.active_source == Source::PlaybackApi {
            let name = match status.host.as_deref() {
                Some("127.0.0.1") => Some("this computer"),
                other => other,
            };
            match name {
                Some(n) if !n.is_empty() => format!("Connected to Playback on {n}."),
                _ => "Connected to Playback.".into(),
            }
        } else if result.active_source == Source::None {
            if s.scan.state == ScanState::NotFound {
                NOT_FOUND_REASON.to_string()
            } else if selected && s.config.enabled {
                match status.last_error.as_deref() {
                    Some(e) if e == SCAN_BLOCK_DETAIL => {
                        format!("Disconnected. {SCAN_BLOCK_DETAIL}. Open Local Network settings to fix it.")
                    }
                    Some(_) => format!("Disconnected. {UNREACHABLE_REASON}"),
                    None => "Disconnected. Looking for Playback…".into(),
                }
            } else {
                "Disconnected. Choose Scan Network to find Playback.".into()
            }
        } else {
            result.reason.to_string()
        };
        (result, reason)
    }

    async fn publish_connection(&self) {
        let _guard = self.connection_lock.lock().await;
        let (connection, reason) = self.connection();
        let revision = self.with_arbiter(|a| a.revision);
        let key = (connection.connected, reason.clone());
        {
            let mut s = lock(&self.shared);
            if s.ownership_revision != revision {
                s.mapper.discard_pending();
                s.ownership_revision = revision;
            }
            if s.published.as_ref() == Some(&key) {
                return;
            }
        }
        self.bus
            .publish(new_event(
                EventType::ConnectionChanged,
                "playback_connection",
                EventPayload::Connection {
                    integration: Integration::Midi,
                    status: if connection.connected {
                        ConnectionStatus::Connected
                    } else {
                        ConnectionStatus::Disconnected
                    },
                    detail: Some(reason),
                },
            ))
            .await;
        lock(&self.shared).published = Some(key);
    }

    async fn tick(self: Arc<Self>) {
        loop {
            self.publish_connection().await;
            tokio::time::sleep(TICK).await;
        }
    }

    // ---- lifecycle --------------------------------------------------------------------------

    async fn start(self: &Arc<Self>) -> Result<(), String> {
        let _life = self.lifecycle.lock().await;
        {
            let mut s = lock(&self.shared);
            if s.running {
                return Ok(());
            }
            s.running = true;
            s.consumer = Some(tokio::spawn(self.clone().consume()));
            s.ticker = Some(tokio::spawn(self.clone().tick()));
        }
        self.publish_connection().await;
        self.start_selected().await
    }

    async fn start_selected(self: &Arc<Self>) -> Result<(), String> {
        let (selected, enabled, config) = {
            let s = lock(&self.shared);
            (
                s.midi_source == MidiSource::PlaybackApi,
                s.config.enabled,
                s.config.clone(),
            )
        };
        self.with_arbiter(|a| a.midi_allowed = !selected);
        if selected && enabled {
            let generation = {
                let mut s = lock(&self.shared);
                s.active = true;
                s.seen_serial = 0;
                s.generation
            };
            let weak: Weak<Inner> = Arc::downgrade(self);
            let observer: Observer = Arc::new(move |status, events| {
                if let Some(inner) = weak.upgrade() {
                    inner.observe(generation, status, events);
                }
            });
            let client = (self.options.client_factory)(options_of(&config, true), observer)?;
            lock(&self.shared).client = Some(Arc::new(client));
        }
        Ok(())
    }

    async fn stop_selected(self: &Arc<Self>) {
        let client = {
            let mut s = lock(&self.shared);
            s.active = false;
            s.generation += 1;
            s.mapper.discard_pending();
            s.client.clone()
        };
        self.with_arbiter(|a| a.observe_api(false, false));
        self.publish_connection().await;
        if let Some(client) = client {
            client.shutdown().await;
        }
        {
            let mut s = lock(&self.shared);
            s.heartbeat = None;
            s.connection = false;
            s.queue.clear();
        }
        self.with_arbiter(|a| a.midi_connected = false);
        self.publish_connection().await;
    }

    async fn stop(self: &Arc<Self>) {
        self.cancel_scan().await;
        // Cancel a running discovery and wait for it to restore the selection.
        lock(&self.shared).discovery_cancel.cancel();
        let _run = self.discovery_run.lock().await;
        let _life = self.lifecycle.lock().await;
        let ticker = {
            let mut s = lock(&self.shared);
            s.running = false;
            s.ticker.take()
        };
        if let Some(t) = ticker {
            t.abort();
            let _ = t.await;
        }
        {
            // Finish any action already in flight before releasing input resources.
            let _dispatch = self.dispatch_lock.lock().await;
            self.stop_selected().await;
        }
        let consumer = lock(&self.shared).consumer.take();
        if let Some(c) = consumer {
            c.abort();
            let _ = c.await;
        }
    }

    async fn reconfigure_locked(
        self: &Arc<Self>,
        new: (MidiSource, PlaybackApiSettings),
    ) -> Result<(), PlaybackError> {
        let same = {
            let s = lock(&self.shared);
            s.midi_source == new.0 && s.config == new.1
        };
        if same {
            return Ok(());
        }
        let _dispatch = self.dispatch_lock.lock().await;
        self.stop_selected().await;
        let running = {
            let mut s = lock(&self.shared);
            s.midi_source = new.0;
            s.config = new.1;
            s.client = None;
            s.running
        };
        if running {
            self.start_selected()
                .await
                .map_err(PlaybackError::Invalid)?;
        }
        Ok(())
    }

    // ---- observation ------------------------------------------------------------------------

    /// Called from the client for every published status (Python `_observe`).
    fn observe(&self, generation: u64, status: &PlaybackStatus, events: &[PlaybackEvent]) {
        let mut stale_lengths: Option<PlaybackApiSettings> = None;
        let wake = {
            let mut s = lock(&self.shared);
            if !s.active || generation != s.generation {
                return;
            }
            let fresh = status.heartbeat.is_some() && status.heartbeat_serial != s.seen_serial;
            if fresh {
                s.heartbeat = status.heartbeat;
                s.seen_serial = status.heartbeat_serial;
            }
            if !status.connected {
                s.heartbeat = None;
                s.mapper.discard_pending();
            }
            self.with_arbiter(|a| a.observe_api(status.connected, fresh));
            let revision = self.with_arbiter(|a| a.revision);
            if s.ownership_revision != revision {
                s.mapper.discard_pending();
                s.ownership_revision = revision;
            }
            let hb = if fresh { status.heartbeat } else { None };
            let setlist = if status.connected {
                status.setlist_id.as_ref()
            } else {
                None
            };
            let observations = s.mapper.observe(hb, events, setlist);
            if s.mapper.stale
                && s.discovery != DiscoveryState::Running
                && !s.config.song_lengths.is_empty()
                && status.connected
            {
                let mut config = s.config.clone();
                config.song_lengths = vec![];
                config.lengths_measured_at = None;
                s.config = config.clone();
                stale_lengths = Some(config);
            }
            let at = Utc::now();
            for o in &observations {
                s.history.push_back(MonitorEntry {
                    event: o.event.clone(),
                    discovery: o.discovery,
                    at,
                });
            }
            while s.history.len() > HISTORY_CAPACITY {
                s.history.pop_front();
            }
            if status.connected != s.connection {
                s.connection = status.connected;
                self.put(&mut s, generation, Item::Connection);
            }
            if self.with_arbiter(|a| a.allows(Source::PlaybackApi)) {
                for o in observations {
                    self.put(&mut s, generation, Item::Observation(o));
                }
            }
            true
        };
        if let Some(config) = stale_lengths {
            if let Err(error) = self.save_playback(config) {
                tracing::warn!(%error, "playback_clear_stale_lengths_failed");
            }
        }
        if wake {
            self.wake.notify_one();
        }
    }

    fn put(&self, s: &mut Shared, generation: u64, item: Item) {
        if s.queue.len() >= QUEUE_CAPACITY {
            // Never replay overflow later. Monitor remains independently bounded.
            s.discovery_error = Some("Playback event queue full; an event was dropped.".into());
            return;
        }
        s.queue.push_back((generation, item));
    }

    async fn next_item(&self) -> (u64, Item) {
        loop {
            if let Some(item) = lock(&self.shared).queue.pop_front() {
                return item;
            }
            self.wake.notified().await;
        }
    }

    async fn consume(self: Arc<Self>) {
        loop {
            let (generation, item) = self.next_item().await;
            let _dispatch = self.dispatch_lock.lock().await;
            {
                let s = lock(&self.shared);
                if !s.active || generation != s.generation {
                    continue;
                }
            }
            match item {
                Item::Connection => self.publish_connection().await,
                Item::Observation(observation) => {
                    if self.with_arbiter(|a| a.allows(Source::PlaybackApi)) {
                        self.publish_connection().await;
                        self.dispatch(&observation).await;
                    }
                }
            }
        }
    }

    /// Execute what the mapper decides (Python `PlaybackMapper.dispatch`).
    async fn dispatch(&self, observation: &Observation) {
        let state = self.store.snapshot();
        let plan = lock(&self.shared).mapper.plan(observation, &state);
        let Some(plan) = plan else { return };
        let source = "playback_api";
        let d = &self.dispatcher;
        let _ = match plan {
            Plan::Stop => d.dispatch(PbAction::StopTimer, source).await,
            Plan::StopAt(p) => d.stop_timer_at(p, source).await,
            Plan::Restart => d.dispatch(PbAction::RestartCurrent, source).await,
            Plan::Position(n) => d.dispatch_song_position(n, source).await,
            Plan::ResumeAt(p) => d.resume_timer_at(p, source).await,
            Plan::SeekTo(p) => d.seek_timer(p, source).await,
        };
    }

    // ---- scanning ---------------------------------------------------------------------------

    async fn cancel_scan(&self) {
        let task = {
            let mut s = lock(&self.shared);
            s.scan_cancel.cancel();
            s.scan_task.take()
        };
        if let Some(task) = task {
            let _ = task.await;
        }
    }

    fn finish_scan(&self, state: ScanState, reason: Option<String>) {
        let mut s = lock(&self.shared);
        if let Some(started) = s.scan_started_at {
            s.scan.elapsed = started.elapsed().as_secs_f64();
        }
        s.scan.state = state;
        s.scan.reason = reason;
    }

    async fn run_scan(self: Arc<Self>, host: Option<String>, cancel: CancelToken) {
        let port = lock(&self.shared).config.port;
        let started = Instant::now();
        playback_log(
            "playback_scan_start",
            json!({"trigger": if host.is_some() {"typed_host"} else {"scan_button"}, "typed_host": host}),
        );
        let progress_shared = self.shared.clone();
        let phase_shared = self.shared.clone();
        let phase_host = host.clone();
        let hooks = ScanHooks {
            cancel: cancel.clone(),
            progress: Some(Arc::new(move |current, total| {
                let mut s = lock(&progress_shared);
                s.scan.current = current;
                s.scan.total = total;
                if let Some(t) = s.scan_started_at {
                    s.scan.elapsed = t.elapsed().as_secs_f64();
                }
            })),
            on_phase: Some(Arc::new(move |name, current, total| {
                let mut s = lock(&phase_shared);
                s.scan.phase = Some(match &phase_host {
                    Some(h) => format!("Connecting to {h}…"),
                    None => name.to_string(),
                });
                s.scan.current = current;
                s.scan.total = total;
            })),
        };
        let report = self.options.scanner.scan(host.clone(), port, hooks).await;
        if cancel.is_cancelled() {
            return;
        }
        let summary = report.summary();
        let found = report.candidates.clone();
        {
            let mut s = lock(&self.shared);
            s.scan.last_scan = Some(summary.clone());
            s.scan.candidates = found.clone();
        }
        playback_log(
            "playback_scan_result",
            json!({"summary": summary, "found": found.len(),
                   "duration_s": (started.elapsed().as_secs_f64() * 100.0).round() / 100.0}),
        );
        if report.outcome == ScanOutcome::Error && found.is_empty() {
            let class = report
                .error_class
                .map(|c| c.as_str().to_string())
                .unwrap_or_default();
            lock(&self.shared).scan.error_class = report.error_class.map(|c| c.as_str().into());
            self.finish_scan(ScanState::NotFound, Some(scan_message(&class)));
            return;
        }
        if found.is_empty() {
            self.finish_scan(ScanState::NotFound, Some(NOT_FOUND_REASON.into()));
            return;
        }
        if found.len() == 1 {
            let _life = self.lifecycle.lock().await;
            if cancel.is_cancelled() || lock(&self.shared).discovery == DiscoveryState::Running {
                return;
            }
            if let Err(error) = self.adopt(&found[0].host).await {
                playback_log(
                    "playback_scan_save_failed",
                    json!({"error": error.to_string()}),
                );
                self.finish_scan(
                    ScanState::NotFound,
                    Some("Found Playback but could not save the address.".into()),
                );
                return;
            }
        }
        self.finish_scan(ScanState::Found, None);
    }

    /// Select Playback, turn it on, and remember the address (one save). The caller holds the
    /// lifecycle lock.
    async fn adopt(self: &Arc<Self>, host: &str) -> Result<(), PlaybackError> {
        let mut saved = self.settings.snapshot();
        saved.playback_api.enabled = true;
        saved.playback_api.host = Some(host.to_string());
        saved.integration_modes.midi_source = MidiSource::PlaybackApi;
        self.settings
            .save(saved)
            .map_err(|e| PlaybackError::Save(e.to_string()))?;
        self.reconfigure_locked(self.effective()).await
    }

    // ---- discovery --------------------------------------------------------------------------

    /// Python `_check_discovery` before a run starts.
    fn check_discovery(&self) -> Result<Heartbeat, PlaybackError> {
        let (active, heartbeat) = {
            let s = lock(&self.shared);
            (s.active, s.heartbeat)
        };
        let connected = self.with_arbiter(|a| a.snapshot().playback_api.connected);
        let Some(heartbeat) = heartbeat.filter(|_| active && connected) else {
            return conflict(
                "Playback is disconnected. Enable remote connections and connect first.",
            );
        };
        if heartbeat.playing {
            return conflict("Stop Playback before discovering song order.");
        }
        if heartbeat.setlist_version.is_none() {
            return conflict("Playback has not supplied a valid setlist version.");
        }
        Ok(heartbeat)
    }

    /// Persist a completed discovery and install it in the mapper.
    fn finish_discovery(
        &self,
        done: crate::discovery::DiscoveryOutcome,
        started: Instant,
    ) -> Result<(), String> {
        let status = self.status();
        let plan = self.store.snapshot().plan;
        let key = self.scan_key(plan.as_ref());
        let mut config = lock(&self.shared).config.clone();
        config.scan_plan_key = key;
        config.song_order = done.order.clone();
        config.captured_version = Some(done.version);
        config.setlist_id = status.setlist_id.as_ref().map(to_cfg);
        config.captured_at = Some(Utc::now());
        config.song_lengths = done.lengths.clone();
        config.discovery_duration_seconds = Some(started.elapsed().as_secs_f64());
        config.lengths_measured_at = Some(Utc::now());
        // A new setlist has new Playback song ids. A saved skip that is not in the scanned order
        // can never match a song again, and it would make every later arrangement save fail with
        // "A skipped Playback song is not in the scanned song order."
        let kept = config.skipped_song_ids.len();
        config.skipped_song_ids.retain(|id| done.order.contains(id));
        if config.skipped_song_ids.len() != kept {
            config.lengths_applied_key = None;
        }
        self.save_playback(config.clone())
            .map_err(|e| e.to_string())?;
        let mut s = lock(&self.shared);
        s.config = config.clone();
        s.discovery_total = done.order.len();
        s.mapper.install_order(
            &done.order,
            done.version,
            config.setlist_id.as_ref().map(from_cfg),
        )?;
        s.mapper.set_skips(&[], &config.skipped_plan_song_ids);
        let heartbeat = s.heartbeat;
        s.mapper.observe(heartbeat, &[], None);
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Plugin trait
// ---------------------------------------------------------------------------------------------

#[async_trait]
impl Plugin for PlaybackPlugin {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    async fn start(&self) -> Result<(), String> {
        self.inner.start().await
    }

    async fn stop(&self) -> Result<(), String> {
        self.inner.stop().await;
        Ok(())
    }

    async fn health(&self) -> Result<PluginHealth, String> {
        // Health describes the lifecycle supervisor, not external connectivity.
        let (running, dead, error) = {
            let s = lock(&self.inner.shared);
            let dead = [&s.consumer, &s.ticker]
                .iter()
                .any(|t| t.as_ref().is_some_and(|t| t.is_finished()));
            (s.running, dead, s.discovery_error.clone())
        };
        let status = if running && dead {
            PluginStatus::Error
        } else if running {
            PluginStatus::Running
        } else {
            PluginStatus::Stopped
        };
        let connected = self.inner.connection().0.connected;
        let last_error = if connected {
            None
        } else {
            error.or_else(|| self.inner.status().last_error)
        };
        Ok(PluginHealth {
            name: PLUGIN_NAME.into(),
            version: PLUGIN_VERSION.into(),
            status,
            last_error,
            last_activity_at: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_key_matches_python_json_dumps() {
        // json.dumps([None, "plan-1", ["a", "b"]])
        let plan = ServicePlan {
            id: "plan-1".into(),
            title: "t".into(),
            date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            service_type: "s".into(),
            service_type_id: None,
            service_times: vec![],
            duration_source: "d".into(),
            songs: vec![
                stagepilot_core::state::Song::new("a", "A", None, 1),
                stagepilot_core::state::Song::new("b", "B", None, 2),
            ],
        };
        assert_eq!(plan_key(&plan), r#"[null, "plan-1", ["a", "b"]]"#);
        // Nested: json.dumps(["cat", plan_key]) escapes the inner quotes.
        assert_eq!(
            py_dumps(&json!(["cat", plan_key(&plan)])),
            r#"["cat", "[null, \"plan-1\", [\"a\", \"b\"]]"]"#
        );
        // ensure_ascii: "é" and an astral char.
        assert_eq!(py_dumps(&json!(["é😀"])), r#"["\u00e9\ud83d\ude00"]"#);
    }

    #[test]
    fn isoformat_matches_python() {
        let t = DateTime::parse_from_rfc3339("2026-10-17T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(isoformat(t), "2026-10-17T12:00:00+00:00");
        let t = DateTime::parse_from_rfc3339("2026-10-17T12:00:00.123456Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(isoformat(t), "2026-10-17T12:00:00.123456+00:00");
    }
}
