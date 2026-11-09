//! The application runtime: the Rust counterpart of `core/runtime.py` plus the parts of
//! `main.create_app` that this phase owns (event bus, state store/service, plugin manager,
//! settings service, startup and shutdown order).

use crate::demo::DemoPlugin;
use crate::outputs::{register, OutputBackends, Outputs};
use crate::remote_feature::RemoteFeature;
use crate::remote_types::{RemoteAccess, RemoteManager, RemoteWiring};
use chrono::{DateTime, Utc};
use stagepilot_core::auth::DashboardSessionStore;
use stagepilot_core::bus::EventBus;
use stagepilot_core::config::{ConnectionMethod, ServiceSource, TimerOutput};
use stagepilot_core::events::{empty_event, EventType};
use stagepilot_core::plugin::Plugin;
use stagepilot_core::plugin::PluginManager;
use stagepilot_core::settings::{RuntimeSettings, SettingsService};
use stagepilot_core::state::ApplicationStatus;
use stagepilot_core::state_service::StateService;
use stagepilot_core::store::StateStore;
use stagepilot_pco::lengths::LengthService;
use stagepilot_pco::oauth::{CompleteError, OAUTH_STARTUP_REFRESH_TIMEOUT_SECONDS};
use stagepilot_pco::runtime::{PcoOptions, PcoRuntime};
use stagepilot_playback::plugin::{PlaybackPlugin, PluginOptions};
use std::sync::{Arc, Mutex, RwLock};

pub struct Runtime {
    pub settings: RuntimeSettings,
    pub bus: EventBus,
    pub store: StateStore,
    pub state_service: StateService,
    pub plugins: PluginManager,
    pub settings_service: Arc<SettingsService>,
    pub playback: Arc<PlaybackPlugin>,
    /// Planning Center: OAuth, setup service, client factory, the live plugin.
    pub pco: Arc<PcoRuntime>,
    oauth_stop: tokio::sync::watch::Sender<bool>,
    /// The lifespan-owned startup activation task (`stagepilot-startup-activation`).
    activation: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// The Planning Center song-length writer: preview, confirm and restore (B5).
    pub lengths: Arc<LengthService>,
    /// MIDI input, lights and ProPresenter.
    pub outputs: Arc<Outputs>,
    /// `PluginManager.reconcile_lock`: serialises live connection changes made by routes.
    pub reconcile: tokio::sync::Mutex<()>,
    pub sessions: DashboardSessionStore,
    /// Lockout for wrong dashboard PINs (m13).
    pub login_throttle: crate::security::LoginThrottle,
    pub dashboard_auth_enforced: bool,
    pub started_at: DateTime<Utc>,
    /// `app.state.remote_access` / `remote_feature` / `remote_manager`: all optional, settable
    /// after construction exactly as the Python tests and the launcher do.
    remote: RwLock<RemoteWiring>,
    /// The Services source the running plugins were built for (changing it needs a restart here).
    live_service_source: ServiceSource,
    startup_activation: std::sync::atomic::AtomicBool,
    /// A keychain read or write has not succeeded yet (waiting on a system dialog, or refused).
    /// The watcher below tries again and re-applies the credentials when the keychain answers.
    keychain_degraded: std::sync::atomic::AtomicBool,
    keychain_interval_ms: std::sync::atomic::AtomicU64,
    keychain_watch: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// The one-time "Set up the Lighting computer" session (token, expiry, optional listener).
    pub helper_install: Arc<crate::helper_install::HelperInstall>,
}

/// How often a keychain that has not answered is asked again.
pub const KEYCHAIN_RETRY_SECONDS: u64 = 30;

/// The length writer, wired to the live Planning Center settings and to the Playback plugin
/// (Python `PlaybackInputPlugin.__init__`: `is_playing`, `plan_current`, `on_applied`).
fn length_service(
    settings: &Arc<SettingsService>,
    store: &StateStore,
    state_service: &StateService,
    pco: &Arc<PcoRuntime>,
    playback: &Arc<PlaybackPlugin>,
) -> LengthService {
    let effective = {
        let pco = pco.clone();
        Arc::new(move || pco.effective())
    };
    let mut service = LengthService::new(
        settings.clone(),
        store.clone(),
        state_service.clone(),
        pco.factory.clone(),
        effective,
    );
    let is_playing: Arc<dyn Fn() -> bool + Send + Sync> = {
        let playback = playback.clone();
        Arc::new(move || playback.status().heartbeat.is_some_and(|h| h.playing))
    };
    service.is_playing = is_playing.clone();
    service.set_write_allowed(Arc::new(move || !is_playing()));
    let current = playback.clone();
    service.plan_current = Arc::new(move || {
        let current = current.clone();
        Box::pin(async move { current.scan_plan_is_current() })
    });
    let applied = playback.clone();
    service.on_applied = Arc::new(move |plan_id| {
        // Never turn a finished write into an error: the times are in Planning Center already.
        if let Err(error) = applied.mark_lengths_applied(plan_id) {
            tracing::warn!(%error, "planning_center_lengths_applied_not_saved");
        }
    });
    service
}

