use crate::lightkey::LightkeyStatus;
use crate::lightkey_link::{LightkeyLink, LinkOptions};
use crate::models::{LightingOutputResponse, LightsStatusResponse};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use stagepilot_core::bus::{EventBus, Subscription};
use stagepilot_core::config::{
    validate_osc_address, LightingCue, LightingMethod, LightkeySettings, LightsSettings,
    OscArgument, OscLightingCue, SongLightingCueMap,
};
use stagepilot_core::events::{new_event, EventPayload, EventType, Integration, StagePilotEvent};
use stagepilot_core::plugin::Plugin;
use stagepilot_core::state::{ConnectionStatus, PluginHealth, PluginStatus, Song};
use stagepilot_midi::output::{MidiOutputBackend, MidiOutputBackendFactory, MidiOutputPort};
use stagepilot_midi::MidiError;
use stagepilot_osc::{OscBackend, OscOutput, OutputError, UdpOscBackend};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::{JoinHandle, JoinSet};
use uuid::Uuid;

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
    /// Take store, HTTP client and clock of the Lightkey link (real ones by default).
    pub lightkey: LinkOptions,
}

impl LightsOptions {
    pub fn new(midi_backend_factory: MidiOutputBackendFactory) -> Self {
        Self {
            midi_backend_factory,
            osc_backend_factory: Arc::new(|| Arc::new(UdpOscBackend)),
            monitor_interval: Duration::from_secs(2),
            lightkey: LinkOptions::default(),
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
}

struct Inner {
    bus: EventBus,
    midi_factory: MidiOutputBackendFactory,
    osc_factory: OscBackendFactory,
    monitor_interval: Duration,
    state: Mutex<State>,
    operation_lock: tokio::sync::Mutex<()>,
    stop: tokio::sync::Notify,
    /// Set once in `LightsPlugin::new` (the link needs a handle back to this struct).
    lightkey: std::sync::OnceLock<LightkeyLink>,
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
        let link_options = options.lightkey;
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
            }),
            operation_lock: tokio::sync::Mutex::new(()),
            stop: tokio::sync::Notify::new(),
            lightkey: std::sync::OnceLock::new(),
        });
        let weak = Arc::downgrade(&inner);
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
        let weak_command = weak.clone();
        let send: crate::lightkey_link::SendOsc = Arc::new(move |address: String| {
            let weak = weak.clone();
            Box::pin(async move {
                match weak.upgrade() {
                    Some(inner) => inner.send_raw_osc(address).await,
                    None => Ok(()),
                }
            })
        });
        let mut link_options = link_options;
        link_options.send_command =
            Some(Arc::new(move |address: String, args: Vec<OscArgument>| {
                let weak = weak_command.clone();
                Box::pin(async move {
                    match weak.upgrade() {
                        Some(inner) => inner.send_command_osc(address, args).await,
                        None => Ok(()),
                    }
                })
            }));
        let _ = inner.lightkey.set(LightkeyLink::new(
            bus_for_link,
            provider,
            send,
            link_options,
        ));
        Ok(Self { inner })
    }

    fn link(&self) -> &LightkeyLink {
        // Always set by `new`.
        self.inner.lightkey.get().expect("lightkey link")
    }

    pub async fn lightkey_status(&self) -> LightkeyStatus {
        self.link().status()
    }

    pub async fn lightkey_set_recording(&self, on: bool) -> String {
        self.link().set_recording(on).await
    }

    pub async fn lightkey_clear_take(&self, song_key: &str) -> bool {
        self.link().clear_take(song_key).await
    }

    /// Apply saved Lightkey settings to the running plugin and make the link re-read them.
    pub async fn lightkey_apply_settings(&self, lightkey: LightkeySettings) {
        lock(&self.inner.state).settings.lightkey = lightkey;
        self.link().settings_changed();
        self.link().refresh_replay().await;
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
        let midi_active = self.st().settings.method == LightingMethod::Midi;
        let names = if midi_active {
            match self.list_output_names().await {
                Ok(names) => names,
                Err(error) => {
                    self.record_midi_error(&error, "Lighting MIDI outputs could not be listed.")
                        .await;
                    Vec::new()
                }
            }
        } else {
            Vec::new()
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
        }
    }

    async fn reconfigure(self: Arc<Self>, settings: LightsSettings) -> LightingOutcome {
        self.cancel_timeline().await;
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
        let cue = OscLightingCue {
            id: Uuid::new_v4(),
            at_seconds: 0.0,
            address: address.clone(),
            args,
            label: "Test cue".into(),
            color: None,
        };
        if let Err(error) = self.send_osc(&cue).await {
            self.record_osc_error(&error, "OSC test cue failed.").await;
            return LightingOutcome::new(
                false,
                self.st()
                    .detail
                    .clone()
                    .unwrap_or_else(|| "OSC test cue failed.".into()),
            );
        }
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
        let active = self.active_cues(&song);
        if !self.st().settings.enabled || active.is_empty() {
            return;
        }
        if !self.output_ready() && !self.clone().connect().await {
            return;
        }
        let cues: Vec<_> = active
            .into_iter()
            .filter(|c| c.at_seconds() <= f64::from(payload.duration_seconds))
            .collect();
        if cues.is_empty() {
            return;
        }
        self.spawn_timeline(cues, payload.started_at, 0.0);
    }

    /// Resume/seek: restart the timeline at the song position, upcoming cues only.
    async fn on_timer_repositioned(self: &Arc<Self>, event: &StagePilotEvent) {
        let EventPayload::Timer(payload) = &event.payload else {
            return;
        };
        self.cancel_timeline().await;
        let Some(position) = payload.position_seconds else {
            return;
        };
        if !payload.running {
            return; // stopped: send nothing
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
        let total = position + f64::from(payload.duration_seconds);
        let cues: Vec<_> = active
            .into_iter()
            .filter(|c| position <= c.at_seconds() && c.at_seconds() <= total)
            .collect();
        if cues.is_empty() {
            return;
        }
        if !self.output_ready() && !self.clone().connect().await {
            return;
        }
        self.spawn_timeline(cues, payload.started_at, position);
    }

    async fn on_timer_stopped(self: &Arc<Self>, _event: &StagePilotEvent) {
        self.cancel_timeline().await;
    }

    fn spawn_timeline(
        self: &Arc<Self>,
        cues: Vec<AnyCue>,
        started_at: Option<DateTime<Utc>>,
        offset: f64,
    ) {
        let inner = self.clone();
        let task = tokio::spawn(async move { inner.run_timeline(cues, started_at, offset).await });
        self.st().timeline = Some(task);
    }

    async fn run_timeline(
        self: Arc<Self>,
        cues: Vec<AnyCue>,
        started_at: Option<DateTime<Utc>>,
        offset: f64,
    ) {
        let elapsed_before_confirmation = started_at
            .map(|s| (Utc::now() - s).num_milliseconds() as f64 / 1000.0)
            .map_or(0.0, |s| s.max(0.0));
        let mut set = JoinSet::new();
        for cue in cues {
            let inner = self.clone();
            let delay = (cue.at_seconds() - elapsed_before_confirmation - offset).max(0.0);
            set.spawn(async move {
                if delay > 0.0 {
                    tokio::time::sleep(Duration::from_secs_f64(delay)).await;
                }
                match cue {
                    AnyCue::Osc(cue) => inner
                        .send_osc(&cue)
                        .await
                        .map_err(|e| (e.message.clone(), e.public)),
                    AnyCue::Midi(cue) => inner
                        .clone()
                        .send_pulse(&cue)
                        .await
                        .map_err(|e| (e.message.clone(), e.public)),
                }
            });
        }
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok(Ok(())) => {}
                Ok(Err((message, public))) => {
                    set.abort_all();
                    self.record_error_text(if public {
                        message
                    } else {
                        "A lighting cue could not be sent.".into()
                    })
                    .await;
                    break;
                }
                Err(_) => {}
            }
        }
        // Dropping the set aborts anything still waiting; a pulse in flight sends its note off.
    }

    async fn cancel_timeline(&self) {
        let task = self.st().timeline.take();
        if let Some(task) = task {
            task.abort();
            let _ = task.await;
        }
    }

    // ---- sending ------------------------------------------------------------------------------

    /// One-shot OSC through the saved output, address only (used by Lightkey replay).
    async fn send_raw_osc(&self, address: String) -> Result<(), String> {
        self.send_command_osc(address, Vec::new()).await
    }

    /// One-shot OSC with arguments through the saved output (Lightkey fixture replay).
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

    async fn send_osc(&self, cue: &OscLightingCue) -> Result<(), OutputError> {
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
        let (address, args) = (cue.address.clone(), cue.args.clone());
        tokio::task::spawn_blocking(move || output.send_message(&address, &args))
            .await
            .map_err(|_| OutputError::internal("OSC send task failed."))??;
        let now = Utc::now();
        let mut st = self.st();
        st.last_osc_cue = Some(cue.clone());
        st.last_cue_at = Some(now);
        st.last_activity_at = Some(now);
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
        tokio::task::spawn_blocking(move || on_port.send_note_on(channel, note, velocity))
            .await
            .map_err(|_| MidiError::internal("MIDI send task failed."))??;
        // The note off is sent even if this future is aborted mid-pulse (timeline cancelled).
        let guard = NoteOffGuard {
            port: Some(port),
            channel,
            note: cue.note,
        };
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
                // UDP has no handshake: "ready" means the socket opened, not that anything listens.
                self.set_connection(
                    ConnectionStatus::Connected,
                    &format!(
                        "OSC ready for {target}. Nothing sent yet; UDP does not confirm delivery."
                    ),
                )
                .await;
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
