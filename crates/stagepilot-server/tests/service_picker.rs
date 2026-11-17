//! HTTP tests of the service date picker routes: `GET /api/v1/planning-center/plans/upcoming`,
//! `POST` and `DELETE /api/v1/planning-center/plans/pin`. One run against the demo Services
//! source (the preview backend) and one against a fake Planning Center.

use axum::body::Body;
use axum::extract::{ConnectInfo, Request as AxumRequest, State};
use axum::http::{header, Request, StatusCode};
use axum::response::Response;
use chrono::{Datelike, Duration as ChronoDuration, Utc, Weekday};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use stagepilot_core::config::{IntegrationModes, ServiceSource};
use stagepilot_core::service_pin::{
    FileServicePinStore, PinKeeper, ServicePin, ServicePinStore, SOURCE_DEMO,
};
use stagepilot_core::settings::{
    PersistentPlanningCenterSettings, PersistentSettings, RuntimeSettings, SettingsService,
};
use stagepilot_core::state::{ConnectionStatus, TimerStatus};
use stagepilot_pco::runtime::PcoOptions;
use stagepilot_server::{build_app, AppOptions, Runtime};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::ServiceExt;
use url::Url;

const LIST: &str = "/api/v1/planning-center/plans/upcoming";
const PIN: &str = "/api/v1/planning-center/plans/pin";

struct Rig {
    router: axum::Router,
    runtime: Arc<Runtime>,
}

