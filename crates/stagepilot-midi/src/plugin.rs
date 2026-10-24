//! Resilient Playback MIDI input plugin and cue-to-action translation: port of
//! `plugins/midi_playback/plugin.py`.
//!
//! Behaviour kept from Python (see `docs/rust-migration/phase4-test-map.md`):
//!
//! * **Connection activation.** A callback carries the id of the connection that produced it. A
//!   message whose connection is not the *active* one is dropped as stale. The active id is set
//!   only after the port is open and the previous port is closed, so a note that arrives before
//!   activation (or from a replaced port) is dropped, never dispatched.
//! * One FIFO queue (capacity 64) between the device thread and a single consumer task: cues are
//!   dispatched strictly in arrival order, a full queue drops the new message and reports an error.
//! * Note-release latching plus a monotonic-clock debounce suppress duplicate triggers.
//! * Velocity 1..=99 is an exact song position, 100.. maps through the configured mapping.
//!
//! Differences from Python: backend calls run on tokio's blocking pool instead of one dedicated
//! thread (so a hung device call cannot wedge the others, and the plugin manager's timeouts apply),
//! and the device callback offers into the queue directly from the device thread (Python hops to
//! the loop with `call_soon_threadsafe`; ordering per device is unchanged).

use crate::models::{
    cue_action, cue_from_str, input_id, note_name, MidiMessage, MidiMessageDisposition,
    MidiMonitorMessage, NoteType,
};
use crate::{
    MidiBackend, MidiBackendFactory, MidiError, MidiInputPort, MidiInputResponse,
    MidiInputsResponse, MidiMessageCallback,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::future::BoxFuture;
use futures_util::FutureExt;
use stagepilot_core::bus::EventBus;
use stagepilot_core::config::MidiSettings;
use stagepilot_core::events::{new_event, ActionName, EventPayload, EventType, Integration};
use stagepilot_core::plugin::Plugin;
use stagepilot_core::state::{ConnectionStatus, PluginHealth, PluginStatus};
use stagepilot_core::state_service::{ActionOutcome, StateService};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot, Notify};
use tokio::task::JoinHandle;
use uuid::Uuid;

pub const QUEUE_CAPACITY: usize = 64;
pub const DEFAULT_RECONNECT_DELAYS: [f64; 5] = [1.0, 2.0, 4.0, 8.0, 15.0];
pub const DEFAULT_MONITOR_INTERVAL: f64 = 1.0;
pub const MIDI_MONITOR_CAPACITY: usize = 50;
pub const MIN_SONG_POSITION_VELOCITY: u8 = 1;
pub const MAX_SONG_POSITION_VELOCITY: u8 = 99;

pub const NOT_RUNNING: &str = "The MIDI Playback plugin is not running.";

/// `ActionDispatcher`: the narrow action boundary used by input integrations such as MIDI.
#[async_trait]
pub trait ActionDispatcher: Send + Sync + 'static {
    async fn dispatch(&self, action: ActionName, source: &str) -> ActionOutcome;
    /// Start the exact one-based service-plan song requested by an input.
    async fn dispatch_song_position(&self, position: i64, source: &str) -> ActionOutcome;
}

#[async_trait]
impl ActionDispatcher for StateService {
    async fn dispatch(&self, action: ActionName, source: &str) -> ActionOutcome {
        StateService::dispatch(self, action, source).await
    }
    async fn dispatch_song_position(&self, position: i64, source: &str) -> ActionOutcome {
        StateService::dispatch_song_position(self, position, source).await
    }
}

pub type ActionGuard = Arc<dyn Fn() -> Option<String> + Send + Sync>;
pub type OwnershipRevision = Arc<dyn Fn() -> i64 + Send + Sync>;
pub type ConnectionObserver = Arc<dyn Fn(ConnectionStatus) -> BoxFuture<'static, ()> + Send + Sync>;
pub type MonotonicClock = Arc<dyn Fn() -> f64 + Send + Sync>;

pub struct MidiPluginOptions {
    pub backend_factory: MidiBackendFactory,
    pub clock: MonotonicClock,
    pub reconnect_delays: Vec<Duration>,
    pub monitor_interval: Duration,
    pub queue_capacity: usize,
}

impl MidiPluginOptions {
    pub fn new(backend_factory: MidiBackendFactory) -> Self {
        let origin = Instant::now();
        Self {
            backend_factory,
            clock: Arc::new(move || origin.elapsed().as_secs_f64()),
            reconnect_delays: DEFAULT_RECONNECT_DELAYS
                .iter()
                .map(|s| Duration::from_secs_f64(*s))
                .collect(),
            monitor_interval: Duration::from_secs_f64(DEFAULT_MONITOR_INTERVAL),
            queue_capacity: QUEUE_CAPACITY,
        }
    }
}

struct Queued {
    message: MidiMessage,
    connection_id: Option<Uuid>,
    simulated: bool,
    release_after: bool,
    completion: Option<oneshot::Sender<ActionOutcome>>,
    ownership_revision: Option<i64>,
}

