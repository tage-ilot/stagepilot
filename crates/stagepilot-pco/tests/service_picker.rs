//! The service date picker's backend half: the list of upcoming plans, the pin (a service the
//! operator picked instead of the upcoming one), and everything that keeps or ends it. The real
//! plugin and the real client run against an in-process Planning Center that answers by date.

mod support;

use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{json, Value};
use stagepilot_core::bus::EventBus;
use stagepilot_core::events::{new_event, EventPayload, EventType};
use stagepilot_core::plugin::Plugin;
use stagepilot_core::service_pin::{
    FileServicePinStore, PinKeeper, PlanPicker, ServicePin, NOTICE_PIN_PASSED, NOTICE_PIN_VANISHED,
    SOURCE_DEMO, SOURCE_PLANNING_CENTER,
};
use stagepilot_core::state::{ApplicationState, ServiceLoadStatus};
use stagepilot_core::state_service::StateService;
use stagepilot_core::store::StateStore;
use stagepilot_pco::client::default_factory;
use stagepilot_pco::models::PcoSettings;
use stagepilot_pco::plugin::{PcoPlugin, TodayProvider, PLUGIN_NAME};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use support::*;

const TZ: &str = "America/Los_Angeles";

#[derive(Clone)]
struct P {
    id: &'static str,
    title: &'static str,
    starts: &'static str,
    service_type: &'static str,
}

struct Cal {
    plans: Mutex<Vec<P>>,
    /// Plans whose songs cannot be read (a transient failure while loading one plan).
    broken: Mutex<Vec<&'static str>>,
    /// The plan lists fail (every service type).
    fail_lists: Mutex<bool>,
    sort_dates: bool,
}

impl Cal {
    fn standard(sort_dates: bool) -> Arc<Self> {
        let p = |id, title, starts, service_type| P {
            id,
            title,
            starts,
            service_type,
        };
        Arc::new(Self {
            plans: Mutex::new(vec![
                p("a", "Sunday A", "2026-07-12T17:00:00Z", "42"),
                p("b", "Sunday B", "2026-07-19T17:00:00Z", "42"),
                p("c", "Sunday C", "2026-07-26T17:00:00Z", "42"),
                p("far", "Sunday Far", "2026-09-06T17:00:00Z", "42"),
                // Past the 180 days the picker lists.
                p("later", "Sunday Later", "2027-02-01T17:00:00Z", "42"),
                // Wed 02:00 UTC is still Tuesday evening in Los Angeles.
                p("youth", "Youth Night", "2026-07-15T02:00:00Z", "7"),
            ]),
            broken: Mutex::new(vec![]),
            fail_lists: Mutex::new(false),
            sort_dates,
        })
    }

    fn remove(&self, id: &str) {
        self.plans.lock().unwrap().retain(|p| p.id != id);
    }

    fn add(&self, plan: P) {
        self.plans.lock().unwrap().push(plan);
    }
}

fn plan_id_of(path: &str) -> Option<&str> {
    let rest = path.split("/plans/").nth(1)?;
    rest.split('/').next()
}

async fn mock(cal: &Arc<Cal>) -> Mock {
    let cal = cal.clone();
    Mock::start(move |r| {
        let path = r.path.as_str();
        if path.ends_with("/service_types") {
            return Resp::json(
                200,
                page(vec![
                    service_type("7", "Youth", 2),
                    service_type("42", "Weekend Services", 1),
                ]),
            );
        }
        let plans = cal.plans.lock().unwrap().clone();
        if path.ends_with("/plans") {
            if *cal.fail_lists.lock().unwrap() {
                return Resp::json(500, json!({"errors": [{"detail": "boom test-secret"}]}));
            }
            let type_id = path
                .split("/service_types/")
                .nth(1)
                .and_then(|s| s.split('/').next())
                .unwrap_or("");
            let parse = |key: &str| -> DateTime<Utc> {
                DateTime::parse_from_rfc3339(r.query.get(key).expect("date filter"))
                    .unwrap()
                    .with_timezone(&Utc)
            };
            let (after, before) = (parse("after"), parse("before"));
            let data = plans
                .iter()
                .filter(|p| p.service_type == type_id)
                .filter(|p| {
                    let at = DateTime::parse_from_rfc3339(p.starts)
                        .unwrap()
                        .with_timezone(&Utc);
                    at > after && at < before
                })
                .map(|p| {
                    let mut v = plan(p.id, p.title);
                    if cal.sort_dates {
                        v["attributes"]["sort_date"] = json!(p.starts);
                    }
                    v
                })
                .collect();
            return Resp::json(200, page(data));
        }
        let Some(id) = plan_id_of(path) else {
            return Resp::json(404, json!({}));
        };
        let Some(found) = plans.iter().find(|p| p.id == id) else {
            return Resp::json(404, json!({}));
        };
        if path.ends_with("/plan_times") {
            Resp::json(
                200,
                page(vec![plan_time(&format!("t-{id}"), found.starts, "service")]),
            )
        } else if path.ends_with("/items") {
            if cal.broken.lock().unwrap().contains(&found.id) {
                return Resp::json(500, json!({"errors": [{"detail": "boom"}]}));
            }
            Resp::json(
                200,
                page(vec![item(
                    &format!("i-{id}"),
                    "song",
                    &format!("Song of {}", found.title),
                    1,
                    Some(200),
                    Some(&format!("s-{id}")),
                )]),
            )
        } else {
            Resp::json(404, json!({}))
        }
    })
    .await
}

