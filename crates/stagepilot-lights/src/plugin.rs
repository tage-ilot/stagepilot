use crate::lightkey::LightkeyLink as LightkeyLinkState;
use crate::lightkey::LightkeyStatus;
use crate::lightkey_discovery::{
    apply_change, Applied, Change, Discovery, DiscoveryHost, DiscoveryOptions, HostInputs, Reach,
};
use crate::lightkey_fixtures::{clear_address, fixture_of, FixtureCommand};
use crate::lightkey_link::{FixtureEvent, LightkeyLink, LinkOptions, MIN_SEND_GAP};
use crate::lightkey_operator::OperatorTracker;
use crate::lightkey_replay::{is_smooth, Kind, PacingQueue, ReplayEngine, ReplayLoad};
use crate::models::{LightingOutputResponse, LightsStatusResponse};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use stagepilot_core::bus::{EventBus, Subscription};
use stagepilot_core::config::{
    validate_osc_address, LightingCue, LightingEffect, LightingMethod, LightkeySettings,
    LightsSettings, OscArgument, OscLightingCue, OscMessage, SongLightingCueMap,
};
use stagepilot_core::events::{new_event, EventPayload, EventType, Integration, StagePilotEvent};
use stagepilot_core::plugin::Plugin;
use stagepilot_core::state::{ConnectionStatus, PluginHealth, PluginStatus, Song};
use stagepilot_midi::output::{MidiOutputBackend, MidiOutputBackendFactory, MidiOutputPort};
use stagepilot_midi::MidiError;
use stagepilot_osc::{OscBackend, OscOutput, OutputError, UdpOscBackend};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::Instant;
use uuid::Uuid;

/// How many seconds before its time a row with fixture-override messages (a "look") leaves, so
/// the fixtures have changed by the beat. Other rows (MIDI, `/live` cues) leave exactly on time.
pub const LOOK_LEAD_SECONDS: f64 = 0.045;
/// How long the last replay load counts as current: a replay that has not ticked for this long
/// has nothing moving.
const REPLAY_LOAD_FRESH: Duration = Duration::from_secs(3);
/// Pause between the two rounds of `overrides/clear` a release sends per fixture.
/// Pause before a failed timeline send is tried once more.
const SEND_RETRY_DELAY: Duration = Duration::from_millis(100);
const RELEASE_ROUND_GAP: Duration = Duration::from_millis(150);

/// A fixture the operator took over goes back to the replay after this many seconds without a
/// further operator change on it. Each new operator change restarts the count.
pub const OPERATOR_TAKE_BACK_SECONDS: f64 = 30.0;

/// `ActionOutcome` for the lighting controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightingOutcome {
    pub accepted: bool,
    pub message: String,
}

impl LightingOutcome {
    fn new(accepted: bool, message: impl Into<String>) -> Self {
        Self {
            accepted,
            message: message.into(),
        }
    }
}

pub type OscBackendFactory = Arc<dyn Fn() -> Arc<dyn OscBackend> + Send + Sync>;

pub struct LightsOptions {
    pub midi_backend_factory: MidiOutputBackendFactory,
    pub osc_backend_factory: OscBackendFactory,
    pub monitor_interval: Duration,
    /// Record-switch store, timeline sink, HTTP client and clock of the Lightkey link.
    pub lightkey: LinkOptions,
    /// The search for the Lighting computer. Off unless the server turns it on.
    pub discovery: DiscoveryOptions,
}

impl LightsOptions {
    pub fn new(midi_backend_factory: MidiOutputBackendFactory) -> Self {
        Self {
            midi_backend_factory,
            osc_backend_factory: Arc::new(|| Arc::new(UdpOscBackend)),
            monitor_interval: Duration::from_secs(2),
            lightkey: LinkOptions::default(),
            discovery: DiscoveryOptions::default(),
        }
    }
}

#[derive(Clone)]
enum AnyCue {
    Midi(LightingCue),
    Osc(OscLightingCue),
}

impl AnyCue {
    fn at_seconds(&self) -> f64 {
        match self {
            AnyCue::Midi(c) => c.at_seconds,
            AnyCue::Osc(c) => c.at_seconds,
        }
    }
}

struct State {
    settings: LightsSettings,
    backend: Option<Arc<dyn MidiOutputBackend>>,
    port: Option<Arc<dyn MidiOutputPort>>,
    osc_output: Option<Arc<dyn OscOutput>>,
    subscriptions: Vec<Subscription>,
    pending_song: Option<Song>,
    timeline: Option<JoinHandle<()>>,
    connection_task: Option<JoinHandle<()>>,
    stopping: bool,
    status: PluginStatus,
    connection_status: ConnectionStatus,
    detail: Option<String>,
    last_error: Option<String>,
    last_activity_at: Option<DateTime<Utc>>,
    last_cue: Option<LightingCue>,
    last_osc_cue: Option<OscLightingCue>,
    last_cue_at: Option<DateTime<Utc>>,
    /// Fixtures the timeline holds with overrides. Kept across pause, stop and song end; emptied
    /// only by a release. A set of names so per-fixture state can grow from it.
    held_fixtures: BTreeSet<String>,
    /// Echo matching and the fixtures the operator has taken over.
    operator: OperatorTracker,
    /// Quiet-time watchers, one per yielded fixture.
    watchers: HashMap<String, JoinHandle<()>>,
    /// The song the timeline is playing, for taking a fixture back mid-song.
    playback: Option<Playback>,
    /// The smooth replay engine of the running song (ramps and effects), when it has any.
    engine: Option<ReplayEngine>,
    /// How well the engine serves the running song, as of the last tick (`None` = no tick yet).
    replay_load: Option<(ReplayLoad, Instant)>,
}

/// Where a timeline starts and which rows it keeps (song seconds).
struct Window {
    now: f64,
    keep_from: f64,
    keep_to: f64,
    remaining: f64,
}

/// Where the running song is: `position` seconds at `at`.
struct Playback {
    cues: Vec<AnyCue>,
    position: f64,
    at: Instant,
    end: f64,
}

impl Playback {
    fn now(&self) -> f64 {
        self.position
            + Instant::now()
                .saturating_duration_since(self.at)
                .as_secs_f64()
    }
}

struct Inner {
    bus: EventBus,
    midi_factory: MidiOutputBackendFactory,
    osc_factory: OscBackendFactory,
    monitor_interval: Duration,
    state: Mutex<State>,
    operation_lock: tokio::sync::Mutex<()>,
    stop: tokio::sync::Notify,
    /// Wakes the smooth replay's scheduler tick early (a fixture was handed back).
    engine_wake: tokio::sync::Notify,
    /// When the next OSC packet of a paced send may leave (see `reserve_slots`).
    next_send: Mutex<Option<tokio::time::Instant>>,
    /// Set once in `LightsPlugin::new` (the link needs a handle back to this struct).
    lightkey: std::sync::OnceLock<LightkeyLink>,
    /// The search for the Lighting computer; it runs only while the link is down.
    discovery: Discovery,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Python `_public_error`: only value/runtime errors reach the operator.
fn public_midi(error: &MidiError, fallback: &str) -> String {
    if error.public {
        error.message.clone()
    } else {
        fallback.to_string()
    }
}

fn public_osc(error: &OutputError, fallback: &str) -> String {
    if error.public {
        error.message.clone()
    } else {
        fallback.to_string()
    }
}

/// The search's view of the plugin (a weak handle: the search must not keep it alive).
struct DiscoveryBridge {
    inner: std::sync::Weak<Inner>,
}

#[async_trait]
impl DiscoveryHost for DiscoveryBridge {
    fn inputs(&self) -> HostInputs {
        match self.inner.upgrade() {
            Some(inner) => inner.discovery_inputs(),
            None => HostInputs::from_settings(
                false,
                false,
                Reach::Pending,
                "",
                &LightkeySettings::default(),
            ),
        }
    }

