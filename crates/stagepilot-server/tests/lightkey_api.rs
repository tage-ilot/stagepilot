//! HTTP surface of the Lightkey link and the saved settings it adds to Lighting: the Rust
//! counterpart of `backend/tests/test_lightkey_api.py`. The agent is an in-process fake behind the
//! link's injected HTTP trait; OSC goes to the in-memory fake. Mapping:
//! `docs/archive/rust-migration/lightkey-test-map.md`.

use async_trait::async_trait;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use stagepilot_core::config::{LightingMethod, LightkeySettings, LightsSettings};
use stagepilot_core::settings::{PersistentSettings, SettingsService};
use stagepilot_lights::lightkey_link::{HttpFailure, LightkeyHttp};
use stagepilot_osc::fake::FakeOscBackend;
use stagepilot_osc::OscBackend;
use stagepilot_server::{build_app, AppOptions, OutputBackends, Runtime};
use std::net::SocketAddr;
use std::sync::Arc;
use tower::ServiceExt;

/// `_fake_agent`: `/time` answers 1000 ms, `/log` reports two active cues at seq 3.
struct FakeAgent {
    down: bool,
}

#[async_trait]
impl LightkeyHttp for FakeAgent {
    async fn get_json(&self, url: &str, _query: &[(&str, String)]) -> Result<Value, HttpFailure> {
        if self.down {
            return Err(HttpFailure::Other("down".into()));
        }
        if url.ends_with("/time") {
            return Ok(json!({"t": 1000.0}));
        }
        Ok(json!({"events": [], "state": {"online": true, "active": ["A", "B"], "seq": 3}}))
    }
}

async fn app(lights: LightsSettings) -> (axum::Router, Arc<Runtime>) {
    app_with(lights, false).await
}

async fn app_with(lights: LightsSettings, down: bool) -> (axum::Router, Arc<Runtime>) {
    let service = SettingsService::ephemeral_persistent(PersistentSettings {
        lights,
        ..PersistentSettings::default()
    });
    let osc = Arc::new(FakeOscBackend::default());
    let backends = OutputBackends {
        osc: Arc::new(move || osc.clone() as Arc<dyn OscBackend>),
        lightkey_http: Arc::new(move || Arc::new(FakeAgent { down })),
        ..OutputBackends::fake_midi()
    };
    let runtime = Runtime::with_backends(Arc::new(service), false, backends);
    runtime.start().await;
    (
        build_app(runtime.clone(), AppOptions { web_root: None }),
        runtime,
    )
}

fn osc_lights() -> LightsSettings {
    LightsSettings {
        enabled: true,
        method: LightingMethod::Osc,
        ..LightsSettings::default()
    }
}

async fn call(
    router: &axum::Router,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
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
    let response = router.clone().oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    let bytes = body.collect().await.unwrap().to_bytes();
    (
        parts.status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

const LK: &str = "/api/v1/lights/lightkey";

#[tokio::test]
async fn defaults_replay_on_and_url_derived_from_the_osc_host() {
    let (router, rt) = app(LightsSettings {
        enabled: true,
        osc_host: "10.30.0.211".into(),
        ..LightsSettings::default()
    })
    .await;
    let (status, body) = call(&router, "GET", LK, None).await;
    rt.stop().await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["replay"], true);
    assert_eq!(body["api_url"], "http://10.30.0.211:8770");
    assert_eq!(body["recording"], false);
    assert_eq!(body["takes"], json!([]));
}

#[tokio::test]
async fn settings_save_only_what_changed_and_persist() {
    let (router, rt) = app(osc_lights()).await;
    let (a_code, a) = call(
        &router,
        "PUT",
        &format!("{LK}/settings"),
        Some(json!({"replay": false})),
    )
    .await;
    let (b_code, _) = call(
        &router,
        "PUT",
        &format!("{LK}/settings"),
        Some(json!({"api_url": "10.30.0.211:9000"})),
    )
    .await;
    let (_, settings) = call(&router, "GET", "/api/v1/settings", None).await;
    let (_, status) = call(&router, "GET", LK, None).await;
    rt.stop().await;
    assert_eq!(a_code, StatusCode::OK);
    assert_eq!(a["lightkey"]["replay"], false);
    assert_eq!(a["accepted"], true);
    assert_eq!(a["message"], "Saved.");
    assert_eq!(b_code, StatusCode::OK);
    assert_eq!(
        settings["settings"]["lights"]["lightkey"],
        json!({"api_url": "http://10.30.0.211:9000", "replay": false}) // replay kept
    );
    assert_eq!(status["api_url"], "http://10.30.0.211:9000");
}

#[tokio::test]
async fn bad_address_is_rejected_with_422_in_the_no_context_form() {
    let (router, rt) = app(osc_lights()).await;
    let (code, body) = call(
        &router,
        "PUT",
        &format!("{LK}/settings"),
        Some(json!({"api_url": "http://a/b c"})),
    )
    .await;
    rt.stop().await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body,
        json!({"detail": [{
            "type": "value_error",
            "loc": ["api_url"],
            "msg": "Value error, Enter the Lightkey API address as host or host:port, e.g. 10.30.0.211:8770.",
            "input": "http://a/b c",
        }]})
    );
}

