//! HTTP-level tests of the Playback routes: the Rust counterparts of the HTTP halves of
//! `test_playback_integration.py`, `test_playback_settings.py`, `test_playback_arrangement.py`
//! and `test_playback_lengths.py`. The server runs in demo mode against the Rust fake Playback
//! (`stagepilot_playback::testkit`); the network scan is replaced by a scripted scanner.
//!
//! Timing: nothing here depends on a real heartbeat margin. Waits poll a condition with a 15 s
//! ceiling (they return the moment it holds), so a slow shared runner only makes a test slower.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use stagepilot_core::settings::{RuntimeSettings, SettingsService};
use stagepilot_playback::find::{
    DiscoverySource, LanCheck, ScanCandidate, ScanHooks, ScanOutcome, ScanReport,
};
use stagepilot_playback::plugin::{PluginOptions, Scanner};
use stagepilot_playback::testkit::FakePlayback;
use stagepilot_server::{build_app, AppOptions, Runtime};
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::ServiceExt;

const STEP: Duration = Duration::from_millis(10);
const LIMIT: usize = 1500; // 15 s

#[derive(Clone, Default)]
struct ScriptedScanner(Arc<Mutex<Option<ScanReport>>>);

impl Scanner for ScriptedScanner {
    fn scan(
        &self,
        _host: Option<String>,
        _port: u16,
        _hooks: ScanHooks,
    ) -> Pin<Box<dyn Future<Output = ScanReport> + Send>> {
        let report = self.0.lock().unwrap().clone().expect("scan report set");
        Box::pin(async move { report })
    }
}

fn report(outcome: ScanOutcome, candidates: Vec<ScanCandidate>) -> ScanReport {
    ScanReport {
        candidates,
        outcome,
        error_class: None,
        message: None,
        typed_host: None,
        networks: vec![],
        interfaces: vec![],
        hosts_total: 0,
        hosts_probed: 0,
        error_counts: Default::default(),
        sample_errors: vec![],
        loopback_ok: None,
        lan_check: LanCheck::Skipped,
        duration: Duration::ZERO,
        phases: vec![],
        classification: None,
        classification_inputs: Value::Null,
    }
}

fn hit(host: &str, source: DiscoverySource) -> ScanCandidate {
    ScanCandidate {
        name: host.into(),
        host: host.into(),
        source,
    }
}

struct Rig {
    router: axum::Router,
    runtime: Arc<Runtime>,
    scanner: ScriptedScanner,
}

async fn rig() -> Rig {
    let scanner = ScriptedScanner::default();
    let options = PluginOptions {
        scanner: Arc::new(scanner.clone()),
        step_timeout: Duration::from_secs(2),
        ..PluginOptions::default()
    };
    let service = SettingsService::ephemeral(RuntimeSettings::demo());
    let runtime = Runtime::with_playback_options(Arc::new(service), false, options);
    runtime.start().await;
    Rig {
        router: build_app(runtime.clone(), AppOptions { web_root: None }),
        runtime,
        scanner,
    }
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

    async fn status(&self) -> Value {
        self.call("GET", "/api/v1/playback-api/status", None)
            .await
            .1
    }

    async fn until(&self, what: &str, mut ready: impl FnMut(&Value) -> bool) -> Value {
        for _ in 0..LIMIT {
            let status = self.status().await;
            if ready(&status) {
                return status;
            }
            tokio::time::sleep(STEP).await;
        }
        panic!("timed out waiting for: {what}");
    }

    /// Point Playback at the fake: manual host and port, which also selects Playback.
    async fn connect(&self, fake: &FakePlayback) {
        let (code, _) = self
            .call(
                "PUT",
                "/api/v1/playback-api/settings",
                Some(
                    json!({"enabled": true, "host": "127.0.0.1", "port": fake.port,
                    "auto_scan": false, "fast_transport": true}),
                ),
            )
            .await;
        assert_eq!(code, StatusCode::OK);
        self.until("connected", |s| s["connected"] == true).await;
    }
}