    async fn apply(&self, change: Change) -> Result<Applied, String> {
        match self.inner.upgrade() {
            Some(inner) => inner.apply_discovery(change).await,
            None => Err("StagePilot is shutting down.".into()),
        }
    }
}

#[derive(Clone)]
pub struct LightsPlugin {
    inner: Arc<Inner>,
}

impl LightsPlugin {
    pub fn new(
        bus: EventBus,
        settings: LightsSettings,
        options: LightsOptions,
    ) -> Result<Self, String> {
        if options.monitor_interval.is_zero() {
            return Err("Lighting MIDI monitor interval must be positive.".into());
        }
        let bus_for_link = bus.clone();
        let mut link_options = options.lightkey;
        let inner = Arc::new(Inner {
            bus,
            midi_factory: options.midi_backend_factory,
            osc_factory: options.osc_backend_factory,
            monitor_interval: options.monitor_interval,
            state: Mutex::new(State {
                settings,
                backend: None,
                port: None,
                osc_output: None,
                subscriptions: Vec::new(),
                pending_song: None,
                timeline: None,
                connection_task: None,
                stopping: false,
                status: PluginStatus::Stopped,
                connection_status: ConnectionStatus::Disconnected,
                detail: None,
                last_error: None,
                last_activity_at: None,
                last_cue: None,
                last_osc_cue: None,
                last_cue_at: None,
                held_fixtures: BTreeSet::new(),
                operator: OperatorTracker::new(),
                watchers: HashMap::new(),
                playback: None,
                engine: None,
                replay_load: None,
            }),
            operation_lock: tokio::sync::Mutex::new(()),
            stop: tokio::sync::Notify::new(),
            engine_wake: tokio::sync::Notify::new(),
            next_send: Mutex::new(None),
            lightkey: std::sync::OnceLock::new(),
            discovery: Discovery::new(options.discovery),
        });
        let weak = Arc::downgrade(&inner);
        let tap_weak = weak.clone();
        link_options.fixture_tap = Some(Arc::new(move |event: FixtureEvent<'_>| {
            let Some(inner) = tap_weak.upgrade() else {
                return;
            };
            match event {
                FixtureEvent::Changed { commands, keyframe } => {
                    inner.operator_changes(commands, keyframe);
                }
                FixtureEvent::StreamReset => inner.operator_stream_reset(),
            }
        }));
        // The Lighting card says Connected only while capture (the helper link) is up as well as the replay
        // address, so it follows the link. The link calls this on its polling task, outside its locks.
        let status_weak = weak.clone();
        link_options.link_changed = Some(Arc::new(move || {
            if let Some(inner) = status_weak.upgrade() {
                if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                    runtime.spawn(async move { inner.refresh_osc_status().await });
                }
            }
        }));
        let captures_weak = weak.clone();
        link_options.captures = Some(Arc::new(move |key: &str| match captures_weak.upgrade() {
            Some(inner) => {
                crate::lightkey_auto::song_capture(lock(&inner.state).settings.cue_maps.get(key))
            }
            None => crate::lightkey_auto::SongCapture::NotCaptured,
        }));
        let settings_weak = weak.clone();
        let provider: crate::lightkey_link::SettingsProvider =
            Arc::new(move || match settings_weak.upgrade() {
                Some(inner) => {
                    let st = lock(&inner.state);
                    (
                        st.settings.enabled,
                        st.settings.method == LightingMethod::Osc,
                        st.settings.osc_host.clone(),
                        st.settings.lightkey.clone(),
                    )
                }
                None => (false, false, String::new(), Default::default()),
            });
        let _ = inner
            .lightkey
            .set(LightkeyLink::new(bus_for_link, provider, link_options));
        Ok(Self { inner })
    }

    fn link(&self) -> &LightkeyLink {
        // Always set by `new`.
        self.inner.lightkey.get().expect("lightkey link")
    }

    pub async fn lightkey_status(&self) -> LightkeyStatus {
        let mut status = self.link().status();
        status.discovery = Some(self.inner.discovery.status(&self.inner.discovery_inputs()));
        let st = lock(&self.inner.state);
        let some = |n: usize| (n > 0).then(|| u32::try_from(n).unwrap_or(u32::MAX));
        status.holding_fixtures = some(st.held_fixtures.len());
        status.yielded_fixtures = some(st.operator.yielded_count());
        status.known_fixtures = some(Self::releasable(&st).len());
        // Warn only: the notice never touches replay.
        if let Some(key) = self.link().current_song_key() {
            if let Some(map) = st.settings.cue_maps.get(&key) {
                status.project_mismatch = crate::lightkey::project_mismatch(
                    &map.song_title,
                    map.capture.as_ref().and_then(|c| c.project.as_deref()),
                    status.project.as_deref(),
                );
            }
        }
        status
    }

    /// "Search again": look for the Lighting computer now. `(false, why)` when nothing may be
    /// searched (Lighting off, or a computer is connected).
    pub fn lightkey_discovery_scan(&self) -> (bool, String) {
        match self
            .inner
            .discovery
            .scan_now(&self.inner.discovery_inputs())
        {
            Ok(message) => (true, message),
            Err(why) => (false, why),
        }
    }

    /// "Use this one" / "Use <ip>": switch to a computer the last search found. `Err` is the
    /// plain reason (not one of the found computers).
    pub async fn lightkey_discovery_use(&self, address: &str) -> Result<String, String> {
        let bridge = DiscoveryBridge {
            inner: Arc::downgrade(&self.inner),
        };
        let (message, note) = self.inner.discovery.use_address(&bridge, address).await?;
        Ok(match note {
            Some(note) => format!("{message} {note}"),
            None => message,
        })
    }

    /// "Keep waiting": stop offering `address` until StagePilot restarts or the chosen address changes.
    pub fn lightkey_discovery_dismiss(&self, address: &str) {
        self.inner.discovery.dismiss(address);
    }

    /// "Read fixtures": ask the helper to re-read the open Lightkey project.
    pub async fn lightkey_read_fixtures(&self) -> crate::lightkey::LightkeyReadFixtures {
        self.link().read_fixtures().await
    }

    /// Fixture properties changed on the Lightkey Mac (relay stream). A change on a held fixture
    /// that is not the replay's own echo is the operator's: the fixture is given up.
    pub fn lightkey_fixture_changes(&self, commands: Vec<FixtureCommand>, keyframe: bool) {
        self.inner.operator_changes(&commands, keyframe);
    }

    /// The relay stream restarted: judge nothing for a few seconds.
    pub fn lightkey_stream_reset(&self) {
        self.inner.operator_stream_reset();
    }

    /// "Release lights": the double clear for every fixture held right now. Fixtures the
    /// operator has taken stay as they are.
    pub async fn lightkey_release_lights(&self) -> (bool, String) {
        // After a restart the held set is empty while Lightkey may still hold the overrides, so
        // every fixture the saved looks can override is cleared, not only the ones held now.
        let targets = {
            let mut st = lock(&self.inner.state);
            let targets = Self::releasable(&st);
            st.held_fixtures.extend(targets.iter().cloned());
            targets.len()
        };
        if targets == 0 {
            return (false, "No fixtures are held.".into());
        }
        self.inner.release_held_except(&BTreeSet::new()).await;
        (
            true,
            format!(
                "Released {targets} fixture{}.",
                if targets == 1 { "" } else { "s" }
            ),
        )
    }

    /// The fixtures Release lights clears: held now, or named by any saved look, except the
    /// ones the operator has taken (their change stays).
    fn releasable(st: &State) -> BTreeSet<String> {
        let mut all = st.held_fixtures.clone();
        for map in st.settings.cue_maps.values() {
            for cue in &map.osc_cues {
                for message in cue.outgoing() {
                    if let Some((name, _)) = fixture_override(&message.address) {
                        all.insert(name.to_string());
                    }
                }
            }
        }
        all.retain(|name| !st.operator.is_yielded(name));
        all
    }

    /// The audio input hub behind the picker and the meter; `None` in a build without audio.
    pub fn lightkey_audio(&self) -> Option<Arc<crate::audio::AudioHub>> {
        self.link().audio()
    }

    pub async fn lightkey_set_recording(&self, on: bool) -> String {
        let message = self.link().set_recording(on).await;
        // Arming the recorder switches replay off: give back what the timeline was holding.
        if !self.inner.replay_allowed() {
            self.inner.release_all().await;
        }
        message
    }

    /// Apply saved Lightkey settings to the running plugin and make the link re-read them.
    pub async fn lightkey_apply_settings(&self, lightkey: LightkeySettings) {
        lock(&self.inner.state).settings.lightkey = lightkey;
        self.link().settings_changed();
        // Replay switched off (or Lighting off, or recording armed): give the fixtures back.
        if !self.inner.replay_allowed() {
            self.inner.release_all().await;
        }
    }

    pub async fn snapshot(&self, refresh: bool) -> LightsStatusResponse {
        self.inner.clone().snapshot(refresh).await
    }

    pub async fn reconfigure(&self, settings: LightsSettings) -> LightingOutcome {
        self.inner.clone().reconfigure(settings).await
    }

    pub async fn replace_cue_map(&self, cue_map: SongLightingCueMap) {
        let mut st = lock(&self.inner.state);
        if !cue_map.cues.is_empty() || !cue_map.osc_cues.is_empty() {
            st.settings
                .cue_maps
                .insert(cue_map.song_key.clone(), cue_map);
        } else {
            st.settings.cue_maps.remove(&cue_map.song_key);
        }
    }

    pub async fn test_osc_cue(&self, address: &str, args: Vec<OscArgument>) -> LightingOutcome {
        self.inner.clone().test_osc_cue(address, args).await
    }

    pub async fn test_cue(&self, note: u8, velocity: u8) -> LightingOutcome {
        self.inner.clone().test_cue(note, velocity).await
    }

    pub fn settings(&self) -> LightsSettings {
        lock(&self.inner.state).settings.clone()
    }
}

#[async_trait]
impl Plugin for LightsPlugin {
    fn name(&self) -> &str {
        "lights"
    }
    fn version(&self) -> &str {
        "0.1.0"
    }

    async fn start(&self) -> Result<(), String> {
        self.inner.clone().start().await
    }
    async fn stop(&self) -> Result<(), String> {
        self.inner.clone().stop().await;
        Ok(())
    }
    async fn health(&self) -> Result<PluginHealth, String> {
        let st = lock(&self.inner.state);
        Ok(PluginHealth {
            name: "lights".into(),
            version: "0.1.0".into(),
            status: st.status,
            last_error: st.last_error.clone(),
            last_activity_at: st.last_activity_at,
        })
    }
}

