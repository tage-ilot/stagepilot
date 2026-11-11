//! F-P1: saving settings that are already running must not restart the service. The dashboard
//! saves every configured service again on each page load (a phone, a tablet, the remote page),
//! also in the middle of a song. ProPresenter and Planning Center are fakes in this process;
//! nothing leaves the machine.

use async_trait::async_trait;
use axum::body::Body;
use axum::extract::{ConnectInfo, Request as AxumRequest, State};
use axum::http::{header, Request, StatusCode};
use axum::response::Response;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use stagepilot_core::config::{IntegrationModes, ProPresenterSettings, ServiceSource, TimerOutput};
use stagepilot_core::settings::{
    PersistentPlanningCenterSettings, PersistentSettings, SettingsService,
};
use stagepilot_core::state::ConnectionStatus;
use stagepilot_pco::runtime::PcoOptions;
use stagepilot_propresenter::client::ProPresenterClient;
use stagepilot_propresenter::{Countdown, Identifier, Look, PpError, Timer};
use stagepilot_server::{build_app, AppOptions, OutputBackends, Runtime};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::ServiceExt;
use url::Url;

async fn post(router: &axum::Router, path: &str, body: Value) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let peer: SocketAddr = "127.0.0.1:50000".parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(peer));
    let response = router.clone().oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    let bytes = body.collect().await.unwrap().to_bytes();
    (
        parts.status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

// ---- ProPresenter ------------------------------------------------------------------------------

fn ident(uuid: &str, name: &str, index: u32) -> Identifier {
    Identifier {
        uuid: uuid.into(),
        name: name.into(),
        index,
    }
}

/// A ProPresenter that has the timer and the Look and counts what the plugin does to it.
#[derive(Default)]
struct FakePp {
    triggered: Mutex<Vec<String>>,
    /// Clients the plugin asked the factory for: one per (re)connection.
    clients: AtomicUsize,
}

#[async_trait]
impl ProPresenterClient for FakePp {
    async fn close(&self) {}
    async fn list_timers(&self) -> Result<Vec<Timer>, PpError> {
        Ok(vec![self.timer()])
    }
    async fn find_timer(&self, name: &str) -> Result<Timer, PpError> {
        if name == "Song Countdown" {
            Ok(self.timer())
        } else {
            Err(PpError::TimerNotFound(name.into()))
        }
    }
    async fn get_timer(&self, _id: &str) -> Result<Timer, PpError> {
        Ok(self.timer())
    }
    async fn list_looks(&self) -> Result<Vec<Look>, PpError> {
        Ok(vec![Look {
            id: ident("look-worship", "Worship", 0),
        }])
    }
    async fn current_look(&self) -> Result<Look, PpError> {
        Ok(Look {
            id: ident("look-worship", "Worship", 0),
        })
    }
    async fn trigger_look(&self, look_id: &str) -> Result<(), PpError> {
        self.triggered.lock().unwrap().push(look_id.to_string());
        Ok(())
    }
    async fn stop_timer(&self, _id: &str) -> Result<(), PpError> {
        Ok(())
    }
    async fn set_timer_duration(&self, t: &Timer, _s: i64) -> Result<Timer, PpError> {
        Ok(t.clone())
    }
    async fn reset_timer(&self, _id: &str) -> Result<(), PpError> {
        Ok(())
    }
    async fn start_timer(&self, _id: &str) -> Result<(), PpError> {
        Ok(())
    }
}

impl FakePp {
    fn timer(&self) -> Timer {
        Timer {
            id: ident("timer-uuid", "Song Countdown", 0),
            allows_overrun: false,
            countdown: Some(Countdown { duration: 60 }),
            count_down_to_time: None,
            elapsed: None,
            state: Some("stopped".into()),
            time: None,
        }
    }
}

async fn pp_app() -> (axum::Router, Arc<Runtime>, Arc<FakePp>) {
    let fake = Arc::new(FakePp::default());
    let service = SettingsService::ephemeral_persistent(PersistentSettings {
        integration_modes: IntegrationModes {
            timer_output: TimerOutput::Propresenter,
            ..Default::default()
        },
        propresenter: ProPresenterSettings {
            enabled: true,
            host: "127.0.0.1".into(),
            port: 1025,
            look_id: Some("look-worship".into()),
            ..ProPresenterSettings::default()
        },
        ..PersistentSettings::default()
    });
    let shared = fake.clone();
    let backends = OutputBackends {
        propresenter: Arc::new(move |_settings| {
            shared.clients.fetch_add(1, Ordering::SeqCst);
            shared.clone() as Arc<dyn ProPresenterClient>
        }),
        ..OutputBackends::fake_midi()
    };
    let runtime = Runtime::with_backends(Arc::new(service), false, backends);
    runtime.start().await;
    for _ in 0..500 {
        if runtime.store.snapshot().propresenter_status == ConnectionStatus::Connected {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    (
        build_app(runtime.clone(), AppOptions { web_root: None }),
        runtime,
        fake,
    )
}

fn pp_body(port: u16, timer: &str) -> Value {
    json!({
        "host": "127.0.0.1", "port": port, "timer_name": timer,
        "request_timeout_seconds": 3.0, "look_id": "look-worship",
    })
}

/// Everything the plugin has done to its client so far: connections made and Looks triggered.
fn pp_activity(fake: &FakePp) -> (usize, usize) {
    (
        fake.clients.load(Ordering::SeqCst),
        fake.triggered.lock().unwrap().len(),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn saving_the_running_propresenter_settings_again_reconnects_nothing() {
    let (router, rt, fake) = pp_app().await;
    assert_eq!(
        rt.store.snapshot().propresenter_status,
        ConnectionStatus::Connected
    );
    let before = pp_activity(&fake);
    assert!(before.0 >= 1, "the plugin connected at start");
    for _ in 0..3 {
        let (status, body) = post(
            &router,
            "/api/v1/propresenter/settings",
            pp_body(1025, "Song Countdown"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["accepted"], true, "{body}");
        assert_eq!(body["propresenter"]["connection_status"], "connected");
    }
    // No new client (no reconnect) and no Look triggered again.
    assert_eq!(pp_activity(&fake), before);
    assert_eq!(
        rt.store.snapshot().propresenter_status,
        ConnectionStatus::Connected
    );
    rt.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn saving_a_changed_propresenter_port_or_timer_still_reconnects() {
    let (router, rt, fake) = pp_app().await;
    let before = pp_activity(&fake).0;
    let (_, body) = post(
        &router,
        "/api/v1/propresenter/settings",
        pp_body(1026, "Song Countdown"),
    )
    .await;
    assert_eq!(body["propresenter"]["port"], 1026, "{body}");
    assert!(pp_activity(&fake).0 > before, "a new port reconnects");
    let again = pp_activity(&fake).0;
    // A timer that the install does not have is a change too, and is reported as missing.
    let (_, body) = post(
        &router,
        "/api/v1/propresenter/settings",
        pp_body(1026, "Other Timer"),
    )
    .await;
    assert_eq!(body["propresenter"]["timer_found"], false, "{body}");
    assert!(pp_activity(&fake).0 > again, "a new timer name reconnects");
    rt.stop().await;
}

// ---- Planning Center ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct FakePco {
    requests: Arc<Mutex<Vec<String>>>,
}

async fn pco_handler(State(fake): State<FakePco>, request: AxumRequest) -> Response {
    let path = request.uri().path().to_string();
    fake.requests.lock().unwrap().push(path.clone());
    let respond = |body: Value| {
        Response::builder()
            .status(200)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    let page = |data: Vec<Value>| json!({"data": data, "meta": {"total_count": null}});
    if path.ends_with("/service_types") {
        return respond(page(vec![
            json!({"type": "ServiceType", "id": "42", "attributes": {"name": "Weekend", "sequence": 1}}),
            json!({"type": "ServiceType", "id": "7", "attributes": {"name": "Other", "sequence": 2}}),
        ]));
    }
    // No plan in either service type: the connection is still good.
    respond(page(vec![]))
}

async fn pco_app() -> (axum::Router, Arc<Runtime>, FakePco) {
    let fake = FakePco::default();
    let app = axum::Router::new()
        .fallback(pco_handler)
        .with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let service = Arc::new(SettingsService::ephemeral_with_secret(
        PersistentSettings {
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
        },
        Some("test-secret".into()),
    ));
    let options = PcoOptions {
        memory_tokens: true,
        api_base: Some(Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap()),
        ..PcoOptions::default()
    };
    let runtime = Runtime::with_pco(service, false, options);
    runtime.start().await;
    for _ in 0..500 {
        if runtime.store.snapshot().planning_center_status == ConnectionStatus::Connected {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        runtime.store.snapshot().planning_center_status,
        ConnectionStatus::Connected
    );
    (
        build_app(runtime.clone(), AppOptions { web_root: None }),
        runtime,
        fake,
    )
}

fn pco_body(service_type: &str) -> Value {
    json!({"app_id": "test-app", "service_type_id": service_type})
}

#[tokio::test(flavor = "multi_thread")]
async fn saving_the_running_planning_center_settings_again_reloads_nothing() {
    let (router, rt, fake) = pco_app().await;
    tokio::time::sleep(Duration::from_millis(300)).await; // the start-up load settles
    let before = fake.requests.lock().unwrap().len();
    assert!(before > 0, "the plugin loaded at start");
    for _ in 0..3 {
        let (status, body) =
            post(&router, "/api/v1/planning-center/settings", pco_body("42")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        fake.requests.lock().unwrap().len(),
        before,
        "{:?}",
        fake.requests.lock().unwrap()
    );
    assert_eq!(
        rt.store.snapshot().planning_center_status,
        ConnectionStatus::Connected
    );
    rt.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn saving_another_planning_center_service_type_still_reloads() {
    let (router, rt, fake) = pco_app().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (status, body) = post(&router, "/api/v1/planning-center/settings", pco_body("7")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        fake.requests
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.contains("/service_types/7/plans")),
        "{:?}",
        fake.requests.lock().unwrap()
    );
    rt.stop().await;
}