fn three_songs(fake: &FakePlayback) {
    fake.auto_beat(Duration::from_millis(100));
    let mut s = fake.state.lock().unwrap();
    s.songs = vec![101, 102, 103];
    s.song = 102;
}

// ---- status, connection, events ---------------------------------------------------------------

#[tokio::test]
async fn a_fresh_install_reports_disconnected_with_the_scan_hint() {
    // test_default_status_remote_connections_diagnostic_and_disable
    let r = rig().await;
    let status = r.status().await;
    assert_eq!(status["selected"], false);
    assert_eq!(status["connected"], false);
    assert_eq!(status["active_source"], "none");
    assert_eq!(
        status["reason"],
        "Disconnected. Choose Scan Network to find Playback."
    );
    assert_eq!(status["scan"]["state"], "idle");
    assert_eq!(status["discovery"], "idle");
    assert_eq!(status["stale"], true);
    assert_eq!(status["song_order"], json!([]));
    assert_eq!(status["plan_song_count"], 4);
    let (code, connection) = r.call("GET", "/api/v1/playback-api/connection", None).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(connection["reason"], status["reason"]);
    let (_, events) = r.call("GET", "/api/v1/playback-api/events", None).await;
    assert_eq!(events, json!({"events": [], "capacity": 100}));
}

#[tokio::test]
async fn connecting_reports_source_host_and_the_monitor_entries() {
    let r = rig().await;
    let fake = FakePlayback::start().await;
    fake.auto_beat(Duration::from_millis(100));
    r.connect(&fake).await;
    let status = r.status().await;
    assert_eq!(status["active_source"], "playback_api");
    assert_eq!(status["host"], "127.0.0.1");
    assert_eq!(status["source"], "manual");
    assert_eq!(status["port"], fake.port);
    assert_eq!(status["selected"], true);
    assert_eq!(status["setlist_cloud_version"], 1);
    assert_eq!(status["sources"]["playback_api"]["connected"], true);
    let (_, connection) = r.call("GET", "/api/v1/playback-api/connection", None).await;
    assert_eq!(connection["connected"], true);
    assert!(connection["reason"]
        .as_str()
        .unwrap()
        .starts_with("Connected to Playback"));
    {
        let mut s = fake.state.lock().unwrap();
        s.playing = true;
    }
    fake.beat();
    r.until("playing", |s| s["playing"] == true).await;
    let (_, events) = r.call("GET", "/api/v1/playback-api/events", None).await;
    let list = events["events"].as_array().unwrap();
    assert!(!list.is_empty(), "the monitor keeps what the client saw");
    assert!(list[0]["at"].as_str().unwrap().ends_with('Z'));
    assert_eq!(list[0]["discovery"], false);
    r.runtime.stop().await;
}

// ---- settings ---------------------------------------------------------------------------------

#[tokio::test]
async fn settings_validation_matches_pydantic() {
    let r = rig().await;
    let put = |body: Value| r.call("PUT", "/api/v1/playback-api/settings", Some(body));
    let (code, body) = put(json!({"port": 0})).await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["detail"][0]["type"], "greater_than_equal");
    assert_eq!(body["detail"][0]["loc"], json!(["body", "port"]));
    let (code, body) = put(json!({"port": 70000})).await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["detail"][0]["type"], "less_than_equal");
    let (code, body) = put(json!({"bogus": 1})).await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["detail"][0]["type"], "extra_forbidden");
    let (code, body) = put(json!({"enabled": "maybe"})).await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["detail"][0]["type"], "bool_parsing");
    let (code, body) = put(json!([1])).await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["detail"][0]["type"], "model_attributes_type");
    // A host the client would refuse is a 422 with the client's own sentence (Python validates
    // before it trims, so a padded host is refused too).
    let (code, body) = put(json!({"host": " padded "})).await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body, json!({"detail": "invalid Playback host"}));
    // Nothing was saved by any of the refusals.
    assert_eq!(r.status().await["selected"], false);
}

