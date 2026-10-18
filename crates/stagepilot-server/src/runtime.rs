//! The application runtime: the Rust counterpart of `core/runtime.py` plus the parts of
//! `main.create_app` that this phase owns (event bus, state store/service, plugin manager,
//! settings service, startup and shutdown order).

use crate::demo::{DemoPlugin, LightsStub, PlaybackStub};
use chrono::{DateTime, Utc};
use stagepilot_core::auth::DashboardSessionStore;
use stagepilot_core::bus::EventBus;
use stagepilot_core::config::{ConnectionMethod, ServiceSource};
use stagepilot_core::events::{empty_event, EventType};
use stagepilot_core::plugin::PluginManager;
use stagepilot_core::settings::{RuntimeSettings, SettingsService};
use stagepilot_core::state::ApplicationStatus;
use stagepilot_core::state_service::StateService;
use stagepilot_core::store::StateStore;
use stagepilot_pco::oauth::{CompleteError, OAUTH_STARTUP_REFRESH_TIMEOUT_SECONDS};
use stagepilot_pco::runtime::{PcoOptions, PcoRuntime};
use std::sync::Arc;

pub struct Runtime {
    pub settings: RuntimeSettings,
    pub bus: EventBus,
    pub store: StateStore,
    pub state_service: StateService,
    pub plugins: PluginManager,
    pub settings_service: Arc<SettingsService>,
    pub playback: Arc<PlaybackStub>,
    /// Planning Center (phase 5): OAuth, setup service, client factory, the live plugin.
    pub pco: Arc<PcoRuntime>,
    oauth_stop: tokio::sync::watch::Sender<bool>,
    pub sessions: DashboardSessionStore,
    pub dashboard_auth_enforced: bool,
    pub started_at: DateTime<Utc>,
    /// The Services source the running plugins were built for (changing it needs a restart here).
    live_service_source: ServiceSource,
}

impl Runtime {
    /// Build the runtime. `settings_service` must already be loaded (`load()` called) so
    /// `effective_runtime_settings()` is the resolved configuration.
    pub fn new(settings_service: Arc<SettingsService>, dashboard_auth_enforced: bool) -> Arc<Self> {
        // Embedding and tests: OAuth tokens and the plan cache stay in memory (no keychain access).
        let options = PcoOptions {
            memory_tokens: true,
            ..PcoOptions::default()
        };
        Self::with_pco(settings_service, dashboard_auth_enforced, options)
    }

    /// Same, with explicit Planning Center wiring (production keychain tokens, test origins).
    pub fn with_pco(
        settings_service: Arc<SettingsService>,
        dashboard_auth_enforced: bool,
        options: PcoOptions,
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
        let playback = Arc::new(PlaybackStub::new(bus.clone()));
        // Registration order is the health order Python reports: lights, services, playback.
        register(&plugins, Arc::new(LightsStub::new(bus.clone())));
        if settings.integration_modes.service_source == ServiceSource::Demo {
            register(&plugins, Arc::new(DemoPlugin::new(bus.clone())));
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
        register(&plugins, playback.clone());
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
            sessions: DashboardSessionStore::new(),
            dashboard_auth_enforced,
            started_at: Utc::now(),
            live_service_source: settings_source,
        })
    }

    pub fn live_service_source(&self) -> ServiceSource {
        self.live_service_source
    }

    /// Lifespan start: OAuth startup refresh, state service, plugins, then `application.started`.
    pub async fn start(&self) {
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
        self.store
            .mutate(|s| s.application_status = ApplicationStatus::Running);
    }

    /// Lifespan stop: `application.stopping`, plugins in reverse, state service.
    pub async fn stop(&self) {
        self.bus
            .publish(empty_event(EventType::ApplicationStopping, "application"))
            .await;
        let _ = self.oauth_stop.send(true);
        self.plugins.stop_all().await;
        self.state_service.stop();
    }
}

fn register(plugins: &PluginManager, plugin: Arc<dyn stagepilot_core::plugin::Plugin>) {
    if let Err(error) = plugins.register(plugin) {
        tracing::error!(%error, "plugin_register_failed");
    }
}