impl Rig {
    async fn call(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let builder = Request::builder().method(method).uri(path);
        let mut request = match body {
            Some(b) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(b.to_string())),
            None => builder.body(Body::empty()),
        }
        .unwrap();
        request.extensions_mut().insert(ConnectInfo(
            "127.0.0.1:50000".parse::<SocketAddr>().unwrap(),
        ));
        let response = self.router.clone().oneshot(request).await.unwrap();
        let (parts, body) = response.into_parts();
        let bytes = body.collect().await.unwrap().to_bytes();
        (
            parts.status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    fn plan_id(&self) -> Option<String> {
        self.runtime.store.snapshot().plan.map(|p| p.id)
    }
}

// ---- the demo source -------------------------------------------------------------------------

async fn demo_rig(pins: Option<Arc<dyn ServicePinStore>>) -> Rig {
    let service = SettingsService::ephemeral(RuntimeSettings::demo());
    let options = PcoOptions {
        memory_tokens: true,
        pin_store: pins,
        ..PcoOptions::default()
    };
    let runtime = Runtime::with_pco(Arc::new(service), false, options);
    runtime.start().await;
    Rig {
        router: build_app(runtime.clone(), AppOptions { web_root: None }),
        runtime,
    }
}

#[tokio::test]
async fn the_demo_offers_weekly_sunday_plans_with_their_own_titles_and_songs() {
    let rig = demo_rig(None).await;
    let (status, list) = rig.call("GET", LIST, None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let plans = list["plans"].as_array().unwrap();
    assert!(plans.len() >= 7, "today's plan and six Sundays: {list}");
    assert_eq!(list["upcoming_id"], "demo-service-plan");
    assert!(list["pinned_id"].is_null() && list["pinned_date"].is_null());
    let sundays: Vec<&Value> = plans.iter().skip(1).collect();
    assert_eq!(sundays.len(), 6);
    for plan in &sundays {
        let date =
            chrono::NaiveDate::parse_from_str(plan["date"].as_str().unwrap(), "%Y-%m-%d").unwrap();
        assert_eq!(date.weekday(), Weekday::Sun);
        assert!(date > Utc::now().date_naive() - ChronoDuration::days(1));
    }
    let titles: std::collections::BTreeSet<&str> =
        plans.iter().map(|p| p["title"].as_str().unwrap()).collect();
    assert_eq!(titles.len(), plans.len(), "every plan has its own title");
    let dates: Vec<&str> = plans.iter().map(|p| p["date"].as_str().unwrap()).collect();
    let mut sorted = dates.clone();
    sorted.sort();
    assert_eq!(dates, sorted, "by date");
    for key in [
        "id",
        "date",
        "title",
        "service_type_id",
        "service_type_name",
    ] {
        assert!(plans[0].get(key).is_some(), "{key}");
    }
}

#[tokio::test]
async fn picking_a_demo_sunday_pins_it_and_back_to_upcoming_unpins() {
    let rig = demo_rig(None).await;
    let (_, list) = rig.call("GET", LIST, None).await;
    let second = list["plans"][2]["id"].as_str().unwrap().to_string();
    let second_title = list["plans"][2]["title"].as_str().unwrap().to_string();

    let (status, body) = rig
        .call("POST", PIN, Some(json!({"plan_id": second})))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["accepted"], true);
    assert_eq!(body["message"], format!("Loaded \"{second_title}\"."));
    assert_eq!(body["state"]["plan"]["id"], second.as_str());
    assert_eq!(body["state"]["selection"]["pinned"], true);
    assert_eq!(body["state"]["selection"]["plan_id"], second.as_str());
    assert_eq!(body["state"]["upcoming"]["plan_id"], "demo-service-plan");
    assert_eq!(rig.plan_id().as_deref(), Some(second.as_str()));
    // Everything that follows state.plan follows the pick: songs, current/next.
    let state = rig.runtime.store.snapshot();
    assert_eq!(
        state.next_song.as_ref(),
        state.plan.as_ref().unwrap().songs.first()
    );
    assert_eq!(state.current_song, None);

    let (_, list) = rig.call("GET", LIST, None).await;
    assert_eq!(list["pinned_id"], second.as_str());
    assert_eq!(list["pinned_date"], list["plans"][2]["date"]);
    assert_eq!(list["upcoming_id"], "demo-service-plan");

    // A reload keeps the pick.
    let (status, _) = rig
        .call("POST", "/api/v1/planning-center/plan/reload", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rig.plan_id().as_deref(), Some(second.as_str()));

    let (status, body) = rig.call("DELETE", PIN, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Back to the upcoming service.");
    assert_eq!(body["state"]["plan"]["id"], "demo-service-plan");
    assert_eq!(body["state"]["selection"]["pinned"], false);
    let (_, list) = rig.call("GET", LIST, None).await;
    assert!(list["pinned_id"].is_null());

    // Again is harmless.
    let (status, body) = rig.call("DELETE", PIN, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"]["plan"]["id"], "demo-service-plan");
}

#[tokio::test]
async fn picking_the_upcoming_plan_is_going_back_to_it() {
    let rig = demo_rig(None).await;
    let (_, list) = rig.call("GET", LIST, None).await;
    let second = list["plans"][1]["id"].as_str().unwrap().to_string();
    rig.call("POST", PIN, Some(json!({"plan_id": second})))
        .await;
    assert_eq!(rig.plan_id().as_deref(), Some(second.as_str()));
    let (status, body) = rig
        .call("POST", PIN, Some(json!({"plan_id": "demo-service-plan"})))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(rig.plan_id().as_deref(), Some("demo-service-plan"));
    assert_eq!(body["state"]["selection"]["pinned"], false);
    let (_, list) = rig.call("GET", LIST, None).await;
    assert!(
        list["pinned_id"].is_null(),
        "no pin is kept for the upcoming plan"
    );
}

#[tokio::test]
async fn a_pick_is_validated_like_a_plan_selection() {
    let rig = demo_rig(None).await;
    let (status, body) = rig
        .call("POST", PIN, Some(json!({"plan_id": "nope-not-listed"})))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body["detail"],
        "That service is not in the list of upcoming services."
    );
    // A plan dated before today is not in the list either.
    let yesterday = (Utc::now().date_naive() - ChronoDuration::days(2)).to_string();
    let (status, _) = rig
        .call(
            "POST",
            PIN,
            Some(json!({"plan_id": format!("demo-plan-{yesterday}")})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    for bad in [
        json!({"plan_id": "../x"}),
        json!({}),
        json!({"plan_id": ""}),
    ] {
        let (status, body) = rig.call("POST", PIN, Some(bad.clone())).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad} {body}");
    }
    assert_eq!(rig.plan_id().as_deref(), Some("demo-service-plan"));
}

#[tokio::test]
async fn the_service_cannot_be_changed_while_the_timer_runs() {
    let rig = demo_rig(None).await;
    let (_, list) = rig.call("GET", LIST, None).await;
    let second = list["plans"][1]["id"].as_str().unwrap().to_string();
    rig.runtime
        .store
        .mutate(|s| s.timer.status = TimerStatus::Running);
    let (status, body) = rig
        .call("POST", PIN, Some(json!({"plan_id": second})))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body["detail"],
        "Stop the timer before changing the service."
    );
    let (status, _) = rig.call("DELETE", PIN, None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(rig.plan_id().as_deref(), Some("demo-service-plan"));
    rig.runtime
        .store
        .mutate(|s| s.timer.status = TimerStatus::Stopped);
    let (status, _) = rig
        .call("POST", PIN, Some(json!({"plan_id": second})))
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_pin_in_the_data_directory_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("service-pin.json");
    let first = demo_rig(Some(Arc::new(FileServicePinStore::new(&path)))).await;
    let (_, list) = first.call("GET", LIST, None).await;
    let third = list["plans"][3]["id"].as_str().unwrap().to_string();
    first
        .call("POST", PIN, Some(json!({"plan_id": third})))
        .await;
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved["plan_id"], third.as_str());
    assert_eq!(saved["source"], "demo");
    assert!(
        !dir.path().join("settings.json").exists(),
        "the pin is not a settings key"
    );
    first.runtime.stop().await;

    let second = demo_rig(Some(Arc::new(FileServicePinStore::new(&path)))).await;
    assert_eq!(second.plan_id().as_deref(), Some(third.as_str()));
    assert_eq!(
        second.runtime.store.snapshot().selection.map(|s| s.pinned),
        Some(true)
    );
}

#[tokio::test]
async fn a_demo_pin_whose_date_passed_ends_with_a_notice() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("service-pin.json");
    let yesterday = Utc::now().date_naive() - ChronoDuration::days(8);
    FileServicePinStore::new(&path)
        .save(&ServicePin::new(
            SOURCE_DEMO,
            &format!("demo-plan-{yesterday}"),
            yesterday,
            None,
            None,
        ))
        .unwrap();
    let rig = demo_rig(Some(Arc::new(FileServicePinStore::new(&path)))).await;
    let state = rig.runtime.store.snapshot();
    assert_eq!(
        state.plan.map(|p| p.id).as_deref(),
        Some("demo-service-plan")
    );
    let selection = state.selection.unwrap();
    assert!(!selection.pinned);
    assert_eq!(
        selection.notice.as_deref(),
        Some("Back to the upcoming service")
    );
    assert!(!path.exists(), "the stale pin file is gone");
    let _ = PinKeeper::in_memory();
}

// ---- Planning Center ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct Fake {
    fail: Arc<Mutex<bool>>,
    starts: Arc<Vec<(String, String)>>,
}

