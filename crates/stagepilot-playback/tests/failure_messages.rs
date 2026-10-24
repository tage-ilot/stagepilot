//! m11: the Playback error text says what failed. A network that cannot reach Playback is not
//! "remote connections may be off"; that advice is only right when Playback ends the connection.

use stagepilot_playback::client::{ConnectionOptions, PlaybackClient};
use std::time::Duration;

const CANNOT_REACH: &str = "Cannot reach Playback at 192.0.2.1: network unreachable";
const REFUSED: &str = "Playback refused the connection; remote connections may be off";

fn opts(host: &str, port: u16) -> ConnectionOptions {
    ConnectionOptions {
        port,
        host_override: Some(host.into()),
        heartbeat_timeout: Duration::from_millis(400),
        reconnect_delay: Duration::from_millis(50),
        ..Default::default()
    }
}

async fn first_error(c: &PlaybackClient) -> String {
    for _ in 0..1500 {
        if let Some(error) = c.status().last_error {
            return error;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no error was reported");
}

#[tokio::test]
async fn an_unreachable_host_says_the_network_cannot_reach_it() {
    // 192.0.2.1 is TEST-NET-1 (RFC 5737): never routed. The connect either times out or fails
    // with "network/host unreachable" at once; both are the network, not Playback.
    let mut c = PlaybackClient::start(opts("192.0.2.1", 8080), None).unwrap();
    assert_eq!(first_error(&c).await, CANNOT_REACH);
    c.stop().await;
}

#[tokio::test]
async fn nothing_listening_says_playback_refused() {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut c = PlaybackClient::start(opts("127.0.0.1", port), None).unwrap();
    assert_eq!(first_error(&c).await, REFUSED);
    c.stop().await;
}

#[tokio::test]
async fn a_websocket_that_is_closed_on_us_says_playback_refused() {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            // Accept and hang up before any WebSocket handshake.
            let _ = listener.accept().await;
        }
    });
    let mut c = PlaybackClient::start(opts("127.0.0.1", port), None).unwrap();
    assert_eq!(first_error(&c).await, REFUSED);
    c.stop().await;
}

#[tokio::test]
async fn a_peer_that_never_answers_the_handshake_says_playback_refused() {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let mut held = Vec::new();
        loop {
            // Keep the sockets open and silent: TCP connects, the handshake never completes.
            if let Ok((socket, _)) = listener.accept().await {
                held.push(socket);
            }
        }
    });
    let mut c = PlaybackClient::start(opts("127.0.0.1", port), None).unwrap();
    assert_eq!(first_error(&c).await, REFUSED);
    c.stop().await;
}