fn complete(completion: Option<oneshot::Sender<ActionOutcome>>, outcome: ActionOutcome) {
    if let Some(tx) = completion {
        let _ = tx.send(outcome);
    }
}

#[derive(Default)]
struct Hooks {
    action_guard: Option<ActionGuard>,
    ownership_revision: Option<OwnershipRevision>,
    connection_observer: Option<ConnectionObserver>,
}

struct State {
    settings: MidiSettings,
    backend: Option<Arc<dyn MidiBackend>>,
    handle: Option<tokio::runtime::Handle>,
    tx: Option<mpsc::Sender<Queued>>,
    consumer: Option<JoinHandle<()>>,
    supervisor: Option<JoinHandle<()>>,
    side_tasks: Vec<JoinHandle<()>>,
    port: Option<Arc<dyn MidiInputPort>>,
    active_connection_id: Option<Uuid>,
    selected_input_name: Option<String>,
    connected_input_name: Option<String>,
    available_input_names: Vec<String>,
    recent: VecDeque<MidiMonitorMessage>,
    held_notes: HashSet<(u8, u8, u8)>,
    last_triggered_at: HashMap<(u8, u8, u8), f64>,
    status: PluginStatus,
    connection_status: ConnectionStatus,
    connection_detail: Option<String>,
    last_error: Option<String>,
    last_activity_at: Option<DateTime<Utc>>,
    stopping: bool,
    overflow_reported: bool,
}

struct Inner {
    bus: EventBus,
    dispatcher: Mutex<Arc<dyn ActionDispatcher>>,
    backend_factory: MidiBackendFactory,
    clock: MonotonicClock,
    reconnect_delays: Vec<Duration>,
    monitor_interval: Duration,
    queue_capacity: usize,
    state: Mutex<State>,
    hooks: Mutex<Hooks>,
    wake: Notify,
    selection_lock: tokio::sync::Mutex<()>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Listen to one MIDI input and dispatch configured StagePilot actions.
#[derive(Clone)]
pub struct MidiPlaybackPlugin {
    inner: Arc<Inner>,
}

impl MidiPlaybackPlugin {
    pub fn new(
        bus: EventBus,
        settings: MidiSettings,
        dispatcher: Arc<dyn ActionDispatcher>,
        options: MidiPluginOptions,
    ) -> Result<Self, String> {
        if options.reconnect_delays.is_empty() {
            return Err("MIDI reconnect delays must be non-negative.".into());
        }
        if options.monitor_interval.is_zero() {
            return Err("MIDI monitor interval must be positive.".into());
        }
        if options.queue_capacity < 1 {
            return Err("MIDI queue capacity must be positive.".into());
        }
        let selected = settings.input_name.clone();
        Ok(Self {
            inner: Arc::new(Inner {
                bus,
                dispatcher: Mutex::new(dispatcher),
                backend_factory: options.backend_factory,
                clock: options.clock,
                reconnect_delays: options.reconnect_delays,
                monitor_interval: options.monitor_interval,
                queue_capacity: options.queue_capacity,
                state: Mutex::new(State {
                    settings,
                    backend: None,
                    handle: None,
                    tx: None,
                    consumer: None,
                    supervisor: None,
                    side_tasks: Vec::new(),
                    port: None,
                    active_connection_id: None,
                    selected_input_name: selected,
                    connected_input_name: None,
                    available_input_names: Vec::new(),
                    recent: VecDeque::new(),
                    held_notes: HashSet::new(),
                    last_triggered_at: HashMap::new(),
                    status: PluginStatus::Stopped,
                    connection_status: ConnectionStatus::Disconnected,
                    connection_detail: None,
                    last_error: None,
                    last_activity_at: None,
                    stopping: false,
                    overflow_reported: false,
                }),
                hooks: Mutex::new(Hooks::default()),
                wake: Notify::new(),
                selection_lock: tokio::sync::Mutex::new(()),
            }),
        })
    }

    /// Rewire the dispatcher (the Playback arbiter wraps it, `_wire_midi`).
    pub fn set_dispatcher(&self, dispatcher: Arc<dyn ActionDispatcher>) {
        *lock(&self.inner.dispatcher) = dispatcher;
    }
    pub fn set_action_guard(&self, guard: Option<ActionGuard>) {
        lock(&self.inner.hooks).action_guard = guard;
    }
    pub fn set_ownership_revision(&self, revision: Option<OwnershipRevision>) {
        lock(&self.inner.hooks).ownership_revision = revision;
    }
    pub fn set_connection_observer(&self, observer: Option<ConnectionObserver>) {
        lock(&self.inner.hooks).connection_observer = observer;
    }

