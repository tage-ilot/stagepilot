//! m16: one failed Planning Center start must record one error, not two. The plugin publishes a
//! connection error and the plugin manager publishes `plugin.failed`, both with the same text.

mod support;

use stagepilot_core::bus::EventBus;
use stagepilot_core::plugin::PluginManager;
use stagepilot_core::state::ApplicationState;
use stagepilot_core::state_service::StateService;
use stagepilot_core::store::StateStore;
use stagepilot_pco::client::default_factory;
use stagepilot_pco::models::PcoSettings;
use stagepilot_pco::plugin::PcoPlugin;
use std::sync::Arc;

#[tokio::test]
async fn a_failed_start_records_its_error_once() {
    let bus = EventBus::new();
    let store = StateStore::new(ApplicationState::default());
    let state_service = StateService::new(bus.clone(), store.clone(), 50, 50);
    state_service.start();
    let manager = PluginManager::new(bus.clone());
    let plugin = PcoPlugin::new(
        bus,
        store.clone(),
        PcoSettings::default(), // no credentials: start fails with a configuration error
        "America/Los_Angeles",
        default_factory(None),
        None,
        None,
    )
    .unwrap();
    manager.register(Arc::new(plugin)).unwrap();
    manager.start_all().await;

    let errors = store.snapshot().recent_errors;
    let messages: Vec<_> = errors.iter().map(|e| e.message.as_str()).collect();
    assert_eq!(errors.len(), 1, "duplicate error entries: {messages:?}");
    assert_eq!(errors[0].component, "planning_center");
}
