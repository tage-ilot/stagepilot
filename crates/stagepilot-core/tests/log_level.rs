//! m1: the saved `log_level` is honoured at startup (the environment variable still wins), and
//! each integration connect/disconnect is one INFO line.

use stagepilot_core::bus::EventBus;
use stagepilot_core::events::{new_event, EventPayload, EventType, Integration};
use stagepilot_core::logging::{filter_for_level, saved_log_level, startup_filter};
use stagepilot_core::state::ConnectionStatus;
use stagepilot_core::state_service::StateService;
use stagepilot_core::store::StateStore;
mod support;
use tracing::Level;

#[test]
fn the_saved_level_is_used_and_the_environment_wins() {
    assert_eq!(startup_filter(None, None), "info");
    assert_eq!(startup_filter(None, Some("WARNING")), "warn");
    assert_eq!(startup_filter(None, Some("ERROR")), "error");
    let debug = startup_filter(None, Some("DEBUG"));
    assert!(debug.starts_with("info,"), "{debug}");
    assert!(debug.contains("stagepilot_core=debug") && debug.contains("stagepilot_playback=debug"));
    assert!(!debug.contains("hyper"), "dependencies stay quiet: {debug}");
    // The environment variable wins, as a level name or as a full filter.
    assert_eq!(startup_filter(Some("error"), Some("DEBUG")), "error");
    assert_eq!(
        startup_filter(Some("stagepilot_pco=trace"), Some("DEBUG")),
        "stagepilot_pco=trace"
    );
    // An unusable saved level falls back to INFO instead of failing the start.
    assert_eq!(startup_filter(None, Some("LOUD")), "info");
    assert_eq!(startup_filter(Some("  "), Some("WARNING")), "warn");
    assert!(filter_for_level("nonsense").is_none());
}

#[test]
fn the_level_is_read_from_the_settings_file_without_touching_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let write = |bytes: &[u8]| std::fs::write(&path, bytes).unwrap();
    write(br#"{"schema_version": 2, "log_level": "DEBUG", "unknown_future_key": 1}"#);
    assert_eq!(saved_log_level(&path).as_deref(), Some("DEBUG"));
    write(b"\xEF\xBB\xBF{\"log_level\": \"WARNING\"}");
    assert_eq!(saved_log_level(&path).as_deref(), Some("WARNING"));
    for bad in [
        &b"{ truncated"[..],
        b"",
        b"[1]",
        br#"{"log_level": 7}"#,
        br#"{"log_level": "LOUD"}"#,
        b"\xff\xfe",
    ] {
        write(bad);
        assert_eq!(saved_log_level(&path), None);
        assert_eq!(std::fs::read(&path).unwrap(), bad, "peeking never writes");
    }
    assert_eq!(saved_log_level(&dir.path().join("missing.json")), None);
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        1,
        "no quarantine copy either"
    );
}

// ---- integration connection lines -------------------------------------------------------------

#[tokio::test]
async fn each_integration_connect_and_disconnect_is_one_info_line() {
    let logs = support::Capture::start();
    let bus = EventBus::new();
    let service = StateService::new(bus.clone(), StateStore::default(), 50, 50);
    service.start();
    let send = |source: &str, integration: Integration, status: ConnectionStatus| {
        let event = new_event(
            EventType::ConnectionChanged,
            source,
            EventPayload::Connection {
                integration,
                status,
                detail: None,
            },
        );
        let bus = bus.clone();
        async move { bus.publish(event).await }
    };
    send(
        "playback_api",
        Integration::Midi,
        ConnectionStatus::Connected,
    )
    .await;
    send(
        "playback_api",
        Integration::Midi,
        ConnectionStatus::Connected,
    )
    .await; // no change
    send(
        "propresenter",
        Integration::Propresenter,
        ConnectionStatus::Connected,
    )
    .await;
    send("lights", Integration::Lights, ConnectionStatus::Connected).await;
    send(
        "playback_api",
        Integration::Midi,
        ConnectionStatus::Disconnected,
    )
    .await;
    let lines: Vec<String> = logs
        .at(Level::INFO)
        .into_iter()
        .filter(|t| t.contains("integration_connection"))
        .collect();
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert!(lines[0].contains("playback_api") && lines[0].contains("Connected"));
    assert!(lines[1].contains("propresenter"));
    assert!(lines[2].contains("lights"));
    assert!(lines[3].contains("playback_api") && lines[3].contains("Disconnected"));
}