impl Runtime {
    /// Build the runtime. `settings_service` must already be loaded (`load()` called) so
    /// `effective_runtime_settings()` is the resolved configuration.
    pub fn new(settings_service: Arc<SettingsService>, dashboard_auth_enforced: bool) -> Arc<Self> {
        let options = PcoOptions {
            memory_tokens: true,
            ..PcoOptions::default()
        };
        Self::with_options(
            settings_service,
            dashboard_auth_enforced,
            PluginOptions::default(),
            options,
        )
    }

    /// Playback seams replaced, in-memory Planning Center (tests).
    pub fn with_playback_options(
        settings_service: Arc<SettingsService>,
        dashboard_auth_enforced: bool,
        playback_options: PluginOptions,
    ) -> Arc<Self> {
        let options = PcoOptions {
            memory_tokens: true,
            ..PcoOptions::default()
        };
        Self::with_options(
            settings_service,
            dashboard_auth_enforced,
            playback_options,
            options,
        )
    }

    /// Planning Center wiring replaced (production keychain tokens, test origins).
    pub fn with_pco(
        settings_service: Arc<SettingsService>,
        dashboard_auth_enforced: bool,
        options: PcoOptions,
    ) -> Arc<Self> {
        Self::with_options(
            settings_service,
            dashboard_auth_enforced,
            PluginOptions::default(),
            options,
        )
    }

    /// Same, with explicit MIDI / OSC / ProPresenter backends (tests and the contract profile).
    pub fn with_backends(
        settings_service: Arc<SettingsService>,
        dashboard_auth_enforced: bool,
        backends: OutputBackends,
    ) -> Arc<Self> {
        let options = PcoOptions {
            memory_tokens: true,
            ..PcoOptions::default()
        };
        Self::with_all(
            settings_service,
            dashboard_auth_enforced,
            PluginOptions::default(),
            options,
            backends,
        )
    }

    /// Explicit seams for Playback (client factory, scanner, timeouts) and Planning Center
    /// (production keychain tokens, test origins).
    pub fn with_options(
        settings_service: Arc<SettingsService>,
        dashboard_auth_enforced: bool,
        playback_options: PluginOptions,
        options: PcoOptions,
    ) -> Arc<Self> {
        Self::with_all(
            settings_service,
            dashboard_auth_enforced,
            playback_options,
            options,
            OutputBackends::default(),
        )
    }