#[tokio::test]
async fn settings_round_trip_and_turning_on_selects_playback() {
    // test_playback_settings: manual host normalised, round trip; the one-switch rule.
    let r = rig().await;
    let (code, body) = r
        .call(
            "PUT",
            "/api/v1/playback-api/settings",
            Some(json!({"enabled": false, "host": "10.1.2.3", "port": 9001,
                "auto_scan": false, "fast_transport": false})),
        )
        .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        body["settings"],
        json!({"enabled": false, "host": "10.1.2.3", "port": 9001,
            "auto_scan": false, "fast_transport": false})
    );
    assert_eq!(body["selected"], false, "disabled does not select");
    let (_, body) = r
        .call(
            "PUT",
            "/api/v1/playback-api/settings",
            Some(json!({"enabled": true, "host": null, "port": 9001})),
        )
        .await;
    assert_eq!(body["settings"]["host"], Value::Null, "no host is unset");
    assert_eq!(body["selected"], true, "turning Playback on chooses it");
    assert_eq!(
        body["settings"]["fast_transport"], true,
        "omitted = default"
    );
    let saved = r.runtime.settings_service.snapshot();
    assert_eq!(
        saved.integration_modes.midi_source,
        stagepilot_core::config::MidiSource::PlaybackApi
    );
    assert_eq!(saved.playback_api.port, 9001);
}

#[tokio::test]
async fn changing_fast_transport_reconfigures_the_client() {
    // test_fast_transport_setting_reconfigures_client
    let r = rig().await;
    let fake = FakePlayback::start().await;
    fake.auto_beat(Duration::from_millis(100));
    r.connect(&fake).await;
    let before = *fake.connections.lock().unwrap();
    let (code, body) = r
        .call(
            "PUT",
            "/api/v1/playback-api/settings",
            Some(
                json!({"enabled": true, "host": "127.0.0.1", "port": fake.port,
                "auto_scan": false, "fast_transport": false}),
            ),
        )
        .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(body["settings"]["fast_transport"], false);
    for _ in 0..LIMIT {
        if *fake.connections.lock().unwrap() > before {
            break;
        }
        tokio::time::sleep(STEP).await;
    }
    assert!(
        *fake.connections.lock().unwrap() > before,
        "a new connection was made with the new transport"
    );
    r.until("connected again", |s| s["connected"] == true).await;
    r.runtime.stop().await;
}

#[tokio::test]
async fn the_general_settings_route_cannot_edit_the_song_order() {
    // test_settings_override_disable_find_no_order_edit_or_controls
    let r = rig().await;
    let settings = json!({"playback_api": {"song_order": [1, 2, 3]}});
    let (code, body) = r.call("PUT", "/api/v1/settings", Some(settings)).await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    assert_eq!(
        body,
        json!({"detail": "Song order can only be changed by Discover Song Order."})
    );
}

// ---- find -------------------------------------------------------------------------------------

#[tokio::test]
async fn one_scan_hit_turns_playback_on_and_connects() {
    // test_scan_from_fresh_install_connects_with_one_click
    let r = rig().await;
    let fake = FakePlayback::start().await;
    fake.auto_beat(Duration::from_millis(100));
    // The scanner finds the fake on loopback; the port is the saved one.
    let (code, _) = r
        .call(
            "PUT",
            "/api/v1/playback-api/settings",
            Some(json!({"enabled": false, "port": fake.port})),
        )
        .await;
    assert_eq!(code, StatusCode::OK);
    *r.scanner.0.lock().unwrap() = Some(report(
        ScanOutcome::Found,
        vec![hit("127.0.0.1", DiscoverySource::Loopback)],
    ));
    let (code, started) = r.call("POST", "/api/v1/playback-api/find", None).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(started["scan"]["state"], "scanning");
    assert_eq!(started["reason"], "Looking for Playback…");
    let status = r.until("connected", |s| s["connected"] == true).await;
    assert_eq!(status["scan"]["state"], "found");
    assert_eq!(status["scan"]["candidates"].as_array().unwrap().len(), 1);
    assert_eq!(status["selected"], true);
    assert_eq!(status["settings"]["host"], "127.0.0.1");
    assert_eq!(status["settings"]["enabled"], true);
    r.runtime.stop().await;
}

