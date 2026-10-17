//! Playback WebSocket client: mirrors `playback_api/client.py` + `ws.py`.
//!
//! Mirrored behaviour: `pr-protocol` subprotocol must be negotiated; 1 MiB message cap;
//! masked server frames rejected; a connection counts as "connected" only after the
//! first valid heartbeat; heartbeat timeout (default 5 s) drops the socket and publishes
//! an error status; reconnect after a fixed 1 s; status is published before the
//! events of each message; a fresh normalizer on every reconnect; 100-event history.
//!
//! Not ported in this spike: LAN scan / discovery (`find.py`), arbitration, length
//! measurement. Discovery is "manual host, else loopback".

use crate::normalizer::{Heartbeat, Normalizer, PlaybackEvent, SetlistId};
use futures_util::{SinkExt, StreamExt};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;

pub const SUBPROTOCOL: &str = "pr-protocol";
pub const MAX_MESSAGE: usize = 1024 * 1024;
const UNAVAILABLE: &str = "Playback unavailable; remote connections may be off";

#[derive(Debug, Clone)]
pub struct ConnectionOptions {
    pub enabled: bool,
    pub host_override: Option<String>,
    pub port: u16,
    pub fast_transport: bool,
    pub heartbeat_timeout: Duration,
    pub reconnect_delay: Duration,
    pub history_capacity: usize,
}

impl Default for ConnectionOptions {
    fn default() -> Self {
        ConnectionOptions {
            enabled: true,
            host_override: None,
            port: 8080,
            fast_transport: true,
            heartbeat_timeout: Duration::from_secs(5),
            reconnect_delay: Duration::from_secs(1),
            history_capacity: 100,
        }
    }
}