    /// The one builder that wires every plugin: Playback, Planning Center, MIDI, lights and
    /// ProPresenter, each with its test seams.
    pub fn with_all(
        settings_service: Arc<SettingsService>,
        dashboard_auth_enforced: bool,
        playback_options: PluginOptions,
        options: PcoOptions,
        backends: OutputBackends,
    ) -> Arc<Self> {
        let settings = settings_service.effective_runtime_settings();
        let settings_source = settings.integration_modes.service_source;
        let bus = EventBus::new();
        let store = StateStore::default();
        let state_service = StateService::new(
            bus.clone(),
            store.clone(),
            settings.recent_event_limit,
            settings.recent_error_limit,
        );
        let plugins = PluginManager::new(bus.clone());
        let playback = Arc::new(PlaybackPlugin::new(
            bus.clone(),
            store.clone(),
            state_service.clone(),
            settings_service.clone(),
            playback_options,
        ));
        let outputs = Arc::new(
            Outputs::build(
                &bus,
                &state_service,
                &settings,
                backends,
                settings_service.clone(),
            )
            .unwrap_or_else(|error| panic!("invalid output settings: {error}")),
        );
        // Registration order is the health order Python reports: lights, services, the Playback
        // input (real MIDI or the Playback API), then ProPresenter.
        register(&plugins, outputs.lights.clone() as Arc<dyn Plugin>);
        if settings.integration_modes.service_source == ServiceSource::Demo {
            let demo = Arc::new(DemoPlugin::with_simulation(
                bus.clone(),
                settings.integration_modes.timer_output == TimerOutput::Simulated,
            ));
            outputs.set_demo(demo.clone());
            register(&plugins, demo);
        }
        let pco = PcoRuntime::new(settings_service.clone(), options);
        if settings.integration_modes.service_source == ServiceSource::PlanningCenter {
            match pco.build_plugin(bus.clone(), store.clone()) {
                Ok(plugin) => {
                    pco.set_plugin(Some(plugin.clone()));
                    register(&plugins, Arc::new(plugin));
                }
                Err(error) => tracing::error!(%error, "planning_center_plugin_build_failed"),
            }
        }
        match &outputs.midi_input {
            Some(input) => register(&plugins, input.clone() as Arc<dyn Plugin>),
            None => register(&plugins, playback.clone()),
        }
        if let Some(plugin) = outputs.startup_propresenter(&settings) {
            register(&plugins, plugin as Arc<dyn Plugin>);
        }
        let lengths = Arc::new(length_service(
            &settings_service,
            &store,
            &state_service,
            &pco,
            &playback,
        ));
        let runtime = Arc::new(Self {
            settings,
            bus,
            store,
            state_service,
            plugins,
            settings_service,
            playback,
            pco,
            oauth_stop: tokio::sync::watch::channel(false).0,
            activation: Mutex::new(None),
            lengths,
            outputs,
            reconcile: tokio::sync::Mutex::new(()),
            sessions: DashboardSessionStore::new(),
            login_throttle: crate::security::LoginThrottle::default(),
            dashboard_auth_enforced,
            started_at: Utc::now(),
            remote: RwLock::new(RemoteWiring::default()),
            live_service_source: settings_source,
            startup_activation: std::sync::atomic::AtomicBool::new(false),
            keychain_degraded: std::sync::atomic::AtomicBool::new(false),
            keychain_interval_ms: std::sync::atomic::AtomicU64::new(KEYCHAIN_RETRY_SECONDS * 1000),
            keychain_watch: Mutex::new(None),
            helper_install: Arc::new(crate::helper_install::HelperInstall::new()),
        });
        runtime.note_keychain_state();
        runtime
    }

    /// Records whether a keychain answer is missing right now (checked once at build time).
    fn note_keychain_state(&self) {
        let degraded = self.settings_service.credential_unread()
            || self.pco.effective().credential_issue.is_some();
        self.keychain_degraded
            .store(degraded, std::sync::atomic::Ordering::SeqCst);
        if degraded {
            tracing::warn!("keychain_access_pending");
        }
    }

