//! M3: valid HTTP 200 plan responses were rejected by the strict models on every launch
//! (`planning_center_response_validation_diagnostic`, resource kind plan). Planning Center sends
//! explicit `null` for optional text attributes (an untitled plan has `"title": null`); a serde
//! `default` only covers a missing key, not a null. These tests pin the null / missing shapes.

mod support;

use chrono::NaiveDate;
use serde_json::{json, Value};
use stagepilot_pco::client::{PcoApi, PcoClient};
use stagepilot_pco::errors::PcoError;
use stagepilot_pco::models::*;
use support::*;

const TZ: &str = "America/Los_Angeles";

fn weekend() -> ServiceType {
    ServiceType {
        id: "42".into(),
        name: "Weekend".into(),
        sequence: 1,
        archived: false,
    }
}

fn day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 7, 12).unwrap()
}

fn mock_with(plans: Value, plan_times: Value, items: Value) -> impl Fn(&Req) -> Resp + Send + Sync {
    move |r| {
        let body = if r.path.ends_with("/plan_times") {
            &plan_times
        } else if r.path.ends_with("/items") {
            &items
        } else if r.path.ends_with("/plans") {
            &plans
        } else {
            return Resp::json(404, json!({}));
        };
        Resp::json(200, json!({"data": body, "meta": {"total_count": null}}))
    }
}

async fn load(
    plans: Value,
    plan_times: Value,
    items: Value,
) -> Result<PlanDiscoveryResult, PcoError> {
    let mock = Mock::start(mock_with(plans, plan_times, items)).await;
    let client = PcoClient::new(&pat_settings(), Some(mock.url.clone())).unwrap();
    client
        .load_plan_for_date(&weekend(), day(), TZ, None, 0)
        .await
}

fn good_time() -> Value {
    json!([plan_time("t1", "2026-07-12T17:00:00Z", "service")])
}

fn good_items() -> Value {
    json!([item("i1", "song", "Holy Forever", 1, Some(240), Some("s1"))])
}

#[tokio::test]
async fn an_untitled_plan_with_null_attributes_loads() {
    let plans = json!([{"type": "Plan", "id": "p1",
        "attributes": {"title": null, "dates": null, "sort_date": "2026-07-12T17:00:00Z"}}]);
    let result = load(plans, good_time(), good_items()).await.expect("loads");
    assert!(
        matches!(result, PlanDiscoveryResult::Loaded { .. }),
        "{result:?}"
    );
}

#[tokio::test]
async fn a_plan_with_no_attributes_at_all_loads() {
    let plans = json!([{"type": "Plan", "id": "p1", "attributes": {}}]);
    let result = load(plans, good_time(), good_items()).await.expect("loads");
    assert!(
        matches!(result, PlanDiscoveryResult::Loaded { .. }),
        "{result:?}"
    );
}

#[tokio::test]
async fn a_plan_time_with_null_name_and_end_loads() {
    let plans = json!([plan("p1", "Sunday")]);
    let times = json!([{"type": "PlanTime", "id": "t1",
        "attributes": {"starts_at": "2026-07-12T17:00:00Z", "ends_at": null, "name": null,
            "time_type": "service"}}]);
    let result = load(plans, times, good_items()).await.expect("loads");
    assert!(
        matches!(result, PlanDiscoveryResult::Loaded { .. }),
        "{result:?}"
    );
}

#[tokio::test]
async fn items_with_null_title_description_and_length_load() {
    let plans = json!([plan("p1", "Sunday")]);
    let items = json!([
        {"type": "Item", "id": "i1", "attributes": {"title": null, "description": null,
            "item_type": "header", "length": null, "sequence": 1, "service_position": null}},
        {"type": "Item", "id": "i2", "attributes": {"title": "Song", "item_type": "song",
            "length": 200, "sequence": 2}, "relationships": {"song": {"data": null}}},
        {"type": "Item", "id": "i3", "attributes": {"title": "Next", "item_type": "song",
            "length": 100, "sequence": 3}, "relationships": {}}
    ]);
    let result = load(plans, good_time(), items).await.expect("loads");
    assert!(
        matches!(result, PlanDiscoveryResult::Loaded { .. }),
        "{result:?}"
    );
}

#[tokio::test]
async fn an_empty_item_list_and_plans_without_service_times_are_not_errors() {
    let plans = json!([plan("p1", "Sunday")]);
    let result = load(plans, json!([]), json!([])).await.expect("no times");
    assert!(
        matches!(result, PlanDiscoveryResult::NotFound { .. }),
        "{result:?}"
    );
}

#[tokio::test]
async fn a_really_invalid_plan_is_still_rejected_without_leaking_values() {
    let plans = json!([{"type": "Plan", "id": "p1", "attributes": {"title": 12345}}]);
    let error = load(plans, good_time(), good_items()).await.unwrap_err();
    assert!(matches!(error, PcoError::Response(_)), "{error:?}");
    assert!(!error.to_string().contains("12345"));
}