    pub async fn reconfigure(&self, settings: MidiSettings) -> ActionOutcome {
        self.inner.reconfigure(settings).await
    }
    pub async fn input_snapshot(&self, refresh: bool) -> MidiInputsResponse {
        self.inner.clone().input_snapshot(refresh).await
    }
    pub async fn select_input(&self, input_id: Option<&str>) -> ActionOutcome {
        self.inner.select_input(input_id).await
    }
    pub async fn simulate_cue(&self, cue: stagepilot_core::api::MidiCueName) -> ActionOutcome {
        self.inner.clone().simulate_cue(cue).await
    }
    /// Newest first, like `reversed(deque)`.
    pub async fn recent_messages(&self) -> Vec<MidiMonitorMessage> {
        lock(&self.inner.state)
            .recent
            .iter()
            .rev()
            .cloned()
            .collect()
    }
    pub fn settings(&self) -> MidiSettings {
        lock(&self.inner.state).settings.clone()
    }
    pub fn note_name(note: u8) -> String {
        note_name(note)
    }
}

/// The system's MIDI input ports, listed read-only: nothing is opened or connected, nothing is
/// selected. For the setup panel while the MIDI plugin is off. An unavailable backend gives none.
pub fn list_system_inputs(factory: &MidiBackendFactory) -> Vec<MidiInputResponse> {
    let Ok(backend) = factory() else {
        return Vec::new();
    };
    let Ok(names) = backend.list_input_names() else {
        return Vec::new();
    };
    let mut unique: Vec<(String, usize)> = Vec::new();
    for name in names {
        match unique.iter_mut().find(|(n, _)| *n == name) {
            Some((_, count)) => *count += 1,
            None => unique.push((name, 1)),
        }
    }
    unique.sort_by_key(|(name, _)| name.to_lowercase());
    unique
        .into_iter()
        .map(|(name, count)| MidiInputResponse {
            id: input_id(&name),
            ambiguous: count > 1,
            selected: false,
            connected: false,
            name,
        })
        .collect()
}

#[async_trait]
impl Plugin for MidiPlaybackPlugin {
    fn name(&self) -> &str {
        "midi_playback"
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
        let mut st = lock(&self.inner.state);
        let finished = |h: &Option<JoinHandle<()>>| h.as_ref().is_some_and(JoinHandle::is_finished);
        if !st.stopping
            && !matches!(st.status, PluginStatus::Stopped | PluginStatus::Stopping)
            && (finished(&st.consumer) || finished(&st.supervisor))
        {
            st.status = PluginStatus::Error;
            st.last_error = Some("A MIDI background task stopped unexpectedly.".into());
        }
        Ok(PluginHealth {
            name: "midi_playback".into(),
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

    fn stopping(&self) -> bool {
        self.st().stopping
    }

    fn dispatcher(&self) -> Arc<dyn ActionDispatcher> {
        lock(&self.dispatcher).clone()
    }

    // ---- lifecycle ----------------------------------------------------------------------------

    async fn start(self: Arc<Self>) -> Result<(), String> {
        let (tx, rx) = {
            let mut st = self.st();
            if st.status != PluginStatus::Stopped {
                return Ok(());
            }
            st.status = PluginStatus::Starting;
            st.stopping = false;
            st.handle = Some(tokio::runtime::Handle::current());
            st.selected_input_name = st.settings.input_name.clone();
            st.recent.clear();
            let (tx, rx) = mpsc::channel(self.queue_capacity);
            st.tx = Some(tx.clone());
            (tx, rx)
        };
        let _ = tx;
        let factory = self.backend_factory.clone();
        let backend = tokio::task::spawn_blocking(move || factory())
            .await
            .map_err(|_| MidiError::internal("backend factory panicked"))
            .and_then(|r| r);
        let backend = match backend {
            Ok(backend) => backend,
            Err(_) => {
                self.cleanup_failed_start();
                return Err("The MIDI backend could not be initialized.".into());
            }
        };
        let consumer = tokio::spawn(self.clone().consume(rx));
        let supervisor = tokio::spawn(self.clone().supervise());
        let mut st = self.st();
        st.backend = Some(backend);
        st.consumer = Some(consumer);
        st.supervisor = Some(supervisor);
        Ok(())
    }

    fn cleanup_failed_start(&self) {
        let mut st = self.st();
        st.backend = None;
        st.tx = None;
        st.handle = None;
        st.status = PluginStatus::Error;
    }

    async fn stop(self: Arc<Self>) {
        let supervisor = {
            let mut st = self.st();
            if st.status == PluginStatus::Stopped && st.backend.is_none() {
                return;
            }
            st.status = PluginStatus::Stopping;
            st.stopping = true;
            st.active_connection_id = None;
            st.supervisor.take()
        };
        self.wake.notify_one();
        if let Some(supervisor) = supervisor {
            let _ = supervisor.await;
        }
        // Dropping the only sender lets the consumer drain what is left (each remaining message
        // is completed as "stopped") and exit.
        let consumer = {
            let mut st = self.st();
            st.tx = None;
            st.consumer.take()
        };
        if let Some(consumer) = consumer {
            let _ = consumer.await;
        }
        let side: Vec<_> = std::mem::take(&mut self.st().side_tasks);
        for task in side {
            let _ = task.await;
        }
        self.set_connection(
            ConnectionStatus::Disconnected,
            "MIDI Playback input stopped.",
            PluginStatus::Stopping,
        )
        .await;
        let mut st = self.st();
        st.backend = None;
        st.handle = None;
        st.held_notes.clear();
        st.last_triggered_at.clear();
        st.selected_input_name = st.settings.input_name.clone();
        st.connected_input_name = None;
        st.status = PluginStatus::Stopped;
        st.last_activity_at = Some(Utc::now());
    }

    // ---- controller surface -------------------------------------------------------------------

    async fn reconfigure(&self, settings: MidiSettings) -> ActionOutcome {
        {
            let st = self.st();
            if st.stopping || matches!(st.status, PluginStatus::Stopped | PluginStatus::Stopping) {
                return ActionOutcome::rejected(NOT_RUNNING);
            }
        }
        if !settings.enabled {
            return ActionOutcome::rejected("MIDI is turned off in the saved settings.");
        }
        let input_changed = {
            let mut st = self.st();
            st.settings = settings.clone();
            st.held_notes.clear();
            st.last_triggered_at.clear();
            settings.input_name != st.selected_input_name
        };
        if input_changed {
            // A different input was saved: connect to it live (the supervisor retries while the
            // device is absent and picks it up when it appears).
            let _guard = self.selection_lock.lock().await;
            {
                let mut st = self.st();
                st.selected_input_name = settings.input_name.clone();
                st.active_connection_id = None;
            }
            self.wake.notify_one();
            match &settings.input_name {
                None => {
                    self.set_connection(
                        ConnectionStatus::Disconnected,
                        "No MIDI input is selected; cue simulation remains available.",
                        PluginStatus::Starting,
                    )
                    .await
                }
                Some(name) => {
                    self.set_connection(
                        ConnectionStatus::Connecting,
                        &format!("Connecting to \"{name}\"."),
                        PluginStatus::Starting,
                    )
                    .await
                }
            }
        }
        tracing::info!(
            channel = settings.channel,
            note = settings.note,
            debounce_ms = settings.debounce_ms,
            "midi_settings_reconfigured"
        );
        ActionOutcome::ok(format!(
            "MIDI cue settings applied: channel {}, note {} ({}).",
            settings.channel,
            note_name(settings.note),
            settings.note
        ))
    }

    async fn input_snapshot(self: Arc<Self>, refresh: bool) -> MidiInputsResponse {
        let ready = {
            let st = self.st();
            st.backend.is_some() && st.tx.is_some()
        };
        if refresh && ready {
            if self.refresh_input_names().await.is_ok() {
                self.wake.notify_one();
            } else {
                self.set_connection(
                    ConnectionStatus::Error,
                    "Could not enumerate MIDI input ports.",
                    PluginStatus::Error,
                )
                .await;
            }
        }
        let st = self.st();
        // Counter(...) keeps first-appearance order; sorted() is stable on the casefold key.
        let mut unique: Vec<(String, usize)> = Vec::new();
        for name in &st.available_input_names {
            match unique.iter_mut().find(|(n, _)| n == name) {
                Some((_, count)) => *count += 1,
                None => unique.push((name.clone(), 1)),
            }
        }
        unique.sort_by_key(|(name, _)| name.to_lowercase());
        let mappings: BTreeMap<_, _> = st
            .settings
            .mappings
            .configured()
            .into_iter()
            .filter_map(|(name, velocity)| cue_from_str(name).map(|cue| (cue, velocity)))
            .collect();
        MidiInputsResponse {
            enabled: st.settings.enabled,
            channel: st.settings.channel,
            note: st.settings.note,
            configured_input_name: st.settings.input_name.clone(),
            selected_input_name: st.selected_input_name.clone(),
            inputs: unique
                .into_iter()
                .map(|(name, count)| MidiInputResponse {
                    id: input_id(&name),
                    ambiguous: count > 1,
                    selected: st.selected_input_name.as_deref() == Some(name.as_str()),
                    connected: st.connected_input_name.as_deref() == Some(name.as_str()),
                    name,
                })
                .collect(),
            mappings,
        }
    }

    async fn select_input(&self, requested: Option<&str>) -> ActionOutcome {
        {
            let st = self.st();
            if st.stopping || st.backend.is_none() || st.tx.is_none() {
                return ActionOutcome::rejected(NOT_RUNNING);
            }
        }
        let _guard = self.selection_lock.lock().await;
        let mut selected_name: Option<String> = None;
        if let Some(requested) = requested {
            let names = match self.refresh_input_names().await {
                Ok(names) => names,
                Err(_) => {
                    self.set_connection(
                        ConnectionStatus::Error,
                        "Could not enumerate MIDI input ports.",
                        PluginStatus::Error,
                    )
                    .await;
                    return ActionOutcome::rejected("MIDI inputs could not be refreshed.");
                }
            };
            let matching: HashSet<&String> =
                names.iter().filter(|n| input_id(n) == requested).collect();
            if matching.len() != 1 {
                return ActionOutcome::rejected("The selected MIDI input is no longer available.");
            }
            let name = (*matching.iter().next().unwrap_or(&&String::new())).clone();
            if names.iter().filter(|n| **n == name).count() != 1 {
                return ActionOutcome::rejected("The selected MIDI input name is ambiguous.");
            }
            selected_name = Some(name);
        }
        {
            let st = self.st();
            if selected_name == st.selected_input_name {
                return match selected_name {
                    None => ActionOutcome::ok("MIDI input is already disconnected."),
                    Some(name) => ActionOutcome::ok(format!("\"{name}\" is already selected.")),
                };
            }
        }
        {
            let mut st = self.st();
            st.selected_input_name = selected_name.clone();
            st.active_connection_id = None;
            st.held_notes.clear();
            st.last_triggered_at.clear();
        }
        self.wake.notify_one();
        match selected_name {
            None => {
                self.set_connection(
                    ConnectionStatus::Disconnected,
                    "No MIDI input is selected; cue simulation remains available.",
                    PluginStatus::Starting,
                )
                .await;
                ActionOutcome::ok("MIDI input disconnected for this session.")
            }
            Some(name) => {
                self.set_connection(
                    ConnectionStatus::Connecting,
                    &format!("Connecting to \"{name}\"."),
                    PluginStatus::Starting,
                )
                .await;
                ActionOutcome::ok(format!("Selected \"{name}\" for this session."))
            }
        }
    }

    async fn simulate_cue(
        self: Arc<Self>,
        cue: stagepilot_core::api::MidiCueName,
    ) -> ActionOutcome {
        let (velocity, channel, note) = {
            let st = self.st();
            if st.stopping || st.tx.is_none() || st.status == PluginStatus::Stopped {
                return ActionOutcome::rejected(NOT_RUNNING);
            }
            let name = crate::models::cue_name_str(cue);
            let velocity = st
                .settings
                .mappings
                .configured()
                .into_iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| v);
            (velocity, st.settings.channel, st.settings.note)
        };
        let Some(velocity) = velocity else {
            return ActionOutcome::rejected(format!(
                "The MIDI cue \"{}\" is not mapped.",
                crate::models::cue_name_str(cue)
            ));
        };
        let (completion, done) = oneshot::channel();
        let queued = Queued {
            message: MidiMessage {
                kind: NoteType::NoteOn,
                channel,
                note,
                velocity,
            },
            connection_id: None,
            simulated: true,
            release_after: true,
            completion: Some(completion),
            ownership_revision: None,
        };
        if !self.offer(queued) {
            return ActionOutcome::rejected(
                "The MIDI input queue is full; the simulated cue was dropped.",
            );
        }
        done.await.unwrap_or_else(|_| {
            ActionOutcome::rejected("MIDI input stopped before the cue was processed.")
        })
    }

    // ---- queue --------------------------------------------------------------------------------

    /// Called from the device thread for every normalised note message.
    fn receive_from_backend(self: &Arc<Self>, connection_id: Uuid, message: MidiMessage) {
        {
            let st = self.st();
            if st.handle.is_none() || st.stopping {
                return;
            }
        }
        self.offer(Queued {
            message,
            connection_id: Some(connection_id),
            simulated: false,
            release_after: false,
            completion: None,
            ownership_revision: None,
        });
    }

    fn offer(self: &Arc<Self>, mut queued: Queued) -> bool {
        let revision_hook = lock(&self.hooks).ownership_revision.clone();
        if let Some(hook) = revision_hook {
            queued.ownership_revision = Some(hook());
        }
        let mut st = self.st();
        if st.stopping || st.tx.is_none() {
            drop(st);
            complete(
                queued.completion.take(),
                ActionOutcome::rejected("The MIDI Playback plugin is stopping."),
            );
            return false;
        }
        if queued.connection_id.is_some() && queued.connection_id != st.active_connection_id {
            drop(st);
            complete(
                queued.completion.take(),
                ActionOutcome::rejected("A stale MIDI message was ignored."),
            );
            return false;
        }
        let Some(tx) = st.tx.clone() else {
            return false;
        };
        match tx.try_send(queued) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Closed(mut queued)) => {
                drop(st);
                complete(
                    queued.completion.take(),
                    ActionOutcome::rejected("The MIDI Playback plugin is stopping."),
                );
                false
            }
            Err(mpsc::error::TrySendError::Full(mut queued)) => {
                if !st.overflow_reported {
                    st.overflow_reported = true;
                    if let Some(handle) = st.handle.clone() {
                        let inner = self.clone();
                        let task = handle.spawn(async move {
                            tracing::error!("midi_input_queue_overflow");
                            inner
                                .set_connection(
                                    ConnectionStatus::Error,
                                    "The MIDI input queue overflowed and a cue was dropped.",
                                    PluginStatus::Error,
                                )
                                .await;
                        });
                        st.side_tasks.retain(|t| !t.is_finished());
                        st.side_tasks.push(task);
                    }
                }
                complete(
                    queued.completion.take(),
                    ActionOutcome::rejected("The MIDI input queue is full; the cue was dropped."),
                );
                Self::record(
                    &mut st,
                    &queued,
                    MidiMessageDisposition::QueueFull,
                    "The MIDI input queue was full; this message was dropped.",
                    None,
                );
                false
            }
        }
    }