#[tokio::test]
async fn several_hits_wait_and_none_says_where_to_look() {
    // test_scan_not_found_wrong_host_multiple_cancel_and_validation
    let r = rig().await;
    *r.scanner.0.lock().unwrap() = Some(report(
        ScanOutcome::Found,
        vec![
            hit("10.0.0.2", DiscoverySource::Lan),
            hit("10.0.0.3", DiscoverySource::Lan),
        ],
    ));
    r.call("POST", "/api/v1/playback-api/find", Some(json!({})))
        .await;
    let status = r
        .until("two found", |s| s["scan"]["state"] == "found")
        .await;
    assert_eq!(status["scan"]["candidates"].as_array().unwrap().len(), 2);
    assert_eq!(
        status["selected"], false,
        "the operator chooses between two"
    );

    *r.scanner.0.lock().unwrap() = Some(report(ScanOutcome::NotFound, vec![]));
    r.call("POST", "/api/v1/playback-api/find", None).await;
    let status = r
        .until("not found", |s| s["scan"]["state"] == "not_found")
        .await;
    assert!(status["scan"]["reason"]
        .as_str()
        .unwrap()
        .starts_with("Couldn't find Playback."));
    assert_eq!(status["reason"], status["scan"]["reason"]);
}

#[tokio::test]
async fn find_validates_the_typed_host_like_python() {
    let r = rig().await;
    let find = |body: Value| r.call("POST", "/api/v1/playback-api/find", Some(body));
    let (code, body) = find(json!({"host": "x".repeat(300)})).await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body, json!({"detail": "Enter a host name or IP address."}));
    let (code, body) = find(json!({"host": "bad host"})).await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body, json!({"detail": "invalid Playback host"}));
    let (code, body) = find(json!({"bogus": true})).await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body, json!({"detail": "Enter a host name or IP address."}));
    let (code, _) = r
        .call("POST", "/api/v1/playback-api/find/cancel", None)
        .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(r.status().await["scan"]["state"], "idle");
}

#[tokio::test]
async fn cancelling_a_running_scan_resets_it() {
    let r = rig().await;
    *r.scanner.0.lock().unwrap() = Some(report(ScanOutcome::NotFound, vec![]));
    r.call("POST", "/api/v1/playback-api/find", None).await;
    let (code, body) = r
        .call("POST", "/api/v1/playback-api/find/cancel", None)
        .await;
    assert_eq!(code, StatusCode::OK);
    assert!(
        matches!(
            body["scan"]["state"].as_str(),
            Some("idle") | Some("not_found")
        ),
        "{}",
        body["scan"]
    );
}

// ---- discovery, arrangement --------------------------------------------------------------------

#[tokio::test]
async fn discovery_is_confirmed_by_the_server_and_refused_while_disconnected() {
    // test_confirmation_is_server_enforced, test_conflicts_playing_disconnected_running_and_settings
    let r = rig().await;
    let path = "/api/v1/playback-api/discover-song-order";
    for body in [
        None,
        Some(json!({})),
        Some(json!({"confirm": "true"})),
        Some(json!({"confirm": 1})),
        Some(json!([true])),
    ] {
        let (code, out) = r.call("POST", path, body.clone()).await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{body:?}");
        assert_eq!(
            out,
            json!({"detail": "Confirm Discover Song Order with {confirm: true}."})
        );
    }
    let (code, out) = r.call("POST", path, Some(json!({"confirm": true}))).await;
    assert_eq!(code, StatusCode::CONFLICT);
    assert_eq!(
        out,
        json!({"detail": "Playback is disconnected. Enable remote connections and connect first."})
    );
}

