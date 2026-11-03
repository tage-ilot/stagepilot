//! Loudness over HTTP: the input picker's data, the saved input choice, averaging repeat
//! recordings into a song, clearing, and that cue-map saves never wipe the envelope.

use async_trait::async_trait;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use stagepilot_core::config::{LightingMethod, LightsSettings, SongLoudness};
use stagepilot_core::settings::{PersistentSettings, SettingsService};
use stagepilot_lights::lightkey_link::{HttpFailure, LightkeyHttp, SongLoudnessTake};
use stagepilot_lights::loudness::{byte_db, level_byte};
use stagepilot_osc::fake::FakeOscBackend;
use stagepilot_osc::OscBackend;
use stagepilot_server::{build_app, AppOptions, OutputBackends, Runtime};
use std::net::SocketAddr;
use std::sync::Arc;
use tower::ServiceExt;

struct FakeAgent;
#[async_trait]
impl LightkeyHttp for FakeAgent {
    async fn get_json(&self, url: &str, _q: &[(&str, String)]) -> Result<Value, HttpFailure> {
        if url.ends_with("/time") {
            return Ok(json!({"t": 1000.0}));
        }
        Ok(json!({"events": [], "state": {"online": true, "active": [], "seq": 1}}))
    }
}

async fn app() -> (axum::Router, Arc<Runtime>) {
    let service = SettingsService::ephemeral_persistent(PersistentSettings {
        lights: LightsSettings {
            enabled: true,
            method: LightingMethod::Osc,
            ..LightsSettings::default()
        },
        ..PersistentSettings::default()
    });
    let osc = Arc::new(FakeOscBackend::default());
    let backends = OutputBackends {
        osc: Arc::new(move || osc.clone() as Arc<dyn OscBackend>),
        lightkey_http: Arc::new(|| Arc::new(FakeAgent)),
        ..OutputBackends::fake_midi()
    };
    let runtime = Runtime::with_backends(Arc::new(service), false, backends);
    runtime.start().await;
    (
        build_app(runtime.clone(), AppOptions { web_root: None }),
        runtime,
    )
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
    request.extensions_mut().insert(ConnectInfo(
        "127.0.0.1:50000".parse::<SocketAddr>().unwrap(),
    ));
    let response = router.clone().oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    let bytes = body.collect().await.unwrap().to_bytes();
    (
        parts.status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn take(values: &[u8]) -> SongLoudnessTake {
    SongLoudnessTake {
        song_key: "song-1".into(),
        song_title: "Holy Forever".into(),
        loudness: SongLoudness {
            step_seconds: 0.25,
            values: values.to_vec(),
            takes: 1,
        },
    }
}

async fn saved_loudness(router: &axum::Router) -> Value {
    let (_, settings) = call(router, "GET", "/api/v1/settings", None).await;
    settings["settings"]["lights"]["cue_maps"]["song-1"]["loudness"].clone()
}

#[tokio::test]
async fn repeat_recordings_average_into_the_song() {
    let (router, rt) = app().await;
    let a = level_byte(-20.0);
    let b = level_byte(-30.0);
    (rt.outputs.loudness_sink)(take(&[a, a])).await.unwrap();
    let first = saved_loudness(&router).await;
    assert_eq!(first["takes"], 1);
    assert_eq!(first["values"], json!([a, a]));
    (rt.outputs.loudness_sink)(take(&[b, 0, b])).await.unwrap();
    let second = saved_loudness(&router).await;
    rt.stop().await;
    assert_eq!(second["takes"], 2);
    let values: Vec<u8> = serde_json::from_value(second["values"].clone()).unwrap();
    assert!(
        (byte_db(values[0]).unwrap() + 22.6).abs() < 0.3,
        "{values:?}"
    );
    assert_eq!(
        values[1], a,
        "a slot the new recording missed keeps the old value"
    );
    assert_eq!(values[2], b);
}

#[tokio::test]
async fn a_cue_map_save_keeps_the_loudness_and_clear_removes_only_it() {
    let (router, rt) = app().await;
    (rt.outputs.loudness_sink)(take(&[100, 120])).await.unwrap();
    let put = json!({
        "song_key": "song-1", "song_title": "Holy Forever", "method": "osc",
        "cues": [],
        "osc_cues": [{"id": "6f1c6a8e-3d63-4e0b-a4a6-1d9b3f0c2a11", "at_seconds": 5.0, "address": "/live/cue/1", "args": [], "label": "Strobe", "color": null}],
    });
    let (status, _) = call(&router, "PUT", "/api/v1/lights/cue-map", Some(put)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        saved_loudness(&router).await["values"],
        json!([100, 120]),
        "kept"
    );

    let (status, body) = call(
        &router,
        "POST",
        "/api/v1/lights/loudness/clear",
        Some(json!({"song_key": "song-1"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["accepted"], true);
    assert!(saved_loudness(&router).await.is_null());
    let (_, settings) = call(&router, "GET", "/api/v1/settings", None).await;
    assert_eq!(
        settings["settings"]["lights"]["cue_maps"]["song-1"]["osc_cues"][0]["label"],
        "Strobe"
    );
    let (_, again) = call(
        &router,
        "POST",
        "/api/v1/lights/loudness/clear",
        Some(json!({"song_key": "song-1"})),
    )
    .await;
    rt.stop().await;
    assert_eq!(again["accepted"], false);
}

#[tokio::test]
async fn clearing_a_song_that_only_had_loudness_removes_its_cue_map() {
    let (router, rt) = app().await;
    (rt.outputs.loudness_sink)(take(&[100])).await.unwrap();
    call(
        &router,
        "POST",
        "/api/v1/lights/loudness/clear",
        Some(json!({"song_key": "song-1"})),
    )
    .await;
    let (_, settings) = call(&router, "GET", "/api/v1/settings", None).await;
    rt.stop().await;
    assert!(settings["settings"]["lights"]["cue_maps"]
        .get("song-1")
        .is_none());
}

#[tokio::test]
async fn the_value_cap_holds_whatever_the_recorder_sends() {
    let (router, rt) = app().await;
    (rt.outputs.loudness_sink)(take(&vec![9; 5000]))
        .await
        .unwrap();
    let saved = saved_loudness(&router).await;
    rt.stop().await;
    assert_eq!(saved["values"].as_array().unwrap().len(), 4000);
}

#[tokio::test]
async fn the_input_choice_is_saved_and_the_picker_hides_where_audio_is_unavailable() {
    let (router, rt) = app().await;
    let (status, body) = call(&router, "GET", "/api/v1/lights/lightkey/audio", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["available"], false);
    assert!(body["reason"].as_str().unwrap().len() > 10);
    assert_eq!(body["devices"], json!([]));

    let (status, _) = call(
        &router,
        "PUT",
        "/api/v1/lights/lightkey/settings",
        Some(json!({"audio_device": "Scarlett 2i2"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = call(&router, "GET", "/api/v1/lights/lightkey/audio", None).await;
    assert_eq!(body["selected"], "Scarlett 2i2");
    // The meter request is harmless where nothing can be opened.
    let (status, _) = call(
        &router,
        "POST",
        "/api/v1/lights/lightkey/audio/meter",
        Some(json!({"on": true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Unrelated settings saves keep the choice; an empty name goes back to the default.
    call(
        &router,
        "PUT",
        "/api/v1/lights/lightkey/settings",
        Some(json!({"replay": true})),
    )
    .await;
    let (_, body) = call(&router, "GET", "/api/v1/lights/lightkey/audio", None).await;
    assert_eq!(body["selected"], "Scarlett 2i2");
    call(
        &router,
        "PUT",
        "/api/v1/lights/lightkey/settings",
        Some(json!({"audio_device": ""})),
    )
    .await;
    let (_, body) = call(&router, "GET", "/api/v1/lights/lightkey/audio", None).await;
    let (bad, _) = call(
        &router,
        "PUT",
        "/api/v1/lights/lightkey/settings",
        Some(json!({"audio_device": 5})),
    )
    .await;
    rt.stop().await;
    assert!(body["selected"].is_null());
    assert_eq!(bad, StatusCode::UNPROCESSABLE_ENTITY);
}