    async fn consume(self: Arc<Self>, mut rx: mpsc::Receiver<Queued>) {
        while let Some(mut queued) = rx.recv().await {
            if self.stopping() {
                complete(
                    queued.completion.take(),
                    ActionOutcome::rejected("MIDI input stopped before the cue was processed."),
                );
                continue;
            }
            let outcome = match std::panic::AssertUnwindSafe(self.process_message(&queued))
                .catch_unwind()
                .await
            {
                Ok(outcome) => outcome,
                Err(_) => {
                    tracing::error!("midi_message_processing_failed");
                    let outcome = ActionOutcome::rejected("The MIDI cue could not be processed.");
                    let mut st = self.st();
                    Self::record(
                        &mut st,
                        &queued,
                        MidiMessageDisposition::Error,
                        &outcome.message,
                        None,
                    );
                    outcome
                }
            };
            if queued.release_after {
                let mut st = self.st();
                Self::release_held_note(&mut st, queued.message.channel, queued.message.note);
            }
            complete(queued.completion.take(), outcome);
        }
    }

    fn record(
        st: &mut State,
        queued: &Queued,
        disposition: MidiMessageDisposition,
        detail: &str,
        action: Option<ActionName>,
    ) {
        let message = queued.message;
        if st.recent.len() >= MIDI_MONITOR_CAPACITY {
            st.recent.pop_front();
        }
        st.recent.push_back(MidiMonitorMessage {
            timestamp: Utc::now(),
            input_name: if queued.simulated {
                Some("Cue simulation".into())
            } else {
                st.connected_input_name.clone()
            },
            message_type: message.kind,
            channel: message.channel,
            note: message.note,
            note_name: note_name(message.note),
            velocity: message.velocity,
            disposition,
            detail: detail.to_string(),
            action,
            simulated: queued.simulated,
        });
    }

