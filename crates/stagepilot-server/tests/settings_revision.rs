//! `settings_revision` (F-U1): the backend counts every save of the settings, whoever caused it, and
//! publishes the number on the Lightkey status and in the settings response, so an open window learns
//! that a recording finished without asking again and again for the (large) settings.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use stagepilot_core::config::{
    LightingMethod, LightsSettings, OscArgument, OscCueSource, OscLightingCue, OscMessage,
    SongLoudness,
};
use stagepilot_core::settings::{PersistentSettings, SettingsService};
use stagepilot_lights::lightkey_link::{SongLooks, SongLoudnessTake};
use stagepilot_server::{build_app, AppOptions, OutputBackends, Runtime};
use std::net::SocketAddr;
use std::sync::Arc;
use tower::ServiceExt;

async fn app() -> (axum::Router, Arc<Runtime>) {
    let service = SettingsService::ephemeral_persistent(PersistentSettings {
        lights: LightsSettings {
            enabled: true,
            method: LightingMethod::Osc,
            ..LightsSettings::default()
        },
        ..PersistentSettings::default()
    });
    let runtime = Runtime::with_backends(Arc::new(service), false, OutputBackends::fake_midi());
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

/// What a window sees: the number on the Lightkey status it polls and the number on the settings it holds.
async fn numbers(router: &axum::Router) -> (u64, u64) {
    let (_, lights) = call(router, "GET", "/api/v1/lights/lightkey", None).await;
    let (_, settings) = call(router, "GET", "/api/v1/settings", None).await;
    (
        lights["settings_revision"]
            .as_u64()
            .expect("on the lights status"),
        settings["settings_revision"]
            .as_u64()
            .expect("on the settings"),
    )
}

fn row(at: f64) -> OscLightingCue {
    OscLightingCue {
        id: uuid::Uuid::new_v4(),
        at_seconds: at,
        address: String::new(),
        args: vec![],
        label: String::new(),
        color: None,
        messages: vec![OscMessage {
            address: "/fixture/P1/overrides/dimmer".into(),
            args: vec![OscArgument::Float(0.5)],
        }],
        source: OscCueSource::Captured,
        edited: false,
        fade_seconds: 0.0,
    }
}

fn recording(key: &str) -> SongLooks {
    SongLooks {
        song_key: key.into(),
        song_title: "Elohim".into(),
        rows: vec![row(1.0), row(2.0)],
        effects: vec![],
        status: stagepilot_core::config::CaptureStatus {
            complete: true,
            coverage_seconds: 100.0,
            recorded_at: "2026-11-15T18:07:00Z".into(),
            project: None,
            simplified: None,
            start_lights: None,
        },
    }
}

#[tokio::test]
async fn the_number_starts_at_zero_and_a_read_never_moves_it() {
    let (router, rt) = app().await;
    assert_eq!(numbers(&router).await, (0, 0));
    for path in [
        "/api/v1/settings",
        "/api/v1/state",
        "/api/v1/lights",
        "/api/v1/lights/lightkey",
        "/api/v1/health/live",
    ] {
        call(&router, "GET", path, None).await;
    }
    assert_eq!(numbers(&router).await, (0, 0));
    rt.stop().await;
}

#[tokio::test]
async fn a_finished_recording_moves_it_on_the_lightkey_status_and_the_settings() {
    let (router, rt) = app().await;
    let (before, _) = numbers(&router).await;
    // The recording is saved by the backend itself: no request, no window involved.
    (rt.outputs.looks_sink)(recording("song-1")).await.unwrap();
    let (state, settings) = numbers(&router).await;
    rt.stop().await;
    assert_eq!(state, before + 1);
    assert_eq!(settings, state, "the settings copy holds that save");
}

#[tokio::test]
async fn a_loudness_take_a_cue_map_save_and_a_settings_save_each_count_once() {
    let (router, rt) = app().await;
    let (start, _) = numbers(&router).await;

    (rt.outputs.loudness_sink)(SongLoudnessTake {
        song_key: "song-1".into(),
        song_title: "Elohim".into(),
        loudness: SongLoudness {
            step_seconds: 0.25,
            values: vec![100, 120],
            takes: 1,
        },
    })
    .await
    .unwrap();
    assert_eq!(numbers(&router).await, (start + 1, start + 1), "loudness");

    let put = json!({
        "song_key": "song-1", "song_title": "Elohim", "method": "osc", "cues": [],
        "osc_cues": [{"id": "6f1c6a8e-3d63-4e0b-a4a6-1d9b3f0c2a11", "at_seconds": 5.0,
                      "address": "/live/cue/1", "args": [], "label": "Strobe", "color": null}],
        "revision": 1,
    });
    let (status, body) = call(&router, "PUT", "/api/v1/lights/cue-map", Some(put)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(numbers(&router).await, (start + 2, start + 2), "cue map");

    let (_, current) = call(&router, "GET", "/api/v1/settings", None).await;
    let mut settings = current["settings"].clone();
    settings["timezone"] = json!("America/Chicago");
    let (status, body) = call(&router, "PUT", "/api/v1/settings", Some(settings)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (state, settings) = numbers(&router).await;
    rt.stop().await;
    assert_eq!(state, settings);
    assert!(state > start + 2, "a settings save counts too: {state}");
}

#[tokio::test]
async fn a_restart_starts_the_count_again_at_zero() {
    let (router, rt) = app().await;
    (rt.outputs.looks_sink)(recording("song-1")).await.unwrap();
    assert!(numbers(&router).await.0 > 0);
    rt.stop().await;
    // A new process: a new runtime. A window reloads its settings whenever its live connection
    // opens, so a lower number than the one it holds is never mistaken for "nothing to do".
    let (router, rt) = app().await;
    assert_eq!(numbers(&router).await, (0, 0));
    rt.stop().await;
}
