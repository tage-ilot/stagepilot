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
use stagepilot_pco::oauth::{CompleteError, OAUTH_STARTUP_REFRESH_TIMEOUT_SECONDS};
use stagepilot_pco::runtime::{PcoOptions, PcoRuntime};
use stagepilot_playback::api::LengthUpdateResult;
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
    /// Planning Center (phase 5): OAuth, setup service, client factory, the live plugin.
    pub pco: Arc<PcoRuntime>,
    oauth_stop: tokio::sync::watch::Sender<bool>,
    /// The lifespan-owned startup activation task (`stagepilot-startup-activation`).
    activation: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// The last Planning Center length operation (always idle until that service is ported).
    pub lengths_result: Mutex<LengthUpdateResult>,
    /// MIDI input, lights and ProPresenter (phase 4).
    pub outputs: Arc<Outputs>,
    /// `PluginManager.reconcile_lock`: serialises live connection changes made by routes.
    pub reconcile: tokio::sync::Mutex<()>,
    pub sessions: DashboardSessionStore,
    pub dashboard_auth_enforced: bool,
    pub started_at: DateTime<Utc>,
    /// `app.state.remote_access` / `remote_feature` / `remote_manager`: all optional, settable
    /// after construction exactly as the Python tests and the launcher do.
    remote: RwLock<RemoteWiring>,
    /// The Services source the running plugins were built for (changing it needs a restart here).
    live_service_source: ServiceSource,
    startup_activation: std::sync::atomic::AtomicBool,
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
                settings_service.settings_path(),
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
        Arc::new(Self {
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
            lengths_result: Mutex::new(LengthUpdateResult::idle()),
            outputs,
            reconcile: tokio::sync::Mutex::new(()),
            sessions: DashboardSessionStore::new(),
            dashboard_auth_enforced,
            started_at: Utc::now(),
            remote: RwLock::new(RemoteWiring::default()),
            live_service_source: settings_source,
            startup_activation: std::sync::atomic::AtomicBool::new(false),
        })
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
        self.plugins.stop_all().await;
        self.state_service.stop();
    }
}
