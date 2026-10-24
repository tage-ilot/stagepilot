//! Event-driven ProPresenter countdown output plugin (`plugin.py`): translate song lifecycle
//! events into one reusable ProPresenter countdown.

use crate::client::{HttpProPresenterClient, ProPresenterClient};
use crate::errors::PpError;
use crate::models::{Look, Timer};
use crate::snapshot::{
    ProPresenterLookResponse, ProPresenterStatusResponse, ProPresenterTimerResponse,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use stagepilot_core::bus::{EventBus, Subscription};
use stagepilot_core::config::ProPresenterSettings;
use stagepilot_core::events::{
    new_event, EventPayload, EventType, Integration, StagePilotEvent, TimerPayload,
};
use stagepilot_core::plugin::Plugin;
use stagepilot_core::state::{ConnectionStatus, PluginHealth, PluginStatus};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

pub const LOOK_UPDATE_VERIFICATION_ATTEMPTS: u32 = 20;
pub const LOOK_UPDATE_VERIFICATION_INTERVAL: Duration = Duration::from_millis(100);

pub type ClientFactory =
    Arc<dyn Fn(&ProPresenterSettings) -> Arc<dyn ProPresenterClient> + Send + Sync>;

/// The factory that talks to a real ProPresenter over HTTP.
pub fn http_client_factory() -> ClientFactory {
    Arc::new(|settings| {
        Arc::new(HttpProPresenterClient::new(settings)) as Arc<dyn ProPresenterClient>
    })
}

struct State {
    settings: ProPresenterSettings,
    client: Option<Arc<dyn ProPresenterClient>>,
    timer: Option<Timer>,
    timers: Vec<Timer>,
    looks: Vec<Look>,
    current_look: Option<Look>,
    subscriptions: Vec<Subscription>,
    status: PluginStatus,
    connection_status: ConnectionStatus,
    connection_detail: Option<String>,
    last_error: Option<String>,
    last_activity_at: Option<DateTime<Utc>>,
    last_checked_at: Option<DateTime<Utc>>,
    supervisor: Option<JoinHandle<()>>,
    stopping: bool,
}

struct Inner {
    bus: EventBus,
    factory: ClientFactory,
    state: Mutex<State>,
    operation_lock: tokio::sync::Mutex<()>,
    probe_event: Notify,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Python `_public_error`: ProPresenter, value and runtime errors reach the operator.
fn public_error(error: &PpError) -> String {
    error.message().to_string()
}

fn casefold(value: &str) -> String {
    value.to_lowercase()
}

#[derive(Clone)]
pub struct ProPresenterPlugin {
    inner: Arc<Inner>,
}

impl ProPresenterPlugin {
    pub fn new(bus: EventBus, settings: ProPresenterSettings, factory: ClientFactory) -> Self {
        Self {
            inner: Arc::new(Inner {
                bus,
                factory,
                state: Mutex::new(State {
                    settings,
                    client: None,
                    timer: None,
                    timers: Vec::new(),
                    looks: Vec::new(),
                    current_look: None,
                    subscriptions: Vec::new(),
                    status: PluginStatus::Stopped,
                    connection_status: ConnectionStatus::Disconnected,
                    connection_detail: None,
                    last_error: None,
                    last_activity_at: None,
                    last_checked_at: None,
                    supervisor: None,
                    stopping: false,
                }),
                operation_lock: tokio::sync::Mutex::new(()),
                probe_event: Notify::new(),
            }),
        }
    }

    pub async fn snapshot(&self, refresh: bool) -> ProPresenterStatusResponse {
        if refresh {
            self.inner.probe(false).await.ok();
        }
        self.inner.snapshot()
    }

    pub async fn test_connection(&self) -> ProPresenterStatusResponse {
        self.inner.probe(false).await.ok();
        self.inner.snapshot()
    }

    pub async fn refresh_timers(&self) -> ProPresenterStatusResponse {
        self.inner.probe(false).await.ok();
        self.inner.snapshot()
    }

    /// Apply validated settings for this backend session and reconnect immediately.
    pub async fn reconfigure(&self, settings: ProPresenterSettings) -> ProPresenterStatusResponse {
        {
            let _guard = self.inner.operation_lock.lock().await;
            self.inner.close_client_locked().await;
            let client = (self.inner.factory)(&settings);
            let base = format!("http://{}:{}", settings.host, settings.port);
            {
                let mut st = self.inner.st();
                st.settings = settings;
                st.client = Some(client);
                st.timer = None;
                st.timers.clear();
                st.looks.clear();
                st.current_look = None;
                st.last_error = None;
            }
            self.inner
                .set_connection(
                    ConnectionStatus::Connecting,
                    &format!("Connecting to ProPresenter at {base}."),
                )
                .await;
            self.inner.probe_locked(false).await.ok();
        }
        self.inner.probe_event.notify_one();
        self.inner.snapshot()
    }

    /// Trigger and verify one saved Look after an explicit settings save.
    pub async fn apply_look(&self, look_id: &str) -> Result<ProPresenterStatusResponse, PpError> {
        let _guard = self.inner.operation_lock.lock().await;
        let known = self.inner.st().looks.iter().any(|l| l.id.uuid == look_id);
        if !known {
            self.inner.probe_locked(true).await?;
        }
        let selected = self
            .inner
            .st()
            .looks
            .iter()
            .find(|l| l.id.uuid == look_id)
            .cloned()
            .ok_or_else(|| {
                PpError::LookNotFound(format!("ProPresenter Look \"{look_id}\" was not found."))
            })?;
        let client = self.inner.require_client()?;
        client.trigger_look(look_id).await?;
        let mut confirmed = None;
        for attempt in 0..LOOK_UPDATE_VERIFICATION_ATTEMPTS {
            let candidate = client.current_look().await?;
            // /v1/look/current reports a new uuid after every trigger, so the uuid alone cannot
            // confirm; the index must match, and then the name (or, when it does, the uuid).
            let same_name = casefold(candidate.id.name.trim()) == casefold(selected.id.name.trim());
            let same_uuid = candidate.id.uuid == selected.id.uuid;
            if candidate.id.index == selected.id.index && (same_name || same_uuid) {
                confirmed = Some(candidate);
                break;
            }
            if attempt + 1 < LOOK_UPDATE_VERIFICATION_ATTEMPTS {
                tokio::time::sleep(LOOK_UPDATE_VERIFICATION_INTERVAL).await;
            }
        }
        let Some(current) = confirmed else {
            return Err(PpError::LookNotFound(
                "ProPresenter did not confirm the selected Look as current.".into(),
            ));
        };
        {
            let mut st = self.inner.st();
            st.current_look = Some(current);
            st.last_activity_at = Some(Utc::now());
        }
        Ok(self.inner.snapshot())
    }

    /// Forget a saved Look that ProPresenter does not know, in this session's settings.
    pub fn set_look_id(&self, look_id: Option<String>) {
        self.inner.st().settings.look_id = look_id;
    }

    pub fn settings(&self) -> ProPresenterSettings {
        self.inner.st().settings.clone()
    }
}

#[async_trait]
impl Plugin for ProPresenterPlugin {
    fn name(&self) -> &str {
        "propresenter"
    }
    fn version(&self) -> &str {
        "0.2.0"
    }

    async fn start(&self) -> Result<(), String> {
        self.inner.clone().start().await;
        Ok(())
    }
    async fn stop(&self) -> Result<(), String> {
        self.inner.clone().stop().await;
        Ok(())
    }
    async fn health(&self) -> Result<PluginHealth, String> {
        let st = self.inner.st();
        Ok(PluginHealth {
            name: "propresenter".into(),
            version: "0.2.0".into(),
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

    fn base_url(settings: &ProPresenterSettings) -> String {
        format!("http://{}:{}", settings.host, settings.port)
    }

    async fn start(self: Arc<Self>) {
        let settings = {
            let mut st = self.st();
            st.status = PluginStatus::Starting;
            st.stopping = false;
            st.settings.clone()
        };
        self.set_connection(
            ConnectionStatus::Connecting,
            &format!(
                "Connecting to ProPresenter at {}.",
                Self::base_url(&settings)
            ),
        )
        .await;
        self.st().client = Some((self.factory)(&settings));
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
        sub!(EventType::TimerStopRequested, on_timer_stop_requested);
        sub!(EventType::TimerResetRequested, on_timer_reset_requested);
        sub!(EventType::TimerResumed, on_timer_repositioned);
        sub!(EventType::TimerSeeked, on_timer_repositioned);
        self.st().subscriptions.extend(subs);
        self.probe(false).await.ok();
        let task = tokio::spawn(self.clone().connection_supervisor());
        self.st().supervisor = Some(task);
    }

    async fn stop(self: Arc<Self>) {
        let task = {
            let mut st = self.st();
            st.status = PluginStatus::Stopping;
            st.stopping = true;
            st.supervisor.take()
        };
        self.probe_event.notify_one();
        if let Some(task) = task {
            task.abort();
            let _ = task.await;
        }
        let subs: Vec<_> = std::mem::take(&mut self.st().subscriptions);
        for sub in subs {
            self.bus.unsubscribe(&sub);
        }
        {
            let _guard = self.operation_lock.lock().await;
            self.close_client_locked().await;
            let mut st = self.st();
            st.timer = None;
            st.timers.clear();
            st.looks.clear();
            st.current_look = None;
        }
        {
            let mut st = self.st();
            st.status = PluginStatus::Stopped;
            st.last_activity_at = Some(Utc::now());
        }
        self.set_connection(
            ConnectionStatus::Disconnected,
            "ProPresenter integration stopped.",
        )
        .await;
    }

    // ---- events -------------------------------------------------------------------------------

    async fn on_song_event(self: &Arc<Self>, event: &StagePilotEvent) {
        let EventPayload::Song { song, .. } = &event.payload else {
            return;
        };
        let duration = song.duration_seconds.unwrap_or(0);
        if duration == 0 {
            self.publish_failure(
                &format!(
                    "Cannot start countdown for \"{}\": the song has no positive duration.",
                    song.title
                ),
                0,
            )
            .await;
            return;
        }
        {
            let _guard = self.operation_lock.lock().await;
            match self.configure_and_start_locked(duration).await {
                Ok(started_at) => {
                    drop(_guard);
                    let name = self.st().settings.timer_name.clone();
                    self.record_success(&format!("Countdown timer \"{name}\" is running."))
                        .await;
                    self.bus
                        .publish(new_event(
                            EventType::TimerStarted,
                            "propresenter",
                            EventPayload::Timer(TimerPayload {
                                duration_seconds: duration,
                                song: Some(song.clone()),
                                started_at: Some(started_at),
                                ..Default::default()
                            }),
                        ))
                        .await;
                }
                Err(error) => {
                    let detail = public_error(&error);
                    self.record_operation_error(&detail).await;
                    self.publish_failure(&detail, duration).await;
                }
            }
        }
    }

    /// Apply the remaining seconds after a resume or seek. Running: set the countdown to the
    /// remaining seconds and start it. Stopped: update the held value without running. Never raises.
    async fn on_timer_repositioned(self: &Arc<Self>, event: &StagePilotEvent) {
        let EventPayload::Timer(payload) = &event.payload else {
            return;
        };
        let remaining = payload.remaining_seconds.unwrap_or(0);
        let run = payload.running && remaining > 0;
        let outcome = {
            let _guard = self.operation_lock.lock().await;
            match self.reposition_locked(remaining, run).await {
                Ok(()) => Ok(()),
                Err(error) if error.is_propresenter_error() => {
                    let _ = error;
                    self.st().timer = None;
                    match self.probe_locked(true).await {
                        Ok(_) => self.reposition_locked(remaining, run).await,
                        Err(error) => Err(error),
                    }
                }
                Err(error) => Err(error),
            }
        };
        match outcome {
            Err(error) => {
                let detail = public_error(&error);
                tracing::warn!(detail = %detail, "propresenter_reposition_failed");
                self.record_operation_error(&detail).await;
            }
            Ok(()) => {
                let name = self.st().settings.timer_name.clone();
                let state = if run { "running" } else { "held" };
                self.record_success(&format!(
                    "Countdown timer \"{name}\" is {state} at {remaining}s."
                ))
                .await;
            }
        }
    }

    async fn reposition_locked(&self, remaining: u32, run: bool) -> Result<(), PpError> {
        let client = self.require_client()?;
        let timer = self.require_timer_locked().await?;
        client.stop_timer(&timer.id.uuid).await?;
        let timer = client
            .set_timer_duration(&timer, i64::from(remaining))
            .await?;
        if run {
            client.start_timer(&timer.id.uuid).await?;
        }
        self.st().timer = Some(timer);
        Ok(())
    }

    async fn on_timer_stop_requested(self: &Arc<Self>, _event: &StagePilotEvent) {
        let result = {
            let _guard = self.operation_lock.lock().await;
            self.stop_timer_locked().await
        };
        match result {
            Err(error) => {
                let detail = public_error(&error);
                self.record_operation_error(&detail).await;
                self.publish_failure(&detail, 0).await;
            }
            Ok(()) => {
                let name = self.st().settings.timer_name.clone();
                self.record_success(&format!("Countdown timer \"{name}\" is stopped."))
                    .await;
                self.publish_stopped().await;
            }
        }
    }

    async fn on_timer_reset_requested(self: &Arc<Self>, _event: &StagePilotEvent) {
        let result = {
            let _guard = self.operation_lock.lock().await;
            self.reset_timer_to_zero_locked().await
        };
        match result {
            Err(error) => {
                let detail = public_error(&error);
                self.record_operation_error(&detail).await;
                self.publish_failure(&detail, 0).await;
            }
            Ok(()) => {
                let name = self.st().settings.timer_name.clone();
                self.record_success(&format!(
                    "Countdown timer \"{name}\" is stopped and reset to zero."
                ))
                .await;
                self.publish_stopped().await;
            }
        }
    }

    async fn publish_stopped(&self) {
        self.bus
            .publish(new_event(
                EventType::TimerStopped,
                "propresenter",
                EventPayload::Timer(TimerPayload::default()),
            ))
            .await;
    }

    async fn stop_timer_locked(&self) -> Result<(), PpError> {
        let first = async {
            let timer = self.require_timer_locked().await?;
            self.require_client()?.stop_timer(&timer.id.uuid).await
        }
        .await;
        match first {
            Err(error) if error.is_propresenter_error() => {
                // Recover from a stale timer UUID after ProPresenter or the timer restarts.
                self.st().timer = None;
                self.probe_locked(true).await?;
                let timer = self.require_timer_locked().await?;
                self.require_client()?.stop_timer(&timer.id.uuid).await
            }
            other => other,
        }
    }

    async fn configure_and_start_locked(&self, duration: u32) -> Result<DateTime<Utc>, PpError> {
        match self.timer_sequence_locked(duration).await {
            Err(error) if error.is_propresenter_error() => {
                // ProPresenter restarts and timer recreation can invalidate cached UUIDs.
                self.st().timer = None;
                self.probe_locked(true).await?;
                self.timer_sequence_locked(duration).await
            }
            other => other,
        }
    }

    async fn timer_sequence_locked(&self, duration: u32) -> Result<DateTime<Utc>, PpError> {
        let client = self.require_client()?;
        let timer = self.require_timer_locked().await?;
        client.stop_timer(&timer.id.uuid).await?;
        let timer = client
            .set_timer_duration(&timer, i64::from(duration))
            .await?;
        let started_at = Utc::now();
        client.start_timer(&timer.id.uuid).await?;
        self.st().timer = Some(timer);
        Ok(started_at)
    }

    async fn reset_timer_to_zero_locked(&self) -> Result<(), PpError> {
        match self.timer_reset_sequence_locked().await {
            Err(error) if error.is_propresenter_error() => {
                self.st().timer = None;
                self.probe_locked(true).await?;
                self.timer_reset_sequence_locked().await
            }
            other => other,
        }
    }

    async fn timer_reset_sequence_locked(&self) -> Result<(), PpError> {
        let client = self.require_client()?;
        let timer = self.require_timer_locked().await?;
        client.stop_timer(&timer.id.uuid).await?;
        let timer = client.set_timer_duration(&timer, 0).await?;
        client.reset_timer(&timer.id.uuid).await?;
        self.st().timer = Some(timer);
        Ok(())
    }

    async fn require_timer_locked(&self) -> Result<Timer, PpError> {
        if self.st().timer.is_none() {
            self.probe_locked(true).await?;
        }
        let timer = self.st().timer.clone();
        timer.ok_or_else(|| {
            let name = self.st().settings.timer_name.clone();
            PpError::TimerNotFound(format!("ProPresenter timer \"{name}\" was not found."))
        })
    }

    // ---- probing ------------------------------------------------------------------------------

    async fn probe(&self, raise: bool) -> Result<bool, PpError> {
        let _guard = self.operation_lock.lock().await;
        self.probe_locked(raise).await
    }

    /// `Ok(true)` connected with the timer found; `Ok(false)` otherwise unless `raise`, in which
    /// case the failure is returned.
    async fn probe_locked(&self, raise: bool) -> Result<bool, PpError> {
        self.st().last_checked_at = Some(Utc::now());
        let fetched = async {
            let client = self.require_client()?;
            let mut timers = client.list_timers().await?;
            // One cheap extra call for the live state; a failure here must not fail the probe.
            if let Ok(live) = client.current_timers().await {
                for timer in &mut timers {
                    let state = live
                        .iter()
                        .find(|l| l.id.uuid == timer.id.uuid)
                        .and_then(|l| l.state.clone());
                    if state.is_some() {
                        timer.state = state;
                    }
                }
            }
            let looks = client.list_looks().await?;
            let current = client.current_look().await?;
            Ok::<_, PpError>((timers, looks, current))
        }
        .await;
        let (timers, looks, current_look) = match fetched {
            Ok(fetched) => fetched,
            Err(error) => {
                let detail = public_error(&error);
                {
                    let mut st = self.st();
                    st.timer = None;
                    st.timers.clear();
                    st.looks.clear();
                    st.current_look = None;
                    st.status = PluginStatus::Starting;
                    st.last_error = Some(detail.clone());
                }
                let status = if error.is_connection() {
                    ConnectionStatus::Disconnected
                } else {
                    ConnectionStatus::Error
                };
                self.set_connection(status, &detail).await;
                return if raise { Err(error) } else { Ok(false) };
            }
        };
        let wanted = casefold(&self.st().settings.timer_name);
        let matches: Vec<Timer> = timers
            .iter()
            .filter(|t| casefold(t.id.name.trim()) == wanted)
            .cloned()
            .collect();
        {
            let mut st = self.st();
            st.timers = timers;
            st.looks = looks;
            st.current_look = Some(current_look);
        }
        let timer_name = self.st().settings.timer_name.clone();
        let timer_error = match matches.len() {
            0 => Some(PpError::TimerNotFound(format!(
                "API connected, but timer \"{timer_name}\" was not found."
            ))),
            1 => None,
            _ => Some(PpError::TimerNotFound(format!(
                "API connected, but multiple timers are named \"{timer_name}\"."
            ))),
        };
        if let Some(error) = timer_error {
            {
                let mut st = self.st();
                st.timer = None;
                st.status = PluginStatus::Error;
                st.last_error = Some(error.message().to_string());
            }
            self.set_connection(ConnectionStatus::Connected, error.message())
                .await;
            return if raise { Err(error) } else { Ok(false) };
        }
        let timer = matches.into_iter().next();
        let name = timer
            .as_ref()
            .map(|t| t.id.name.clone())
            .unwrap_or_default();
        {
            let mut st = self.st();
            st.timer = timer;
            st.status = PluginStatus::Running;
            st.last_error = None;
            st.last_activity_at = Some(Utc::now());
        }
        self.set_connection(
            ConnectionStatus::Connected,
            &format!(
                "Connected; timer \"{name}\" is ready and will be configured as a countdown when cued."
            ),
        )
        .await;
        Ok(true)
    }

    async fn connection_supervisor(self: Arc<Self>) {
        let mut delay = Duration::from_secs_f64(self.st().settings.reconnect_initial_seconds);
        loop {
            if self.st().stopping {
                return;
            }
            let wait = {
                let st = self.st();
                if st.timer.is_some() {
                    Duration::from_secs_f64(st.settings.health_check_interval_seconds)
                } else {
                    delay
                }
            };
            let _ = tokio::time::timeout(wait, self.probe_event.notified()).await;
            if self.st().stopping {
                return;
            }
            let connected = self.probe(false).await.unwrap_or(false);
            let (initial, max) = {
                let st = self.st();
                (
                    Duration::from_secs_f64(st.settings.reconnect_initial_seconds),
                    Duration::from_secs_f64(st.settings.reconnect_max_seconds),
                )
            };
            delay = if connected {
                initial
            } else {
                (delay * 2).min(max)
            };
        }
    }

    // ---- snapshot -----------------------------------------------------------------------------

    fn snapshot(&self) -> ProPresenterStatusResponse {
        let st = self.st();
        let s = &st.settings;
        let current_saved = st.current_look.as_ref().and_then(|current| {
            st.looks
                .iter()
                .find(|l| l.id.index == current.id.index)
                .map(|l| l.id.uuid.clone())
        });
        ProPresenterStatusResponse {
            enabled: s.enabled,
            host: s.host.clone(),
            port: s.port,
            timer_name: s.timer_name.clone(),
            look_id: s.look_id.clone(),
            request_timeout_seconds: s.request_timeout_seconds,
            connection_status: st.connection_status,
            detail: st.connection_detail.clone(),
            timers: st
                .timers
                .iter()
                .map(|t| ProPresenterTimerResponse {
                    id: t.id.uuid.clone(),
                    name: t.id.name.clone(),
                    index: t.id.index,
                    is_countdown: t.countdown.is_some(),
                    state: t.state.clone(),
                })
                .collect(),
            selected_timer_id: st.timer.as_ref().map(|t| t.id.uuid.clone()),
            timer_found: st.timer.is_some(),
            looks: st
                .looks
                .iter()
                .map(|l| ProPresenterLookResponse {
                    id: l.id.uuid.clone(),
                    name: l.id.name.clone(),
                    index: l.id.index,
                })
                .collect(),
            current_look_id: current_saved,
            look_found: s.look_id.is_none()
                || st
                    .looks
                    .iter()
                    .any(|l| Some(&l.id.uuid) == s.look_id.as_ref()),
            last_checked_at: st.last_checked_at,
        }
    }

    fn require_client(&self) -> Result<Arc<dyn ProPresenterClient>, PpError> {
        self.st()
            .client
            .clone()
            .ok_or_else(|| PpError::Value("The ProPresenter client is not running.".into()))
    }

    async fn close_client_locked(&self) {
        let client = self.st().client.take();
        if let Some(client) = client {
            client.close().await;
        }
    }

    async fn set_connection(&self, status: ConnectionStatus, detail: &str) {
        {
            let mut st = self.st();
            st.connection_status = status;
            st.connection_detail = Some(detail.to_string());
            st.last_checked_at = Some(Utc::now());
        }
        self.bus
            .publish(new_event(
                EventType::ConnectionChanged,
                "propresenter",
                EventPayload::Connection {
                    integration: Integration::Propresenter,
                    status,
                    detail: Some(detail.to_string()),
                },
            ))
            .await;
    }

    async fn record_success(&self, detail: &str) {
        {
            let mut st = self.st();
            st.last_error = None;
            st.last_activity_at = Some(Utc::now());
            st.status = PluginStatus::Running;
        }
        self.set_connection(ConnectionStatus::Connected, detail)
            .await;
    }

    async fn record_operation_error(&self, detail: &str) {
        {
            let mut st = self.st();
            st.status = PluginStatus::Error;
            st.last_error = Some(detail.to_string());
            st.last_activity_at = Some(Utc::now());
            st.timer = None;
        }
        tracing::error!(detail = %detail, "propresenter_operation_failed");
        self.set_connection(ConnectionStatus::Error, detail).await;
        self.probe_event.notify_one();
    }

    async fn publish_failure(&self, message: &str, duration_seconds: u32) {
        self.bus
            .publish(new_event(
                EventType::TimerFailed,
                "propresenter",
                EventPayload::Timer(TimerPayload {
                    duration_seconds,
                    message: Some(message.to_string()),
                    ..Default::default()
                }),
            ))
            .await;
    }
}
