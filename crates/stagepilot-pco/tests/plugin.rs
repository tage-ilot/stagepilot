//! Behaviour tests for `PcoPlugin` (the Rust counterpart of the core cases in
//! `test_planning_center_plugin.py`): the real plugin and real client run against the in-process
//! mock server, with a real bus and state store. The remaining Python cases (cache expiry
//! arithmetic, single-flight reload ordering) are covered by `plan_cache.rs`, the `pco` contract
//! profile and the unit tests in `src/plugin.rs`.

mod support;

use chrono::NaiveDate;
use serde_json::json;
use stagepilot_core::bus::EventBus;
use stagepilot_core::events::EventType;
use stagepilot_core::plugin::Plugin;
use stagepilot_core::state::{ApplicationState, ConnectionStatus, ServiceLoadStatus};
use stagepilot_core::state_service::StateService;
use stagepilot_core::store::StateStore;
use stagepilot_pco::client::default_factory;
use stagepilot_pco::models::PcoSettings;
use stagepilot_pco::plugin::{PcoPlugin, TodayProvider, PLUGIN_NAME};
use std::sync::{Arc, Mutex};
use support::*;

const TZ: &str = "America/Los_Angeles";

fn today() -> TodayProvider {
    Arc::new(|_| NaiveDate::from_ymd_opt(2026, 7, 12).unwrap())
}

fn settings() -> PcoSettings {
    PcoSettings {
        service_type_id: Some("42".into()),
        ..pat_settings()
    }
}

struct Harness {
    plugin: PcoPlugin,
    store: StateStore,
    events: Arc<Mutex<Vec<EventType>>>,
    _state_service: StateService,
}

fn harness(mock: &Mock, settings: PcoSettings) -> Harness {
    let bus = EventBus::new();
    let store = StateStore::new(ApplicationState::default());
    let state_service = StateService::new(bus.clone(), store.clone(), 50, 50);
    state_service.start();
    let events = Arc::new(Mutex::new(Vec::new()));
    let seen = events.clone();
    bus.add_tap(move |e| seen.lock().unwrap().push(e.event_type));
    let plugin = PcoPlugin::new(
        bus,
        store.clone(),
        settings,
        TZ,
        default_factory(Some(mock.url.clone())),
        Some(today()),
        None,
    )
    .expect("plugin builds");
    Harness {
        plugin,
        store,
        events,
        _state_service: state_service,
    }
}

/// Weekend Services (42) has one plan today with one song; every other path is a 404.
async fn weekend_mock() -> Mock {
    Mock::start(|r| {
        let p = r.path.as_str();
        if p.ends_with("/service_types") {
            Resp::json(
                200,
                page(vec![
                    service_type("7", "Other Services", 2),
                    service_type("42", "Weekend Services", 1),
                ]),
            )
        } else if p.contains("/service_types/42/plans") && p.ends_with("/plans") {
            Resp::json(200, page(vec![plan("plan-1", "Sunday")]))
        } else if p.ends_with("/plan_times") {
            Resp::json(
                200,
                page(vec![plan_time("t1", "2026-07-12T17:00:00Z", "service")]),
            )
        } else if p.ends_with("/items") {
            Resp::json(
                200,
                page(vec![
                    item("h1", "header", "Opening", 1, Some(90), None),
                    item("i1", "song", "Holy Forever", 2, Some(240), Some("s1")),
                ]),
            )
        } else {
            Resp::json(404, json!({}))
        }
    })
    .await
}