impl Inner {
    fn st(&self) -> std::sync::MutexGuard<'_, State> {
        lock(&self.state)
    }

    async fn start(self: Arc<Self>) -> Result<(), String> {
        {
            let mut st = self.st();
            st.stopping = false;
            st.status = PluginStatus::Starting;
        }
        // Python builds the MIDI backend here and fails the start if it cannot. An OSC-only
        // lighting setup must not depend on MIDI hardware, so a backend that cannot be built is
        // deferred: it is retried (and reported as a MIDI error) only when MIDI is actually used.
        self.try_backend();
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
        sub!(EventType::SongStarted, on_song_event);
        sub!(EventType::SongRestarted, on_song_event);
        sub!(EventType::TimerStarted, on_timer_started);
        sub!(EventType::TimerResumed, on_timer_repositioned);
        sub!(EventType::TimerSeeked, on_timer_repositioned);
        sub!(EventType::TimerStopRequested, on_timer_stopped);
        sub!(EventType::TimerResetRequested, on_timer_stopped);
        sub!(EventType::TimerStopped, on_timer_stopped);
        sub!(EventType::TimerFailed, on_timer_stopped);
        self.st().subscriptions.extend(subs);
        self.st().status = PluginStatus::Running;
        if let Some(link) = self.lightkey.get() {
            link.start().await;
        }
        self.discovery.start(Arc::new(DiscoveryBridge {
            inner: Arc::downgrade(&self),
        }));
        if self.st().settings.enabled {
            self.clone().connect().await;
        } else {
            let detail = self.not_configured_detail();
            self.set_connection(ConnectionStatus::Disconnected, &detail)
                .await;
        }
        let task = tokio::spawn(self.clone().supervise_connection());
        self.st().connection_task = Some(task);
        Ok(())
    }

    async fn stop(self: Arc<Self>) {
        let task = {
            let mut st = self.st();
            st.status = PluginStatus::Stopping;
            st.stopping = true;
            st.connection_task.take()
        };
        self.stop.notify_waiters();
        if let Some(task) = task {
            task.abort();
            let _ = task.await;
        }
        self.cancel_timeline().await;
        // Shutdown gives the held fixtures back while the output is still open.
        self.release_all().await;
        self.discovery.stop();
        if let Some(link) = self.lightkey.get() {
            link.stop().await;
        }
        let subs: Vec<_> = std::mem::take(&mut self.st().subscriptions);
        for sub in subs {
            self.bus.unsubscribe(&sub);
        }
        {
            let _guard = self.operation_lock.lock().await;
            self.close_port_locked().await;
            self.st().backend = None;
        }
        self.st().status = PluginStatus::Stopped;
        self.set_connection(
            ConnectionStatus::Disconnected,
            "Disconnected. Lighting output stopped.",
        )
        .await;
    }

    fn try_backend(&self) -> Option<Arc<dyn MidiOutputBackend>> {
        if let Some(backend) = self.st().backend.clone() {
            return Some(backend);
        }
        match (self.midi_factory)() {
            Ok(backend) => {
                self.st().backend = Some(backend.clone());
                Some(backend)
            }
            Err(_) => None,
        }
    }

    fn require_backend(&self) -> Result<Arc<dyn MidiOutputBackend>, MidiError> {
        if let Some(backend) = self.st().backend.clone() {
            return Ok(backend);
        }
        let backend = (self.midi_factory)()?;
        self.st().backend = Some(backend.clone());
        Ok(backend)
    }

    async fn list_output_names(&self) -> Result<Vec<String>, MidiError> {
        let backend = self.require_backend()?;
        tokio::task::spawn_blocking(move || backend.list_output_names())
            .await
            .map_err(|_| MidiError::internal("MIDI backend task failed."))?
    }

    fn not_configured_detail(&self) -> String {
        if self.st().settings.method == LightingMethod::Osc {
            "Disconnected. Lighting OSC output is not configured.".into()
        } else {
            "Lighting MIDI output is not configured.".into()
        }
    }

    fn output_ready(&self) -> bool {
        let st = self.st();
        match st.settings.method {
            LightingMethod::Osc => st.osc_output.as_ref().is_some_and(|o| !o.closed()),
            LightingMethod::Midi => st.port.as_ref().is_some_and(|p| !p.closed()),
        }
    }

    // ---- snapshot / controller ----------------------------------------------------------------

    async fn snapshot(self: Arc<Self>, refresh: bool) -> LightsStatusResponse {
        if refresh {
            let enabled = self.st().settings.enabled;
            if enabled && !self.output_ready() {
                self.clone().connect().await;
            }
        }
        // The port list is read-only and shown whatever the method is. Only while MIDI is the
        // method does a listing failure count as an error on the connection.
        let midi_active = self.st().settings.method == LightingMethod::Midi;
        let names = match self.list_output_names().await {
            Ok(names) => names,
            Err(error) => {
                if midi_active {
                    self.record_midi_error(&error, "Lighting MIDI outputs could not be listed.")
                        .await;
                }
                Vec::new()
            }
        };
        let st = self.st();
        let s = &st.settings;
        let mut seen: Vec<&String> = Vec::new();
        let outputs = names
            .iter()
            .map(|name| {
                let _ = &mut seen;
                LightingOutputResponse {
                    name: name.clone(),
                    ambiguous: names.iter().filter(|n| *n == name).count() > 1,
                    selected: s.output_name.as_deref() == Some(name.as_str()),
                    connected: s.output_name.as_deref() == Some(name.as_str())
                        && st.port.as_ref().is_some_and(|p| !p.closed()),
                }
            })
            .collect();
        LightsStatusResponse {
            enabled: s.enabled,
            method: s.method,
            osc_host: s.osc_host.clone(),
            osc_port: s.osc_port,
            output_name: s.output_name.clone(),
            channel: s.channel,
            pulse_ms: s.pulse_ms,
            connection_status: st.connection_status,
            detail: st.detail.clone(),
            outputs,
            last_cue: st.last_cue.clone(),
            last_osc_cue: st.last_osc_cue.clone(),
            last_cue_at: st.last_cue_at,
            // The load of the last tick; an engine that has not ticked for a while is idle.
            replay_load: st
                .replay_load
                .filter(|(_, at)| at.elapsed() < REPLAY_LOAD_FRESH)
                .map(|(load, _)| load),
        }
    }

    /// What the search needs to know right now.
    fn discovery_inputs(&self) -> HostInputs {
        let reach = match self.lightkey.get().map(LightkeyLink::link_state) {
            Some(LightkeyLinkState::Connected | LightkeyLinkState::LightkeyClosed) => Reach::Up,
            Some(LightkeyLinkState::Unreachable) => Reach::Down,
            Some(LightkeyLinkState::Off) | None => Reach::Pending,
        };
        let st = self.st();
        HostInputs::from_settings(
            st.settings.enabled,
            st.settings.method == LightingMethod::Osc,
            reach,
            &st.settings.osc_host,
            &st.settings.lightkey,
        )
    }

    /// Save a found computer (the search, or the operator pressing Use) and let the link and the
    /// OSC output follow.
    async fn apply_discovery(self: Arc<Self>, change: Change) -> Result<Applied, String> {
        let (updated, applied) = match self.discovery.persist() {
            Some(persist) => persist(change).await?,
            None => {
                let mut settings = self.st().settings.clone();
                let applied = apply_change(&mut settings, &change);
                (settings, applied)
            }
        };
        let replay_host_changed = self.st().settings.osc_host != updated.osc_host;
        if replay_host_changed {
            self.clone().reconfigure(updated).await;
        } else {
            self.st().settings.lightkey = updated.lightkey;
            if let Some(link) = self.lightkey.get() {
                link.settings_changed();
            }
        }
        Ok(applied)
    }

    async fn reconfigure(self: Arc<Self>, settings: LightsSettings) -> LightingOutcome {
        self.cancel_timeline().await;
        // Lighting off, replay off, another method or another rig: give back what was held, on
        // the output it was held through, before that output is closed.
        let leaving = {
            let st = self.st();
            let old = &st.settings;
            !settings.enabled
                || settings.method != LightingMethod::Osc
                || !settings.lightkey.replay
                || old.osc_host != settings.osc_host
                || old.osc_port != settings.osc_port
        };
        if leaving {
            self.release_all().await;
        }
        {
            let _guard = self.operation_lock.lock().await;
            self.close_port_locked().await;
            self.st().settings = settings.clone();
        }
        if let Some(link) = self.lightkey.get() {
            link.settings_changed();
        }
        if !settings.enabled {
            {
                let mut st = self.st();
                st.status = PluginStatus::Running;
                st.last_error = None;
            }
            self.set_connection(
                ConnectionStatus::Disconnected,
                "Disconnected. Lighting output is disabled.",
            )
            .await;
            return LightingOutcome::new(true, "Lighting output disabled.");
        }
        let connected = self.clone().connect().await;
        let detail = self.st().detail.clone();
        LightingOutcome::new(
            connected,
            detail.unwrap_or_else(|| {
                if connected {
                    "Lighting output ready.".into()
                } else {
                    "Connection failed.".into()
                }
            }),
        )
    }

    async fn test_osc_cue(
        self: Arc<Self>,
        address: &str,
        args: Vec<OscArgument>,
    ) -> LightingOutcome {
        if self.st().settings.method != LightingMethod::Osc {
            return LightingOutcome::new(
                false,
                "Lighting is set to MIDI. Switch to OSC and save first.",
            );
        }
        let address = address.trim().to_string();
        if let Err(message) = validate_osc_address(&address) {
            return LightingOutcome::new(false, message);
        }
        let closed = self.st().osc_output.as_ref().is_none_or(|o| o.closed());
        if closed && !self.clone().connect().await {
            return LightingOutcome::new(
                false,
                self.st()
                    .detail
                    .clone()
                    .unwrap_or_else(|| "Disconnected. OSC output is unavailable.".into()),
            );
        }
        let test_cue = OscLightingCue {
            id: Uuid::new_v4(),
            at_seconds: 0.0,
            address: address.clone(),
            args: args.clone(),
            label: "Test cue".into(),
            color: None,
            messages: Vec::new(),
            source: Default::default(),
            edited: false,
            fade_seconds: 0.0,
        };
        if let Err(error) = self.send_osc(&address, &args).await {
            self.record_osc_error(&error, "OSC test cue failed.").await;
            return LightingOutcome::new(
                false,
                self.st()
                    .detail
                    .clone()
                    .unwrap_or_else(|| "OSC test cue failed.".into()),
            );
        }
        self.st().last_osc_cue = Some(test_cue); // the status shows the last cue sent, tests included
        let target = {
            let st = self.st();
            format!("{}:{}", st.settings.osc_host, st.settings.osc_port)
        };
        self.set_connection(
            ConnectionStatus::Connected,
            &format!("Sent {address} to {target}."),
        )
        .await;
        LightingOutcome::new(
            true,
            format!("Sent {address} to {target} (UDP, delivery not confirmed)."),
        )
    }

    async fn test_cue(self: Arc<Self>, note: u8, velocity: u8) -> LightingOutcome {
        if self.st().settings.method != LightingMethod::Midi {
            return LightingOutcome::new(
                false,
                "Lighting is set to OSC. Switch to MIDI and save first.",
            );
        }
        let cue = LightingCue {
            id: Uuid::new_v4(),
            at_seconds: 0.0,
            note,
            velocity,
            label: "Test cue".into(),
            color: None,
        };
        let closed = self.st().port.as_ref().is_none_or(|p| p.closed());
        if closed && !self.clone().connect().await {
            return LightingOutcome::new(
                false,
                self.st()
                    .detail
                    .clone()
                    .unwrap_or_else(|| "Lighting MIDI output is unavailable.".into()),
            );
        }
        if let Err(error) = self.clone().send_pulse(&cue).await {
            self.record_midi_error(&error, "Lighting test cue failed.")
                .await;
            return LightingOutcome::new(
                false,
                self.st()
                    .detail
                    .clone()
                    .unwrap_or_else(|| "Lighting test cue failed.".into()),
            );
        }
        LightingOutcome::new(true, format!("Sent lighting test cue on MIDI note {note}."))
    }

    // ---- events -------------------------------------------------------------------------------

    async fn on_song_event(self: &Arc<Self>, event: &StagePilotEvent) {
        let EventPayload::Song { song, .. } = &event.payload else {
            return;
        };
        self.cancel_timeline().await;
        self.st().pending_song = Some(song.clone());
    }

    fn active_cues(&self, song: &Song) -> Vec<AnyCue> {
        let st = self.st();
        let key = song
            .source_song_id
            .clone()
            .unwrap_or_else(|| song.id.clone());
        let Some(map) = st.settings.cue_maps.get(&key) else {
            return Vec::new();
        };
        match st.settings.method {
            LightingMethod::Osc => map.osc_cues.iter().cloned().map(AnyCue::Osc).collect(),
            LightingMethod::Midi => map.cues.iter().cloned().map(AnyCue::Midi).collect(),
        }
    }

    /// The song's effects (OSC method only; MIDI cues have none).
    fn active_effects(&self, song: &Song) -> Vec<LightingEffect> {
        let st = self.st();
        if st.settings.method != LightingMethod::Osc {
            return Vec::new();
        }
        let key = song
            .source_song_id
            .clone()
            .unwrap_or_else(|| song.id.clone());
        st.settings
            .cue_maps
            .get(&key)
            .map(|map| map.effects.clone())
            .unwrap_or_default()
    }

    /// May timeline fixture-override messages go out right now? Needs Lighting on, the OSC
    /// method, the Replay switch on and no recording armed.
    fn replay_allowed(&self) -> bool {
        let capturing = self.lightkey.get().is_some_and(LightkeyLink::capturing);
        let st = self.st();
        st.settings.enabled
            && st.settings.method == LightingMethod::Osc
            && st.settings.lightkey.replay
            && !capturing
    }

    /// Seconds into the song now, for a timer event that says the song was at `position` at
    /// `started_at`.
    fn song_position_now(position: f64, started_at: Option<DateTime<Utc>>) -> f64 {
        let elapsed = started_at
            .map(|s| (Utc::now() - s).num_milliseconds() as f64 / 1000.0)
            .map_or(0.0, |s| s.max(0.0));
        position + elapsed
    }

    async fn on_timer_started(self: &Arc<Self>, event: &StagePilotEvent) {
        let EventPayload::Timer(payload) = &event.payload else {
            return;
        };
        self.cancel_timeline().await;
        let song = payload
            .song
            .clone()
            .or_else(|| self.st().pending_song.clone());
        let Some(song) = song else {
            return;
        };
        // Capture or replay is decided once, here (the link decides the same way on this start).
        if let Some(link) = self.lightkey.get() {
            link.decide_for_start(&song, payload.started_at);
        }
        let active = self.active_cues(&song);
        self.take_back_due();
        // A new song: fixtures the last look held that this song does not use go back to
        // Lightkey. The ones it does use are taken over by its own rows.
        let used = if self.replay_allowed() {
            fixtures_used_by(&active)
        } else {
            BTreeSet::new()
        };
        let proceed = self.st().settings.enabled
            && !active.is_empty()
            && (self.output_ready() || self.clone().connect().await);
        if !proceed {
            self.release_fixtures_except(&used).await;
            return;
        }
        let now = Self::song_position_now(0.0, payload.started_at);
        let effects = self.active_effects(&song);
        self.start_timeline(
            &song,
            active,
            effects,
            Window {
                now,
                keep_from: f64::NEG_INFINITY,
                keep_to: f64::from(payload.duration_seconds),
                remaining: f64::from(payload.duration_seconds),
            },
        );
        // The release goes out after the timeline has started (S-01): releasing 100 unused
        // fixtures takes about a second and must not delay the new song's first look. It
        // reserves its packet slots one at a time, so rows interleave with it.
        let inner = self.clone();
        tokio::spawn(async move { inner.release_fixtures_except(&used).await });
    }

    /// Resume/seek: restart the timeline at the song position. Upcoming rows are scheduled; each
    /// fixture property is first brought to its latest value at or before the position.
    async fn on_timer_repositioned(self: &Arc<Self>, event: &StagePilotEvent) {
        let EventPayload::Timer(payload) = &event.payload else {
            return;
        };
        self.cancel_timeline().await;
        let Some(position) = payload.position_seconds else {
            return;
        };
        if !payload.running {
            return; // stopped: send nothing, and the held look stays
        }
        let position = position.max(0.0);
        let song = payload
            .song
            .clone()
            .or_else(|| self.st().pending_song.clone());
        let Some(song) = song else {
            return;
        };
        let active = self.active_cues(&song);
        if !self.st().settings.enabled || active.is_empty() {
            return;
        }
        self.take_back_due();
        let total = position + f64::from(payload.duration_seconds);
        if !self.output_ready() && !self.clone().connect().await {
            return;
        }
        let now = Self::song_position_now(position, payload.started_at);
        let effects = self.active_effects(&song);
        self.start_timeline(
            &song,
            active,
            effects,
            Window {
                now,
                keep_from: position,
                keep_to: total,
                remaining: f64::from(payload.duration_seconds),
            },
        );
    }

    async fn on_timer_stopped(self: &Arc<Self>, _event: &StagePilotEvent) {
        // Pause, stop, reset and song end cancel what is still pending. Nothing is cleared:
        // fixtures the timeline holds keep their last look.
        self.cancel_timeline().await;
    }

    fn start_timeline(
        self: &Arc<Self>,
        song: &Song,
        cues: Vec<AnyCue>,
        effects: Vec<LightingEffect>,
        window: Window,
    ) {
        let Window {
            now,
            keep_from,
            keep_to,
            remaining,
        } = window;
        let cues_for_playback = cues.clone();
        let allowed = self.replay_allowed();
        // A song with ramps or effects is played by the replay engine, which owns every
        // fixture message; the rows then carry only what is not a fixture override. Songs
        // captured before smooth replay keep the plain row-by-row path.
        let rows: Vec<OscLightingCue> = cues
            .iter()
            .filter_map(|c| match c {
                AnyCue::Osc(cue) => Some(cue.clone()),
                AnyCue::Midi(_) => None,
            })
            .collect();
        let smooth = allowed && is_smooth(&rows, &effects);
        let mut plan = plan_cues(cues, now, keep_from, keep_to, allowed && !smooth);
        let engine = smooth.then(|| ReplayEngine::new(&rows, &effects));
        if let Some(engine) = &engine {
            plan.not_yet = engine
                .fixtures_without_value(now + LOOK_LEAD_SECONDS)
                .into_iter()
                .collect();
        }
        if keep_from.is_finite() {
            // A resume or seek (S-03): a fixture the replay holds but the song has not touched
            // yet at this position would keep a later look. Give it back to Lightkey; its first
            // row takes it over again.
            let held = self.st().held_fixtures.clone();
            let mut clears: Vec<OscMessage> = plan
                .not_yet
                .iter()
                .filter(|name| held.contains(*name))
                .map(|name| OscMessage {
                    address: clear_address(name),
                    args: Vec::new(),
                })
                .collect();
            clears.append(&mut plan.sync);
            plan.sync = clears;
        }
        if plan.sync.is_empty() && plan.rows.is_empty() && engine.is_none() {
            return;
        }
        {
            let mut st = self.st();
            st.playback = Some(Playback {
                cues: cues_for_playback.clone(),
                position: now,
                at: Instant::now(),
                end: now + remaining,
            });
            st.engine = engine;
        }
        let replaying =
            (plan.has_fixtures() || self.st().engine.is_some()).then(|| song.title.clone());
        if let Some(link) = self.lightkey.get() {
            link.set_replaying(replaying.clone());
        }
        let inner = self.clone();
        let task = tokio::spawn(async move {
            inner
                .run_timeline(plan, now, replaying.is_some(), remaining)
                .await;
        });
        self.st().timeline = Some(task);
    }

    async fn run_timeline(self: Arc<Self>, plan: Plan, now: f64, replaying: bool, remaining: f64) {
        let mut set = JoinSet::new();
        if self.st().engine.is_some() {
            let inner = self.clone();
            set.spawn(async move { inner.run_engine(now, remaining).await });
        }
        // The re-sync goes out first and claims its packet slots before any row can.
        if !plan.sync.is_empty() {
            let first = self.reserve_slots(plan.sync.len());
            let inner = self.clone();
            let sync = plan.sync;
            set.spawn(async move {
                inner
                    .send_sync(sync, first)
                    .await
                    .map_err(|e| (e.message.clone(), e.public))
            });
        }
        for cue in plan.rows {
            let inner = self.clone();
            let (lead, paced) = row_timing(&cue);
            let delay = (cue.at_seconds() - lead - now).max(0.0);
            set.spawn(async move {
                if delay > 0.0 {
                    tokio::time::sleep(Duration::from_secs_f64(delay)).await;
                }
                match cue {
                    AnyCue::Osc(cue) => {
                        // One failed send is retried once, never the end of the show (S-02).
                        let mut result = inner.send_row(&cue, paced).await;
                        if result.is_err() {
                            tokio::time::sleep(SEND_RETRY_DELAY).await;
                            result = inner.send_row(&cue, false).await;
                        }
                        result.map_err(|e| (e.message.clone(), e.public))
                    }
                    AnyCue::Midi(cue) => inner
                        .clone()
                        .send_pulse(&cue)
                        .await
                        .map_err(|e| (e.message.clone(), e.public)),
                }
            });
        }
        let mut failed = false;
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok(Ok(())) => {
                    if failed {
                        // Sending works again: the error no longer describes the output.
                        failed = false;
                        {
                            let mut st = self.st();
                            st.status = PluginStatus::Running;
                            st.last_error = None;
                        }
                        self.set_connection(
                            ConnectionStatus::Connected,
                            "Lighting cues are going out again.",
                        )
                        .await;
                    }
                }
                Ok(Err((message, public))) => {
                    tracing::warn!(error = %message, "lighting_timeline_send_failed");
                    failed = true;
                    self.record_error_text(if public {
                        message
                    } else {
                        "A lighting cue could not be sent.".into()
                    })
                    .await;
                    if !self.output_ready() {
                        // The output is closed: nothing later can go out. The status stops
                        // saying "Replaying".
                        set.abort_all();
                        if let Some(link) = self.lightkey.get() {
                            link.set_replaying(None);
                        }
                        return;
                    }
                }
                Err(_) => {}
            }
        }
        // Stay "replaying" until the song's end; cancelling the timeline ends it sooner.
        if replaying {
            tokio::time::sleep(Duration::from_secs_f64(remaining.max(0.0))).await;
            if let Some(link) = self.lightkey.get() {
                link.set_replaying(None);
            }
        }
        // Dropping the set aborts anything still waiting; a pulse in flight sends its note off.
    }

    /// The smooth replay's one scheduler tick: ask the engine what changed at the song position
    /// (a little ahead, like a look row), send it paced, sleep until the engine needs the next
    /// tick. It runs until the song's end; pause, stop and seek cancel it, nothing is cleared.
    async fn run_engine(self: Arc<Self>, start: f64, remaining: f64) -> Result<(), (String, bool)> {
        let base = Instant::now();
        let mut failed = false;
        // Between the ticks and the 5 ms pacing: steps first, then ramps, then effects, and a
        // waiting ramp or effect value is replaced by a newer one for its address.
        let mut queue = PacingQueue::new();
        // When the next tick is due; `None` = nothing to look at until the song's end.
        let mut next_tick: Option<f64> = Some(0.0);
        loop {
            let now = Instant::now().saturating_duration_since(base).as_secs_f64();
            if now >= remaining {
                return Ok(());
            }
            if next_tick.is_some_and(|t| now + 1e-9 >= t) {
                let tick = {
                    let mut st = self.st();
                    let State {
                        engine, operator, ..
                    } = &mut *st;
                    let Some(engine) = engine.as_mut() else {
                        return Ok(());
                    };
                    let tick = engine.tick(now, start + now + LOOK_LEAD_SECONDS, &|address| {
                        fixture_of(address).is_some_and(|name| operator.is_yielded(name))
                    });
                    st.replay_load = Some((engine.load(), Instant::now()));
                    tick
                };
                let tick_position = start + now + LOOK_LEAD_SECONDS;
                for ((message, kind), position) in tick
                    .messages
                    .into_iter()
                    .zip(tick.kinds)
                    .zip(tick.positions)
                {
                    let reduced = tick.reduced && kind == Kind::Effect;
                    let early = (position - tick_position).abs() > 1e-9;
                    queue.push_for(message, kind, now, reduced, early.then_some(position));
                }
                next_tick = tick.wake_in.map(|w| now + w);
            }
            if !queue.is_empty() {
                // Wait for the next 5 ms slot, but not past the next tick: a step it brings
                // goes out before the older ramp and effect values still waiting.
                let slot = self.peek_slot();
                let tick_at = next_tick.map(|t| base + Duration::from_secs_f64(t.min(remaining)));
                if slot > tokio::time::Instant::now() {
                    let until = tick_at.map_or(slot, |t| t.min(slot));
                    tokio::select! {
                        () = tokio::time::sleep_until(until) => {}
                        () = self.engine_wake.notified() => { next_tick = Some(0.0); }
                    }
                    continue;
                }
                // A fixture the operator took over while its value waited (the middle of a step
                // burst, say) is skipped; it takes no slot and delays no one.
                {
                    let st = self.st();
                    queue.drop_where(&|m| {
                        fixture_of(&m.address).is_some_and(|name| st.operator.is_yielded(name))
                    });
                }
                let Some(item) = queue.pop() else {
                    continue;
                };
                self.reserve_slots(1);
                let message = item.message;
                // One failed send is retried once, never the end of the show (S-02).
                let mut result = self.send_message(&message).await;
                if result.is_err() {
                    tokio::time::sleep(SEND_RETRY_DELAY).await;
                    result = self.send_message(&message).await;
                }
                match result {
                    Ok(()) => {
                        if failed {
                            failed = false;
                            {
                                let mut st = self.st();
                                st.status = PluginStatus::Running;
                                st.last_error = None;
                            }
                            self.set_connection(
                                ConnectionStatus::Connected,
                                "Lighting cues are going out again.",
                            )
                            .await;
                        }
                    }
                    Err(error) => {
                        tracing::warn!(error = %error.message, "lighting_timeline_send_failed");
                        failed = true;
                        if let Some(engine) = self.st().engine.as_mut() {
                            engine.mark_unsent(&message.address);
                        }
                        self.record_error_text(if error.public {
                            error.message.clone()
                        } else {
                            "A lighting cue could not be sent.".into()
                        })
                        .await;
                        if !self.output_ready() {
                            if let Some(link) = self.lightkey.get() {
                                link.set_replaying(None);
                            }
                            return Ok(());
                        }
                    }
                }
                continue;
            }
            let (wake_at, forever) = match next_tick {
                Some(t) => (
                    base + Duration::from_secs_f64(t.min(remaining.max(0.0))),
                    false,
                ),
                None => (base + Duration::from_secs_f64(remaining.max(0.0)), true),
            };
            tokio::select! {
                () = tokio::time::sleep_until(wake_at) => {
                    if forever {
                        return Ok(());
                    }
                }
                () = self.engine_wake.notified() => { next_tick = Some(0.0); }
            }
        }
    }

    async fn cancel_timeline(&self) {
        let task = {
            let mut st = self.st();
            st.playback = None;
            st.engine = None;
            st.replay_load = None;
            st.timeline.take()
        };
        if let Some(task) = task {
            task.abort();
            let _ = task.await;
        }
        if let Some(link) = self.lightkey.get() {
            link.set_replaying(None);
        }
    }

    // ---- operator override ---------------------------------------------------------------------

    fn operator_stream_reset(&self) {
        self.st().operator.stream_reset(Instant::now());
    }

    fn operator_changes(self: &Arc<Self>, commands: &[FixtureCommand], keyframe: bool) {
        if !self.replay_allowed() {
            return;
        }
        let now = Instant::now();
        let verdict = {
            let mut guard = self.st();
            let st = &mut *guard;
            st.operator
                .observe(&mut st.held_fixtures, commands, keyframe, now)
        };
        for fixture in verdict.newly_yielded.iter().chain(&verdict.restarted) {
            self.ensure_watcher(fixture);
        }
    }

    /// One watcher per yielded fixture; a finished one (song paused when it ran out) is replaced.
    fn ensure_watcher(self: &Arc<Self>, fixture: &str) {
        let mut st = self.st();
        if st.watchers.get(fixture).is_some_and(|h| !h.is_finished()) {
            return;
        }
        let inner = self.clone();
        let name = fixture.to_string();
        let handle = tokio::spawn(async move { inner.watch_quiet(name).await });
        st.watchers.insert(fixture.to_string(), handle);
    }

    /// Wait out the quiet time (it restarts with every operator change), then take the fixture
    /// back if the song is running. Otherwise it stays due until the song resumes.
    async fn watch_quiet(self: Arc<Self>, fixture: String) {
        let quiet = Duration::from_secs_f64(OPERATOR_TAKE_BACK_SECONDS);
        loop {
            let Some(deadline) = self.st().operator.deadline(&fixture, quiet) else {
                return;
            };
            if Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep_until(deadline).await;
        }
        self.take_back(&fixture).await;
    }

    /// Replay takes `fixture` back: its latest message at or before the position, then its
    /// later rows (already scheduled) carry on.
    async fn take_back(&self, fixture: &str) {
        let (sync, ok) = {
            let mut st = self.st();
            let Some(playback) = st.playback.as_ref() else {
                return;
            };
            let position = playback.now();
            if position > playback.end {
                return;
            }
            if st.engine.is_some() {
                // The engine knows every value at every position: forget what it last sent to
                // this fixture and its next tick sends the value for this moment.
                let ok = st.operator.take_back(fixture);
                st.watchers.remove(fixture);
                if ok {
                    if let Some(engine) = st.engine.as_mut() {
                        engine.forget_fixture(fixture);
                    }
                }
                drop(st);
                if ok {
                    self.engine_wake.notify_one();
                }
                return;
            }
            let only: Vec<AnyCue> = playback
                .cues
                .iter()
                .filter_map(|cue| match cue {
                    AnyCue::Osc(cue) => {
                        let mut row = cue.clone();
                        row.messages = cue
                            .outgoing()
                            .into_iter()
                            .filter(|m| {
                                fixture_override(&m.address).is_some_and(|(n, _)| n == fixture)
                            })
                            .collect();
                        (!row.messages.is_empty()).then_some(AnyCue::Osc(row))
                    }
                    AnyCue::Midi(_) => None,
                })
                .collect();
            let ok = st.operator.take_back(fixture);
            st.watchers.remove(fixture);
            (
                plan_cues(only, position, f64::NEG_INFINITY, f64::NEG_INFINITY, true).sync,
                ok,
            )
        };
        if !ok || !self.replay_allowed() || sync.is_empty() {
            return;
        }
        let first = self.reserve_slots(sync.len());
        if let Err(error) = self.send_sync(sync, first).await {
            tracing::warn!(fixture, error = %error.message, "lightkey_take_back_failed");
        }
    }

    /// Resume, seek or a new song: fixtures whose quiet time ran out are the replay's again (the
    /// re-sync that follows sends them); the others stay with the operator.
    fn take_back_due(&self) {
        let quiet = Duration::from_secs_f64(OPERATOR_TAKE_BACK_SECONDS);
        let mut st = self.st();
        for fixture in st.operator.due(quiet, Instant::now()) {
            st.operator.take_back(&fixture);
            if let Some(handle) = st.watchers.remove(&fixture) {
                handle.abort();
            }
        }
    }

    // ---- pacing, holding and releasing --------------------------------------------------------

    /// Reserve `count` consecutive packet slots, `MIN_SEND_GAP` apart (<= 200 packets a second),
    /// and return the first one. Every paced send takes its slots here, so a burst from one row
    /// or a re-sync queues behind the one before it and order is kept. An aborted sender leaves
    /// its slots unused, which only costs a few milliseconds.
    fn reserve_slots(&self, count: usize) -> tokio::time::Instant {
        let now = tokio::time::Instant::now();
        let mut next = lock(&self.next_send);
        let first = next.map_or(now, |at| at.max(now));
        let count = u32::try_from(count).unwrap_or(u32::MAX);
        *next = Some(first + MIN_SEND_GAP * count);
        first
    }

    /// The next free packet slot (now when none is reserved), without reserving it.
    fn peek_slot(&self) -> tokio::time::Instant {
        let now = tokio::time::Instant::now();
        lock(&self.next_send).map_or(now, |at| at.max(now))
    }

    async fn pace(&self) {
        let slot = self.reserve_slots(1);
        if slot > tokio::time::Instant::now() {
            tokio::time::sleep_until(slot).await;
        }
    }

    /// Give back to Lightkey every held fixture that is not in `keep`. One clear per fixture,
    /// repeated once: a single wildcard clear was seen to leave fixtures held on a real rig.
    async fn release_fixtures_except(&self, keep: &BTreeSet<String>) {
        {
            // Nothing held means nothing left to take back either.
            let mut st = self.st();
            st.operator.retain_yielded(keep);
            let stale: Vec<String> = st
                .watchers
                .keys()
                .filter(|name| !keep.contains(*name))
                .cloned()
                .collect();
            for name in stale {
                if let Some(handle) = st.watchers.remove(&name) {
                    handle.abort();
                }
            }
        }
        self.release_held_except(keep).await;
    }

    /// The double clear for held fixtures not in `keep`; the operator's fixtures are not touched.
    async fn release_held_except(&self, keep: &BTreeSet<String>) {
        let released: Vec<String> = {
            let mut st = self.st();
            let release: Vec<String> = st.held_fixtures.difference(keep).cloned().collect();
            for name in &release {
                st.held_fixtures.remove(name);
                st.operator.forget_sent(name);
            }
            release
        };
        if released.is_empty() {
            return;
        }
        for round in 0..2 {
            for name in &released {
                self.pace().await;
                if let Err(error) = self.send_command_osc(clear_address(name), Vec::new()).await {
                    tracing::warn!(fixture = %name, error = %error, "lightkey_fixture_clear_failed");
                }
            }
            if round == 0 {
                tokio::time::sleep(RELEASE_ROUND_GAP).await;
            }
        }
    }

    async fn release_all(&self) {
        self.release_fixtures_except(&BTreeSet::new()).await;
    }

    /// Send the re-sync messages from slot `first` on, one slot each.
    async fn send_sync(
        &self,
        messages: Vec<OscMessage>,
        first: tokio::time::Instant,
    ) -> Result<(), OutputError> {
        for (i, message) in messages.iter().enumerate() {
            self.wait_slot(first, i).await;
            self.send_message(message).await?;
        }
        Ok(())
    }

    async fn wait_slot(&self, first: tokio::time::Instant, index: usize) {
        let slot = first + MIN_SEND_GAP * u32::try_from(index).unwrap_or(u32::MAX);
        if slot > tokio::time::Instant::now() {
            tokio::time::sleep_until(slot).await;
        }
    }

    /// Send every message of a row together. A paced row takes one packet slot per message.
    /// Fixture-override messages are held (and not sent at all while replay is not allowed).
    async fn send_row(&self, cue: &OscLightingCue, paced: bool) -> Result<(), OutputError> {
        let messages = cue.outgoing();
        let first = paced.then(|| self.reserve_slots(messages.len()));
        for (i, message) in messages.iter().enumerate() {
            if let Some(first) = first {
                self.wait_slot(first, i).await;
            }
            self.send_message(message).await?;
        }
        self.st().last_osc_cue = Some(cue.clone());
        Ok(())
    }

    /// One timeline message. A fixture-override message is dropped unless replay is allowed;
    /// when it goes out its fixture is held until a release (`overrides/clear` releases it).
    async fn send_message(&self, message: &OscMessage) -> Result<(), OutputError> {
        if let Some((name, property)) = fixture_override(&message.address) {
            if !self.replay_allowed() {
                return Ok(());
            }
            let mut st = self.st();
            if property == "clear" {
                st.held_fixtures.remove(name);
            } else if st.operator.is_yielded(name) {
                return Ok(()); // the operator has this fixture
            } else {
                st.held_fixtures.insert(name.to_string());
                st.operator
                    .note_send(&message.address, &message.args, Instant::now());
            }
        }
        self.send_osc(&message.address, &message.args).await
    }

    // ---- sending ------------------------------------------------------------------------------

    /// One-shot OSC with arguments through the saved output (the release clears).
    async fn send_command_osc(
        &self,
        address: String,
        args: Vec<OscArgument>,
    ) -> Result<(), String> {
        let output = {
            let st = self.st();
            if st.settings.method != LightingMethod::Osc {
                return Ok(());
            }
            st.osc_output.clone()
        };
        let Some(output) = output.filter(|o| !o.closed()) else {
            return Ok(());
        };
        tokio::task::spawn_blocking(move || output.send_message(&address, &args))
            .await
            .map_err(|_| "OSC send task failed.".to_string())?
            .map_err(|e| e.message)?;
        self.st().last_activity_at = Some(Utc::now());
        Ok(())
    }

    async fn send_osc(&self, address: &str, args: &[OscArgument]) -> Result<(), OutputError> {
        let output = {
            let st = self.st();
            if st.settings.method != LightingMethod::Osc {
                return Ok(()); // never send OSC while MIDI is the saved method
            }
            st.osc_output.clone()
        };
        let Some(output) = output.filter(|o| !o.closed()) else {
            return Err(OutputError::public(
                "Disconnected. The OSC output closed before a cue was sent.",
            ));
        };
        let (sent_address, sent_args) = (address.to_string(), args.to_vec());
        tokio::task::spawn_blocking(move || output.send_message(&sent_address, &sent_args))
            .await
            .map_err(|_| OutputError::internal("OSC send task failed."))??;
        let now = Utc::now();
        {
            let mut st = self.st();
            st.last_cue_at = Some(now);
            st.last_activity_at = Some(now);
        }
        Ok(())
    }

    async fn send_pulse(self: Arc<Self>, cue: &LightingCue) -> Result<(), MidiError> {
        let (port, channel, pulse_ms) = {
            let st = self.st();
            if st.settings.method != LightingMethod::Midi {
                return Ok(()); // never send MIDI while OSC is the saved method
            }
            (st.port.clone(), st.settings.channel, st.settings.pulse_ms)
        };
        let Some(port) = port.filter(|p| !p.closed()) else {
            return Err(MidiError::public(
                "Lighting MIDI output disconnected before a cue was sent.",
            ));
        };
        let (on_port, note, velocity) = (port.clone(), cue.note, cue.velocity);
        // The note off is sent even if this future is aborted mid-pulse (timeline cancelled). The
        // guard exists before the note on goes out: an abort between the send and the guard would
        // otherwise leave the note stuck on (a harmless extra note off is the other way round).
        let guard = NoteOffGuard {
            port: Some(port),
            channel,
            note: cue.note,
        };
        tokio::task::spawn_blocking(move || on_port.send_note_on(channel, note, velocity))
            .await
            .map_err(|_| MidiError::internal("MIDI send task failed."))??;
        tokio::time::sleep(Duration::from_secs_f64(f64::from(pulse_ms) / 1000.0)).await;
        guard.fire_now().await;
        let now = Utc::now();
        let mut st = self.st();
        st.last_cue = Some(cue.clone());
        st.last_cue_at = Some(now);
        st.last_activity_at = Some(now);
        Ok(())
    }

    // ---- connection ---------------------------------------------------------------------------

    async fn connect_osc(self: &Arc<Self>) -> bool {
        let (host, port) = {
            let st = self.st();
            (st.settings.osc_host.clone(), st.settings.osc_port)
        };
        let target = format!("{host}:{port}");
        let _guard = self.operation_lock.lock().await;
        self.close_port_locked().await;
        let factory = self.osc_factory.clone();
        let opened = tokio::task::spawn_blocking(move || factory().open_output(&host, port))
            .await
            .map_err(|_| OutputError::internal("OSC open task failed."))
            .and_then(|r| r);
        match opened {
            Ok(output) => {
                {
                    let mut st = self.st();
                    st.osc_output = Some(output);
                    st.status = PluginStatus::Running;
                    st.last_error = None;
                    st.last_activity_at = Some(Utc::now());
                }
                // UDP has no handshake, so the output opening proves nothing about delivery. The card says
                // "Ready" while only the replay address is available, and "Connected" once capture (the
                // helper link) is up too.
                let (status, detail) = osc_connection(self.capture_connected(), &target);
                self.set_connection(status, &detail).await;
                true
            }
            Err(error) => {
                let message = public_osc(&error, &format!("OSC output to {target} failed."));
                {
                    let mut st = self.st();
                    st.status = PluginStatus::Error;
                    st.last_error = Some(message.clone());
                }
                self.set_connection(ConnectionStatus::Error, &message).await;
                false
            }
        }
    }

    /// The helper link is up, so capture (and the fixture stream replay needs) is available.
    fn capture_connected(&self) -> bool {
        self.lightkey
            .get()
            .is_some_and(|link| link.status().link == crate::lightkey::LightkeyLink::Connected)
    }

    /// Re-judge the Lighting card after the helper link changed. Only moves between Ready and Connected while
    /// the OSC output is open; an error or a closed output keeps its own status.
    async fn refresh_osc_status(self: &Arc<Self>) {
        let target = {
            let st = self.st();
            let open = st.settings.enabled
                && st.settings.method == LightingMethod::Osc
                && st.osc_output.as_ref().is_some_and(|o| !o.closed())
                && matches!(
                    st.connection_status,
                    ConnectionStatus::Ready | ConnectionStatus::Connected
                );
            if !open {
                return;
            }
            format!("{}:{}", st.settings.osc_host, st.settings.osc_port)
        };
        let (status, detail) = osc_connection(self.capture_connected(), &target);
        self.set_connection(status, &detail).await;
    }

    async fn connect(self: Arc<Self>) -> bool {
        let settings = self.st().settings.clone();
        if !settings.enabled {
            return false;
        }
        if settings.method == LightingMethod::Osc {
            return self.connect_osc().await;
        }
        let Some(output_name) = settings.output_name.clone() else {
            {
                let mut st = self.st();
                st.status = PluginStatus::Running;
                st.last_error = None;
            }
            self.set_connection(
                ConnectionStatus::Disconnected,
                "No lighting MIDI output is selected.",
            )
            .await;
            return false;
        };
        let names = match self.list_output_names().await {
            Ok(names) => names,
            Err(error) => {
                self.record_midi_error(&error, "Lighting MIDI outputs could not be listed.")
                    .await;
                return false;
            }
        };
        match names.iter().filter(|n| **n == output_name).count() {
            0 => {
                {
                    let mut st = self.st();
                    st.status = PluginStatus::Running;
                    st.last_error = None;
                }
                self.set_connection(
                    ConnectionStatus::Disconnected,
                    &format!("Lighting MIDI output \"{output_name}\" is unavailable; retrying."),
                )
                .await;
                return false;
            }
            1 => {}
            _ => {
                let message =
                    format!("Multiple lighting MIDI outputs are named \"{output_name}\".");
                {
                    let mut st = self.st();
                    st.status = PluginStatus::Error;
                    st.last_error = Some(message.clone());
                }
                self.set_connection(ConnectionStatus::Error, &message).await;
                return false;
            }
        }
        self.set_connection(
            ConnectionStatus::Connecting,
            &format!("Connecting to lighting MIDI output {output_name}."),
        )
        .await;
        let _guard = self.operation_lock.lock().await;
        self.close_port_locked().await;
        let opened = match self.require_backend() {
            Ok(backend) => {
                let name = output_name.clone();
                tokio::task::spawn_blocking(move || backend.open_output(&name))
                    .await
                    .map_err(|_| MidiError::internal("MIDI open task failed."))
                    .and_then(|r| r)
            }
            Err(e) => Err(e),
        };
        match opened {
            Ok(port) => {
                {
                    let mut st = self.st();
                    st.port = Some(port);
                    st.status = PluginStatus::Running;
                    st.last_error = None;
                    st.last_activity_at = Some(Utc::now());
                }
                self.set_connection(
                    ConnectionStatus::Connected,
                    &format!("Connected to lighting MIDI output {output_name}."),
                )
                .await;
                true
            }
            Err(error) => {
                let message = public_midi(&error, "Lighting MIDI output failed.");
                {
                    let mut st = self.st();
                    st.status = PluginStatus::Error;
                    st.last_error = Some(message.clone());
                }
                self.set_connection(ConnectionStatus::Error, &message).await;
                false
            }
        }
    }

    async fn supervise_connection(self: Arc<Self>) {
        loop {
            if self.st().stopping {
                return;
            }
            tokio::select! {
                () = self.stop.notified() => return,
                () = tokio::time::sleep(self.monitor_interval) => {}
            }
            let (enabled, method, configured) = {
                let st = self.st();
                (
                    st.settings.enabled,
                    st.settings.method,
                    st.settings.output_name.clone(),
                )
            };
            if !enabled || method != LightingMethod::Midi {
                continue; // OSC over UDP has nothing to monitor
            }
            let names = match self.list_output_names().await {
                Ok(names) => names,
                Err(error) => {
                    self.record_midi_error(&error, "Lighting MIDI outputs could not be listed.")
                        .await;
                    continue;
                }
            };
            let matches = configured
                .as_ref()
                .map_or(0, |c| names.iter().filter(|n| *n == c).count());
            let port_ok = self.st().port.as_ref().is_some_and(|p| !p.closed());
            if configured.is_none() || matches != 1 || !port_ok {
                {
                    let _guard = self.operation_lock.lock().await;
                    self.close_port_locked().await;
                }
                if matches > 1 {
                    let message = format!(
                        "Multiple lighting MIDI outputs are named \"{}\".",
                        configured.clone().unwrap_or_default()
                    );
                    {
                        let mut st = self.st();
                        st.status = PluginStatus::Error;
                        st.last_error = Some(message.clone());
                    }
                    self.set_connection(ConnectionStatus::Error, &message).await;
                } else {
                    {
                        let mut st = self.st();
                        st.status = PluginStatus::Running;
                        st.last_error = None;
                    }
                    let detail = match &configured {
                        Some(name) => {
                            format!("Lighting MIDI output \"{name}\" is unavailable; retrying.")
                        }
                        None => "No lighting MIDI output is selected.".into(),
                    };
                    self.set_connection(ConnectionStatus::Disconnected, &detail)
                        .await;
                }
                if configured.is_some() && matches == 1 {
                    self.clone().connect().await;
                }
            }
        }
    }

    async fn close_port_locked(&self) {
        let (port, osc) = {
            let mut st = self.st();
            (st.port.take(), st.osc_output.take())
        };
        if let Some(port) = port.filter(|p| !p.closed()) {
            let _ = tokio::task::spawn_blocking(move || port.close()).await;
        }
        if let Some(osc) = osc.filter(|o| !o.closed()) {
            let _ = tokio::task::spawn_blocking(move || osc.close()).await;
        }
    }

    async fn record_error_text(&self, detail: String) {
        {
            let mut st = self.st();
            st.status = PluginStatus::Error;
            st.last_error = Some(detail.clone());
            st.last_activity_at = Some(Utc::now());
        }
        tracing::error!(detail = %detail, "lighting_midi_failed");
        self.set_connection(ConnectionStatus::Error, &detail).await;
    }

    async fn record_midi_error(&self, error: &MidiError, fallback: &str) {
        self.record_error_text(public_midi(error, fallback)).await;
    }

    async fn record_osc_error(&self, error: &OutputError, fallback: &str) {
        self.record_error_text(public_osc(error, fallback)).await;
    }

    async fn set_connection(&self, status: ConnectionStatus, detail: &str) {
        {
            let mut st = self.st();
            if st.connection_status == status && st.detail.as_deref() == Some(detail) {
                return;
            }
            st.connection_status = status;
            st.detail = Some(detail.to_string());
        }
        self.bus
            .publish(new_event(
                EventType::ConnectionChanged,
                "lights",
                EventPayload::Connection {
                    integration: Integration::Lights,
                    status,
                    detail: Some(detail.to_string()),
                },
            ))
            .await;
    }
}