    /// A keychain answer is still missing (the watcher is retrying).
    pub fn keychain_degraded(&self) -> bool {
        self.keychain_degraded
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Tests: how often the watcher asks again.
    pub fn set_keychain_retry_interval(&self, interval: std::time::Duration) {
        self.keychain_interval_ms.store(
            interval.as_millis().max(1) as u64,
            std::sync::atomic::Ordering::SeqCst,
        );
    }

    /// One keychain check: ask again what failed, pick up what has arrived, and when everything
    /// answers hand the credentials to the running Planning Center plugin. Never holds up the
    /// caller beyond the guard's short wait, and never throws a stored secret away.
    pub async fn retry_keychain(self: &Arc<Self>) {
        if !self.keychain_degraded() {
            return;
        }
        let me = self.clone();
        let checked = tokio::task::spawn_blocking(move || {
            me.settings_service.refresh_credential();
            me.pco.oauth.retry_credentials();
            let effective = me.pco.effective();
            let answering = !me.settings_service.credential_unread()
                && me.settings_service.credential_access()
                    == stagepilot_core::settings::CredentialAccess::Ready
                && me.pco.oauth.credential_access()
                    == stagepilot_core::settings::CredentialAccess::Ready
                && effective.credential_issue.is_none();
            (answering, effective)
        })
        .await;
        let Ok((true, effective)) = checked else {
            return;
        };
        self.keychain_degraded
            .store(false, std::sync::atomic::Ordering::SeqCst);
        tracing::info!("keychain_access_recovered");
        if let Some(plugin) = self.pco.plugin() {
            let _serialised = self.reconcile.lock().await;
            plugin.reconfigure(effective).await;
            // The plugin may have failed to start while the keychain was silent (and failed its
            // one startup retry): the manager then still reports that error and the event
            // subscriptions were dropped, so /health stays "degraded" although Planning Center
            // is connected. A normal start brings both back.
            if self
                .plugins
                .restart_if_failed(stagepilot_pco::plugin::PLUGIN_NAME)
                .await
            {
                tracing::info!("planning_center_restarted_after_keychain");
            }
        }
    }

    fn spawn_keychain_watch(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            loop {
                let wait = match weak.upgrade() {
                    Some(rt) => std::time::Duration::from_millis(
                        rt.keychain_interval_ms
                            .load(std::sync::atomic::Ordering::SeqCst),
                    ),
                    None => return,
                };
                tokio::time::sleep(wait).await;
                let Some(rt) = weak.upgrade() else { return };
                rt.retry_keychain().await;
            }
        });
        *self
            .keychain_watch
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(task);
    }

    /// `_apply_presentation`: make the running Presentation output match the saved settings.
    /// `Err` is the 409 text; `Ok(Some(_))` a warning when the new output failed to start.
    pub async fn apply_presentation(&self) -> Result<Option<String>, String> {
        let settings = self.settings_service.effective_runtime_settings();
        self.outputs
            .sync_presentation(&settings, &self.plugins, &self.store, &self.state_service)
            .await
    }

    /// A snapshot of the Remote wiring (cheap: three optional `Arc`s).
    pub fn remote_wiring(&self) -> RemoteWiring {
        self.remote
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn update_remote(&self, change: impl FnOnce(&mut RemoteWiring)) {
        change(
            &mut self
                .remote
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
    }

    pub fn set_remote_access(&self, access: Option<Arc<RemoteAccess>>) {
        self.update_remote(|w| w.access = access);
    }

    pub fn set_remote_feature(&self, feature: Option<Arc<RemoteFeature>>) {
        self.update_remote(|w| w.feature = feature);
    }

    pub fn set_remote_manager(&self, manager: Option<Arc<dyn RemoteManager>>) {
        self.update_remote(|w| w.manager = manager);
    }

    /// Run the delayed startup reconcile after `start` (the launcher turns this on; tests don't).
    pub fn enable_startup_activation(&self) {
        self.startup_activation
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn live_service_source(&self) -> ServiceSource {
        self.live_service_source
    }

    /// Lifespan start: OAuth startup refresh, state service, plugins, then `application.started`.
    pub async fn start(self: &Arc<Self>) {
        // A persisted access token can expire while StagePilot is closed. Refresh it before the
        // plugins start and hand the fresh Bearer token to the stopped Planning Center plugin, so
        // startup never makes its first request with stale auth.
        if self
            .settings_service
            .effective_snapshot()
            .planning_center
            .connection_method
            == ConnectionMethod::Oauth
        {
            let timeout = std::time::Duration::from_secs_f64(OAUTH_STARTUP_REFRESH_TIMEOUT_SECONDS);
            match tokio::time::timeout(timeout, self.pco.oauth.valid_access_token()).await {
                Err(_) => tracing::warn!("planning_center_oauth_startup_refresh_timed_out"),
                Ok(Err(CompleteError::OAuth(error))) => tracing::warn!(
                    permanent = error.permanent,
                    "planning_center_oauth_startup_refresh_failed"
                ),
                Ok(Err(CompleteError::Credential(_))) => {
                    tracing::warn!("planning_center_oauth_startup_refresh_unavailable")
                }
                Ok(Ok(_)) => {}
            }
            if let Some(plugin) = self.pco.plugin() {
                let _ = plugin.prepare_start(self.pco.effective());
            }
        }
        self.state_service.start();
        self.spawn_keychain_watch();
        self.plugins.start_all().await;
        // Proactive OAuth refresh: one lifespan-owned task polling a stop signal.
        let _ = self.oauth_stop.send(false);
        tokio::spawn(stagepilot_pco::oauth::proactive_refresh_loop(
            self.pco.oauth.clone(),
            self.oauth_stop.subscribe(),
            std::time::Duration::from_secs_f64(stagepilot_pco::oauth::REFRESH_POLL_SECONDS),
        ));
        self.bus
            .publish(empty_event(EventType::ApplicationStarted, "application"))
            .await;
        // Python creates the activation task right after `application.started`; production only,
        // so route and contract tests never see a delayed reconfigure.
        if self
            .startup_activation
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            let targets = Arc::new(crate::startup_activation::RuntimeActivation(
                Arc::downgrade(self),
            ));
            *self.activation.lock().unwrap_or_else(|p| p.into_inner()) = Some(tokio::spawn(
                crate::startup_activation::run_after_settle(targets),
            ));
        }
        self.store
            .mutate(|s| s.application_status = ApplicationStatus::Running);
    }

    /// Lifespan stop: `application.stopping`, plugins in reverse, state service.
    pub async fn stop(&self) {
        self.bus
            .publish(empty_event(EventType::ApplicationStopping, "application"))
            .await;
        let activation = self
            .activation
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(task) = activation {
            task.abort();
            let _ = task.await;
        }
        let _ = self.oauth_stop.send(true);
        if let Some(task) = self
            .keychain_watch
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            task.abort();
        }
        self.plugins.stop_all().await;
        self.state_service.stop();
    }
}
