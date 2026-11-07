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
    assert!(body.get("takes").is_none(), "the takes list is gone");
    assert!(body.get("captured").is_none());
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
async fn the_takes_route_is_gone() {
    let (router, rt) = app(osc_lights()).await;
    let (code, _) = call(&router, "DELETE", &format!("{LK}/takes/nope"), None).await;
    rt.stop().await;
    assert!(
        code == StatusCode::NOT_FOUND || code == StatusCode::METHOD_NOT_ALLOWED,
        "{code}"
    );
}

#[tokio::test]
async fn detail_is_saved_and_validated() {
    let (router, rt) = app(osc_lights()).await;
    let (code, body) = call(
        &router,
        "PUT",
        &format!("{LK}/settings"),
        Some(json!({"detail": "fine"})),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert!(body["lightkey"]["api_url"].is_string());
    let (_, settings) = call(&router, "GET", "/api/v1/settings", None).await;
    assert_eq!(settings["settings"]["lights"]["lightkey"]["detail"], "fine");
    // sending only the address keeps the detail
    call(
        &router,
        "PUT",
        &format!("{LK}/settings"),
        Some(json!({"replay": false})),
    )
    .await;
    let (_, settings) = call(&router, "GET", "/api/v1/settings", None).await;
    assert_eq!(settings["settings"]["lights"]["lightkey"]["detail"], "fine");
    let (code, body) = call(
        &router,
        "PUT",
        &format!("{LK}/settings"),
        Some(json!({"detail": "ultra"})),
    )
    .await;
    rt.stop().await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["detail"][0]["loc"], json!(["body", "detail"]));
}

#[tokio::test]
async fn frontend_detail_level_round_trips_through_settings_and_status() {
    // The exact body `saveLightkeySettings({ detail_level })` sends (frontend/src/api.ts), and the
    // status field `LightkeyStrip` reads back (`status.detail_level`).
    let (router, rt) = app(osc_lights()).await;
    for (sent, expect_status) in [("fine", json!("fine")), ("coarse", json!("coarse"))] {
        let (code, body) = call(
            &router,
            "PUT",
            &format!("{LK}/settings"),
            Some(json!({ "detail_level": sent })),
        )
        .await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body["lightkey"]["detail_level"], expect_status);
        let (_, status) = call(&router, "GET", LK, None).await;
        assert_eq!(status["detail_level"], expect_status);
        let (_, settings) = call(&router, "GET", "/api/v1/settings", None).await;
        assert_eq!(settings["settings"]["lights"]["lightkey"]["detail"], sent);
    }
    // normal is the default: left out of the status, so the frontend's `?? "normal"` applies
    let (_, body) = call(
        &router,
        "PUT",
        &format!("{LK}/settings"),
        Some(json!({ "detail_level": "normal" })),
    )
    .await;
    assert!(body["lightkey"].get("detail_level").is_none());
    let (code, body) = call(
        &router,
        "PUT",
        &format!("{LK}/settings"),
        Some(json!({ "detail_level": "Fine" })),
    )
    .await;
    rt.stop().await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["detail"][0]["loc"], json!(["body", "detail_level"]));
}