/// What the Lighting card says while the OSC output is open: Connected when capture is up as well as the
/// replay address, Ready when only the replay address is available.
fn osc_connection(capture_connected: bool, target: &str) -> (ConnectionStatus, String) {
    if capture_connected {
        (
            ConnectionStatus::Connected,
            format!("Capture and replay connected. Replay sends to {target}."),
        )
    } else {
        (
            ConnectionStatus::Ready,
            format!("Replay ready for {target}. Capture is not connected yet."),
        )
    }
}

/// `(fixture, property)` of a `/fixture/<name>/overrides/<property>` address.
fn fixture_override(address: &str) -> Option<(&str, &str)> {
    let name = fixture_of(address)?;
    let property = address.rsplit('/').next()?;
    Some((name, property))
}

fn fixtures_used_by(cues: &[AnyCue]) -> BTreeSet<String> {
    cues.iter()
        .filter_map(|cue| match cue {
            AnyCue::Osc(cue) => Some(cue),
            AnyCue::Midi(_) => None,
        })
        .flat_map(OscLightingCue::outgoing)
        .filter_map(|m| fixture_override(&m.address).map(|(name, _)| name.to_string()))
        .collect()
}

/// How a row leaves: seconds early, and whether its messages are paced. Rows that carry fixture
/// overrides leave `LOOK_LEAD_SECONDS` early and paced; look rows are paced; everything else
/// (MIDI, a single `/live` cue) leaves exactly as it always did.
fn row_timing(cue: &AnyCue) -> (f64, bool) {
    match cue {
        AnyCue::Midi(_) => (0.0, false),
        AnyCue::Osc(cue) => {
            let fixtures = cue
                .outgoing()
                .iter()
                .any(|m| fixture_override(&m.address).is_some());
            (
                if fixtures { LOOK_LEAD_SECONDS } else { 0.0 },
                fixtures || !cue.messages.is_empty(),
            )
        }
    }
}