fn settings(service_type: &str) -> PcoSettings {
    PcoSettings {
        service_type_id: Some(service_type.into()),
        ..pat_settings()
    }
}

/// A clock a test can move: the Planning Center time zone's date comes from this instant.
#[derive(Clone)]
struct Clock(Arc<Mutex<DateTime<Utc>>>);

impl Clock {
    fn at(rfc3339: &str) -> Self {
        Self(Arc::new(Mutex::new(
            DateTime::parse_from_rfc3339(rfc3339)
                .unwrap()
                .with_timezone(&Utc),
        )))
    }
    fn set(&self, rfc3339: &str) {
        *self.0.lock().unwrap() = DateTime::parse_from_rfc3339(rfc3339)
            .unwrap()
            .with_timezone(&Utc);
    }
    fn provider(&self) -> TodayProvider {
        let now = self.0.clone();
        Arc::new(move |tz| now.lock().unwrap().with_timezone(&tz).date_naive())
    }
}

struct Harness {
    plugin: PcoPlugin,
    bus: EventBus,
    store: StateStore,
    keeper: Arc<PinKeeper>,
    _state_service: StateService,
}

fn harness(mock: &Mock, settings: PcoSettings, clock: &Clock, keeper: Arc<PinKeeper>) -> Harness {
    let bus = EventBus::new();
    let store = StateStore::new(ApplicationState::default());
    let state_service = StateService::new(bus.clone(), store.clone(), 50, 50);
    state_service.start();
    let plugin = PcoPlugin::new(
        bus.clone(),
        store.clone(),
        settings,
        TZ,
        default_factory(Some(mock.url.clone())),
        Some(clock.provider()),
        None,
    )
    .expect("plugin builds");
    plugin.set_pin_keeper(keeper.clone());
    plugin.set_plan_list_ttl(Duration::ZERO);
    Harness {
        plugin,
        bus,
        store,
        keeper,
        _state_service: state_service,
    }
}

fn july_12() -> Clock {
    // 10:00 in Los Angeles.
    Clock::at("2026-07-12T17:00:00Z")
}

impl Harness {
    fn plan_id(&self) -> Option<String> {
        self.store.snapshot().plan.map(|p| p.id)
    }

    /// What the route does for a pick: save the pin, publish the selection event.
    async fn pick(&self, id: &str) {
        let list = self.plugin.upcoming_plans().await.expect("list");
        let plan = list.plans.iter().find(|p| p.id == id).expect("listed");
        self.plugin.set_pin(plan).unwrap();
        self.bus
            .publish(new_event(
                EventType::ServicePlanSelectionRequested,
                "api",
                EventPayload::ServicePlanSelection {
                    plan_id: id.to_string(),
                },
            ))
            .await;
    }

    /// What the route does for Back to upcoming.
    async fn back(&self) {
        self.plugin.clear_pin().unwrap();
        self.bus
            .publish(new_event(
                EventType::ServicePlanSelectionRequested,
                "api",
                EventPayload::ServicePlanSelection {
                    plan_id: String::new(),
                },
            ))
            .await;
    }
}

fn plans_calls(mock: &Mock) -> usize {
    mock.requests()
        .iter()
        .filter(|r| r.path.ends_with("/plans"))
        .count()
}

fn per_plan_calls(mock: &Mock) -> usize {
    mock.requests()
        .iter()
        .filter(|r| r.path.contains("/plans/"))
        .count()
}

// ---- the list ------------------------------------------------------------------------------

