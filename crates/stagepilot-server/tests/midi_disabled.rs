//! While the MIDI plugin is off (m12): the setup panel still gets the system's MIDI ports, read-only.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use stagepilot_core::config::{IntegrationModes, MidiSource};
use stagepilot_core::settings::{PersistentSettings, SettingsService};
use stagepilot_server::{build_app, AppOptions, OutputBackends, Runtime};
use std::net::SocketAddr;
use std::sync::Arc;
use tower::ServiceExt;

async fn get(router: &axum::Router, path: &str) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method("GET")
        .uri(path)
        .header(header::ACCEPT, "application/json")
        .body(Body::empty())
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

async fn app(source: MidiSource) -> (axum::Router, Arc<Runtime>) {
    let service = SettingsService::ephemeral_persistent(PersistentSettings {
        integration_modes: IntegrationModes {
            midi_source: source,
            ..Default::default()
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

#[tokio::test(flavor = "multi_thread")]
async fn midi_inputs_lists_system_ports_read_only_while_the_plugin_is_off() {
    let (router, rt) = app(MidiSource::PlaybackApi).await;
    let (status, body) = get(&router, "/api/v1/midi/inputs").await;
    rt.stop().await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["enabled"], false);
    assert_eq!(body["selected_input_name"], Value::Null);
    let inputs = body["inputs"].as_array().unwrap();
    assert!(!inputs.is_empty(), "{body}");
    assert!(inputs
        .iter()
        .all(|i| i["selected"] == false && i["connected"] == false));
}
