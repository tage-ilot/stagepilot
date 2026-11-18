//! The song-length preview matches the scanned Playback lengths to a Planning Center plan. While
//! the operator has picked a service other than the upcoming one, that plan is the one StagePilot
//! has loaded (and the one the scan was made for), so the preview must read the picked plan too.

mod support;

use chrono::{Duration as ChronoDuration, NaiveDate, Utc};
use serde_json::json;
use stagepilot_core::bus::EventBus;
use stagepilot_core::service_pin::{PinKeeper, ServicePin, SOURCE_PLANNING_CENTER};
use stagepilot_core::settings::{MemoryCredentialStore, MemorySettingsStore, SettingsService};
use stagepilot_core::state::ApplicationState;
use stagepilot_core::state_service::StateService;
use stagepilot_core::store::StateStore;
use stagepilot_pco::client::default_factory;
use stagepilot_pco::lengths::{LengthError, LengthService};
use std::collections::BTreeMap;
use std::sync::Arc;
use support::*;

fn noon(days: i64) -> (NaiveDate, String) {
    let date = Utc::now().date_naive() + ChronoDuration::days(days);
    (
        date,
        date.and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc()
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string(),
    )
}

async fn service(keeper: Option<Arc<PinKeeper>>, gone: bool) -> LengthService {
    let (_, tomorrow) = noon(1);
    let (_, later) = noon(10);
    let mock = Mock::start(move |r| {
        let p = r.path.as_str();
        let plans = if gone {
            vec![plan("p1", "Plan p1")]
        } else {
            vec![plan("p1", "Plan p1"), plan("p2", "Plan p2")]
        };
        if p.ends_with("/service_types") {
            Resp::json(200, page(vec![service_type("42", "Weekend", 1)]))
        } else if p.ends_with("/plans") {
            Resp::json(200, page(plans))
        } else if p.ends_with("/plan_times") {
            let starts = if p.contains("/plans/p2/") {
                &later
            } else {
                &tomorrow
            };
            Resp::json(200, page(vec![plan_time("t", starts, "service")]))
        } else if p.ends_with("/items") {
            Resp::json(
                200,
                page(vec![item("i1", "song", "One", 1, Some(100), Some("s1"))]),
            )
        } else {
            Resp::json(404, json!({}))
        }
    })
    .await;
    let settings = SettingsService::new(
        Box::new(MemorySettingsStore::new(None)),
        Box::new(MemoryCredentialStore::default()),
        BTreeMap::new(),
    );
    settings.load();
    let bus = EventBus::new();
    let store = StateStore::new(ApplicationState::default());
    let dispatcher = StateService::new(bus, store.clone(), 50, 50);
    let service = LengthService::new(
        Arc::new(settings),
        store,
        dispatcher,
        default_factory(Some(mock.url.clone())),
        Arc::new(pat_settings),
    );
    // The mock lives as long as the test process; leak it so the service can keep asking.
    std::mem::forget(mock);
    if let Some(keeper) = keeper {
        service.set_pin_keeper(keeper);
    }
    service
}

fn pin_for(plan_id: &str, days: i64) -> Arc<PinKeeper> {
    let keeper = PinKeeper::in_memory();
    keeper
        .set(ServicePin::new(
            SOURCE_PLANNING_CENTER,
            plan_id,
            noon(days).0,
            Some("42".into()),
            None,
        ))
        .unwrap();
    keeper
}

#[tokio::test]
async fn the_preview_reads_the_upcoming_plan_when_nothing_is_picked() {
    let service = service(Some(PinKeeper::in_memory()), false).await;
    let preview = service.preview("42", &[Some(100.0)], "sig").await.unwrap();
    assert_eq!(preview.plan_title, "Plan p1");
}

#[tokio::test]
async fn the_preview_reads_the_picked_plan_while_one_is_picked() {
    let service = service(Some(pin_for("p2", 10)), false).await;
    let preview = service.preview("42", &[Some(100.0)], "sig").await.unwrap();
    assert_eq!(preview.plan_title, "Plan p2");
    assert_eq!(preview.plan_date, noon(10).0.to_string());
}

#[tokio::test]
async fn a_pick_in_another_category_does_not_change_this_category() {
    let keeper = PinKeeper::in_memory();
    keeper
        .set(ServicePin::new(
            SOURCE_PLANNING_CENTER,
            "p2",
            noon(10).0,
            Some("7".into()),
            None,
        ))
        .unwrap();
    let service = service(Some(keeper), false).await;
    let preview = service.preview("42", &[Some(100.0)], "sig").await.unwrap();
    assert_eq!(preview.plan_title, "Plan p1");
}

#[tokio::test]
async fn a_picked_plan_that_is_gone_is_said_plainly() {
    let service = service(Some(pin_for("p2", 10)), true).await;
    let error = service
        .preview("42", &[Some(100.0)], "sig")
        .await
        .unwrap_err();
    assert_eq!(
        error,
        LengthError::Value("The service you picked is not in Planning Center any more.".into())
    );
}