/// What a timer event schedules.
struct Plan {
    /// Latest message per fixture-override address at or before the position, sent once, paced.
    sync: Vec<OscMessage>,
    /// Rows still to send, in time order.
    rows: Vec<AnyCue>,
    /// Fixtures the song only touches after the position (no message at or before it).
    not_yet: BTreeSet<String>,
}

impl Plan {
    fn has_fixtures(&self) -> bool {
        !self.sync.is_empty() || self.rows.iter().any(|r| row_timing(r).0 > 0.0)
    }
}

/// Split the song's rows at the song position `now`.
///
/// Fixture-override messages (when `allow_fixtures`): those at or before `now` are reduced to
/// the latest per address (the re-sync); later ones stay on their rows. Everything else keeps
/// the old rule: a row is scheduled when `keep_from <= at <= keep_to`, and a row already past
/// goes out at once. A row that mixes both is split into the parts that still apply.
fn plan_cues(
    mut cues: Vec<AnyCue>,
    now: f64,
    keep_from: f64,
    keep_to: f64,
    allow_fixtures: bool,
) -> Plan {
    cues.sort_by(|a, b| a.at_seconds().total_cmp(&b.at_seconds()));
    let wanted = |at: f64| keep_from <= at && at <= keep_to;
    let mut latest: BTreeMap<String, (usize, OscMessage)> = BTreeMap::new();
    let mut order = 0usize;
    let mut rows = Vec::new();
    let mut touched_earlier = BTreeSet::new();
    let mut touched_any = BTreeSet::new();
    for cue in cues {
        let cue = match cue {
            AnyCue::Midi(cue) => {
                if wanted(cue.at_seconds) {
                    rows.push(AnyCue::Midi(cue));
                }
                continue;
            }
            AnyCue::Osc(cue) => cue,
        };
        let (fixture, other): (Vec<_>, Vec<_>) = cue
            .outgoing()
            .into_iter()
            .partition(|m| fixture_override(&m.address).is_some());
        if fixture.is_empty() {
            if wanted(cue.at_seconds) {
                rows.push(AnyCue::Osc(cue));
            }
            continue;
        }
        let mut keep = Vec::new();
        if allow_fixtures {
            for message in &fixture {
                if let Some((name, _)) = fixture_override(&message.address) {
                    touched_any.insert(name.to_string());
                    if cue.at_seconds <= now {
                        touched_earlier.insert(name.to_string());
                    }
                }
            }
            if cue.at_seconds <= now {
                for message in fixture {
                    order += 1;
                    latest.insert(message.address.clone(), (order, message));
                }
            } else if cue.at_seconds <= keep_to {
                keep.extend(fixture);
            }
        }
        if wanted(cue.at_seconds) {
            keep.extend(other);
        }
        if !keep.is_empty() {
            let mut row = cue;
            row.messages = keep;
            rows.push(AnyCue::Osc(row));
        }
    }
    let mut sync: Vec<_> = latest.into_values().collect();
    sync.sort_by_key(|(order, _)| *order);
    Plan {
        sync: sync.into_iter().map(|(_, message)| message).collect(),
        rows,
        not_yet: touched_any.difference(&touched_earlier).cloned().collect(),
    }
}

/// Sends the note off when dropped (an aborted pulse) unless `fire_now` already did.
struct NoteOffGuard {
    port: Option<Arc<dyn MidiOutputPort>>,
    channel: u8,
    note: u8,
}

impl NoteOffGuard {
    async fn fire_now(mut self) {
        if let Some(port) = self.port.take() {
            let (channel, note) = (self.channel, self.note);
            match tokio::task::spawn_blocking(move || port.send_note_off(channel, note)).await {
                Ok(Ok(())) => {}
                _ => tracing::warn!(note, "lighting_note_off_failed"),
            }
        }
    }
}

impl Drop for NoteOffGuard {
    fn drop(&mut self) {
        if let Some(port) = self.port.take() {
            let (channel, note) = (self.channel, self.note);
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                drop(handle.spawn_blocking(move || {
                    let _ = port.send_note_off(channel, note);
                }));
            }
        }
    }
}