#[tokio::test]
async fn recording_refused_until_connected() {
    // Python's test points at an address nothing answers; here the fake agent is down.
    let (router, rt) = app_with(osc_lights(), true).await;
    let (code, body) = call(
        &router,
        "POST",
        &format!("{LK}/recording"),
        Some(json!({"recording": true})),
    )
    .await;
    rt.stop().await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(body["accepted"], false);
    assert_eq!(body["lightkey"]["recording"], false);
}

#[tokio::test]
async fn clearing_a_missing_take_is_reported_not_an_error() {
    let (router, rt) = app(osc_lights()).await;
    let (code, body) = call(&router, "DELETE", &format!("{LK}/takes/nope"), None).await;
    rt.stop().await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(body["accepted"], false);
    assert_eq!(body["message"], "That song has no capture.");
}

#[tokio::test]
async fn saving_lighting_settings_keeps_the_lightkey_block() {
    let (router, rt) = app(LightsSettings {
        enabled: true,
        lightkey: LightkeySettings {
            api_url: "http://10.0.0.5:8770".into(),
            replay: false,
        },
        ..LightsSettings::default()
    })
    .await;
    call(
        &router,
        "POST",
        "/api/v1/lights/settings",
        Some(json!({"enabled": true, "method": "osc", "osc_host": "10.0.0.9", "osc_port": 21600})),
    )
    .await;
    let (_, settings) = call(&router, "GET", "/api/v1/settings", None).await;
    rt.stop().await;
    let saved = &settings["settings"]["lights"];
    assert_eq!(
        saved["lightkey"],
        json!({"api_url": "http://10.0.0.5:8770", "replay": false})
    );
    assert_eq!(saved["osc_host"], "10.0.0.9");
}

#[tokio::test]
async fn old_settings_without_a_lightkey_block_load_with_replay_on() {
    let old = json!({"enabled": true, "method": "osc", "osc_host": "1.2.3.4", "cue_maps": {}});
    let loaded: LightsSettings = serde_json::from_value(old).unwrap();
    assert_eq!(
        loaded.lightkey,
        LightkeySettings {
            api_url: String::new(),
            replay: true
        }
    );
    // And a whole saved settings file from before the feature loads the same way.
    let file = json!({"schema_version": 2, "lights": {"enabled": true, "method": "osc"}});
    let persistent: PersistentSettings = serde_json::from_value(file).unwrap();
    assert!(persistent.lights.lightkey.replay);
}

#[tokio::test]
async fn the_four_lightkey_routes_validate_their_bodies_like_fastapi() {
    let (router, rt) = app(osc_lights()).await;
    let (code, body) = call(&router, "POST", &format!("{LK}/recording"), Some(json!({}))).await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["detail"][0]["type"], "missing");
    assert_eq!(body["detail"][0]["loc"], json!(["body", "recording"]));
    let (code, body) = call(
        &router,
        "PUT",
        &format!("{LK}/settings"),
        Some(json!({"replay": "maybe"})),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["detail"][0]["type"], "bool_parsing");
    let (code, body) = call(
        &router,
        "PUT",
        &format!("{LK}/settings"),
        Some(json!({"api_url": "h".repeat(301)})),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["detail"][0]["type"], "string_too_long");
    rt.stop().await;
}