impl ConnectionOptions {
    /// Same validation as the Python dataclass / constructor.
    pub fn validate(&self) -> Result<(), ClientError> {
        if self.port == 0 {
            return Err(ClientError::Invalid("invalid Playback port"));
        }
        if let Some(h) = &self.host_override {
            if h.is_empty() || h.chars().any(|c| "\r\n /?#".contains(c)) {
                return Err(ClientError::Invalid("invalid Playback host"));
            }
        }
        if self.heartbeat_timeout.is_zero()
            || self.reconnect_delay.is_zero()
            || self.history_capacity < 1
        {
            return Err(ClientError::Invalid(
                "Playback timeouts and capacities must be positive",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaybackStatus {
    pub connected: bool,
    pub host: Option<String>,
    pub port: u16,
    pub last_error: Option<String>,
    pub heartbeat: Option<Heartbeat>,
    pub setlist_id: Option<SetlistId>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    // Sent by the Python client (song-order discovery / length measurement).
    Previous,
    Next,
    SeekEnd,
    ReturnToStart,
    // Wire shapes taken from what normalizer.py parses; Python never sends these.
    Play(bool),
    Seek(f64),
    SelectSong(i64),
    Pad(bool),
    /// direction: 1 = fade out, 0 = fade in
    Fade(u8),
    DoubleTap {
        section_id: i64,
        active: bool,
    },
    Loop {
        section_id: i64,
        active: bool,
    },
}

impl Command {
    /// Python-sent commands are byte-identical to `ws.py::_discovery_step`.
    pub fn payload(&self) -> String {
        match self {
            Command::Previous => r#"{"transportPreviousSong":{}}"#.into(),
            Command::Next => r#"{"transportNextSong":{}}"#.into(),
            Command::SeekEnd => r#"{"waveformSeek":{"sequenceTime":86400.0}}"#.into(),
            Command::ReturnToStart => r#"{"transportReturnToStart":{}}"#.into(),
            Command::Play(p) => format!(r#"{{"transportPlay":{{"playing":{p}}}}}"#),
            Command::Seek(t) => {
                serde_json::json!({"waveformSeek": {"sequenceTime": t}}).to_string()
            }
            Command::SelectSong(id) => {
                format!(r#"{{"setlistSelectSong":{{"setlistSongID":{id}}}}}"#)
            }
            Command::Pad(p) => format!(r#"{{"transportPad":{{"playing":{p}}}}}"#),
            Command::Fade(d) => format!(r#"{{"transportFade":{{"direction":{d}}}}}"#),
            Command::DoubleTap { section_id, active } => format!(
                r#"{{"waveformDoubleTap":{{"setlistSongSectionID":{section_id},"active":{active}}}}}"#
            ),
            Command::Loop { section_id, active } => format!(
                r#"{{"waveformLoop":{{"setlistSongSectionID":{section_id},"active":{active}}}}}"#
            ),
        }
    }

    /// Python only allows its four discovery commands while stopped.
    fn needs_stopped(&self) -> bool {
        matches!(
            self,
            Command::Previous | Command::Next | Command::SeekEnd | Command::ReturnToStart
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    Invalid(&'static str),
    /// Playback is not connected, or the connection changed under the command.
    Disconnected,
    /// Command refused locally: Playback is (or may be) playing.
    NotStopped,
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Invalid(m) => f.write_str(m),
            ClientError::Disconnected => f.write_str("Playback is disconnected"),
            ClientError::NotStopped => f.write_str("Stop Playback before discovering song order"),
        }
    }
}

impl std::error::Error for ClientError {}

pub type Observer = Arc<dyn Fn(&PlaybackStatus, &[PlaybackEvent]) + Send + Sync>;

struct Shared {
    status: Mutex<PlaybackStatus>,
    history: Mutex<VecDeque<PlaybackEvent>>,
    generation: Mutex<u64>,
}

struct Request {
    command: Command,
    generation: u64,
    reply: oneshot::Sender<Result<(), ClientError>>,
}

pub struct PlaybackClient {
    shared: Arc<Shared>,
    cmd_tx: mpsc::Sender<Request>,
    stop_tx: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
    port: u16,
}

impl PlaybackClient {
    pub fn start(
        options: ConnectionOptions,
        observer: Option<Observer>,
    ) -> Result<Self, ClientError> {
        options.validate()?;
        let shared = Arc::new(Shared {
            status: Mutex::new(PlaybackStatus {
                port: options.port,
                ..Default::default()
            }),
            history: Mutex::new(VecDeque::new()),
            generation: Mutex::new(0),
        });
        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        let (stop_tx, stop_rx) = watch::channel(false);
        let port = options.port;
        let task = if options.enabled {
            Some(tokio::spawn(run(
                options,
                shared.clone(),
                observer,
                cmd_rx,
                stop_rx,
            )))
        } else {
            None
        };
        Ok(PlaybackClient {
            shared,
            cmd_tx,
            stop_tx,
            task,
            port,
        })
    }

    pub fn status(&self) -> PlaybackStatus {
        self.shared.status.lock().unwrap().clone()
    }

    pub fn events(&self) -> Vec<PlaybackEvent> {
        self.shared
            .history
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .collect()
    }

    /// Send a command. Refused locally when disconnected, or (for the four
    /// discovery commands) unless the last heartbeat says stopped. The check is
    /// repeated by the connection task immediately before the write.
    pub async fn send(&self, command: Command) -> Result<(), ClientError> {
        let generation = {
            let st = self.shared.status.lock().unwrap();
            if !st.connected {
                return Err(ClientError::Disconnected);
            }
            if command.needs_stopped() && st.heartbeat.is_none_or(|h| h.playing) {
                return Err(ClientError::NotStopped);
            }
            *self.shared.generation.lock().unwrap()
        };
        let (reply, rx) = oneshot::channel();
        self.cmd_tx
            .send(Request {
                command,
                generation,
                reply,
            })
            .await
            .map_err(|_| ClientError::Disconnected)?;
        rx.await.unwrap_or(Err(ClientError::Disconnected))
    }

    pub async fn stop(&mut self) {
        let _ = self.stop_tx.send(true);
        if let Some(t) = self.task.take() {
            let _ = t.await;
        }
        *self.shared.status.lock().unwrap() = PlaybackStatus {
            port: self.port,
            ..Default::default()
        };
    }
}

fn publish(
    shared: &Shared,
    observer: &Option<Observer>,
    cap: usize,
    status: PlaybackStatus,
    events: &[PlaybackEvent],
) {
    *shared.status.lock().unwrap() = status.clone();
    {
        let mut h = shared.history.lock().unwrap();
        h.extend(events.iter().cloned());
        while h.len() > cap {
            h.pop_front();
        }
    }
    if let Some(o) = observer {
        // An observer panic must not kill reconnect (Python suppresses exceptions).
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| o(&status, events)));
    }
}

#[derive(Debug)]
#[allow(dead_code)] // reason strings kept for diagnostics; Python publishes a generic message
enum ConnError {
    Closed(&'static str),
    Io,
}

async fn connect(
    host: &str,
    port: u16,
    timeout: Duration,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    ConnError,
> {
    let authority = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    let mut req = format!("ws://{authority}:{port}/")
        .into_client_request()
        .map_err(|_| ConnError::Closed("invalid host"))?;
    req.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        HeaderValue::from_static(SUBPROTOCOL),
    );
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE))
        .max_frame_size(Some(MAX_MESSAGE));
    let (ws, resp) = tokio::time::timeout(
        timeout,
        tokio_tungstenite::connect_async_with_config(req, Some(config), true),
    )
    .await
    .map_err(|_| ConnError::Closed("Playback upgrade timed out"))?
    .map_err(|_| ConnError::Io)?;
    let negotiated = resp
        .headers()
        .get("sec-websocket-protocol")
        .and_then(|v| v.to_str().ok());
    if negotiated != Some(SUBPROTOCOL) {
        return Err(ConnError::Closed("invalid Playback protocol negotiation"));
    }
    Ok(ws)
}

async fn run(
    options: ConnectionOptions,
    shared: Arc<Shared>,
    observer: Option<Observer>,
    mut cmd_rx: mpsc::Receiver<Request>,
    mut stop: watch::Receiver<bool>,
) {
    let epoch = Instant::now();
    let host = options
        .host_override
        .clone()
        .unwrap_or_else(|| "127.0.0.1".into());
    while !*stop.borrow() {
        let mut valid_heartbeat = false;
        let result: Result<(), ConnError> = async {
            let mut ws = connect(&host, options.port, options.heartbeat_timeout).await?;
            let generation = {
                let mut g = shared.generation.lock().unwrap();
                *g += 1;
                *g
            };
            let mut norm = Normalizer::new(options.fast_transport);
            let mut last_heartbeat = Instant::now();
            loop {
                let remaining = options
                    .heartbeat_timeout
                    .checked_sub(last_heartbeat.elapsed())
                    .filter(|d| !d.is_zero())
                    .ok_or(ConnError::Closed("Playback heartbeat timed out"))?;
                tokio::select! {
                    _ = stop.changed() => { let _ = ws.close(None).await; return Ok(()); }
                    req = cmd_rx.recv() => {
                        let Some(req) = req else { return Ok(()) };
                        let allowed = {
                            let st = shared.status.lock().unwrap();
                            req.generation == generation && st.connected
                                && (!req.command.needs_stopped()
                                    || st.heartbeat.is_some_and(|h| !h.playing))
                        };
                        let r = if !allowed {
                            Err(if req.generation != generation || !shared.status.lock().unwrap().connected {
                                ClientError::Disconnected
                            } else { ClientError::NotStopped })
                        } else {
                            ws.send(Message::text(req.command.payload()))
                                .await
                                .map_err(|_| ClientError::Disconnected)
                        };
                        let _ = req.reply.send(r);
                    }
                    msg = tokio::time::timeout(remaining, ws.next()) => {
                        let msg = msg.map_err(|_| ConnError::Closed("Playback heartbeat timed out"))?;
                        let raw = match msg {
                            Some(Ok(Message::Text(t))) => t,
                            // Ping/pong handled by tungstenite; binary frames are
                            // "unexpected" in Python (opcode not 0/1).
                            Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                            Some(Ok(Message::Close(_))) | None => return Err(ConnError::Closed("Playback sent close")),
                            Some(Ok(_)) => return Err(ConnError::Closed("unexpected Playback frame")),
                            Some(Err(_)) => return Err(ConnError::Io),
                        };
                        let before = norm.heartbeat_serial;
                        let ts = epoch.elapsed().as_secs_f64();
                        let events = norm.feed(raw.as_str(), ts);
                        if norm.heartbeat_serial != before {
                            last_heartbeat = Instant::now();
                            valid_heartbeat = true;
                        }
                        if *stop.borrow() { return Ok(()); }
                        publish(&shared, &observer, options.history_capacity, PlaybackStatus {
                            connected: valid_heartbeat,
                            host: Some(host.clone()),
                            port: options.port,
                            last_error: None,
                            heartbeat: norm.heartbeat,
                            setlist_id: norm.setlist_id.clone(),
                        }, &events);
                    }
                }
            }
        }
        .await;
        // Anything that was in flight belongs to the dead connection.
        *shared.generation.lock().unwrap() += 1;
        if result.is_err() && !*stop.borrow() {
            let _ = &result;
            publish(
                &shared,
                &observer,
                options.history_capacity,
                PlaybackStatus {
                    connected: false,
                    host: Some(host.clone()),
                    port: options.port,
                    last_error: Some(UNAVAILABLE.into()),
                    heartbeat: None,
                    setlist_id: None,
                },
                &[],
            );
        }
        // Commands queued for the dead connection are refused, not replayed later.
        while let Ok(req) = cmd_rx.try_recv() {
            let _ = req.reply.send(Err(ClientError::Disconnected));
        }
        if *stop.borrow() {
            break;
        }
        let _ = tokio::time::timeout(options.reconnect_delay, stop.changed()).await;
    }
}
