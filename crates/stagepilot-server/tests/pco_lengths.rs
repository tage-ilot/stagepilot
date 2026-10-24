//! HTTP tests of the Planning Center length routes (`/api/v1/playback-api/planning-center-lengths/*`)
//! against a fake Planning Center server (B5). The routes were hard-coded 503 stubs in rc.4.
//!
//! The fake speaks just enough of the Services API: service types, one plan with four songs,
//! plan times, items, and `PATCH` of one item's length. Its item lengths are the "remote" state, so
//! the tests can assert what was really written and what restore put back.

use axum::body::Body;
use axum::extract::{ConnectInfo, Request as AxumRequest, State};
use axum::http::{header, Request, StatusCode};
use axum::response::Response;
use chrono::{Duration as ChronoDuration, Utc};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use stagepilot_core::config::{IntegrationModes, ServiceSource, SetlistId};
use stagepilot_core::settings::{
    PersistentPlanningCenterSettings, PersistentSettings, SettingsService,
};
use stagepilot_core::state::ConnectionStatus;
use stagepilot_pco::runtime::PcoOptions;
use stagepilot_playback::testkit::FakePlayback;
use stagepilot_server::{build_app, AppOptions, Runtime};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::ServiceExt;
use url::Url;

const BASE: &str = "/api/v1/playback-api/planning-center-lengths";
/// Measured Playback lengths and the lengths Planning Center holds now, per song.
/// 279.53 -> 280, 265.85 -> 266 (already 266), 260.17 -> 260, 410.83 -> 411.
const MEASURED: [f64; 4] = [279.53, 265.85, 260.17, 410.83];
const PLAN_LENGTHS: [i64; 4] = [275, 266, 250, 400];
const IDS: [&str; 4] = ["i1", "i2", "i3", "i4"];

// ---- fake Planning Center ----------------------------------------------------------------------

#[derive(Clone, Default)]
struct Fake {
    /// Remote item lengths.
    lengths: Arc<Mutex<BTreeMap<String, i64>>>,
    /// `(method, path)` log.
    log: Arc<Mutex<Vec<(String, String)>>>,
    /// Status every request answers with instead of normal service when set.
    fail: Arc<Mutex<Option<u16>>>,
    /// Starts of the plan: tomorrow at noon UTC (so it is always inside the look-ahead window).
    starts_at: String,
}

fn item(id: &str, title: &str, sequence: i64, length: i64) -> Value {
    json!({"type": "Item", "id": id,
        "attributes": {"title": title, "item_type": "song", "sequence": sequence, "length": length},
        "relationships": {"song": {"data": {"type": "Song", "id": format!("s-{id}")}}}})
}

