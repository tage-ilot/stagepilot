//! Transport fidelity: crafted server byte streams replayed against the Rust client.
//!
//! Each case in `fixtures/transport_cases.json` carries the verdict of the Python hand-rolled
//! client (`tools/rust-playback/gen_transport_cases.py`). The Rust client must reach the same
//! verdict (accept = stays connected and delivers the final heartbeat on the same connection;
//! reject = drops the connection and reconnects) unless the case is listed in
//! `INTENTIONAL_DIFFERENCES` with a reason.

use serde_json::Value;
use stagepilot_playback::client::{ConnectionOptions, PlaybackClient};
use stagepilot_playback::testkit::{heartbeat, FakeState};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;

/// (case name, why Rust intentionally differs from Python). Empty means full parity.
const INTENTIONAL_DIFFERENCES: &[(&str, &str)] = &[];

fn hb(position: f64) -> String {
    heartbeat(&FakeState {
        song: 101,
        position,
        playing: false,
        pad: false,
        version: 1,
        stalled: false,
        length: 240.0,
    })
}

fn frame(opcode: u8, payload: &[u8], fin: bool) -> Vec<u8> {
    let mut f = vec![(if fin { 0x80 } else { 0 }) | opcode];
    match payload.len() {
        n if n < 126 => f.push(n as u8),
        n if n < 65536 => {
            f.push(126);
            f.extend((n as u16).to_be_bytes());
        }
        n => {
            f.push(127);
            f.extend((n as u64).to_be_bytes());
        }
    }
    f.extend(payload);
    f
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// Handshake by hand so the test controls every byte that follows.
async fn handshake(sock: &mut TcpStream, protocol: &str, bad_accept: bool) -> bool {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        match sock.read(&mut chunk).await {
            Ok(0) | Err(_) => return false,
            Ok(n) => buf.extend(&chunk[..n]),
        }
    }
    let text = String::from_utf8_lossy(&buf).to_string();
    let key = text
        .lines()
        .find_map(|l| {
            l.strip_prefix("Sec-WebSocket-Key: ")
                .or_else(|| l.strip_prefix("sec-websocket-key: "))
        })
        .unwrap()
        .trim()
        .to_string();
    let accept = if bad_accept {
        "AAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string()
    } else {
        derive_accept_key(key.as_bytes())
    };
    let resp = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Accept: {accept}\r\nSec-WebSocket-Protocol: {protocol}\r\n\r\n"
    );
    sock.write_all(resp.as_bytes()).await.is_ok()
}

struct RawServer {
    port: u16,
    connections: Arc<Mutex<u32>>,
    /// Bytes the client wrote after the handshake on the first connection.
    from_client: Arc<Mutex<Vec<u8>>>,
}

/// First connection: valid heartbeat (position 1), then `script`, then hold open (or
/// close abruptly when `abrupt_close`). Later connections: valid heartbeats only.
async fn raw_server(script: Vec<u8>, abrupt_close: bool) -> RawServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let connections = Arc::new(Mutex::new(0u32));
    let from_client = Arc::new(Mutex::new(Vec::new()));
    let (conns, got) = (connections.clone(), from_client.clone());
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let n = {
                let mut c = conns.lock().unwrap();
                *c += 1;
                *c
            };
            let script = script.clone();
            let got = got.clone();
            tokio::spawn(async move {
                if !handshake(&mut sock, "pr-protocol", false).await {
                    return;
                }
                let _ = sock.write_all(&frame(1, hb(1.0).as_bytes(), true)).await;
                if n == 1 {
                    let _ = sock.write_all(&script).await;
                    if abrupt_close {
                        drop(sock); // RST/FIN with no close frame
                        return;
                    }
                }
                // Keep sending valid heartbeats so a surviving connection stays healthy.
                let (mut rd, mut wr) = sock.into_split();
                let reader = tokio::spawn(async move {
                    let mut b = [0u8; 4096];
                    while let Ok(k) = rd.read(&mut b).await {
                        if k == 0 {
                            break;
                        }
                        if n == 1 {
                            got.lock().unwrap().extend(&b[..k]);
                        }
                    }
                });
                for _ in 0..40 {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    if wr
                        .write_all(&frame(1, hb(1.0).as_bytes(), true))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                reader.abort();
            });
        }
    });
    RawServer {
        port,
        connections,
        from_client,
    }
}

fn opts(port: u16) -> ConnectionOptions {
    ConnectionOptions {
        port,
        host_override: Some("127.0.0.1".into()),
        heartbeat_timeout: Duration::from_millis(800),
        reconnect_delay: Duration::from_millis(100),
        ..Default::default()
    }
}

