//! The application runtime: the Rust counterpart of `core/runtime.py` plus the parts of
//! `main.create_app` that this phase owns (event bus, state store/service, plugin manager,
//! settings service, startup and shutdown order).

use crate::demo::{DemoPlugin, LightsStub, PlaybackStub};
use chrono::{DateTime, Utc};
use stagepilot_core::auth::DashboardSessionStore;
use stagepilot_core::bus::EventBus;
use stagepilot_core::config::ServiceSource;
use stagepilot_core::events::{empty_event, EventType};
use stagepilot_core::plugin::PluginManager;
use stagepilot_core::settings::{RuntimeSettings, SettingsService};
use stagepilot_core::state::ApplicationStatus;
use stagepilot_core::state_service::StateService;
use stagepilot_core::store::StateStore;
use std::sync::Arc;

pub struct Runtime {
    pub settings: RuntimeSettings,
    pub bus: EventBus,
    pub store: StateStore,
    pub state_service: StateService,
    pub plugins: PluginManager,
    pub settings_service: Arc<SettingsService>,
    pub playback: Arc<PlaybackStub>,
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
        register(&plugins, playback.clone());
        Arc::new(Self {
            settings,
            bus,
            store,
            state_service,
            plugins,
            settings_service,
            playback,
            sessions: DashboardSessionStore::new(),
            dashboard_auth_enforced,
            started_at: Utc::now(),
            live_service_source: settings_source,
        })
    }

    pub fn live_service_source(&self) -> ServiceSource {
        self.live_service_source
    }

    /// Lifespan start: state service, plugins, then `application.started`.
    pub async fn start(&self) {
        self.state_service.start();
        self.plugins.start_all().await;
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
        self.plugins.stop_all().await;
        self.state_service.stop();
    }
}

fn register(plugins: &PluginManager, plugin: Arc<dyn stagepilot_core::plugin::Plugin>) {
    if let Err(error) = plugins.register(plugin) {
        tracing::error!(%error, "plugin_register_failed");
    }
}