    /// Release every velocity latch associated with one MIDI note.
    fn release_held_note(st: &mut State, channel: u8, note: u8) {
        st.held_notes
            .retain(|(c, n, _)| !(*c == channel && *n == note));
    }

    /// Synchronous part of the pipeline: release / channel / note / mapping filters. `Err` is the
    /// final outcome (already recorded in the monitor); `Ok` is what to publish and dispatch.
    fn classify(
        &self,
        queued: &Queued,
    ) -> Result<(ActionName, Option<u8>, MidiSettings), ActionOutcome> {
        let message = queued.message;
        let mut st = self.st();
        if message.kind == NoteType::NoteOff || message.velocity == 0 {
            Self::release_held_note(&mut st, message.channel, message.note);
            let outcome = ActionOutcome::rejected("MIDI note release received.");
            Self::record(
                &mut st,
                queued,
                MidiMessageDisposition::NoteRelease,
                &outcome.message,
                None,
            );
            return Err(outcome);
        }

        let settings = st.settings.clone();
        if message.channel != settings.channel {
            let outcome = ActionOutcome::rejected(format!(
                "Ignored: StagePilot listens on channel {}.",
                settings.channel
            ));
            Self::record(
                &mut st,
                queued,
                MidiMessageDisposition::WrongChannel,
                &outcome.message,
                None,
            );
            return Err(outcome);
        }
        if message.note != settings.note {
            let outcome = ActionOutcome::rejected(format!(
                "Ignored: StagePilot listens for MIDI note {} ({}).",
                note_name(settings.note),
                settings.note
            ));
            Self::record(
                &mut st,
                queued,
                MidiMessageDisposition::Unmapped,
                &outcome.message,
                None,
            );
            return Err(outcome);
        }

        let song_position = (MIN_SONG_POSITION_VELOCITY..=MAX_SONG_POSITION_VELOCITY)
            .contains(&message.velocity)
            .then_some(message.velocity);
        let cue = if song_position.is_some() {
            None
        } else {
            settings
                .mappings
                .configured()
                .into_iter()
                .find(|(_, v)| *v == message.velocity)
                .and_then(|(name, _)| cue_from_str(name))
        };
        let action = match (song_position, cue) {
            (Some(_), _) => ActionName::StartNext,
            (None, Some(cue)) => cue_action(cue),
            (None, None) => {
                let outcome = ActionOutcome::rejected(format!(
                    "Ignored: velocity {} is not mapped to a StagePilot cue.",
                    message.velocity
                ));
                Self::record(
                    &mut st,
                    queued,
                    MidiMessageDisposition::Unmapped,
                    &outcome.message,
                    None,
                );
                return Err(outcome);
            }
        };
        Ok((action, song_position, settings))
    }

