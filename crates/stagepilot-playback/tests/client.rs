//! Behavioural tests against the Rust fake Playback. Each mirrors a rule in the
//! Python client (`client.py` / `ws.py`); the comment names it.

use stagepilot_playback::client::{ClientError, Command, ConnectionOptions, PlaybackClient};
use stagepilot_playback::normalizer::EventType;
use stagepilot_playback::testkit::FakePlayback;
use std::time::Duration;

fn opts(port: u16) -> ConnectionOptions {
    ConnectionOptions {
        port,
        host_override: Some("127.0.0.1".into()),
        heartbeat_timeout: Duration::from_millis(600),
        reconnect_delay: Duration::from_millis(100),
        ..Default::default()
    }
}

async fn until(what: &str, mut f: impl FnMut() -> bool) {
    for _ in 0..400 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for: {what}");
}

#[tokio::test]
async fn connects_only_after_first_valid_heartbeat_and_snapshots() {
    let fake = FakePlayback::start().await;
    let mut c = PlaybackClient::start(opts(fake.port), None).unwrap();
    until("connected", || c.status().connected).await;
    let hb = c.status().heartbeat.unwrap();
    assert_eq!((hb.song_id, hb.playing, hb.position), (101, false, 0.0));
    assert_eq!(c.events()[0].kind, EventType::StateSnapshot);
    c.stop().await;
    assert!(!c.status().connected, "stop resets status");
}

#[tokio::test]
async fn stalled_playback_is_not_connected_and_times_out() {
    // client.py: connected is False until a valid heartbeat; heartbeat gap > timeout drops.
    let fake = FakePlayback::start().await;
    fake.stall(true);
    let mut c = PlaybackClient::start(opts(fake.port), None).unwrap();
    until("handshake", || *fake.connections.lock().unwrap() >= 1).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!c.status().connected);
    // Reconnects keep happening after the timeout, with the unavailable message.
    until("reconnect", || *fake.connections.lock().unwrap() >= 2).await;
    assert_eq!(
        c.status().last_error.as_deref(),
        Some("Playback unavailable; remote connections may be off")
    );
    c.stop().await;
}

#[tokio::test]
async fn heartbeat_timeout_after_connected_then_recovers() {
    let fake = FakePlayback::start().await;
    let mut c = PlaybackClient::start(opts(fake.port), None).unwrap();
    until("connected", || c.status().connected).await;
    fake.stall(true);
    until("dropped by timeout", || !c.status().connected).await;
    fake.stall(false);
    until("reconnected", || c.status().connected).await;
    assert!(*fake.connections.lock().unwrap() >= 2);
    // fresh normalizer on reconnect: a new snapshot is emitted
    let snaps = c
        .events()
        .iter()
        .filter(|e| e.kind == EventType::StateSnapshot)
        .count();
    assert!(snaps >= 2, "snapshots: {snaps}");
    c.stop().await;
}