#[tokio::test]
async fn the_list_has_plans_from_today_to_180_days_by_date_one_call_per_service_type() {
    let cal = Cal::standard(true);
    let m = mock(&cal).await;
    let h = harness(&m, settings("42"), &july_12(), PinKeeper::in_memory());
    h.plugin.start().await.unwrap();
    let before = m.requests().len();
    let list = h.plugin.upcoming_plans().await.unwrap();
    let ids: Vec<&str> = list.plans.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["a", "b", "c", "far"],
        "dated by sort_date, no 2027 plan"
    );
    assert_eq!(list.upcoming_id.as_deref(), Some("a"));
    assert_eq!(list.pinned_id, None);
    assert_eq!(list.pinned_date, None);
    assert_eq!(list.plans[0].service_type_id, "42");
    assert_eq!(list.plans[0].service_type_name, "Weekend Services");
    let new: Vec<_> = m.requests().into_iter().skip(before).collect();
    let plan_lists = new.iter().filter(|r| r.path.ends_with("/plans")).count();
    assert_eq!(plan_lists, 1, "one plans call for the one service type");
    assert!(
        !new.iter().any(|r| r.path.contains("/plans/")),
        "no per-plan call when Planning Center says the date"
    );
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn a_plan_without_a_sort_date_is_dated_by_its_service_times() {
    let cal = Cal::standard(false);
    let m = mock(&cal).await;
    let h = harness(&m, settings("42"), &july_12(), PinKeeper::in_memory());
    h.plugin.start().await.unwrap();
    let list = h.plugin.upcoming_plans().await.unwrap();
    let ids: Vec<&str> = list.plans.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, vec!["a", "b", "c", "far"]);
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn all_service_types_lists_every_type_and_dates_in_the_configured_zone() {
    let cal = Cal::standard(true);
    let m = mock(&cal).await;
    let h = harness(
        &m,
        settings("stagepilot:all-service-types"),
        &july_12(),
        PinKeeper::in_memory(),
    );
    h.plugin.start().await.unwrap();
    let before = plans_calls(&m);
    let list = h.plugin.upcoming_plans().await.unwrap();
    let got: Vec<(&str, String)> = list
        .plans
        .iter()
        .map(|p| (p.id.as_str(), p.date.to_string()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("a", "2026-07-12".to_string()),
            // Wed 02:00 UTC is Tuesday in Los Angeles.
            ("youth", "2026-07-14".to_string()),
            ("b", "2026-07-19".to_string()),
            ("c", "2026-07-26".to_string()),
            ("far", "2026-09-06".to_string()),
        ]
    );
    assert_eq!(plans_calls(&m) - before, 2, "one call per service type");
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn the_list_is_cached_for_a_while_and_refetched_after() {
    let cal = Cal::standard(true);
    let m = mock(&cal).await;
    let h = harness(&m, settings("42"), &july_12(), PinKeeper::in_memory());
    h.plugin.start().await.unwrap();
    h.plugin.set_plan_list_ttl(Duration::from_secs(60));
    h.plugin.upcoming_plans().await.unwrap();
    let after_first = m.requests().len();
    h.plugin.upcoming_plans().await.unwrap();
    h.plugin.upcoming_plans().await.unwrap();
    assert_eq!(m.requests().len(), after_first, "answered from the cache");
    h.plugin.set_plan_list_ttl(Duration::ZERO);
    h.plugin.upcoming_plans().await.unwrap();
    assert!(m.requests().len() > after_first, "asked again");
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn a_failing_list_is_a_message_and_never_touches_the_loaded_plan() {
    let cal = Cal::standard(true);
    let m = mock(&cal).await;
    let h = harness(&m, settings("42"), &july_12(), PinKeeper::in_memory());
    h.plugin.start().await.unwrap();
    assert_eq!(h.plan_id().as_deref(), Some("a"));
    *cal.fail_lists.lock().unwrap() = true;
    let error = h.plugin.upcoming_plans().await.unwrap_err();
    assert!(!error.is_empty());
    assert!(!error.contains("test-secret"), "{error}");
    assert_eq!(h.plan_id().as_deref(), Some("a"), "the loaded plan stays");
    assert_eq!(
        h.store.snapshot().service_load.status,
        ServiceLoadStatus::Loaded
    );
    *cal.fail_lists.lock().unwrap() = false;
    assert!(h.plugin.upcoming_plans().await.is_ok(), "and it recovers");
    h.plugin.stop().await.unwrap();
}

// ---- the pin ---------------------------------------------------------------------------------

#[tokio::test]
async fn picking_a_plan_loads_it_like_the_upcoming_one_and_back_returns() {
    let cal = Cal::standard(true);
    let m = mock(&cal).await;
    let h = harness(&m, settings("42"), &july_12(), PinKeeper::in_memory());
    h.plugin.start().await.unwrap();
    let state = h.store.snapshot();
    // Nothing is picked: the state says nothing about it (older states look the same).
    assert!(state.selection.is_none());
    assert!(state.upcoming.is_none());

    h.pick("c").await;
    let state = h.store.snapshot();
    let plan = state.plan.expect("plan");
    assert_eq!(plan.id, "c");
    assert_eq!(plan.date.to_string(), "2026-07-26");
    assert_eq!(
        plan.songs[0].title, "Song of Sunday C",
        "the pinned plan's own songs"
    );
    assert_eq!(state.service_load.status, ServiceLoadStatus::Loaded);
    assert_eq!(state.service_load.target_date, Some(plan.date));
    let selection = state.selection.expect("selection");
    assert!(selection.pinned);
    assert_eq!(selection.plan_id.as_deref(), Some("c"));
    assert_eq!(
        selection.date.map(|d| d.to_string()).as_deref(),
        Some("2026-07-26")
    );
    let upcoming = state.upcoming.expect("the upcoming service is still known");
    assert_eq!(
        (upcoming.plan_id.as_str(), upcoming.title.as_str()),
        ("a", "Sunday A")
    );
    let list = h.plugin.upcoming_plans().await.unwrap();
    assert_eq!(list.pinned_id.as_deref(), Some("c"));
    assert_eq!(
        list.pinned_date.map(|d| d.to_string()).as_deref(),
        Some("2026-07-26")
    );
    assert_eq!(list.upcoming_id.as_deref(), Some("a"));

    h.back().await;
    let state = h.store.snapshot();
    assert_eq!(state.plan.map(|p| p.id).as_deref(), Some("a"));
    assert!(state.selection.is_none() && state.upcoming.is_none());
    assert_eq!(h.keeper.get(SOURCE_PLANNING_CENTER), None);
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn a_pin_beyond_the_upcoming_search_window_loads() {
    let cal = Cal::standard(true);
    let m = mock(&cal).await;
    let h = harness(&m, settings("42"), &july_12(), PinKeeper::in_memory());
    h.plugin.start().await.unwrap();
    h.pick("far").await;
    let state = h.store.snapshot();
    assert_eq!(state.plan.map(|p| p.id).as_deref(), Some("far"));
    assert_eq!(state.service_load.status, ServiceLoadStatus::Loaded);
    assert!(state.selection.unwrap().pinned);
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn the_pin_stays_across_plan_reloads_and_restarts() {
    let cal = Cal::standard(true);
    let m = mock(&cal).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("service-pin.json");
    let keeper = PinKeeper::new(Arc::new(FileServicePinStore::new(&path)));
    let h = harness(&m, settings("42"), &july_12(), keeper);
    h.plugin.start().await.unwrap();
    h.pick("b").await;
    h.plugin.refresh_now().await;
    h.bus
        .publish(new_event(
            EventType::ServiceReloadRequested,
            "test",
            EventPayload::Empty,
        ))
        .await;
    h.plugin.refresh_now().await;
    assert_eq!(h.plan_id().as_deref(), Some("b"), "a reload keeps the pick");
    h.plugin.stop().await.unwrap();
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved["plan_id"], "b");
    assert_eq!(saved["date"], "2026-07-19");
    assert_eq!(saved["source"], "planning_center");

    // A new run reads the same file.
    let again = harness(
        &m,
        settings("42"),
        &july_12(),
        PinKeeper::new(Arc::new(FileServicePinStore::new(&path))),
    );
    again.plugin.start().await.unwrap();
    assert_eq!(
        again.plan_id().as_deref(),
        Some("b"),
        "still picked after a restart"
    );
    assert!(again.store.snapshot().selection.unwrap().pinned);
    again.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn a_pin_for_today_holds_at_23_59_and_ends_at_midnight() {
    let cal = Cal::standard(true);
    // A second service the same day, so a plan dated today can be picked while another is the
    // upcoming one (the title preference makes "Sunday A" the upcoming one).
    cal.add(P {
        id: "a2",
        title: "Evening Service",
        starts: "2026-07-13T02:00:00Z",
        service_type: "42",
    });
    let m = mock(&cal).await;
    let clock = july_12();
    let mut s = settings("42");
    s.plan_title_preference = Some("Sunday A".into());
    let h = harness(&m, s, &clock, PinKeeper::in_memory());
    h.plugin.start().await.unwrap();
    assert_eq!(h.plan_id().as_deref(), Some("a"));
    h.pick("a2").await;
    assert_eq!(h.plan_id().as_deref(), Some("a2"));
    assert!(h.store.snapshot().selection.unwrap().pinned);

    // Sunday 23:59 in Los Angeles: still today, the pin holds through a reload.
    clock.set("2026-07-13T06:59:00Z");
    h.plugin.refresh_now().await;
    assert_eq!(h.plan_id().as_deref(), Some("a2"));
    assert!(h.store.snapshot().selection.unwrap().pinned);
    assert!(h.keeper.get(SOURCE_PLANNING_CENTER).is_some());

    // Monday 00:00: the date has passed; the upcoming plan loads and a note says so, once.
    clock.set("2026-07-13T07:00:00Z");
    h.plugin.refresh_now().await;
    let state = h.store.snapshot();
    assert_eq!(
        state.plan.map(|p| p.id).as_deref(),
        Some("b"),
        "next Sunday is upcoming"
    );
    let selection = state.selection.unwrap();
    assert!(!selection.pinned);
    assert_eq!(selection.notice.as_deref(), Some(NOTICE_PIN_PASSED));
    assert_eq!(h.keeper.get(SOURCE_PLANNING_CENTER), None);
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn the_clock_check_ends_a_pin_by_itself_at_midnight() {
    let cal = Cal::standard(true);
    cal.add(P {
        id: "a2",
        title: "Evening Service",
        starts: "2026-07-13T02:00:00Z",
        service_type: "42",
    });
    let m = mock(&cal).await;
    let clock = july_12();
    let mut s = settings("42");
    s.plan_title_preference = Some("Sunday A".into());
    let h = harness(&m, s, &clock, PinKeeper::in_memory());
    h.plugin
        .set_expiry_check_interval(Duration::from_millis(20));
    h.plugin.start().await.unwrap();
    h.pick("a2").await;
    assert_eq!(h.plan_id().as_deref(), Some("a2"));
    clock.set("2026-07-13T07:00:00Z");
    let mut loaded = None;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(30)).await;
        loaded = h.plan_id();
        if loaded.as_deref() == Some("b") {
            break;
        }
    }
    assert_eq!(loaded.as_deref(), Some("b"), "no reload was asked for");
    assert_eq!(
        h.store.snapshot().selection.unwrap().notice.as_deref(),
        Some(NOTICE_PIN_PASSED)
    );
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn a_plan_that_is_gone_from_planning_center_drops_the_pin_with_a_notice() {
    let cal = Cal::standard(true);
    let m = mock(&cal).await;
    let h = harness(&m, settings("42"), &july_12(), PinKeeper::in_memory());
    h.plugin.start().await.unwrap();
    h.pick("c").await;
    assert_eq!(h.plan_id().as_deref(), Some("c"));
    cal.remove("c");
    h.plugin.refresh_now().await;
    let state = h.store.snapshot();
    assert_eq!(
        state.plan.map(|p| p.id).as_deref(),
        Some("a"),
        "the upcoming plan loads"
    );
    let selection = state.selection.unwrap();
    assert!(!selection.pinned);
    assert_eq!(selection.notice.as_deref(), Some(NOTICE_PIN_VANISHED));
    assert_eq!(
        h.keeper.get(SOURCE_PLANNING_CENTER),
        None,
        "the pin is cleared"
    );
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn a_failed_load_keeps_the_pin_and_the_loaded_plan() {
    let cal = Cal::standard(true);
    let m = mock(&cal).await;
    let h = harness(&m, settings("42"), &july_12(), PinKeeper::in_memory());
    h.plugin.start().await.unwrap();
    h.pick("b").await;
    assert_eq!(h.plan_id().as_deref(), Some("b"));
    // Planning Center fails while reading the pinned plan's songs: a hiccup, not a missing plan.
    cal.broken.lock().unwrap().push("b");
    h.plugin.refresh_now().await;
    let state = h.store.snapshot();
    assert_eq!(
        state.plan.map(|p| p.id).as_deref(),
        Some("b"),
        "the loaded plan stays"
    );
    assert_eq!(state.service_load.status, ServiceLoadStatus::Error);
    assert!(state.service_load.is_stale);
    assert!(
        h.keeper.get(SOURCE_PLANNING_CENTER).is_some(),
        "the pin stays"
    );
    cal.broken.lock().unwrap().clear();
    h.bus
        .publish(new_event(
            EventType::ServiceReloadRequested,
            "test",
            EventPayload::Empty,
        ))
        .await;
    for _ in 0..100 {
        if h.store.snapshot().service_load.status == ServiceLoadStatus::Loaded {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(h.plan_id().as_deref(), Some("b"));
    assert_eq!(
        h.store.snapshot().service_load.status,
        ServiceLoadStatus::Loaded
    );
    assert!(h.store.snapshot().selection.unwrap().pinned);
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn several_plans_on_one_date_are_resolved_by_picking_one_by_id() {
    let cal = Cal::standard(true);
    cal.add(P {
        id: "a2",
        title: "Evening Service",
        starts: "2026-07-13T02:00:00Z",
        service_type: "42",
    });
    let m = mock(&cal).await;
    let h = harness(&m, settings("42"), &july_12(), PinKeeper::in_memory());
    h.plugin.start().await.unwrap();
    // Without a preference two plans today are a question for the operator, as before.
    let state = h.store.snapshot();
    assert_eq!(state.service_load.status, ServiceLoadStatus::Ambiguous);
    assert!(state.plan.is_none());
    let list = h.plugin.upcoming_plans().await.unwrap();
    assert_eq!(list.upcoming_id, None, "no single upcoming plan to name");
    assert_eq!(
        list.plans
            .iter()
            .filter(|p| p.date.to_string() == "2026-07-12")
            .count(),
        2
    );

    h.pick("a2").await;
    let state = h.store.snapshot();
    assert_eq!(state.plan.map(|p| p.id).as_deref(), Some("a2"));
    assert_eq!(state.service_load.status, ServiceLoadStatus::Loaded);
    assert!(state.selection.unwrap().pinned);

    h.back().await;
    assert_eq!(
        h.store.snapshot().service_load.status,
        ServiceLoadStatus::Ambiguous,
        "back to the operator's own question"
    );
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn a_damaged_pin_file_is_ignored_and_loading_goes_on() {
    let cal = Cal::standard(true);
    let m = mock(&cal).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("service-pin.json");
    std::fs::write(&path, "{ this is not json").unwrap();
    let h = harness(
        &m,
        settings("42"),
        &july_12(),
        PinKeeper::new(Arc::new(FileServicePinStore::new(&path))),
    );
    h.plugin.start().await.unwrap();
    assert_eq!(h.plan_id().as_deref(), Some("a"));
    assert!(h.store.snapshot().selection.is_none());
    assert_eq!(h.plugin.pin(), None);
    // A new pick replaces the damaged file.
    h.pick("b").await;
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved["plan_id"], "b");
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn a_pin_from_another_source_or_service_type_is_not_this_plugins_pick() {
    let cal = Cal::standard(true);
    let m = mock(&cal).await;
    // A demo pin is ignored here (and kept for the demo).
    let keeper = PinKeeper::in_memory();
    keeper
        .set(ServicePin::new(
            SOURCE_DEMO,
            "b",
            NaiveDate::from_ymd_opt(2026, 7, 19).unwrap(),
            None,
            None,
        ))
        .unwrap();
    let h = harness(&m, settings("42"), &july_12(), keeper.clone());
    h.plugin.start().await.unwrap();
    assert_eq!(h.plan_id().as_deref(), Some("a"));
    assert!(keeper.get(SOURCE_DEMO).is_some());
    h.plugin.stop().await.unwrap();

    // A pick made under another service type does not apply once the setting names another.
    let keeper = PinKeeper::in_memory();
    keeper
        .set(ServicePin::new(
            SOURCE_PLANNING_CENTER,
            "b",
            NaiveDate::from_ymd_opt(2026, 7, 19).unwrap(),
            Some("7".into()),
            None,
        ))
        .unwrap();
    let h = harness(&m, settings("42"), &july_12(), keeper.clone());
    h.plugin.start().await.unwrap();
    assert_eq!(h.plan_id().as_deref(), Some("a"));
    assert_eq!(
        h.store.snapshot().selection.unwrap().notice.as_deref(),
        Some(NOTICE_PIN_VANISHED)
    );
    assert_eq!(keeper.get(SOURCE_PLANNING_CENTER), None);
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn the_pinned_plan_is_loaded_with_the_calls_the_upcoming_plan_takes() {
    let cal = Cal::standard(true);
    let m = mock(&cal).await;
    let h = harness(&m, settings("42"), &july_12(), PinKeeper::in_memory());
    h.plugin.start().await.unwrap();
    let before = per_plan_calls(&m);
    h.pick("b").await;
    let calls: Vec<String> = m
        .requests()
        .into_iter()
        .skip(before)
        .map(|r| r.path)
        .filter(|p| p.contains("/plans/b/"))
        .collect();
    assert!(calls.iter().any(|p| p.ends_with("/plan_times")));
    assert!(calls.iter().any(|p| p.ends_with("/items")));
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn the_upcoming_plan_picked_is_not_a_pin_once_it_is_the_upcoming_one() {
    // A pin whose date has become the upcoming plan's date is just the upcoming service.
    let cal = Cal::standard(true);
    let m = mock(&cal).await;
    let clock = july_12();
    let h = harness(&m, settings("42"), &clock, PinKeeper::in_memory());
    h.plugin.start().await.unwrap();
    h.pick("b").await;
    assert!(h.store.snapshot().selection.unwrap().pinned);
    clock.set("2026-07-14T17:00:00Z");
    h.plugin.refresh_now().await;
    let state = h.store.snapshot();
    assert_eq!(state.plan.map(|p| p.id).as_deref(), Some("b"));
    assert!(
        state.selection.is_none(),
        "b is the upcoming service now, so nothing is 'not the upcoming service'"
    );
    h.plugin.stop().await.unwrap();
}

// ---- reviewing a service of the days gone by ---------------------------------------------------

/// The standard calendar with a service 7 days before July 12, one exactly 20 days before, and
/// one 21 days before (the last is out of the picker's reach).
fn with_past(cal: &Arc<Cal>) {
    let past = |id, title, starts| P {
        id,
        title,
        starts,
        service_type: "42",
    };
    cal.add(past("p7", "Sunday Past", "2026-07-05T17:00:00Z"));
    cal.add(past("p20", "Sunday Twenty", "2026-06-22T17:00:00Z"));
    cal.add(past("p21", "Sunday Too Old", "2026-06-21T17:00:00Z"));
}

#[tokio::test]
async fn the_list_reaches_back_20_days_and_the_upcoming_service_is_unchanged() {
    let cal = Cal::standard(true);
    with_past(&cal);
    let m = mock(&cal).await;
    let h = harness(&m, settings("42"), &july_12(), PinKeeper::in_memory());
    h.plugin.start().await.unwrap();
    assert_eq!(h.plan_id().as_deref(), Some("a"), "the upcoming plan loads");
    let before = plans_calls(&m);
    let list = h.plugin.upcoming_plans().await.unwrap();
    let ids: Vec<&str> = list.plans.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["p20", "p7", "a", "b", "c", "far"],
        "20 days back is listed, 21 is not, oldest first"
    );
    assert_eq!(list.upcoming_id.as_deref(), Some("a"));
    assert_eq!(
        plans_calls(&m) - before,
        1,
        "still one call per service type"
    );
    assert_eq!(list.pinned_id, None);
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn the_upcoming_service_is_the_nearest_from_today_not_the_oldest_past_one() {
    // Nothing is known as upcoming from the loaded plan (the pick is a past plan and the state
    // knows no upcoming one): the answer comes from the first plan dated today or later.
    let cal = Cal::standard(true);
    with_past(&cal);
    let m = mock(&cal).await;
    let h = harness(&m, settings("42"), &july_12(), PinKeeper::in_memory());
    h.plugin.start().await.unwrap();
    h.store.mutate(|s| s.upcoming = None);
    let list = h.plugin.upcoming_plans().await.unwrap();
    assert_eq!(list.upcoming_id.as_deref(), Some("a"));
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn a_busy_calendar_never_pushes_an_upcoming_plan_out_of_the_list() {
    let cal = Cal::standard(true);
    with_past(&cal);
    // 70 services on the 70 days after today, more than the cap of 60 plans from today on.
    for day in 0..70 {
        let id: &'static str = Box::leak(format!("x{day:02}").into_boxed_str());
        let starts: &'static str = Box::leak(
            format!(
                "2026-{}T18:00:00Z",
                (NaiveDate::from_ymd_opt(2026, 7, 13).unwrap() + chrono::Duration::days(day))
                    .format("%m-%d")
            )
            .into_boxed_str(),
        );
        cal.add(P {
            id,
            title: "Daily",
            starts,
            service_type: "42",
        });
    }
    let m = mock(&cal).await;
    let h = harness(&m, settings("42"), &july_12(), PinKeeper::in_memory());
    h.plugin.start().await.unwrap();
    let list = h.plugin.upcoming_plans().await.unwrap();
    let ids: Vec<&str> = list.plans.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids.len(), 62, "the past two on top of the 60 from today on");
    assert_eq!(&ids[..3], ["p20", "p7", "a"]);
    assert!(
        ids.iter().filter(|id| ["p20", "p7"].contains(id)).count() == 2,
        "{ids:?}"
    );
    assert_eq!(list.upcoming_id.as_deref(), Some("a"));
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn a_past_plan_loads_for_review_and_holds_until_the_end_of_the_day_it_was_picked() {
    let cal = Cal::standard(true);
    with_past(&cal);
    let m = mock(&cal).await;
    let clock = july_12();
    let h = harness(&m, settings("42"), &clock, PinKeeper::in_memory());
    h.plugin.start().await.unwrap();
    h.pick("p7").await;
    let state = h.store.snapshot();
    let plan = state.plan.clone().expect("plan");
    assert_eq!(
        (plan.id.as_str(), plan.date.to_string().as_str()),
        ("p7", "2026-07-05")
    );
    assert_eq!(plan.songs[0].title, "Song of Sunday Past");
    assert_eq!(state.service_load.status, ServiceLoadStatus::Loaded);
    let selection = state.selection.expect("selection");
    assert!(selection.pinned);
    assert_eq!(selection.plan_id.as_deref(), Some("p7"));
    assert_eq!(state.upcoming.expect("upcoming").plan_id, "a");
    let pin = h.keeper.get(SOURCE_PLANNING_CENTER).expect("pin");
    assert_eq!(pin.date.to_string(), "2026-07-05");
    assert_eq!(
        pin.held_until.map(|d| d.to_string()).as_deref(),
        Some("2026-07-12")
    );
    let list = h.plugin.upcoming_plans().await.unwrap();
    assert_eq!(list.pinned_id.as_deref(), Some("p7"));
    assert_eq!(list.upcoming_id.as_deref(), Some("a"));

    // A reload during the day keeps the review loaded.
    clock.set("2026-07-13T06:59:00Z");
    h.plugin.refresh_now().await;
    let state = h.store.snapshot();
    assert_eq!(state.plan.map(|p| p.id).as_deref(), Some("p7"));
    assert_eq!(state.service_load.status, ServiceLoadStatus::Loaded);
    assert!(state.selection.unwrap().pinned);
    assert!(h.keeper.get(SOURCE_PLANNING_CENTER).is_some());

    // The next local midnight: back to the upcoming service, with the same note as any pin.
    clock.set("2026-07-13T07:00:00Z");
    h.plugin.refresh_now().await;
    let state = h.store.snapshot();
    assert_eq!(state.plan.map(|p| p.id).as_deref(), Some("b"));
    let selection = state.selection.unwrap();
    assert!(!selection.pinned);
    assert_eq!(selection.notice.as_deref(), Some(NOTICE_PIN_PASSED));
    assert_eq!(h.keeper.get(SOURCE_PLANNING_CENTER), None);
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn the_clock_check_drops_a_review_at_midnight_without_a_reload() {
    let cal = Cal::standard(true);
    with_past(&cal);
    let m = mock(&cal).await;
    let clock = july_12();
    let h = harness(&m, settings("42"), &clock, PinKeeper::in_memory());
    h.plugin
        .set_expiry_check_interval(Duration::from_millis(20));
    h.plugin.start().await.unwrap();
    h.pick("p7").await;
    assert_eq!(h.plan_id().as_deref(), Some("p7"));
    clock.set("2026-07-13T07:00:00Z");
    let mut loaded = None;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(30)).await;
        loaded = h.plan_id();
        if loaded.as_deref() == Some("b") {
            break;
        }
    }
    assert_eq!(loaded.as_deref(), Some("b"), "no reload was asked for");
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn a_review_survives_a_restart_the_same_day_and_not_the_next() {
    let cal = Cal::standard(true);
    with_past(&cal);
    let m = mock(&cal).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("service-pin.json");
    let keeper = || PinKeeper::new(Arc::new(FileServicePinStore::new(&path)));
    let first = harness(&m, settings("42"), &july_12(), keeper());
    first.plugin.start().await.unwrap();
    first.pick("p7").await;
    first.plugin.stop().await.unwrap();

    let same_day = harness(
        &m,
        settings("42"),
        &Clock::at("2026-07-12T20:00:00Z"),
        keeper(),
    );
    same_day.plugin.start().await.unwrap();
    assert_eq!(same_day.plan_id().as_deref(), Some("p7"));
    assert!(same_day.store.snapshot().selection.unwrap().pinned);
    same_day.plugin.stop().await.unwrap();

    let next_day = harness(
        &m,
        settings("42"),
        &Clock::at("2026-07-13T08:00:00Z"),
        keeper(),
    );
    next_day.plugin.start().await.unwrap();
    assert_eq!(next_day.plan_id().as_deref(), Some("b"));
    assert_eq!(
        next_day
            .store
            .snapshot()
            .selection
            .unwrap()
            .notice
            .as_deref(),
        Some(NOTICE_PIN_PASSED)
    );
    next_day.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn back_to_upcoming_ends_a_review() {
    let cal = Cal::standard(true);
    with_past(&cal);
    let m = mock(&cal).await;
    let h = harness(&m, settings("42"), &july_12(), PinKeeper::in_memory());
    h.plugin.start().await.unwrap();
    h.pick("p20").await;
    assert_eq!(h.plan_id().as_deref(), Some("p20"));
    h.back().await;
    let state = h.store.snapshot();
    assert_eq!(state.plan.map(|p| p.id).as_deref(), Some("a"));
    assert!(state.selection.is_none() && state.upcoming.is_none());
    assert_eq!(h.keeper.get(SOURCE_PLANNING_CENTER), None);
    h.plugin.stop().await.unwrap();
}

#[tokio::test]
async fn a_pin_of_a_past_plan_from_an_older_file_expires_as_before() {
    // An older build wrote no `held_until`: a past date is a pin that has already ended.
    let cal = Cal::standard(true);
    with_past(&cal);
    let m = mock(&cal).await;
    let keeper = PinKeeper::in_memory();
    keeper
        .set(ServicePin::new(
            SOURCE_PLANNING_CENTER,
            "p7",
            NaiveDate::from_ymd_opt(2026, 7, 5).unwrap(),
            Some("42".into()),
            None,
        ))
        .unwrap();
    let h = harness(&m, settings("42"), &july_12(), keeper);
    h.plugin.start().await.unwrap();
    assert_eq!(h.plan_id().as_deref(), Some("a"));
    assert_eq!(h.keeper.get(SOURCE_PLANNING_CENTER), None);
    h.plugin.stop().await.unwrap();
}

#[test]
fn the_plugin_name_is_the_pin_source() {
    assert_eq!(PLUGIN_NAME, SOURCE_PLANNING_CENTER);
}