    async fn process_message(&self, queued: &Queued) -> ActionOutcome {
        let message = queued.message;
        let hooks = {
            let h = lock(&self.hooks);
            (h.ownership_revision.clone(), h.action_guard.clone())
        };
        {
            let st = self.st();
            if queued.connection_id.is_some() && queued.connection_id != st.active_connection_id {
                return ActionOutcome::rejected("A stale MIDI message was ignored.");
            }
        }
        if message.kind != NoteType::NoteOff && message.velocity != 0 {
            if let Some(revision) = &hooks.0 {
                if queued.ownership_revision != Some(revision()) {
                    let detail = hooks
                        .1
                        .as_ref()
                        .and_then(|guard| guard())
                        .unwrap_or_else(|| "ignored: Playback source ownership changed".into());
                    let mut st = self.st();
                    Self::record(
                        &mut st,
                        queued,
                        MidiMessageDisposition::ActionRejected,
                        &detail,
                        None,
                    );
                    return ActionOutcome::rejected(detail);
                }
            }
        }

        let (action, song_position, settings) = match self.classify(queued) {
            Ok(decided) => decided,
            Err(outcome) => return outcome,
        };
        tracing::info!(
            channel = message.channel,
            note = message.note,
            velocity = message.velocity,
            action = action.as_str(),
            simulated = queued.simulated,
            "midi_cue_recognized"
        );
        self.bus
            .publish(new_event(
                EventType::MidiNoteReceived,
                if queued.simulated {
                    "midi.simulation"
                } else {
                    "midi_playback"
                },
                EventPayload::MidiNote {
                    channel: message.channel,
                    note: message.note,
                    velocity: message.velocity,
                    action,
                    song_position,
                    connection_id: queued.connection_id,
                    simulated: queued.simulated,
                },
            ))
            .await;

        let trigger_key = (message.channel, message.note, message.velocity);
        let now = (self.clock)();
        let debounce_seconds = f64::from(settings.debounce_ms) / 1000.0;
        {
            let mut st = self.st();
            let previous = st.last_triggered_at.get(&trigger_key).copied();
            if st.held_notes.contains(&trigger_key)
                || previous.is_some_and(|p| now - p < debounce_seconds)
            {
                tracing::info!(
                    channel = message.channel,
                    note = message.note,
                    velocity = message.velocity,
                    "midi_duplicate_ignored"
                );
                let outcome = ActionOutcome::rejected("Duplicate MIDI cue ignored.");
                Self::record(
                    &mut st,
                    queued,
                    MidiMessageDisposition::Duplicate,
                    &outcome.message,
                    Some(action),
                );
                return outcome;
            }
            st.held_notes.insert(trigger_key);
            st.last_triggered_at.insert(trigger_key, now);
        }
        if let Some(ignored) = hooks.1.as_ref().and_then(|guard| guard()) {
            let outcome = ActionOutcome::rejected(ignored.clone());
            let mut st = self.st();
            Self::record(
                &mut st,
                queued,
                MidiMessageDisposition::ActionRejected,
                &ignored,
                Some(action),
            );
            return outcome;
        }
        let source = if queued.simulated {
            "midi.simulation"
        } else {
            "midi_playback"
        };
        let dispatcher = self.dispatcher();
        let outcome = match song_position {
            Some(position) => {
                dispatcher
                    .dispatch_song_position(i64::from(position), source)
                    .await
            }
            None => dispatcher.dispatch(action, source).await,
        };
        let mut st = self.st();
        Self::record(
            &mut st,
            queued,
            if outcome.accepted {
                MidiMessageDisposition::Dispatched
            } else {
                MidiMessageDisposition::ActionRejected
            },
            &outcome.message,
            Some(action),
        );
        st.last_activity_at = Some(Utc::now());
        outcome
    }