#[tokio::test]
async fn discovery_over_http_saves_the_order_and_arrangement_follows() {
    // test_loopback_discovery_ends_restore_persist_and_suppress, then test_playback_arrangement's
    // HTTP half.
    let r = rig().await;
    let fake = FakePlayback::start().await;
    three_songs(&fake);
    r.connect(&fake).await;
    let (code, status) = r
        .call(
            "POST",
            "/api/v1/playback-api/discover-song-order",
            Some(json!({"confirm": true})),
        )
        .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(status["discovery"], "done", "{status}");
    assert_eq!(status["song_order"], json!([101, 102, 103]));
    assert_eq!(status["song_count"], 3);
    assert_eq!(status["captured_version"], 1);
    assert!(status["captured_at"].as_str().unwrap().ends_with('Z'));
    assert_eq!(status["song_lengths"].as_array().unwrap().len(), 3);
    let status = r.until("order valid", |s| s["stale"] == false).await;
    assert_eq!(status["last_error"], Value::Null);

    // Arrangement: refuse what is not true, save what is.
    let put = |body: Value| r.call("PUT", "/api/v1/playback-api/arrangement", Some(body));
    let (code, body) = put(json!({"skipped_song_ids": [101, 101]})).await;
    assert_eq!(
        (code, &body),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            &json!({"detail": "Skipped songs must be unique."})
        )
    );
    let (code, body) = put(json!({"skipped_song_ids": [555]})).await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body,
        json!({"detail": "A skipped Playback song is not in the scanned song order."})
    );
    let (code, body) = put(json!({"skipped_plan_song_ids": ["not-in-the-plan"]})).await;
    assert_eq!(code, StatusCode::CONFLICT);
    assert_eq!(
        body,
        json!({"detail": "A skipped song is not in the loaded service plan."})
    );
    let (code, body) =
        put(json!({"skipped_song_ids": [103], "skipped_plan_song_ids": ["demo-gratitude"]})).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(body["skipped_song_ids"], json!([103]));
    assert_eq!(body["skipped_plan_song_ids"], json!(["demo-gratitude"]));
    assert_eq!(body["arrangement_current"], true);
    assert_eq!(
        body["song_order"],
        json!([101, 102, 103]),
        "order never changes"
    );
    r.runtime.stop().await;
}

#[tokio::test]
async fn arrangement_before_a_scan_asks_for_one_first() {
    let r = rig().await;
    let (code, body) = r
        .call("PUT", "/api/v1/playback-api/arrangement", Some(json!({})))
        .await;
    assert_eq!(code, StatusCode::CONFLICT);
    assert_eq!(
        body,
        json!({"detail": "Scan songs first, then choose which songs to skip."})
    );
}

// ---- Planning Center length routes ---------------------------------------------------------------

#[tokio::test]
async fn length_routes_answer_the_not_connected_errors() {
    // test_playback_lengths HTTP half, for the part that does not need Planning Center.
    let r = rig().await;
    let base = "/api/v1/playback-api/planning-center-lengths";
    let (code, body) = r.call("POST", &format!("{base}/preview"), None).await;
    assert_eq!(
        (code, body),
        (
            StatusCode::CONFLICT,
            json!({"detail": "Connect Planning Center first."})
        )
    );
    let (code, body) = r
        .call(
            "POST",
            &format!("{base}/confirm"),
            Some(json!({"token": "t", "confirm": true})),
        )
        .await;
    assert_eq!(
        (code, body),
        (
            StatusCode::CONFLICT,
            json!({"detail": "Connect Planning Center first."})
        )
    );
    let (code, body) = r
        .call(
            "POST",
            &format!("{base}/confirm"),
            Some(json!({"token": "t", "confirm": false})),
        )
        .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["detail"][0]["type"], "literal_error");
    let (code, body) = r.call("GET", &format!("{base}/categories"), None).await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    assert!(body["detail"].as_str().unwrap().contains("Try again later"));
    let (code, body) = r
        .call(
            "PUT",
            &format!("{base}/category"),
            Some(json!({"service_type_id": "a b"})),
        )
        .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["detail"][0]["type"], "string_pattern_mismatch");
}

