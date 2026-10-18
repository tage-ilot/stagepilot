//! The Rust Playback API models must agree with the Python schema frozen in
//! `contract/openapi.json`: same field names, and every field Python requires is present in the
//! Rust JSON (Rust always sends every field, including the ones Python defaults).

use serde_json::Value;
use stagepilot_playback::api::*;
use std::collections::BTreeSet;

fn schema(name: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let doc: Value = serde_json::from_str(include_str!("../../../contract/openapi.json")).unwrap();
    let s = &doc["components"]["schemas"][name];
    let props = s["properties"]
        .as_object()
        .unwrap_or_else(|| panic!("openapi has no {name}"))
        .keys()
        .cloned()
        .collect();
    let required = s["required"]
        .as_array()
        .map(|a| a.iter().map(|v| v.as_str().unwrap().to_string()).collect())
        .unwrap_or_default();
    (props, required)
}

fn keys<T: serde::Serialize>(value: &T) -> BTreeSet<String> {
    serde_json::to_value(value)
        .unwrap()
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect()
}

fn agrees<T: serde::Serialize>(name: &str, value: &T) {
    let (props, required) = schema(name);
    let rust = keys(value);
    assert_eq!(rust, props, "{name}: field names");
    assert!(required.is_subset(&rust), "{name}: required fields");
}

fn status() -> SourceStatus {
    SourceStatus {
        connected: false,
        reason: "r".into(),
    }
}

fn sources() -> SourcesStatus {
    SourcesStatus {
        midi: status(),
        playback_api: status(),
    }
}

fn settings() -> PlaybackSettingsRequest {
    PlaybackSettingsRequest {
        enabled: true,
        host: None,
        port: 8080,
        auto_scan: true,
        fast_transport: true,
    }
}

fn scan() -> ScanResult {
    ScanResult {
        state: "idle".into(),
        candidates: vec![],
        reason: None,
        current: 0,
        total: 0,
        phase: None,
        error_class: None,
        typed: false,
        elapsed: 0.0,
        networks: vec![],
        hosts_probed: 0,
        hosts_total: 0,
        details: None,
        settings_url: None,
    }
}

#[test]
fn small_models_match_openapi() {
    agrees("SourceStatus", &status());
    agrees("PlaybackSettingsRequest", &settings());
    agrees("ScanResult", &scan());
    agrees(
        "ScanCandidateModel",
        &ScanCandidateModel {
            name: "n".into(),
            host: "h".into(),
        },
    );
    agrees(
        "PlaybackPlanSong",
        &PlaybackPlanSong {
            id: "i".into(),
            title: "t".into(),
            duration_seconds: None,
        },
    );
    agrees("LengthUpdateResult", &LengthUpdateResult::idle());
    agrees(
        "LengthItemResult",
        &LengthItemResult {
            item_id: "i".into(),
            title: "t".into(),
            status: "updated".into(),
            old_length: None,
            new_length: None,
            reason: None,
        },
    );
}

#[test]
fn connection_and_events_match_openapi() {
    let (props, _) = schema("PlaybackConnection");
    let rust = keys(&PlaybackConnectionResponse {
        active_source: "none".into(),
        connected: false,
        reason: "r".into(),
        sources: sources(),
    });
    assert_eq!(rust, props);
    agrees(
        "PlaybackEventsResponse",
        &PlaybackEventsResponse {
            events: vec![],
            capacity: 100,
        },
    );
    let event: stagepilot_playback::normalizer::PlaybackEvent =
        serde_json::from_value(serde_json::json!({"type": "seek", "timestamp": 1.0})).unwrap();
    agrees("PlaybackEvent", &event);
    agrees(
        "PlaybackMonitorEntry",
        &PlaybackMonitorEntry {
            event,
            discovery: false,
            at: "2026-01-01T00:00:00Z".into(),
        },
    );
}

#[test]
fn the_status_response_matches_openapi() {
    let response = PlaybackStatusResponse {
        planning_center_lengths: LengthUpdateResult::idle(),
        planning_center_undo_available: false,
        planning_center_connected: false,
        plan_title: None,
        plan_date: None,
        scan: scan(),
        selected: false,
        enabled: true,
        connected: false,
        active_source: "none".into(),
        sources: sources(),
        reason: "r".into(),
        host: None,
        port: 8080,
        source: None,
        last_error: None,
        playing: false,
        setlist_cloud_version: None,
        discovery: "idle".into(),
        progress: 0,
        discovery_song: 0,
        discovery_total: 0,
        song_order: vec![],
        song_lengths: vec![],
        skipped_song_ids: vec![],
        skipped_plan_song_ids: vec![],
        arrangement_current: true,
        discovery_duration_seconds: None,
        planning_center_update_service_type_id: None,
        planning_center_update_reason: None,
        lengths_measured_at: None,
        plan_songs: vec![],
        song_count: 0,
        plan_song_count: 0,
        captured_version: None,
        captured_at: None,
        stale: true,
        plan_changed: false,
        lengths_applied: false,
        settings: settings(),
    };
    agrees("PlaybackStatusResponse", &response);
}

#[test]
fn event_types_match_the_python_enum() {
    let doc: Value = serde_json::from_str(include_str!("../../../contract/openapi.json")).unwrap();
    let python: BTreeSet<String> = doc["components"]["schemas"]["PlaybackEvent"]["properties"]
        ["type"]["enum"]
        .as_array()
        .expect("openapi lists the event types")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    use stagepilot_playback::normalizer::EventType::*;
    let rust: BTreeSet<String> = [
        StateSnapshot,
        SetlistChanged,
        SongSelected,
        SongChanged,
        SongEnded,
        SongStarted,
        SongResumed,
        SongPaused,
        SongStopped,
        TransportReverted,
        Seek,
        SectionJump,
        SectionLoop,
        FadeOut,
        FadeIn,
        PadRequested,
        PadOn,
        PadOff,
        PositionJump,
        MessageUnknown,
    ]
    .iter()
    .map(|e| {
        serde_json::to_value(e)
            .unwrap()
            .as_str()
            .unwrap()
            .to_string()
    })
    .collect();
    assert_eq!(rust, python);
}
