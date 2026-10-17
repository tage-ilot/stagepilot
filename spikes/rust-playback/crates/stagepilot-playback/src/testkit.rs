//! Small scriptable fake Playback, equivalent to `backend/tests/playback_fake.py`
//! (Rust-native; the Python fake is not used by the Rust tests).
//!
//! It speaks the same wire: `pr-protocol` subprotocol, a heartbeat immediately after the
//! handshake, state messages echoed on commands, scriptable drop / stall / offline.

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, watch};
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, Clone)]
pub struct FakeState {
    pub song: i64,
    pub position: f64,
    pub playing: bool,
    pub pad: bool,
    pub version: i64,
    pub stalled: bool,
    /// Song length used to clamp `waveformSeek` (the real one seeks to the end).
    pub length: f64,
}

pub fn heartbeat(s: &FakeState) -> String {
    json!({"heartbeat": {"stateData": {
        "setlistSongID": s.song,
        "sequenceTime": s.position,
        "sequencerPlayState": { if s.playing {"playing"} else {"stopped"}: {} },
        "padPlayerPlayState": { if s.pad {"playing"} else {"stopped"}: {} },
        "setlistCloudVersion": s.version,
    }}})
    .to_string()
}

pub struct FakePlayback {
    pub port: u16,
    pub state: Arc<Mutex<FakeState>>,
    /// Every text frame the client sent, in order.
    pub received: Arc<Mutex<Vec<String>>>,
    pub connections: Arc<Mutex<u32>>,
    out: broadcast::Sender<String>,
    drop_tx: watch::Sender<u64>,
    stop_tx: watch::Sender<bool>,
    /// Test hook: subprotocol to answer with (None = correct `pr-protocol`).
    pub protocol: Arc<Mutex<Option<&'static str>>>,
}

impl FakePlayback {
    pub async fn start() -> Self {
        Self::start_on(0, None).await
    }

    pub async fn start_on(port: u16, state: Option<Arc<Mutex<FakeState>>>) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", port)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = state.unwrap_or_else(|| {
            Arc::new(Mutex::new(FakeState {
                song: 101,
                position: 0.0,
                playing: false,
                pad: false,
                version: 1,
                stalled: false,
                length: 240.0,
            }))
        });
        let (out, _) = broadcast::channel(4096);
        let (drop_tx, _) = watch::channel(0u64);
        let (stop_tx, stop_rx) = watch::channel(false);
        let fake = FakePlayback {
            port,
            state: state.clone(),
            received: Arc::default(),
            connections: Arc::default(),
            out: out.clone(),
            drop_tx: drop_tx.clone(),
            stop_tx,
            protocol: Arc::default(),
        };
        let (received, connections, protocol) = (
            fake.received.clone(),
            fake.connections.clone(),
            fake.protocol.clone(),
        );
        tokio::spawn(async move {
            let mut stop_rx = stop_rx;
            loop {
                tokio::select! {
                    _ = stop_rx.changed() => break,
                    acc = listener.accept() => {
                        let Ok((sock, _)) = acc else { continue };
                        tokio::spawn(serve(sock, state.clone(), received.clone(), connections.clone(),
                            out.subscribe(), drop_tx.subscribe(), protocol.clone(), stop_rx.clone()));
                    }
                }
            }
        });
        fake
    }

    /// Send one heartbeat with the current state (no-op while stalled).
    pub fn beat(&self) {
        let s = self.state.lock().unwrap();
        if !s.stalled {
            let _ = self.out.send(heartbeat(&s));
        }
    }

    pub fn send_raw(&self, text: &str) {
        let _ = self.out.send(text.to_string());
    }

    pub fn stall(&self, stalled: bool) {
        self.state.lock().unwrap().stalled = stalled;
    }

    /// Close the current client connection; the listener stays up.
    pub fn drop_connection(&self) {
        self.drop_tx.send_modify(|v| *v += 1);
    }

    /// Drop the connection and refuse new ones. Returns the port and shared state so
    /// `start_on` can bring it back on the same port.
    pub async fn go_offline(self) -> (u16, Arc<Mutex<FakeState>>) {
        self.drop_connection();
        let _ = self.stop_tx.send(true);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        (self.port, self.state)
    }

    pub fn received_kinds(&self) -> Vec<String> {
        self.received
            .lock()
            .unwrap()
            .iter()
            .filter_map(|t| serde_json::from_str::<Value>(t).ok())
            .filter_map(|v| v.as_object().and_then(|m| m.keys().next().cloned()))
            .collect()
    }
}

#[allow(clippy::too_many_arguments)]
async fn serve(
    sock: TcpStream,
    state: Arc<Mutex<FakeState>>,
    received: Arc<Mutex<Vec<String>>>,
    connections: Arc<Mutex<u32>>,
    mut out: broadcast::Receiver<String>,
    mut drop_rx: watch::Receiver<u64>,
    protocol: Arc<Mutex<Option<&'static str>>>,
    mut stop: watch::Receiver<bool>,
) {
    let proto = *protocol.lock().unwrap();
    #[allow(clippy::result_large_err)] // callback signature is fixed by tungstenite
    let cb = |req: &Request, mut resp: Response| {
        let _ = req;
        let name = proto.unwrap_or("pr-protocol");
        resp.headers_mut()
            .insert("Sec-WebSocket-Protocol", HeaderValue::from_static(name));
        Ok(resp)
    };
    let _ = sock.set_nodelay(true);
    let Ok(mut ws) = tokio_tungstenite::accept_hdr_async(sock, cb).await else {
        return;
    };
    *connections.lock().unwrap() += 1;
    let first = {
        let s = state.lock().unwrap();
        (!s.stalled).then(|| heartbeat(&s))
    };
    if let Some(first) = first {
        let _ = ws.send(Message::text(first)).await;
    }
    drop_rx.borrow_and_update();
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            _ = drop_rx.changed() => { let _ = ws.close(None).await; break }
            text = out.recv() => {
                let Ok(text) = text else { continue };
                if ws.send(Message::text(text)).await.is_err() { break }
            }
            msg = ws.next() => {
                let Some(Ok(Message::Text(t))) = msg else {
                    if matches!(msg, Some(Ok(_))) { continue } else { break }
                };
                received.lock().unwrap().push(t.to_string());
                let reply = apply(&state, t.as_str());
                for r in reply { if ws.send(Message::text(r)).await.is_err() { return } }
            }
        }
    }
}

/// React to a client command like Playback does: change state, echo, then a heartbeat.
fn apply(state: &Mutex<FakeState>, text: &str) -> Vec<String> {
    let Ok(Value::Object(m)) = serde_json::from_str::<Value>(text) else {
        return vec![];
    };
    let Some((kind, body)) = m.into_iter().next() else {
        return vec![];
    };
    let mut s = state.lock().unwrap();
    match kind.as_str() {
        "transportPlay" => s.playing = body["playing"].as_bool().unwrap_or(s.playing),
        "transportPad" => s.pad = body["playing"].as_bool().unwrap_or(s.pad),
        "waveformSeek" => s.position = body["sequenceTime"].as_f64().unwrap_or(0.0).min(s.length),
        "transportReturnToStart" => s.position = 0.0,
        "setlistSelectSong" => {
            if let Some(id) = body["setlistSongID"].as_i64() {
                s.song = id;
                s.position = 0.0;
                s.playing = false;
            }
        }
        _ => {}
    }
    let mut out = vec![text.to_string()];
    if !s.stalled {
        out.push(heartbeat(&s));
    }
    out
}
