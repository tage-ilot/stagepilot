//! The application runtime: the Rust counterpart of `core/runtime.py` plus the parts of
//! `main.create_app` that this phase owns (event bus, state store/service, plugin manager,
//! settings service, startup and shutdown order).

use crate::demo::{DemoPlugin, PlaybackStub};
use crate::outputs::{register, OutputBackends, Outputs};
use chrono::{DateTime, Utc};
use stagepilot_core::auth::DashboardSessionStore;
use stagepilot_core::bus::EventBus;
use stagepilot_core::config::ServiceSource;
use stagepilot_core::config::TimerOutput;
use stagepilot_core::events::{empty_event, EventType};
use stagepilot_core::plugin::Plugin;
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
    /// MIDI input, lights and ProPresenter (phase 4).
    pub outputs: Arc<Outputs>,
    /// `PluginManager.reconcile_lock`: serialises live connection changes made by routes.
    pub reconcile: tokio::sync::Mutex<()>,
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
        Self::with_backends(
            settings_service,
            dashboard_auth_enforced,
            OutputBackends::default(),
        )
    }

    /// Same, with explicit MIDI / OSC / ProPresenter backends (tests and the contract profile).
    pub fn with_backends(
        settings_service: Arc<SettingsService>,
        dashboard_auth_enforced: bool,
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
        let playback = Arc::new(PlaybackStub::new(bus.clone()));
        let outputs = Arc::new(
            Outputs::build(&bus, &state_service, &settings, backends)
                .unwrap_or_else(|error| panic!("invalid output settings: {error}")),
        );
        // Registration order is the health order Python reports: lights, services, the Playback
        // input, then ProPresenter.
        register(&plugins, outputs.lights.clone() as Arc<dyn Plugin>);
        if settings.integration_modes.service_source == ServiceSource::Demo {
            let demo = Arc::new(DemoPlugin::with_simulation(
                bus.clone(),
                settings.integration_modes.timer_output == TimerOutput::Simulated,
            ));
            outputs.set_demo(demo.clone());
            register(&plugins, demo);
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
            outputs,
            reconcile: tokio::sync::Mutex::new(()),
            sessions: DashboardSessionStore::new(),
            dashboard_auth_enforced,
            started_at: Utc::now(),
            live_service_source: settings_source,
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