#[tokio::test]
async fn saving_lighting_settings_keeps_the_lightkey_block() {
    let (router, rt) = app(LightsSettings {
        enabled: true,
        lightkey: LightkeySettings {
            api_url: "http://10.0.0.5:8770".into(),
            replay: false,
            ..LightkeySettings::default()
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
            replay: true,
            mode: stagepilot_core::config::LightkeyMode::Auto,
            detail: stagepilot_core::config::LightkeyDetail::Normal,
            audio_device: None,
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

// ------------------------------------------------------------------------------ look rows

#[tokio::test]
async fn a_look_row_round_trips_through_the_cue_map_api_and_the_saved_settings() {
    let (router, rt) = app(osc_lights()).await;
    let look = json!({
        "id": "6f0e2f0e-7a8e-4c52-9c7d-5f3f0f0f0f01",
        "at_seconds": 12.5,
        "address": "",
        "args": [],
        "label": "Look - 2 fixtures",
        "color": null,
        "messages": [
            {"address": "/fixture/MH 1/overrides/dimmer", "args": [{"type": "float", "value": 0.5}]},
            {"address": "/fixture/MH 2/overrides/dimmer", "args": [{"type": "float", "value": 1.0}]}
        ],
        "source": "captured"
    });
    let plain = json!({
        "id": "6f0e2f0e-7a8e-4c52-9c7d-5f3f0f0f0f02",
        "at_seconds": 20.0,
        "address": "/live/cue/1",
        "args": [],
        "label": "Verse",
        "color": null
    });
    let (code, body) = call(
        &router,
        "PUT",
        "/api/v1/lights/cue-map",
        Some(json!({
            "song_key": "s1", "song_title": "Song", "method": "osc",
            "osc_cues": [look.clone(), plain.clone()]
        })),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let (_, settings) = call(&router, "GET", "/api/v1/settings", None).await;
    let saved = &settings["settings"]["lights"]["cue_maps"]["s1"]["osc_cues"];
    assert_eq!(saved[0], look, "look row saved as sent");
    assert_eq!(saved[1], plain, "plain row gains no new keys");
    // Over the per-row cap the API answers 422.
    let many: Vec<Value> = (0..=stagepilot_core::config::MAX_OSC_MESSAGES_PER_ROW)
        .map(|_| json!({"address": "/a", "args": []}))
        .collect();
    let mut too_big = look.clone();
    too_big["messages"] = json!(many);
    let (code, _) = call(
        &router,
        "PUT",
        "/api/v1/lights/cue-map",
        Some(
            json!({"song_key": "s1", "song_title": "Song", "method": "osc", "osc_cues": [too_big]}),
        ),
    )
    .await;
    rt.stop().await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn row_saves_keep_effects_and_only_a_remove_list_drops_one() {
    use stagepilot_core::config::{EffectMember, EffectStep, LightingEffect, OscCueSource};
    let (router, rt) = app(osc_lights()).await;
    let effect = |label: &str| LightingEffect {
        id: uuid::Uuid::new_v4(),
        label: label.into(),
        start_seconds: 10.0,
        end_seconds: 50.0,
        period_seconds: 4.0,
        members: vec![EffectMember {
            fixture: "P1".into(),
            offset_seconds: 0.0,
        }],
        cycle: vec![EffectStep {
            at_seconds: 0.0,
            property: "dimmer".into(),
            value: vec![1.0],
            fade_seconds: 2.0,
        }],
        source: OscCueSource::Captured,
        edited: false,
    };
    let (keep, drop_it) = (effect("keep"), effect("drop"));
    let mut lights = osc_lights();
    lights.cue_maps.insert(
        "s1".into(),
        stagepilot_core::config::SongLightingCueMap {
            song_key: "s1".into(),
            song_title: "Song".into(),
            cues: vec![],
            osc_cues: vec![],
            loudness: None,
            captured_edited: false,
            capture: None,
            effects: vec![keep.clone(), drop_it.clone()],
        },
    );
    rt.settings_service.persist_lights(lights).unwrap();
    let count = |settings: &Value| {
        settings["settings"]["lights"]["cue_maps"]["s1"]["effects"]
            .as_array()
            .map_or(0, Vec::len)
    };
    let put = |extra: Value| {
        let mut body =
            json!({"song_key": "s1", "song_title": "Song", "method": "osc", "osc_cues": []});
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        body
    };
    let (code, _) = call(
        &router,
        "PUT",
        "/api/v1/lights/cue-map",
        Some(put(json!({}))),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let (_, settings) = call(&router, "GET", "/api/v1/settings", None).await;
    assert_eq!(count(&settings), 2, "a plain row save keeps every effect");
    let (code, _) = call(
        &router,
        "PUT",
        "/api/v1/lights/cue-map",
        Some(put(
            json!({"remove_effect_ids": [drop_it.id.to_string(), "no-such-id"]}),
        )),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let (_, settings) = call(&router, "GET", "/api/v1/settings", None).await;
    rt.stop().await;
    assert_eq!(count(&settings), 1);
    assert_eq!(
        settings["settings"]["lights"]["cue_maps"]["s1"]["effects"][0]["label"],
        "keep"
    );
}

// ------------------------------------------------------------ recordings save to the timeline

fn captured_row(at: f64, address: &str, x: f64) -> stagepilot_core::config::OscLightingCue {
    use stagepilot_core::config::{OscArgument, OscCueSource, OscLightingCue, OscMessage};
    OscLightingCue {
        id: uuid::Uuid::new_v4(),
        at_seconds: at,
        address: address.into(),
        args: vec![],
        label: "Look - 1 fixture".into(),
        color: None,
        messages: vec![OscMessage {
            address: address.into(),
            args: vec![OscArgument::Float(x)],
        }],
        source: OscCueSource::Captured,
        edited: false,
        fade_seconds: 0.0,
    }
}

#[tokio::test]
async fn a_finished_recording_replaces_only_the_captured_rows_of_that_song() {
    use stagepilot_lights::lightkey_link::SongLooks;
    let (router, rt) = app(osc_lights()).await;
    let a = "/fixture/P1/overrides/dimmer";
    let save = |rows| {
        (rt.outputs.looks_sink)(SongLooks {
            song_key: "song-1".into(),
            song_title: "Holy Forever".into(),
            rows,
            effects: vec![],
            status: complete(),
        })
    };
    // a manual row the operator added by hand
    let mut manual = captured_row(5.0, "/live/cue/1", 0.0);
    manual.source = stagepilot_core::config::OscCueSource::Manual;
    manual.messages.clear();
    manual.label = "Strobe".into();
    let mut lights = osc_lights();
    lights.cue_maps.insert(
        "song-1".into(),
        stagepilot_core::config::SongLightingCueMap {
            song_key: "song-1".into(),
            song_title: "Holy Forever".into(),
            cues: vec![],
            osc_cues: vec![manual],
            loudness: None,
            captured_edited: false,
            capture: None,
            effects: Vec::new(),
        },
    );
    rt.settings_service.persist_lights(lights).unwrap();
    save(vec![captured_row(1.0, a, 0.1), captured_row(2.0, a, 0.9)])
        .await
        .unwrap();
    save(vec![captured_row(3.0, a, 0.5)]).await.unwrap();
    let (_, settings) = call(&router, "GET", "/api/v1/settings", None).await;
    rt.stop().await;
    let rows = settings["settings"]["lights"]["cue_maps"]["song-1"]["osc_cues"]
        .as_array()
        .unwrap();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(rows[0]["source"], "captured");
    assert_eq!(rows[0]["at_seconds"], 3.0);
    assert_eq!(rows[1]["label"], "Strobe", "the manual row stays");
}

#[tokio::test]
async fn a_recording_past_the_timeline_caps_asks_for_a_coarser_detail() {
    use stagepilot_lights::lightkey_link::{LooksError, SongLooks};
    let (_router, rt) = app(osc_lights()).await;
    let rows: Vec<_> = (0..=1_000)
        .map(|i| captured_row(f64::from(i), "/fixture/P1/overrides/dimmer", 0.5))
        .collect();
    let result = (rt.outputs.looks_sink)(SongLooks {
        song_key: "big".into(),
        song_title: "Big".into(),
        rows,
        effects: vec![],
        status: complete(),
    })
    .await;
    rt.stop().await;
    assert_eq!(result, Err(LooksError::TooLarge));
}

#[tokio::test]
async fn an_old_takes_file_next_to_the_settings_is_ignored_and_left_alone() {
    use stagepilot_core::settings::MemoryCredentialStore;
    use stagepilot_core::settings_file::SettingsFileStore;
    let dir = tempfile::tempdir().unwrap();
    let takes = dir.path().join("lightkey-takes.json");
    let old = r#"{"version":1,"takes":[{"song_key":"x","song_title":"Old","recorded_at":"2026-10-20T20:00:00Z","duration_seconds":10}]}"#;
    std::fs::write(&takes, old).unwrap();
    let service = SettingsService::new(
        Box::new(SettingsFileStore::new(dir.path().join("settings.json"))),
        Box::new(MemoryCredentialStore::new(None)),
        Default::default(),
    );
    service.load();
    let backends = OutputBackends {
        lightkey_http: Arc::new(|| Arc::new(FakeAgent { down: false })),
        ..OutputBackends::fake_midi()
    };
    let runtime = Runtime::with_backends(Arc::new(service), false, backends);
    runtime.start().await;
    let body = runtime.outputs.lights.lightkey_status().await;
    runtime.stop().await;
    assert!(body.captured.is_none());
    assert_eq!(
        std::fs::read_to_string(&takes).unwrap(),
        old,
        "not read, not rewritten, not deleted"
    );
    assert!(!dir.path().join("lightkey-takes.json.bad").exists());
}

// ------------------------------------------------------------ Simplify this song, on the backend

/// A song with a manual row and `count` captured dimmer ramp rows 0.1 s apart (dense).
async fn seed_dense_song(rt: &Runtime, count: usize) -> Vec<uuid::Uuid> {
    use stagepilot_core::config::{OscCueSource, SongLightingCueMap};
    let mut manual = captured_row(5.0, "/live/cue/1", 0.0);
    manual.source = OscCueSource::Manual;
    manual.messages.clear();
    manual.label = "Strobe".into();
    let mut osc_cues = vec![manual];
    for i in 0..count {
        osc_cues.push(captured_row(
            i as f64 * 0.1,
            "/fixture/P1/overrides/dimmer",
            0.2 + 0.001 * i as f64 * 5.0,
        ));
    }
    let ids = osc_cues.iter().map(|c| c.id).collect();
    let mut lights = osc_lights();
    lights.cue_maps.insert(
        "song-1".into(),
        SongLightingCueMap {
            song_key: "song-1".into(),
            song_title: "Holy Forever".into(),
            cues: vec![],
            osc_cues,
            loudness: None,
            captured_edited: false,
            capture: None,
            effects: Vec::new(),
        },
    );
    rt.settings_service.persist_lights(lights).unwrap();
    ids
}

#[tokio::test]
async fn simplify_previews_then_applies_and_leaves_manual_rows_alone() {
    let (router, rt) = app(osc_lights()).await;
    seed_dense_song(&rt, 100).await;
    let url = format!("{LK}/simplify");
    let (code, preview) = call(
        &router,
        "POST",
        &url,
        Some(json!({"song_key": "song-1", "detail_level": "coarse"})),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(preview["accepted"], true);
    assert_eq!(preview["applied"], false);
    assert_eq!(preview["rows_before"], 100);
    let after = preview["rows_after"].as_u64().unwrap();
    assert!(after < 100 && after > 0, "{preview}");
    // a preview changes nothing
    let (_, settings) = call(&router, "GET", "/api/v1/settings", None).await;
    let rows = |s: &Value| s["settings"]["lights"]["cue_maps"]["song-1"]["osc_cues"].clone();
    assert_eq!(rows(&settings).as_array().unwrap().len(), 101);
    // apply saves exactly what the preview counted
    let (_, applied) = call(
        &router,
        "POST",
        &url,
        Some(json!({"song_key": "song-1", "detail_level": "coarse", "apply": true})),
    )
    .await;
    assert_eq!(applied["applied"], true);
    assert_eq!(applied["rows_after"].as_u64().unwrap(), after);
    let (_, settings) = call(&router, "GET", "/api/v1/settings", None).await;
    let saved = rows(&settings);
    let saved = saved.as_array().unwrap();
    assert_eq!(
        saved.len() as u64,
        after + 1,
        "captured rows + the manual row"
    );
    assert_eq!(
        saved.iter().filter(|r| r["label"] == "Strobe").count(),
        1,
        "the manual row stays"
    );
    assert!(saved
        .iter()
        .filter(|r| r["source"] == "captured")
        .all(|r| r["label"].as_str().unwrap().starts_with("Look - ")));
    // simplifying again finds nothing more to do at the same detail
    let (_, again) = call(
        &router,
        "POST",
        &url,
        Some(json!({"song_key": "song-1", "detail_level": "coarse", "apply": true})),
    )
    .await;
    rt.stop().await;
    assert_eq!(again["applied"], false);
}

#[tokio::test]
async fn simplify_validates_its_body_and_a_song_without_looks() {
    let (router, rt) = app(osc_lights()).await;
    let url = format!("{LK}/simplify");
    let (code, body) = call(&router, "POST", &url, Some(json!({}))).await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["detail"][0]["loc"], json!(["body", "song_key"]));
    let (code, body) = call(
        &router,
        "POST",
        &url,
        Some(json!({"song_key": "x", "detail_level": "Coarse"})),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["detail"][0]["loc"], json!(["body", "detail_level"]));
    let (code, body) = call(&router, "POST", &url, Some(json!({"song_key": "nope"}))).await;
    rt.stop().await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(body["accepted"], false);
    assert_eq!(body["rows_before"], 0);
}

// ------------------------------------------------------------ captured_edited

#[tokio::test]
async fn deleting_a_whole_captured_row_sets_the_song_flag_and_a_recording_clears_it() {
    use stagepilot_lights::lightkey_link::SongLooks;
    let (router, rt) = app(osc_lights()).await;
    let ids = seed_dense_song(&rt, 3).await;
    let put = |keep: &[uuid::Uuid]| {
        let cues: Vec<Value> = {
            // read the current rows back and keep the chosen ones
            let lights = rt.settings_service.effective_runtime_settings().lights;
            lights.cue_maps["song-1"]
                .osc_cues
                .iter()
                .filter(|c| keep.contains(&c.id))
                .map(|c| serde_json::to_value(c).unwrap())
                .collect()
        };
        json!({"song_key": "song-1", "song_title": "Holy Forever", "method": "osc", "osc_cues": cues})
    };
    let flag = |s: &Value| s["settings"]["lights"]["cue_maps"]["song-1"]["captured_edited"].clone();
    // saving everything unchanged does not flag
    let (code, _) = call(&router, "PUT", "/api/v1/lights/cue-map", Some(put(&ids))).await;
    assert_eq!(code, StatusCode::OK);
    let (_, settings) = call(&router, "GET", "/api/v1/settings", None).await;
    assert_eq!(flag(&settings), Value::Null, "omitted while false");
    // dropping one captured row flags the song
    let (code, _) = call(
        &router,
        "PUT",
        "/api/v1/lights/cue-map",
        Some(put(&ids[..3])),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let (_, settings) = call(&router, "GET", "/api/v1/settings", None).await;
    assert_eq!(flag(&settings), json!(true));
    // the flag survives a later unchanged save
    call(
        &router,
        "PUT",
        "/api/v1/lights/cue-map",
        Some(put(&ids[..3])),
    )
    .await;
    let (_, settings) = call(&router, "GET", "/api/v1/settings", None).await;
    assert_eq!(flag(&settings), json!(true));
    // a new recording replaces the captured rows and clears it
    (rt.outputs.looks_sink)(SongLooks {
        song_key: "song-1".into(),
        song_title: "Holy Forever".into(),
        rows: vec![captured_row(1.0, "/fixture/P1/overrides/dimmer", 0.5)],
        effects: vec![],
        status: complete(),
    })
    .await
    .unwrap();
    let (_, settings) = call(&router, "GET", "/api/v1/settings", None).await;
    rt.stop().await;
    assert_eq!(flag(&settings), Value::Null);
}

// ------------------------------------------------------------ cue-map safety (U-01, C-01, K-01)

fn revision(settings: &Value, key: &str) -> Value {
    settings["settings"]["lights"]["cue_revisions"][key].clone()
}

fn cue_map_body(
    rows: Vec<stagepilot_core::config::OscLightingCue>,
    revision: Option<u32>,
) -> Value {
    let mut body = json!({
        "song_key": "song-1",
        "song_title": "Holy Forever",
        "method": "osc",
        "osc_cues": rows.iter().map(|c| serde_json::to_value(c).unwrap()).collect::<Vec<_>>(),
    });
    if let Some(r) = revision {
        body["revision"] = json!(r);
    }
    body
}

#[tokio::test]
async fn a_stale_cue_map_put_is_refused_with_409_and_the_fresh_recording_survives() {
    use stagepilot_lights::lightkey_link::SongLooks;
    let (router, rt) = app(osc_lights()).await;
    let first = captured_row(1.0, "/fixture/P1/overrides/dimmer", 0.5);
    let (code, _) = call(
        &router,
        "PUT",
        "/api/v1/lights/cue-map",
        Some(cue_map_body(vec![first.clone()], None)),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let (_, settings) = call(&router, "GET", "/api/v1/settings", None).await;
    assert_eq!(revision(&settings, "song-1"), json!(1));
    // The panel opened the song at revision 1. A recording finishes and saves new rows.
    (rt.outputs.looks_sink)(SongLooks {
        song_key: "song-1".into(),
        song_title: "Holy Forever".into(),
        rows: vec![captured_row(2.0, "/fixture/P1/overrides/dimmer", 0.9)],
        effects: vec![],
        status: complete(),
    })
    .await
    .unwrap();
    let (_, settings) = call(&router, "GET", "/api/v1/settings", None).await;
    assert_eq!(revision(&settings, "song-1"), json!(2));
    // The operator then edits the old list: refused, nothing overwritten.
    let (code, body) = call(
        &router,
        "PUT",
        "/api/v1/lights/cue-map",
        Some(cue_map_body(vec![first], Some(1))),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    let saved = rt.settings_service.effective_runtime_settings().lights;
    assert_eq!(
        saved.cue_maps["song-1"].osc_cues[0].at_seconds, 2.0,
        "the recording is still there"
    );
    // With the current revision the same edit goes through, and a request without one is accepted.
    let rows = saved.cue_maps["song-1"].osc_cues.clone();
    let (code, _) = call(
        &router,
        "PUT",
        "/api/v1/lights/cue-map",
        Some(cue_map_body(rows.clone(), Some(2))),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let (code, _) = call(
        &router,
        "PUT",
        "/api/v1/lights/cue-map",
        Some(cue_map_body(rows, None)),
    )
    .await;
    rt.stop().await;
    assert_eq!(code, StatusCode::OK);
}

#[tokio::test]
async fn the_manual_cue_map_save_and_the_lights_settings_save_wait_for_the_cue_lock() {
    use std::time::Duration;
    let (router, rt) = app(osc_lights()).await;
    let guard = rt.outputs.cue_lock.lock().await; // a recording is being saved
    let row = captured_row(1.0, "/fixture/P1/overrides/dimmer", 0.5);
    let put = {
        let router = router.clone();
        tokio::spawn(async move {
            call(
                &router,
                "PUT",
                "/api/v1/lights/cue-map",
                Some(cue_map_body(vec![row], None)),
            )
            .await
        })
    };
    let post = {
        let router = router.clone();
        tokio::spawn(async move {
            call(
                &router,
                "POST",
                "/api/v1/lights/settings",
                Some(json!({"enabled": true, "method": "osc"})),
            )
            .await
        })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !put.is_finished(),
        "the cue-map save must wait for the cue lock (C-01)"
    );
    assert!(
        !post.is_finished(),
        "the lights settings save must wait for the cue lock (C-01)"
    );
    drop(guard);
    let (code, _) = put.await.unwrap();
    assert_eq!(code, StatusCode::OK);
    let (code, _) = post.await.unwrap();
    rt.stop().await;
    assert_eq!(code, StatusCode::OK);
}

#[tokio::test]
async fn a_cue_map_put_over_the_default_two_megabyte_limit_is_accepted() {
    // K-01: a song near the per-song caps is about 3 MB of JSON.
    use stagepilot_core::config::{OscArgument, OscMessage};
    let (router, rt) = app(osc_lights()).await;
    let rows: Vec<_> = (0..1000)
        .map(|i| {
            let mut row = captured_row(f64::from(i) * 0.1, "/fixture/P1/overrides/dimmer", 0.5);
            row.messages = (0..15)
                .map(|m| OscMessage {
                    address: format!("/fixture/Moving Head {m}/overrides/dimmer"),
                    args: vec![OscArgument::String("x".repeat(60)), OscArgument::Float(0.5)],
                })
                .collect();
            row
        })
        .collect();
    let body = cue_map_body(rows, None);
    assert!(
        body.to_string().len() > 2 * 1024 * 1024,
        "{}",
        body.to_string().len()
    );
    let (code, response) = call(&router, "PUT", "/api/v1/lights/cue-map", Some(body)).await;
    rt.stop().await;
    assert_eq!(code, StatusCode::OK, "{response}");
}

fn complete() -> stagepilot_core::config::CaptureStatus {
    stagepilot_core::config::CaptureStatus {
        complete: true,
        coverage_seconds: 100.0,
        recorded_at: "2026-11-02T00:00:00Z".into(),
    }
}

#[tokio::test]
async fn saving_general_settings_from_an_older_page_keeps_captures_revisions_and_audio_device() {
    use stagepilot_lights::lightkey_link::SongLooks;
    let (router, rt) = app(osc_lights()).await;
    // A page loads the settings, then stays open.
    let (_, old) = call(&router, "GET", "/api/v1/settings", None).await;
    let stale = old["settings"].clone();
    // Meanwhile a song is captured and an audio device is chosen.
    (rt.outputs.looks_sink)(SongLooks {
        song_key: "song-1".into(),
        song_title: "Holy Forever".into(),
        rows: vec![captured_row(1.0, "/fixture/P1/overrides/dimmer", 0.5)],
        effects: vec![],
        status: complete(),
    })
    .await
    .unwrap();
    let (code, _) = call(
        &router,
        "PUT",
        &format!("{LK}/settings"),
        Some(json!({"audio_device": "Scarlett 2i2"})),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let (_, fresh) = call(&router, "GET", "/api/v1/settings", None).await;
    let fresh_lights = fresh["settings"]["lights"].clone();
    assert_eq!(fresh_lights["lightkey"]["audio_device"], "Scarlett 2i2");
    // Saving a setting the page really edits, sent with the older copy.
    let mut body = stale;
    body["timezone"] = json!("America/Chicago");
    let (code, response) = call(&router, "PUT", "/api/v1/settings", Some(body)).await;
    let (_, after) = call(&router, "GET", "/api/v1/settings", None).await;
    rt.stop().await;
    assert_eq!(code, StatusCode::OK, "{response}");
    assert_eq!(after["settings"]["timezone"], "America/Chicago");
    assert_eq!(after["settings"]["lights"], fresh_lights);
    assert_eq!(
        after["settings"]["lights"]["cue_maps"]["song-1"]["osc_cues"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
}