async fn fake_handler(State(fake): State<Fake>, request: AxumRequest) -> Response {
    let path = request.uri().path().to_string();
    let respond = |status: u16, body: Value| {
        Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    let page = |data: Vec<Value>| json!({"data": data, "meta": {"total_count": null}});
    if *fake.fail.lock().unwrap() && !path.ends_with("/service_types") {
        return respond(500, json!({"errors": [{"detail": "boom"}]}));
    }
    if path.ends_with("/service_types") {
        return respond(
            200,
            page(vec![json!({"type": "ServiceType", "id": "42",
                "attributes": {"name": "Weekend", "sequence": 1}})]),
        );
    }
    if path.ends_with("/plans") {
        return respond(
            200,
            page(
                fake.starts
                    .iter()
                    .map(|(id, starts)| {
                        json!({"type": "Plan", "id": id,
                            "attributes": {"title": format!("Plan {id}"), "dates": "", "sort_date": starts}})
                    })
                    .collect(),
            ),
        );
    }
    for (id, starts) in fake.starts.iter() {
        if path.contains(&format!("/plans/{id}/plan_times")) {
            return respond(
                200,
                page(vec![json!({"type": "PlanTime", "id": format!("t-{id}"),
                    "attributes": {"starts_at": starts, "time_type": "service", "name": ""}})]),
            );
        }
        if path.contains(&format!("/plans/{id}/items")) {
            return respond(
                200,
                page(vec![json!({"type": "Item", "id": format!("i-{id}"),
                    "attributes": {"title": format!("Song {id}"), "item_type": "song", "sequence": 1, "length": 200},
                    "relationships": {"song": {"data": {"type": "Song", "id": format!("s-{id}")}}}})]),
            );
        }
    }
    respond(404, json!({}))
}

async fn pco_rig() -> (Rig, Fake) {
    let noon = |days: i64| {
        (Utc::now().date_naive() + ChronoDuration::days(days))
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc()
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string()
    };
    let fake = Fake {
        fail: Arc::default(),
        starts: Arc::new(vec![
            ("plan-1".into(), noon(1)),
            ("plan-2".into(), noon(10)),
            ("plan-3".into(), noon(17)),
        ]),
    };
    let app = axum::Router::new()
        .fallback(fake_handler)
        .with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let persistent = PersistentSettings {
        integration_modes: IntegrationModes {
            service_source: ServiceSource::PlanningCenter,
            ..IntegrationModes::default()
        },
        timezone: "UTC".into(),
        planning_center: PersistentPlanningCenterSettings {
            app_id: Some("test-app".into()),
            service_type_id: Some("42".into()),
            ..PersistentPlanningCenterSettings::default()
        },
        ..PersistentSettings::default()
    };
    let service = Arc::new(SettingsService::ephemeral_with_secret(
        persistent,
        Some("test-secret".into()),
    ));
    let options = PcoOptions {
        memory_tokens: true,
        api_base: Some(Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap()),
        ..PcoOptions::default()
    };
    let runtime = Runtime::with_pco(service, false, options);
    runtime.start().await;
    for _ in 0..1500 {
        let state = runtime.store.snapshot();
        if state.planning_center_status == ConnectionStatus::Connected && state.plan.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        runtime.store.snapshot().plan.is_some(),
        "the plan was not loaded"
    );
    (
        Rig {
            router: build_app(runtime.clone(), AppOptions { web_root: None }),
            runtime,
        },
        fake,
    )
}

#[tokio::test]
async fn planning_center_plans_are_listed_picked_and_released() {
    let (rig, _fake) = pco_rig().await;
    assert_eq!(rig.plan_id().as_deref(), Some("plan-1"));
    let (status, list) = rig.call("GET", LIST, None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let ids: Vec<&str> = list["plans"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["plan-1", "plan-2", "plan-3"]);
    assert_eq!(list["upcoming_id"], "plan-1");
    assert_eq!(list["plans"][0]["service_type_name"], "Weekend");

    let (status, body) = rig
        .call("POST", PIN, Some(json!({"plan_id": "plan-3"})))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"]["plan"]["id"], "plan-3");
    assert_eq!(body["state"]["plan"]["songs"][0]["title"], "Song plan-3");
    assert_eq!(body["state"]["selection"]["pinned"], true);
    assert_eq!(body["state"]["upcoming"]["plan_id"], "plan-1");
    assert_eq!(body["state"]["service_load"]["status"], "loaded");

    let (_, list) = rig.call("GET", LIST, None).await;
    assert_eq!(list["pinned_id"], "plan-3");

    let (status, body) = rig.call("DELETE", PIN, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"]["plan"]["id"], "plan-1");
    assert_eq!(body["state"]["selection"]["pinned"], false);
}

#[tokio::test]
async fn a_planning_center_failure_is_a_message_and_changes_nothing() {
    let (rig, fake) = pco_rig().await;
    *fake.fail.lock().unwrap() = true;
    let (status, body) = rig.call("GET", LIST, None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    let detail = body["detail"].as_str().unwrap();
    assert!(
        !detail.is_empty() && !detail.contains("test-secret"),
        "{detail}"
    );
    let (status, body) = rig
        .call("POST", PIN, Some(json!({"plan_id": "plan-2"})))
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(
        rig.plan_id().as_deref(),
        Some("plan-1"),
        "the loaded plan stays"
    );
    *fake.fail.lock().unwrap() = false;
    let (_, list) = rig.call("GET", LIST, None).await;
    assert!(list["pinned_id"].is_null(), "nothing was pinned");
}

#[tokio::test]
async fn a_pick_whose_load_fails_leaves_the_loaded_plan_and_no_pin() {
    let (rig, fake) = pco_rig().await;
    // The list answers (it is cached by the first call) but loading plan-2's songs fails.
    rig.call("GET", LIST, None).await;
    *fake.fail.lock().unwrap() = true;
    // A fresh plan list needs Planning Center, so ask with the list still fresh: the route
    // reads the (cached) list, saves the pin, the refresh fails, the pin is put back.
    let (status, body) = rig
        .call("POST", PIN, Some(json!({"plan_id": "plan-2"})))
        .await;
    assert!(status == StatusCode::SERVICE_UNAVAILABLE, "{status} {body}");
    assert_eq!(rig.plan_id().as_deref(), Some("plan-1"));
    *fake.fail.lock().unwrap() = false;
    let (_, list) = rig.call("GET", LIST, None).await;
    assert!(list["pinned_id"].is_null(), "{list}");
    assert!(
        rig.runtime.pco.pins.get("planning_center").is_none(),
        "the pin file is not left behind"
    );
}