async fn fake_handler(State(fake): State<Fake>, request: AxumRequest) -> Response {
    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    fake.log
        .lock()
        .unwrap()
        .push((method.clone(), path.clone()));
    let respond = |status: u16, body: Value| {
        Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    if let Some(status) = *fake.fail.lock().unwrap() {
        return respond(status, json!({"errors": []}));
    }
    let page = |data: Vec<Value>| json!({"data": data, "meta": {"total_count": null}});
    if path.ends_with("/service_types") {
        return respond(
            200,
            page(vec![
                json!({"type": "ServiceType", "id": "42", "attributes": {"name": "Weekend", "sequence": 1}}),
                json!({"type": "ServiceType", "id": "7", "attributes": {"name": "Other", "sequence": 2}}),
                json!({"type": "ServiceType", "id": "9", "attributes": {"name": "Old", "sequence": 3,
                    "archived_at": "2026-01-01T00:00:00Z"}}),
            ]),
        );
    }
    if path.contains("/service_types/42/plans") && path.ends_with("/plans") {
        return respond(
            200,
            page(vec![json!({"type": "Plan", "id": "plan-1",
                "attributes": {"title": "Sunday", "dates": "", "sort_date": null}})]),
        );
    }
    if path.ends_with("/plan_times") {
        return respond(
            200,
            page(vec![json!({"type": "PlanTime", "id": "t1",
                "attributes": {"starts_at": fake.starts_at, "time_type": "service", "name": ""}})]),
        );
    }
    if path.ends_with("/items") {
        let lengths = fake.lengths.lock().unwrap().clone();
        return respond(
            200,
            page(
                IDS.iter()
                    .enumerate()
                    .map(|(i, id)| item(id, &format!("Song {i}"), i as i64 + 1, lengths[*id]))
                    .collect(),
            ),
        );
    }
    if let Some(id) = path
        .rsplit("/items/")
        .next()
        .filter(|_| path.contains("/items/"))
    {
        let id = id.to_string();
        if method == "PATCH" {
            let bytes = request.into_body().collect().await.unwrap().to_bytes();
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            let length = body["data"]["attributes"]["length"].as_i64().unwrap();
            fake.lengths.lock().unwrap().insert(id.clone(), length);
        }
        let length = fake.lengths.lock().unwrap()[&id];
        let n = IDS.iter().position(|i| *i == id).unwrap();
        return respond(
            200,
            json!({"data": item(&id, &format!("Song {n}"), n as i64 + 1, length)}),
        );
    }
    respond(404, json!({}))
}

async fn start_fake() -> (Fake, Url) {
    let tomorrow = Utc::now() + ChronoDuration::days(1);
    let fake = Fake {
        lengths: Arc::new(Mutex::new(
            IDS.iter()
                .zip(PLAN_LENGTHS)
                .map(|(id, length)| ((*id).to_string(), length))
                .collect(),
        )),
        starts_at: tomorrow.format("%Y-%m-%dT12:00:00Z").to_string(),
        ..Fake::default()
    };
    let app = axum::Router::new()
        .fallback(fake_handler)
        .with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (
        fake,
        Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap(),
    )
}

// ---- the application under test ----------------------------------------------------------------

struct Rig {
    router: axum::Router,
    runtime: Arc<Runtime>,
    fake: Fake,
    /// A connected Playback whose heartbeat matches the saved scan, so the scan is not stale.
    playback: FakePlayback,
}

async fn rig() -> Rig {
    let (fake, api) = start_fake().await;
    let mut persistent = PersistentSettings {
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
    persistent.playback_api.song_order = vec![11, 12, 13, 14];
    persistent.playback_api.song_lengths = MEASURED.iter().map(|l| Some(*l)).collect();
    persistent.playback_api.captured_at = Some(Utc::now());
    persistent.playback_api.captured_version = Some(1);
    persistent.playback_api.setlist_id = Some(SetlistId::Int(5));
    let service = Arc::new(SettingsService::ephemeral_with_secret(
        persistent,
        Some("test-secret".into()),
    ));
    let options = PcoOptions {
        memory_tokens: true,
        api_base: Some(api),
        ..PcoOptions::default()
    };
    let runtime = Runtime::with_pco(service, false, options);
    runtime.start().await;
    // The real Planning Center plugin reads the plan (and re-reads it after a write) from the fake.
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
    let playback = FakePlayback::start().await;
    playback.auto_beat(Duration::from_millis(50));
    {
        let mut state = playback.state.lock().unwrap();
        state.songs = vec![11, 12, 13, 14];
        state.song = 11;
        state.version = 1;
    }
    let rig = Rig {
        router: build_app(runtime.clone(), AppOptions { web_root: None }),
        runtime,
        fake,
        playback,
    };
    rig.connect_playback().await;
    rig
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
        let peer: SocketAddr = "127.0.0.1:50000".parse().unwrap();
        request.extensions_mut().insert(ConnectInfo(peer));
        let response = self.router.clone().oneshot(request).await.unwrap();
        let (parts, body) = response.into_parts();
        let bytes = body.collect().await.unwrap().to_bytes();
        (
            parts.status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// Point Playback at the fake and wait until the saved scan is confirmed by a heartbeat.
    async fn connect_playback(&self) {
        let (code, body) = self
            .call(
                "PUT",
                "/api/v1/playback-api/settings",
                Some(
                    json!({"enabled": true, "host": "127.0.0.1", "port": self.playback.port,
                    "auto_scan": false, "fast_transport": true}),
                ),
            )
            .await;
        assert_eq!(code, StatusCode::OK, "{body}");
        for _ in 0..1500 {
            let (_, status) = self.call("GET", "/api/v1/playback-api/status", None).await;
            if status["connected"] == true && status["stale"] == false {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let (_, status) = self.call("GET", "/api/v1/playback-api/status", None).await;
        panic!("Playback never confirmed the saved scan: {status}");
    }

    fn remote(&self, id: &str) -> i64 {
        self.fake.lengths.lock().unwrap()[id]
    }

    fn patches(&self) -> usize {
        self.fake
            .log
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, _)| m == "PATCH")
            .count()
    }

    async fn choose(&self) {
        let (code, body) = self
            .call(
                "PUT",
                &format!("{BASE}/category"),
                Some(json!({"service_type_id": "42"})),
            )
            .await;
        assert_eq!(code, StatusCode::OK, "{body}");
    }

    async fn preview(&self) -> Value {
        let (code, body) = self.call("POST", &format!("{BASE}/preview"), None).await;
        assert_eq!(code, StatusCode::OK, "{body}");
        body
    }
}

// ---- tests -------------------------------------------------------------------------------------

#[tokio::test]
async fn categories_lists_the_active_service_types() {
    let r = rig().await;
    let (code, body) = r.call("GET", &format!("{BASE}/categories"), None).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let mut ids: Vec<&str> = body
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, ["42", "7"], "archived service types are not offered");
    let weekend = body
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "42")
        .unwrap();
    assert_eq!(
        weekend,
        &json!({"id": "42", "name": "Weekend", "sequence": 1, "archived": false})
    );
}

#[tokio::test]
async fn choosing_a_category_is_saved_in_the_playback_settings() {
    let r = rig().await;
    let (code, body) = r
        .call(
            "PUT",
            &format!("{BASE}/category"),
            Some(json!({"service_type_id": "7"})),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body["planning_center_update_service_type_id"], "7");
    let saved = r.runtime.settings_service.snapshot().playback_api;
    assert_eq!(
        saved.planning_center_update_service_type_id.as_deref(),
        Some("7")
    );
    // The running Playback plugin sees it too (the scan key and the status use it).
    assert_eq!(
        r.runtime
            .playback
            .config()
            .planning_center_update_service_type_id
            .as_deref(),
        Some("7")
    );
}

#[tokio::test]
async fn choosing_an_unknown_or_archived_category_is_a_409() {
    let r = rig().await;
    for id in ["nope", "9"] {
        let (code, body) = r
            .call(
                "PUT",
                &format!("{BASE}/category"),
                Some(json!({"service_type_id": id})),
            )
            .await;
        assert_eq!(code, StatusCode::CONFLICT, "{id}: {body}");
        assert_eq!(
            body,
            json!({"detail": "Choose an available Planning Center category."})
        );
    }
    let saved = r.runtime.settings_service.snapshot().playback_api;
    assert_eq!(saved.planning_center_update_service_type_id, None);
}

#[tokio::test]
async fn preview_shows_each_song_with_the_rounded_measured_length() {
    let r = rig().await;
    r.choose().await;
    let preview = r.preview().await;
    assert_eq!(preview["category"], "Weekend");
    assert_eq!(preview["plan_title"], "Sunday");
    assert!(!preview["token"].as_str().unwrap().is_empty());
    let rows: Vec<(i64, i64, &str)> = preview["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["old_length"].as_i64().unwrap(),
                i["new_length"].as_i64().unwrap(),
                i["status"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (275, 280, "updated"),
            (266, 266, "skipped"),
            (250, 260, "updated"),
            (400, 411, "updated"),
        ]
    );
    assert_eq!(preview["items"][1]["reason"], "Already the same.");
    assert_eq!(r.patches(), 0, "a preview never writes");
}

#[tokio::test]
async fn confirm_with_the_wrong_token_is_a_409_and_writes_nothing() {
    let r = rig().await;
    r.choose().await;
    r.preview().await;
    let (code, body) = r
        .call(
            "POST",
            &format!("{BASE}/confirm"),
            Some(json!({"token": "not-the-token", "confirm": true})),
        )
        .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body,
        json!({"detail": "The scan or Playback changed. Preview the plan again."})
    );
    assert_eq!(r.patches(), 0);
}