#[tokio::test]
async fn commands_have_the_python_wire_shape() {
    let fake = FakePlayback::start().await;
    let mut c = PlaybackClient::start(opts(fake.port), None).unwrap();
    until("connected", || c.status().connected).await;
    // Discovery commands are byte-identical to ws.py::_discovery_step.
    for (cmd, wire) in [
        (Command::Previous, r#"{"transportPreviousSong":{}}"#),
        (Command::Next, r#"{"transportNextSong":{}}"#),
        (
            Command::SeekEnd,
            r#"{"waveformSeek":{"sequenceTime":86400.0}}"#,
        ),
        (Command::ReturnToStart, r#"{"transportReturnToStart":{}}"#),
    ] {
        c.send(cmd).await.unwrap();
        until("echo", || {
            fake.received.lock().unwrap().last().map(String::as_str) == Some(wire)
        })
        .await;
    }
    c.stop().await;
}

#[tokio::test]
async fn discovery_commands_refused_while_playing_but_transport_allowed() {
    // client.py::_discovery_command: "Stop Playback before discovering song order".
    let fake = FakePlayback::start().await;
    let mut c = PlaybackClient::start(opts(fake.port), None).unwrap();
    until("connected", || c.status().connected).await;
    c.send(Command::Play(true)).await.unwrap();
    until("playing", || {
        c.status().heartbeat.is_some_and(|h| h.playing)
    })
    .await;
    assert_eq!(c.send(Command::SeekEnd).await, Err(ClientError::NotStopped));
    assert_eq!(c.send(Command::Next).await, Err(ClientError::NotStopped));
    assert!(!fake
        .received_kinds()
        .contains(&"transportNextSong".to_string()));
    // play/pause/seek/select/pad/fade/loop are allowed while playing.
    c.send(Command::Seek(12.5)).await.unwrap();
    c.send(Command::Pad(true)).await.unwrap();
    c.send(Command::Fade(1)).await.unwrap();
    c.send(Command::SectionLoop {
        section_id: 7,
        active: true,
    })
    .await
    .unwrap();
    c.send(Command::Play(false)).await.unwrap();
    until("paused", || {
        c.status().heartbeat.is_some_and(|h| !h.playing)
    })
    .await;
    c.send(Command::SelectSong(102)).await.unwrap();
    until("song 102", || {
        c.status().heartbeat.is_some_and(|h| h.song_id == 102)
    })
    .await;
    let kinds = fake.received_kinds();
    for k in [
        "transportPlay",
        "waveformSeek",
        "transportPad",
        "transportFade",
        "waveformLoop",
        "setlistSelectSong",
    ] {
        assert!(kinds.contains(&k.to_string()), "{k} missing in {kinds:?}");
    }
    c.stop().await;
}

#[tokio::test]
async fn commands_refused_when_disconnected() {
    let fake = FakePlayback::start().await;
    let mut c = PlaybackClient::start(opts(fake.port), None).unwrap();
    assert_eq!(
        c.send(Command::ReturnToStart).await,
        Err(ClientError::Disconnected)
    );
    until("connected", || c.status().connected).await;
    c.stop().await;
    assert_eq!(
        c.send(Command::ReturnToStart).await,
        Err(ClientError::Disconnected)
    );
}

#[tokio::test]
async fn reconnects_after_drop_and_after_offline() {
    let fake = FakePlayback::start().await;
    let mut c = PlaybackClient::start(opts(fake.port), None).unwrap();
    until("connected", || c.status().connected).await;
    fake.drop_connection();
    until("reconnected after drop", || {
        *fake.connections.lock().unwrap() >= 2 && c.status().connected
    })
    .await;
    let (port, state) = fake.go_offline().await;
    until("offline", || !c.status().connected).await;
    assert!(c.status().last_error.is_some());
    let fake = FakePlayback::start_on(port, Some(state)).await;
    until("back", || c.status().connected).await;
    assert_eq!(*fake.connections.lock().unwrap(), 1);
    c.stop().await;
}

#[tokio::test]
async fn wrong_subprotocol_is_rejected() {
    // ws.py::_handshake: Sec-WebSocket-Protocol must be exactly pr-protocol.
    let fake = FakePlayback::start().await;
    *fake.protocol.lock().unwrap() = Some("chat");
    let mut c = PlaybackClient::start(opts(fake.port), None).unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!c.status().connected);
    c.stop().await;
}

#[tokio::test]
async fn invalid_options_rejected() {
    // ConnectionOptions.__post_init__
    let bad_host = ConnectionOptions {
        host_override: Some("a b".into()),
        ..Default::default()
    };
    assert!(PlaybackClient::start(bad_host, None).is_err());
    let bad_port = ConnectionOptions {
        port: 0,
        ..Default::default()
    };
    assert!(PlaybackClient::start(bad_port, None).is_err());
}

#[tokio::test]
async fn garbage_messages_do_not_refresh_heartbeat_deadline() {
    // client.py: only a *valid heartbeat* resets the timeout, not any traffic.
    let fake = FakePlayback::start().await;
    let mut c = PlaybackClient::start(opts(fake.port), None).unwrap();
    until("connected", || c.status().connected).await;
    fake.stall(true);
    for _ in 0..8 {
        fake.send_raw(r#"{"weird":{}}"#);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        !c.status().connected,
        "unknown traffic must not keep the link alive"
    );
    assert!(c
        .events()
        .iter()
        .any(|e| e.kind == EventType::MessageUnknown));
    c.stop().await;
}