    // ---- connection supervisor ----------------------------------------------------------------

    async fn wait_before_retry(&self, delay: Duration) {
        if self.stopping() {
            return;
        }
        let _ = tokio::time::timeout(delay, self.wake.notified()).await;
    }

    fn backend(&self) -> Result<Arc<dyn MidiBackend>, MidiError> {
        self.st()
            .backend
            .clone()
            .ok_or_else(|| MidiError::internal("MIDI backend is unavailable."))
    }

    async fn refresh_input_names(&self) -> Result<Vec<String>, MidiError> {
        let backend = self.backend()?;
        let names = tokio::task::spawn_blocking(move || backend.list_input_names())
            .await
            .map_err(|_| MidiError::internal("MIDI backend task failed."))??;
        self.st().available_input_names = names.clone();
        Ok(names)
    }

    async fn close_port(&self, port: Arc<dyn MidiInputPort>) {
        if tokio::task::spawn_blocking(move || port.close())
            .await
            .is_err()
        {
            tracing::warn!("midi_input_close_failed");
        }
    }

    async fn retry_wait(&self, retry_index: &mut usize) {
        self.wait_before_retry(self.reconnect_delays[*retry_index])
            .await;
        *retry_index = (*retry_index + 1).min(self.reconnect_delays.len() - 1);
    }

