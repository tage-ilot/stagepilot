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
    /// Setlist order for `transportPreviousSong` / `transportNextSong` (empty = no navigation).
    /// Navigation stops at both ends like the real Playback (no wrap).
    pub songs: Vec<i64>,
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
                songs: vec![],
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

    /// Heartbeat every `period` until the fake goes away, like the real Playback's ~10 Hz.
    pub fn auto_beat(&self, period: std::time::Duration) {
        let (out, state, mut stop) = (
            self.out.clone(),
            self.state.clone(),
            self.stop_tx.subscribe(),
        );
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(period);
            loop {
                tokio::select! {
                    _ = stop.changed() => break,
                    _ = tick.tick() => {
                        let s = state.lock().unwrap();
                        if !s.stalled {
                            let _ = out.send(heartbeat(&s));
                        }
                    }
                }
            }
        });
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
        "transportPreviousSong" | "transportNextSong" => {
            if let Some(at) = s.songs.iter().position(|id| *id == s.song) {
                let to = if kind == "transportNextSong" {
                    (at + 1).min(s.songs.len() - 1)
                } else {
                    at.saturating_sub(1)
                };
                if to != at {
                    s.song = s.songs[to];
                    s.position = 0.0;
                    s.playing = false;
                }
            }
        }
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

// ---------------------------------------------------------------------------------------------
// SimPlayback: in-process scripted Playback for measurement / discovery tests (no sockets).
// Implements `TransportPort`, so it runs on tokio's paused virtual clock.
// ---------------------------------------------------------------------------------------------

use crate::client::{ClientError, Command, PlaybackStatus, TransportPort};
use crate::normalizer::{Heartbeat, SetlistId};

#[derive(Debug, Clone)]
pub struct SimState {
    pub songs: Vec<i64>,
    /// Length per song; `None` = a seek-to-end that has no effect (Playback ignores it).
    pub lengths: Vec<Option<f64>>,
    pub index: usize,
    pub position: f64,
    pub playing: bool,
    pub version: i64,
    pub setlist_id: Option<i64>,
    pub connected: bool,
    pub generation: u64,
    /// Every command kind received, in order (`transportNextSong`, ...).
    pub commands: Vec<String>,
    /// When set, a seek-to-end reports a different end every time (never steady).
    pub unsteady: bool,
    /// When set, commands are accepted but the state never changes.
    pub deaf: bool,
}

pub struct SimPlayback {
    pub state: Arc<Mutex<SimState>>,
    serial: Arc<std::sync::atomic::AtomicU64>,
    last: Arc<Mutex<tokio::time::Instant>>,
    changed: watch::Sender<u64>,
}

impl SimPlayback {
    pub fn new(songs: Vec<i64>, lengths: Vec<Option<f64>>, index: usize) -> Arc<Self> {
        let sim = Arc::new(SimPlayback {
            state: Arc::new(Mutex::new(SimState {
                songs,
                lengths,
                index,
                position: 0.0,
                playing: false,
                version: 1,
                setlist_id: Some(122),
                connected: true,
                generation: 1,
                commands: vec![],
                unsteady: false,
                deaf: false,
            })),
            serial: Arc::default(),
            last: Arc::new(Mutex::new(tokio::time::Instant::now())),
            changed: watch::channel(0).0,
        });
        sim.beat();
        sim
    }

    /// One fresh heartbeat with the current state (what Playback does ~10 times a second).
    pub fn beat(&self) {
        {
            // `unsteady`: an end position that keeps creeping, so it is never a stable length.
            let mut s = self.state.lock().unwrap();
            if s.unsteady && s.position > 0.5 {
                s.position += 0.5;
            }
        }
        self.serial
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        *self.last.lock().unwrap() = tokio::time::Instant::now();
        self.changed.send_modify(|v| *v += 1);
    }

    /// Heartbeats every `every` of (virtual) time until the returned handle is aborted.
    pub fn spawn_heartbeats(
        self: &Arc<Self>,
        every: std::time::Duration,
    ) -> tokio::task::JoinHandle<()> {
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(every).await;
                me.beat();
            }
        })
    }

    pub fn snapshot(&self) -> SimState {
        self.state.lock().unwrap().clone()
    }

    pub fn commands(&self) -> Vec<String> {
        self.state.lock().unwrap().commands.clone()
    }

    pub fn disconnect(&self) {
        let mut s = self.state.lock().unwrap();
        s.connected = false;
        s.generation += 1;
        drop(s);
        self.changed.send_modify(|v| *v += 1);
    }

    fn apply(&self, command: Command) {
        let mut s = self.state.lock().unwrap();
        let kind = command
            .payload()
            .trim_start_matches('{')
            .split('"')
            .nth(1)
            .unwrap_or("")
            .to_string();
        s.commands.push(kind);
        if s.deaf {
            return;
        }
        match command {
            Command::Previous => {
                s.index = s.index.saturating_sub(1);
                s.position = 0.0;
            }
            Command::Next => {
                s.index = (s.index + 1).min(s.songs.len() - 1);
                s.position = 0.0;
            }
            Command::ReturnToStart => s.position = 0.0,
            Command::SeekEnd | Command::Seek(_) => {
                let target = match command {
                    Command::Seek(t) => t,
                    _ => 86400.0,
                };
                if let Some(Some(len)) = s.lengths.get(s.index).copied() {
                    s.position = if target >= len { len } else { target };
                }
            }
            Command::Play(p) => s.playing = p,
            Command::SelectSong(id) => {
                if let Some(i) = s.songs.iter().position(|x| *x == id) {
                    s.index = i;
                    s.position = 0.0;
                    s.playing = false;
                }
            }
            _ => {}
        }
    }
}

impl TransportPort for SimPlayback {
    fn status(&self) -> PlaybackStatus {
        let s = self.state.lock().unwrap();
        PlaybackStatus {
            connected: s.connected,
            host: Some("sim".into()),
            port: 0,
            last_error: None,
            heartbeat: s.connected.then(|| Heartbeat {
                song_id: s.songs[s.index],
                position: s.position,
                playing: s.playing,
                pad: false,
                setlist_version: Some(s.version),
            }),
            setlist_id: s.setlist_id.map(SetlistId::Int),
            heartbeat_serial: 0,
        }
    }
    fn generation(&self) -> u64 {
        self.state.lock().unwrap().generation
    }
    fn heartbeat_serial(&self) -> u64 {
        self.serial.load(std::sync::atomic::Ordering::SeqCst)
    }
    fn last_heartbeat_at(&self) -> tokio::time::Instant {
        *self.last.lock().unwrap()
    }
    fn subscribe(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }
    async fn send(&self, command: Command) -> Result<(), ClientError> {
        {
            let s = self.state.lock().unwrap();
            if !s.connected {
                return Err(ClientError::Disconnected);
            }
            if matches!(
                command,
                Command::Previous | Command::Next | Command::SeekEnd | Command::ReturnToStart
            ) && s.playing
            {
                return Err(ClientError::NotStopped);
            }
        }
        self.apply(command);
        // Playback answers a command with a heartbeat (after real latency; here: immediately).
        self.beat();
        Ok(())
    }
}
