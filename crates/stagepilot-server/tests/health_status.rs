//! M1/M2: `/api/v1/health` reports the real version, and `status` follows the real Playback link
//! (rc.4 hard-coded "degraded" for the Playback API source, so production never said "healthy").

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Request};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use stagepilot_core::config::MidiSource;
use stagepilot_core::settings::{RuntimeSettings, SettingsService, APP_VERSION};
use stagepilot_core::state::ServiceLoadStatus;
use stagepilot_playback::testkit::FakePlayback;
use stagepilot_server::{build_app, AppOptions, Runtime};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

struct Rig {
    router: axum::Router,
    runtime: Arc<Runtime>,
}

async fn rig(source: MidiSource) -> Rig {
    let mut settings = RuntimeSettings::demo();
    settings.integration_modes.midi_source = source;
    let runtime = Runtime::new(Arc::new(SettingsService::ephemeral(settings)), false);
    runtime.start().await;
    Rig {
        router: build_app(runtime.clone(), AppOptions { web_root: None }),
        runtime,
    }
}

impl Rig {
    async fn call(&self, method: &str, path: &str, body: Option<Value>) -> Value {
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
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    }

    async fn health(&self) -> Value {
        self.call("GET", "/api/v1/health", None).await
    }

    async fn playback_settings(&self, body: Value) {
        let reply = self
            .call("PUT", "/api/v1/playback-api/settings", Some(body))
            .await;
        assert!(reply.get("detail").is_none(), "{reply}");
    }

    async fn until_status(&self, wanted: &str) -> Value {
        for _ in 0..1500 {
            let health = self.health().await;
            if health["status"] == wanted {
                return health;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("health never became {wanted}: {}", self.health().await);
    }
}

#[tokio::test]
async fn health_reports_the_build_version() {
    let r = rig(MidiSource::Simulated).await;
    assert_eq!(r.health().await["version"], APP_VERSION);
    assert_ne!(r.health().await["version"], "2.41.1");
}

#[tokio::test]
async fn playback_source_is_degraded_until_playback_connects_then_healthy() {
    let r = rig(MidiSource::PlaybackApi).await;
    // Playback is enabled by default and not connected yet.
    assert_eq!(r.health().await["status"], "degraded");

    let fake = FakePlayback::start().await;
    fake.auto_beat(Duration::from_millis(50));
    r.playback_settings(
        json!({"enabled": true, "host": "127.0.0.1", "port": fake.port,
        "auto_scan": false, "fast_transport": true}),
    )
    .await;
    r.until_status("healthy").await;

    // The link goes away: back to degraded, with no other change.
    let _offline = fake.go_offline().await;
    r.until_status("degraded").await;
    r.runtime.stop().await;
}

#[tokio::test]
async fn playback_turned_off_does_not_hold_health_down() {
    let r = rig(MidiSource::PlaybackApi).await;
    assert_eq!(r.health().await["status"], "degraded");
    r.playback_settings(json!({"enabled": false, "host": null, "port": 8080,
        "auto_scan": false, "fast_transport": true}))
        .await;
    r.until_status("healthy").await;
}

#[tokio::test]
async fn a_failed_service_load_or_a_stopped_plugin_still_degrades_health() {
    let r = rig(MidiSource::Simulated).await;
    assert_eq!(r.health().await["status"], "healthy");
    r.runtime
        .store
        .mutate(|s| s.service_load.status = ServiceLoadStatus::Error);
    assert_eq!(r.health().await["status"], "degraded");
    r.runtime
        .store
        .mutate(|s| s.service_load.status = ServiceLoadStatus::Loaded);
    assert_eq!(r.health().await["status"], "healthy");
    r.runtime.stop().await;
    assert_eq!(r.health().await["status"], "degraded");
}