    async fn supervise(self: Arc<Self>) {
        let mut retry_index = 0usize;
        while !self.stopping() {
            let configured = self.st().selected_input_name.clone();
            let Some(configured) = configured else {
                self.set_connection(
                    ConnectionStatus::Disconnected,
                    "No MIDI input is selected; cue simulation remains available.",
                    PluginStatus::Starting,
                )
                .await;
                self.wait_before_retry(self.monitor_interval).await;
                retry_index = 0;
                continue;
            };

            if self.refresh_input_names().await.is_err() {
                self.set_connection(
                    ConnectionStatus::Error,
                    "Could not enumerate MIDI input ports.",
                    PluginStatus::Error,
                )
                .await;
                self.retry_wait(&mut retry_index).await;
                continue;
            }
            if Some(&configured) != self.st().selected_input_name.as_ref() {
                retry_index = 0;
                continue;
            }

            let matches = self
                .st()
                .available_input_names
                .iter()
                .filter(|n| **n == configured)
                .count();
            if matches == 0 {
                self.set_connection(
                    ConnectionStatus::Disconnected,
                    "The selected MIDI input is unavailable; retrying.",
                    PluginStatus::Starting,
                )
                .await;
                self.retry_wait(&mut retry_index).await;
                continue;
            }
            if matches > 1 {
                self.set_connection(
                    ConnectionStatus::Error,
                    "Multiple MIDI inputs have the selected name; select a unique input.",
                    PluginStatus::Error,
                )
                .await;
                self.retry_wait(&mut retry_index).await;
                continue;
            }

            self.set_connection(
                ConnectionStatus::Connecting,
                "Opening the selected MIDI input.",
                PluginStatus::Starting,
            )
            .await;
            let connection_id = Uuid::new_v4();
            let opened = match self.backend() {
                Ok(backend) => {
                    let weak: Weak<Inner> = Arc::downgrade(&self);
                    let callback: MidiMessageCallback = Box::new(move |message| {
                        if let Some(inner) = weak.upgrade() {
                            inner.receive_from_backend(connection_id, message);
                        }
                    });
                    let name = configured.clone();
                    tokio::task::spawn_blocking(move || backend.open_input(&name, callback))
                        .await
                        .map_err(|_| MidiError::internal("MIDI backend task failed."))
                        .and_then(|r| r)
                }
                Err(e) => Err(e),
            };
            let port = match opened {
                Ok(port) => port,
                Err(_) => {
                    self.set_connection(
                        ConnectionStatus::Error,
                        "Could not open the selected MIDI input; retrying.",
                        PluginStatus::Error,
                    )
                    .await;
                    self.retry_wait(&mut retry_index).await;
                    continue;
                }
            };

            let (stopping, changed) = {
                let st = self.st();
                (
                    st.stopping,
                    st.selected_input_name.as_ref() != Some(&configured),
                )
            };
            if stopping || changed {
                self.close_port(port).await;
                if stopping {
                    break;
                }
                retry_index = 0;
                continue;
            }
            {
                let mut st = self.st();
                st.port = Some(port.clone());
                st.active_connection_id = Some(connection_id);
                st.connected_input_name = Some(configured.clone());
                st.held_notes.clear();
                st.last_triggered_at.clear();
                st.overflow_reported = false;
            }
            retry_index = 0;
            self.set_connection(
                ConnectionStatus::Connected,
                "Playback MIDI input is connected.",
                PluginStatus::Running,
            )
            .await;

            let mut disconnect_detail = "The selected MIDI input disconnected; retrying.";
            while !self.stopping() {
                self.wait_before_retry(self.monitor_interval).await;
                if self.stopping() {
                    break;
                }
                let probe = port.clone();
                let closed = tokio::task::spawn_blocking(move || probe.closed()).await;
                let refreshed = self.refresh_input_names().await;
                let (Ok(closed), Ok(names)) = (closed, refreshed) else {
                    disconnect_detail = "The MIDI input failed during monitoring; retrying.";
                    break;
                };
                if Some(&configured) != self.st().selected_input_name.as_ref() {
                    disconnect_detail = "The MIDI input selection changed; reconnecting.";
                    break;
                }
                if closed || names.iter().filter(|n| **n == configured).count() != 1 {
                    break;
                }
                self.set_connection(
                    ConnectionStatus::Connected,
                    "Playback MIDI input is connected.",
                    PluginStatus::Running,
                )
                .await;
            }

            {
                let mut st = self.st();
                st.active_connection_id = None;
                st.connected_input_name = None;
                st.held_notes.clear();
            }
            self.close_port(port).await;
            self.st().port = None;
            if !self.stopping() {
                self.set_connection(
                    ConnectionStatus::Disconnected,
                    disconnect_detail,
                    PluginStatus::Starting,
                )
                .await;
            }
        }
        let final_port = {
            let mut st = self.st();
            st.active_connection_id = None;
            st.connected_input_name = None;
            st.port.take()
        };
        if let Some(port) = final_port {
            self.close_port(port).await;
        }
    }

    async fn set_connection(
        &self,
        status: ConnectionStatus,
        detail: &str,
        plugin_status: PluginStatus,
    ) {
        {
            let mut st = self.st();
            st.status = plugin_status;
            st.last_error = (plugin_status == PluginStatus::Error).then(|| detail.to_string());
            st.last_activity_at = Some(Utc::now());
            if st.connection_status == status && st.connection_detail.as_deref() == Some(detail) {
                return;
            }
            st.connection_status = status;
            st.connection_detail = Some(detail.to_string());
        }
        let observer = lock(&self.hooks).connection_observer.clone();
        if let Some(observer) = observer {
            observer(status).await;
            return;
        }
        self.bus
            .publish(new_event(
                EventType::ConnectionChanged,
                "midi_playback",
                EventPayload::Connection {
                    integration: Integration::Midi,
                    status,
                    detail: Some(detail.to_string()),
                },
            ))
            .await;
    }
}