#[tokio::test]
async fn startup_resolves_the_service_type_and_loads_the_local_date() {
    let mock = weekend_mock().await;
    let h = harness(&mock, settings());
    h.plugin.start().await.expect("starts");
    let state = h.store.snapshot();
    assert_eq!(state.planning_center_status, ConnectionStatus::Connected);
    assert_eq!(state.service_load.status, ServiceLoadStatus::Loaded);
    assert!(!state.service_load.is_stale);
    assert_eq!(state.plan.as_ref().map(|p| p.id.as_str()), Some("plan-1"));
    assert_eq!(
        state.plan.as_ref().unwrap().service_type_id.as_deref(),
        Some("42")
    );
    assert_eq!(
        state.next_song.as_ref().map(|s| s.title.as_str()),
        Some("Holy Forever")
    );
    assert_eq!(state.service_load.skipped_items.len(), 1);
    assert_eq!(state.service_load.skipped_items[0].item_id, "h1");
    assert!(state.last_successful_plan_reload_at.is_some());
    let health = h.plugin.health().await.unwrap();
    assert_eq!(health.name, PLUGIN_NAME);
    assert!(health.last_error.is_none());
    let seen = h.events.lock().unwrap().clone();
    assert_eq!(
        seen.iter()
            .filter(|e| **e == EventType::ServiceLoaded)
            .count(),
        1
    );
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn no_plan_today_is_not_found_and_the_connection_stays_healthy() {
    let mock = Mock::start(|r| {
        if r.path.ends_with("/service_types") {
            Resp::json(200, page(vec![service_type("42", "Weekend Services", 1)]))
        } else {
            Resp::json(200, page(vec![]))
        }
    })
    .await;
    let h = harness(&mock, settings());
    h.plugin.start().await.expect("starts");
    let state = h.store.snapshot();
    assert_eq!(state.service_load.status, ServiceLoadStatus::NotFound);
    assert_eq!(state.planning_center_status, ConnectionStatus::Connected);
    assert!(state.plan.is_none());
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn an_api_failure_reports_an_error_without_leaking_credentials() {
    let mock =
        Mock::start(|_| Resp::json(500, json!({"errors": [{"detail": "boom test-secret"}]}))).await;
    let h = harness(&mock, settings());
    // The plugin starts (the failure is a load failure, retried later), but reports the error.
    let _ = h.plugin.start().await;
    let state = h.store.snapshot();
    assert_eq!(state.planning_center_status, ConnectionStatus::Error);
    assert_eq!(state.service_load.status, ServiceLoadStatus::Error);
    let dump = serde_json::to_string(&state).unwrap();
    assert!(!dump.contains("test-secret"), "{dump}");
    assert!(!dump.contains("test-app-id"), "{dump}");
    let health = h.plugin.health().await.unwrap();
    assert!(!format!("{:?}", health.last_error).contains("test-secret"));
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn a_missing_credential_fails_startup_without_building_a_client() {
    let mock = weekend_mock().await;
    let h = harness(
        &mock,
        PcoSettings {
            app_id: None,
            secret: None,
            ..settings()
        },
    );
    assert!(h.plugin.start().await.is_err());
    assert!(mock.requests().is_empty());
    assert_eq!(
        h.store.snapshot().planning_center_status,
        ConnectionStatus::Error
    );
}

#[tokio::test]
async fn stop_is_idempotent_and_leaves_the_plugin_stopped() {
    let mock = weekend_mock().await;
    let h = harness(&mock, settings());
    h.plugin.start().await.unwrap();
    h.plugin.stop().await.unwrap();
    h.plugin.stop().await.unwrap();
    let health = h.plugin.health().await.unwrap();
    assert_eq!(format!("{:?}", health.status), "Stopped");
    assert_eq!(
        h.store.snapshot().planning_center_status,
        ConnectionStatus::Disconnected
    );
}

#[tokio::test]
async fn reconfigure_applies_a_new_service_type_without_a_restart() {
    let mock = weekend_mock().await;
    let h = harness(&mock, settings());
    h.plugin.start().await.unwrap();
    let outcome = h
        .plugin
        .reconfigure(PcoSettings {
            service_type_id: Some("7".into()),
            ..settings()
        })
        .await;
    assert!(outcome.accepted, "{outcome:?}");
    assert!(mock
        .requests()
        .iter()
        .any(|r| r.path.contains("/service_types/7/plans")));
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn reconfigure_on_a_stopped_plugin_is_rejected() {
    let mock = weekend_mock().await;
    let h = harness(&mock, settings());
    let outcome = h.plugin.reconfigure(settings()).await;
    assert!(!outcome.accepted);
}