#[tokio::test]
async fn restore_needs_confirmation_and_with_nothing_to_restore_is_a_quiet_success() {
    let r = rig().await;
    let path = "/api/v1/playback-api/planning-center-lengths/restore";
    let (code, body) = r.call("POST", path, None).await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    assert_eq!(
        body,
        json!({"detail": "Confirm restoring Planning Center times with {confirm: true}."})
    );
    let (code, body) = r.call("POST", path, Some(json!({"confirm": true}))).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(body["planning_center_lengths"]["operation"], "restore");
    assert_eq!(body["planning_center_lengths"]["status"], "done");
    assert_eq!(
        body["planning_center_lengths"]["message"],
        "No Planning Center times needed changing."
    );
}

// ---- lifecycle ----------------------------------------------------------------------------------

#[tokio::test]
async fn shutdown_while_connected_is_clean() {
    // test_shutdown_cancels_discovery_without_saving (the no-discovery half: a clean stop).
    let r = rig().await;
    let fake = FakePlayback::start().await;
    fake.auto_beat(Duration::from_millis(100));
    r.connect(&fake).await;
    r.runtime.stop().await;
    let health = r.runtime.plugins.health().await;
    assert!(health.iter().any(|h| h.name == "playback_api"));
}

#[tokio::test]
async fn a_rescan_of_a_new_setlist_drops_skips_that_are_no_longer_in_the_order() {
    // rc.5 on the rig: a skip saved for an earlier setlist stayed in the settings, the save screen
    // sent it back and every Playback save failed with "A skipped Playback song is not in the
    // scanned song order." A scan must forget skips whose song is gone.
    let r = rig().await;
    let fake = FakePlayback::start().await;
    three_songs(&fake);
    r.connect(&fake).await;
    let scan = || {
        r.call(
            "POST",
            "/api/v1/playback-api/discover-song-order",
            Some(json!({"confirm": true})),
        )
    };
    let (code, status) = scan().await;
    assert_eq!(
        (code, &status["discovery"]),
        (StatusCode::OK, &json!("done"))
    );
    r.until("order valid", |s| s["stale"] == false).await;
    let put = |body: Value| r.call("PUT", "/api/v1/playback-api/arrangement", Some(body));
    let (code, body) = put(json!({"skipped_song_ids": [103, 101]})).await;
    assert_eq!(
        (code, &body["skipped_song_ids"]),
        (StatusCode::OK, &json!([103, 101]))
    );

    // A new setlist: 103 is gone, 101 is still there.
    {
        let mut s = fake.state.lock().unwrap();
        s.songs = vec![101, 201, 202];
        s.song = 201;
        s.version += 1;
    }
    r.until("the new setlist is seen", |s| s["stale"] == true)
        .await;
    let (code, status) = scan().await;
    assert_eq!(
        (code, &status["discovery"]),
        (StatusCode::OK, &json!("done")),
        "{status}"
    );
    assert_eq!(status["song_order"], json!([101, 201, 202]));
    assert_eq!(
        status["skipped_song_ids"],
        json!([101]),
        "stale 103 is gone: {status}"
    );
    // What the save screen sends back is accepted.
    let (code, body) = put(json!({"skipped_song_ids": [101, 202]})).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    r.runtime.stop().await;
}

#[tokio::test]
async fn a_stale_saved_skip_is_not_reported_so_the_save_screen_cannot_send_it_back() {
    // A settings file written by rc.5 or Python can already hold a skip that is not in the order.
    let r = rig().await;
    let fake = FakePlayback::start().await;
    three_songs(&fake);
    r.connect(&fake).await;
    r.call(
        "POST",
        "/api/v1/playback-api/discover-song-order",
        Some(json!({"confirm": true})),
    )
    .await;
    r.until("order valid", |s| s["stale"] == false).await;
    let mut seeded = r.runtime.settings_service.snapshot();
    seeded.playback_api.skipped_song_ids = vec![555, 102];
    r.runtime
        .settings_service
        .save(seeded)
        .expect("seed a stale skip");
    r.runtime
        .playback
        .reconfigure()
        .await
        .expect("reload the saved settings");
    let status = r.status().await;
    assert_eq!(status["skipped_song_ids"], json!([102]), "{status}");
    r.runtime.stop().await;
}
