//! Saving ProPresenter settings with a Look (m3): an unknown Look is reported and not kept; a known
//! one is applied and saved. ProPresenter is an in-process fake client; nothing leaves the machine.

use async_trait::async_trait;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use stagepilot_core::config::{ProPresenterSettings, TimerOutput};
use stagepilot_core::settings::{PersistentSettings, SettingsService};
use stagepilot_propresenter::client::ProPresenterClient;
use stagepilot_propresenter::{Countdown, Identifier, Look, PpError, Timer};
use stagepilot_server::{build_app, AppOptions, OutputBackends, Runtime};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

fn id(uuid: &str, name: &str, index: u32) -> Identifier {
    Identifier {
        uuid: uuid.into(),
        name: name.into(),
        index,
    }
}

#[derive(Default)]
struct FakePp {
    current: Mutex<Option<Look>>,
    triggered: Mutex<Vec<String>>,
}

#[async_trait]
impl ProPresenterClient for FakePp {
    async fn close(&self) {}
    async fn list_timers(&self) -> Result<Vec<Timer>, PpError> {
        Ok(vec![Timer {
            id: id("timer-uuid", "Song Countdown", 0),
            allows_overrun: false,
            countdown: Some(Countdown { duration: 60 }),
            count_down_to_time: None,
            elapsed: None,
            state: None,
            time: None,
        }])
    }
    async fn find_timer(&self, name: &str) -> Result<Timer, PpError> {
        Err(PpError::TimerNotFound(name.into()))
    }
    async fn get_timer(&self, _id: &str) -> Result<Timer, PpError> {
        Err(PpError::TimerNotFound("-".into()))
    }
    async fn list_looks(&self) -> Result<Vec<Look>, PpError> {
        Ok(vec![
            Look {
                id: id("look-worship", "Worship", 0),
            },
            Look {
                id: id("look-sermon", "Sermon", 1),
            },
        ])
    }
    async fn current_look(&self) -> Result<Look, PpError> {
        Ok(self.current.lock().unwrap().clone().unwrap_or(Look {
            id: id("live-sermon", "Sermon", 1),
        }))
    }
    async fn trigger_look(&self, look_id: &str) -> Result<(), PpError> {
        self.triggered.lock().unwrap().push(look_id.to_string());
        if look_id == "look-worship" {
            *self.current.lock().unwrap() = Some(Look {
                id: id("a-new-live-uuid", "Worship", 0),
            });
        }
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

async fn call(router: &axum::Router, path: &str, body: Value) -> (StatusCode, Value) {
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

async fn app(saved_look: Option<&str>) -> (axum::Router, Arc<Runtime>, Arc<FakePp>) {
    let fake = Arc::new(FakePp::default());
    let service = SettingsService::ephemeral_persistent(PersistentSettings {
        integration_modes: stagepilot_core::config::IntegrationModes {
            timer_output: TimerOutput::Propresenter,
            ..Default::default()
        },
        propresenter: ProPresenterSettings {
            enabled: true,
            host: "127.0.0.1".into(),
            port: 1025,
            look_id: saved_look.map(str::to_string),
            ..ProPresenterSettings::default()
        },
        ..PersistentSettings::default()
    });
    let shared = fake.clone();
    let backends = OutputBackends {
        propresenter: Arc::new(move |_settings| shared.clone() as Arc<dyn ProPresenterClient>),
        ..OutputBackends::fake_midi()
    };
    let service = Arc::new(service);
    let runtime = Runtime::with_backends(service, false, backends);
    runtime.start().await;
    (
        build_app(runtime.clone(), AppOptions { web_root: None }),
        runtime,
        fake,
    )
}

fn request(look: &str) -> Value {
    json!({
        "host": "127.0.0.1", "port": 1025, "timer_name": "Song Countdown",
        "request_timeout_seconds": 3.0, "look_id": look,
    })
}

fn saved_look(rt: &Runtime) -> Option<String> {
    rt.settings_service
        .effective_runtime_settings()
        .propresenter
        .look_id
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_look_is_reported_and_the_previous_look_stays_saved() {
    let (router, rt, fake) = app(Some("look-sermon")).await;
    let (status, body) = call(
        &router,
        "/api/v1/propresenter/settings",
        request("not-a-look"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["accepted"], false);
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("ProPresenter Look \"not-a-look\" was not found."),
        "{body}"
    );
    assert_eq!(saved_look(&rt).as_deref(), Some("look-sermon"));
    assert_eq!(body["propresenter"]["look_id"], json!("look-sermon"));
    assert!(fake.triggered.lock().unwrap().is_empty());
    rt.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_known_look_is_applied_and_saved() {
    let (router, rt, fake) = app(None).await;
    let (status, body) = call(
        &router,
        "/api/v1/propresenter/settings",
        request("look-worship"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["accepted"], true, "{body}");
    assert_eq!(saved_look(&rt).as_deref(), Some("look-worship"));
    assert_eq!(*fake.triggered.lock().unwrap(), ["look-worship"]);
    rt.stop().await;
}