#[tokio::test]
async fn confirm_without_a_preview_is_a_409() {
    let r = rig().await;
    r.choose().await;
    let (code, body) = r
        .call(
            "POST",
            &format!("{BASE}/confirm"),
            Some(json!({"token": "x", "confirm": true})),
        )
        .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body,
        json!({"detail": "Preview the plan again before updating."})
    );
}

#[tokio::test]
async fn confirm_writes_the_new_lengths_with_undo_data_and_restore_puts_them_back() {
    let r = rig().await;
    r.choose().await;
    let preview = r.preview().await;
    let token = preview["token"].as_str().unwrap();

    let (code, body) = r
        .call(
            "POST",
            &format!("{BASE}/confirm"),
            Some(json!({"token": token, "confirm": true})),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body["planning_center_lengths"]["operation"], "update");
    assert_eq!(body["planning_center_lengths"]["status"], "done");
    assert_eq!(body["planning_center_undo_available"], true);
    assert_eq!(
        [
            r.remote("i1"),
            r.remote("i2"),
            r.remote("i3"),
            r.remote("i4")
        ],
        [280, 266, 260, 411]
    );
    assert_eq!(
        r.patches(),
        3,
        "the song that already matched is not written"
    );
    let undo = r
        .runtime
        .settings_service
        .snapshot()
        .planning_center_length_undo;
    let mut recorded: Vec<(&str, i64, i64)> = undo
        .iter()
        .map(|u| (u.item_id.as_str(), u.old_length, u.new_length))
        .collect();
    recorded.sort_unstable();
    assert_eq!(
        recorded,
        [("i1", 275, 280), ("i3", 250, 260), ("i4", 400, 411)]
    );
    // This scan is now written: the next preview is refused until the songs are scanned again.
    assert!(
        r.runtime.playback.lengths_applied(),
        "{:?} / {:?}",
        r.runtime.playback.config().lengths_applied_key,
        body["planning_center_lengths"]
    );

    let (code, body) = r
        .call(
            "POST",
            &format!("{BASE}/restore"),
            Some(json!({"confirm": true})),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body["planning_center_lengths"]["operation"], "restore");
    assert_eq!(body["planning_center_lengths"]["status"], "done");
    assert_eq!(
        [
            r.remote("i1"),
            r.remote("i2"),
            r.remote("i3"),
            r.remote("i4")
        ],
        PLAN_LENGTHS
    );
    assert_eq!(body["planning_center_undo_available"], false);
    assert!(r
        .runtime
        .settings_service
        .snapshot()
        .planning_center_length_undo
        .is_empty());
}

#[tokio::test]
async fn planning_center_errors_are_reported_as_what_they_are() {
    let r = rig().await;
    r.choose().await;
    // The account may not read: 403 with the "not allowed" text, for every route that reads.
    *r.fake.fail.lock().unwrap() = Some(403);
    let (code, body) = r.call("GET", &format!("{BASE}/categories"), None).await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(
        body["detail"],
        stagepilot_pco::lengths::DENIED_MESSAGE,
        "{body}"
    );
    let (code, body) = r.call("POST", &format!("{BASE}/preview"), None).await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{body}");
    // A Planning Center outage is "try again later", not a stub: the same request works again
    // as soon as Planning Center does.
    *r.fake.fail.lock().unwrap() = Some(500);
    let (code, body) = r.call("GET", &format!("{BASE}/categories"), None).await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(body["detail"].as_str().unwrap().contains("Try again later"));
    let (code, _) = r.call("POST", &format!("{BASE}/preview"), None).await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    *r.fake.fail.lock().unwrap() = None;
    let (code, body) = r.call("GET", &format!("{BASE}/categories"), None).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    r.preview().await;
}
