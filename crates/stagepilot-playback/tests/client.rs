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
        heartbeat_timeout: Duration::from_secs(2),
        reconnect_delay: Duration::from_millis(100),
        ..Default::default()
    }
}

async fn until(what: &str, mut f: impl FnMut() -> bool) {
    // Generous: only an upper bound for slow shared runners; a passing test returns at once.
    for _ in 0..6000 {
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
    // Keep sending unknown traffic until the (2 s) heartbeat deadline drops the link. If
    // unknown traffic refreshed the deadline this would never drop and `until` would fail.
    let mut sent = 0;
    until("dropped despite unknown traffic", || {
        fake.send_raw(r#"{"weird":{}}"#);
        sent += 1;
        !c.status().connected
    })
    .await;
    assert!(sent > 1, "traffic was flowing when the link dropped");
    assert!(c
        .events()
        .iter()
        .any(|e| e.kind == EventType::MessageUnknown));
    c.stop().await;
}

#[tokio::test]
async fn failed_connection_checks_for_a_blocked_network_only_for_blocked_classes() {
    // client.py: `saved_address_blocked` is consulted only after permission_denied / no_route.
    // A refused connection (nothing listening) must NOT trigger it and keeps the generic text.
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let asked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let a = asked.clone();
    let check: stagepilot_playback::client::BlockedCheck = std::sync::Arc::new(move || {
        a.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async { true })
    });
    let mut c = PlaybackClient::start_with(opts(port), None, check).unwrap();
    until("unavailable", || c.status().last_error.is_some()).await;
    assert_eq!(
        c.status().last_error.as_deref(),
        Some("Playback unavailable; remote connections may be off")
    );
    c.stop().await;
    assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 0);
}

/// Rig D finding: Playback still held half-closed sockets from a previous process, so new
/// connections were accepted at TCP level but never completed the WebSocket handshake. The
/// client must bound each attempt and recover within seconds once Playback is healthy again.
async fn recovers_after_stalled_handshakes(host_override: bool) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
    // Phase 1: accept and hold every socket open, never answering.
    let stall = tokio::spawn(async move {
        let mut held = Vec::new();
        loop {
            tokio::select! {
                _ = stop_rx.changed() => return held,
                acc = listener.accept() => if let Ok((s, _)) = acc { held.push(s) },
            }
        }
    });
    let mut o = opts(port);
    o.heartbeat_timeout = Duration::from_millis(500);
    if !host_override {
        o.host_override = None;
    }
    let c = PlaybackClient::start(o, None).unwrap();
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(!c.status().connected);
    // Playback "restarts" healthy on the same port; the stalled sockets stay open.
    let _ = stop_tx.send(true);
    let held = stall.await.unwrap();
    assert!(!held.is_empty(), "client attempted to connect");
    let fake = FakePlayback::start_on(port, None).await;
    fake.auto_beat(Duration::from_millis(50));
    let t = std::time::Instant::now();
    until("connected after stall", || c.status().connected).await;
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    drop(held);
    c.shutdown().await;
}

#[tokio::test]
async fn recovers_after_stalled_handshakes_manual_host() {
    recovers_after_stalled_handshakes(true).await;
}

#[tokio::test]
async fn recovers_after_stalled_handshakes_auto_discovery() {
    recovers_after_stalled_handshakes(false).await;
}

#[tokio::test]
async fn recovers_after_sockets_that_close_or_go_silent_after_accept() {
    // Variants of the rig D condition: accept then close at once, and a bad (non-WS) reply.
    use tokio::io::AsyncWriteExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
    let bad = tokio::spawn(async move {
        let mut n = 0u32;
        loop {
            tokio::select! {
                _ = stop_rx.changed() => return n,
                acc = listener.accept() => if let Ok((mut s, _)) = acc {
                    n += 1;
                    if n.is_multiple_of(2) { let _ = s.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await; }
                    drop(s);
                },
            }
        }
    });
    let c = PlaybackClient::start(opts(port), None).unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(!c.status().connected);
    let _ = stop_tx.send(true);
    assert!(bad.await.unwrap() >= 2, "client retried");
    let fake = FakePlayback::start_on(port, None).await;
    fake.auto_beat(Duration::from_millis(50));
    until("connected after bad accepts", || c.status().connected).await;
    c.shutdown().await;
}