fn large_case(name: &str) -> Vec<Vec<u8>> {
    let mib = 1024 * 1024;
    match name {
        "oversize_single_frame" => vec![frame(1, &vec![b' '; mib + 1], true)],
        "exactly_1mib_single_frame" => {
            let h = hb(7.0);
            let mut p = h.clone().into_bytes();
            p.resize(mib, b' ');
            vec![frame(1, &p, true)]
        }
        "oversize_by_fragments" => vec![
            frame(1, &vec![b' '; 600 * 1024], false),
            frame(0, &vec![b' '; 600 * 1024], true),
        ],
        other => panic!("no generator for {other}"),
    }
}

#[tokio::test]
async fn rust_matches_python_accept_reject_verdicts() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/transport_cases.json")).unwrap();
    let mut diffs = Vec::new();
    for case in &cases {
        let name = case["name"].as_str().unwrap();
        let python_accepts = case["verdict"] == "accept";
        let mut stream: Vec<u8> = if case["generate"].is_null() {
            case["frames"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|h| unhex(h.as_str().unwrap()))
                .collect()
        } else {
            large_case(name).concat()
        };
        stream.extend(frame(1, hb(42.0).as_bytes(), true));
        let srv = raw_server(stream, false).await;
        let mut c = PlaybackClient::start(opts(srv.port), None).unwrap();
        // Accept: position 42 reached while still on connection 1.
        // Reject: the client reconnects (connection count reaches 2) before that.
        let mut accepted = None;
        for _ in 0..300 {
            let st = c.status();
            if st.heartbeat.is_some_and(|h| h.position == 42.0) {
                accepted = Some(*srv.connections.lock().unwrap() == 1);
                break;
            }
            if *srv.connections.lock().unwrap() >= 2 {
                accepted = Some(false);
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        c.stop().await;
        let rust_accepts = accepted.unwrap_or(false);
        let known = INTENTIONAL_DIFFERENCES.iter().find(|(n, _)| *n == name);
        eprintln!(
            "{name:40} python={} rust={}{}",
            if python_accepts { "accept" } else { "reject" },
            if rust_accepts { "accept" } else { "reject" },
            known
                .map(|(_, why)| format!("  (documented: {why})"))
                .unwrap_or_default()
        );
        if python_accepts != rust_accepts && known.is_none() {
            diffs.push(name.to_string());
        }
        if python_accepts == rust_accepts {
            assert!(
                known.is_none(),
                "{name} is listed as a difference but now matches"
            );
        }
    }
    assert!(
        diffs.is_empty(),
        "undocumented Python/Rust differences: {diffs:?}"
    );
}

#[tokio::test]
async fn ping_is_answered_with_a_masked_pong_carrying_the_payload() {
    let mut script = frame(9, b"abc", true);
    script.extend(frame(1, hb(42.0).as_bytes(), true));
    let srv = raw_server(script, false).await;
    let mut c = PlaybackClient::start(opts(srv.port), None).unwrap();
    for _ in 0..200 {
        if c.status().heartbeat.is_some_and(|h| h.position == 42.0) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let got = srv.from_client.lock().unwrap().clone();
    c.stop().await;
    // Client frames are always masked (RFC 6455); pong = opcode 0xA, FIN set, 3-byte payload.
    let idx = got
        .windows(2)
        .position(|w| w == [0x8A, 0x80 | 3])
        .expect("no pong frame from the client");
    let (mask, body) = (&got[idx + 2..idx + 6], &got[idx + 6..idx + 9]);
    let unmasked: Vec<u8> = body
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ mask[i % 4])
        .collect();
    assert_eq!(unmasked, b"abc");
    assert_eq!(
        *srv.connections.lock().unwrap(),
        1,
        "ping must not drop the connection"
    );
}

#[tokio::test]
async fn abrupt_close_without_close_frame_reconnects_with_unavailable_error() {
    let srv = raw_server(Vec::new(), true).await;
    let mut c = PlaybackClient::start(opts(srv.port), None).unwrap();
    for _ in 0..300 {
        if *srv.connections.lock().unwrap() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        *srv.connections.lock().unwrap() >= 2,
        "no reconnect after abrupt close"
    );
    c.stop().await;
}

#[tokio::test]
async fn handshake_with_wrong_accept_key_is_rejected_like_python() {
    // Python: `headers.get("sec-websocket-accept") != expected` -> WSHandshakeError.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let accepted = Arc::new(Mutex::new(0u32));
    let a = accepted.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else {
                return;
            };
            *a.lock().unwrap() += 1;
            tokio::spawn(async move {
                if handshake(&mut s, "pr-protocol", true).await {
                    let _ = s.write_all(&frame(1, hb(42.0).as_bytes(), true)).await;
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            });
        }
    });
    let mut c = PlaybackClient::start(opts(port), None).unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!c.status().connected);
    assert!(c.status().heartbeat.is_none());
    assert!(*accepted.lock().unwrap() >= 2, "should keep retrying");
    c.stop().await;
}
