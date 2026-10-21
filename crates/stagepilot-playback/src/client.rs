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

use crate::find::{
    classify_error, find_playback, saved_address_blocked, CancelToken, DiscoverySource, ErrorClass,
    Network, ProbeError, SystemNetwork, SCAN_BLOCK_DETAIL,
};
use crate::normalizer::{Heartbeat, Normalizer, PlaybackEvent, SetlistId};
use futures_util::{SinkExt, StreamExt};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
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
    /// Allow the occasional LAN scan when no host is saved (Python `auto_scan`). Off by default
    /// here so a bare client only tries loopback; the plugin passes the saved setting.
    pub auto_scan: bool,
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
            auto_scan: false,
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
    /// Valid heartbeats seen over the client's whole life; changes exactly when Python's
    /// `status.heartbeat is not previous` would (the plugin's "fresh heartbeat" test).
    pub heartbeat_serial: u64,
    /// How the address was found (Python `PlaybackStatus.source`).
    pub source: Option<DiscoverySource>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    // Sent by the Python client (song-order discovery / length measurement).
    Previous,
    Next,
    SeekEnd,
    ReturnToStart,
    // Wire shapes verified against huntrw6/playback-api PROTOCOL.md and live captures
    // (tests/fixtures/wire_commands.json); StagePilot's Python never sends these.
    Play(bool),
    Seek(f64),
    SelectSong(i64),
    Pad(bool),
    /// direction: 1 = fade out, 0 = fade in
    Fade(u8),
    /// Section jump (`waveformDoubleTap`): only the section id, no `active` field.
    SectionJump(i64),
    /// Per-section loop (`waveformLoop`).
    SectionLoop {
        section_id: i64,
        active: bool,
    },
    /// Loop the current section until switched off (`mixerInfiniteLoop`).
    InfiniteLoop(bool),
    /// The Loop button: repeat the playing section once (`mixerLoop`).
    LoopOnce(bool),
    /// The MUTE MIDI toggle (`mixerMuteMIDI`).
    MuteMidi(bool),
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
            Command::SectionJump(section_id) => {
                format!(r#"{{"waveformDoubleTap":{{"setlistSongSectionID":{section_id}}}}}"#)
            }
            Command::SectionLoop { section_id, active } => format!(
                r#"{{"waveformLoop":{{"setlistSongSectionID":{section_id},"active":{active}}}}}"#
            ),
            Command::InfiniteLoop(a) => format!(r#"{{"mixerInfiniteLoop":{{"active":{a}}}}}"#),
            Command::LoopOnce(a) => format!(r#"{{"mixerLoop":{{"active":{a}}}}}"#),
            Command::MuteMidi(a) => format!(r#"{{"mixerMuteMIDI":{{"active":{a}}}}}"#),
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
    /// Valid heartbeats seen over the client's whole life (Python `_heartbeat_serial`).
    heartbeat_serial: AtomicU64,
    /// When the last valid heartbeat arrived (virtual-time friendly).
    last_heartbeat_at: Mutex<tokio::time::Instant>,
    /// Bumped on every publish; lets measurement code wait for "anything changed".
    changed: watch::Sender<u64>,
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
    task: Mutex<Option<JoinHandle<()>>>,
    cancel: CancelToken,
    port: u16,
}

/// Cheap probe after a failure while a host is known: Python `RECONNECT_PROBE_TIMEOUT`.
const RECONNECT_PROBE_TIMEOUT: Duration = Duration::from_secs(1);
/// Python `FULL_SCAN_INTERVAL` / `FULL_SCAN_PROBE_TIMEOUT`.
const FULL_SCAN_INTERVAL: Duration = Duration::from_secs(45);
const FULL_SCAN_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

impl PlaybackClient {
    pub fn start(
        options: ConnectionOptions,
        observer: Option<Observer>,
    ) -> Result<Self, ClientError> {
        Self::start_with(options, observer, system_blocked_check())
    }

    /// Like [`start`](Self::start) with an injectable blocked-address check (tests, other OSes).
    pub fn start_with(
        options: ConnectionOptions,
        observer: Option<Observer>,
        blocked_check: BlockedCheck,
    ) -> Result<Self, ClientError> {
        Self::start_with_network(options, observer, blocked_check, SystemNetwork)
    }

    /// Like [`start_with`](Self::start_with) with an injectable network (tests).
    pub fn start_with_network<N: Network + Clone + 'static>(
        options: ConnectionOptions,
        observer: Option<Observer>,
        blocked_check: BlockedCheck,
        network: N,
    ) -> Result<Self, ClientError> {
        options.validate()?;
        let shared = Arc::new(Shared {
            status: Mutex::new(PlaybackStatus {
                port: options.port,
                ..Default::default()
            }),
            history: Mutex::new(VecDeque::new()),
            generation: Mutex::new(0),
            heartbeat_serial: AtomicU64::new(0),
            last_heartbeat_at: Mutex::new(tokio::time::Instant::now()),
            changed: watch::channel(0).0,
        });
        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        let (stop_tx, stop_rx) = watch::channel(false);
        let port = options.port;
        let cancel = CancelToken::new();
        let task = if options.enabled {
            Some(tokio::spawn(run(
                options,
                shared.clone(),
                observer,
                cmd_rx,
                stop_rx,
                blocked_check,
                network,
                cancel.clone(),
            )))
        } else {
            None
        };
        Ok(PlaybackClient {
            shared,
            cmd_tx,
            stop_tx,
            task: Mutex::new(task),
            cancel,
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
        self.shutdown().await;
    }

    /// [`stop`](Self::stop) without exclusive access, for a client shared with other tasks.
    pub async fn shutdown(&self) {
        let _ = self.stop_tx.send(true);
        self.cancel.cancel();
        let task = self.task.lock().unwrap().take();
        if let Some(t) = task {
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
    shared.changed.send_modify(|v| *v += 1);
    if let Some(o) = observer {
        // An observer panic must not kill reconnect (Python suppresses exceptions).
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| o(&status, events)));
    }
}

/// Why a connection ended. Classified with [`crate::find::classify_error`] exactly like Python's
/// `classify_exception`, so the log and the published message can tell a blocked network from
/// a Playback that is simply off.
#[derive(Debug)]
enum ConnError {
    Closed(&'static str),
    Probe(ProbeError),
}

impl ConnError {
    fn probe_error(&self) -> ProbeError {
        match self {
            ConnError::Closed(m) => ProbeError::Closed((*m).to_string()),
            ConnError::Probe(e) => e.clone(),
        }
    }
}

pub(crate) fn map_ws_error(e: tokio_tungstenite::tungstenite::Error) -> ProbeError {
    use tokio_tungstenite::tungstenite::Error;
    match e {
        Error::Io(io) => ProbeError::from(io),
        Error::Url(_) => ProbeError::InvalidInput,
        Error::Http(_) | Error::HttpFormat(_) | Error::Protocol(_) => {
            ProbeError::Handshake(e.to_string())
        }
        other => ProbeError::Closed(other.to_string()),
    }
}

/// The `last_error` published when a connection attempt fails.
pub fn unavailable_message(blocked: bool) -> &'static str {
    if blocked {
        SCAN_BLOCK_DETAIL
    } else {
        UNAVAILABLE
    }
}

/// "Is the OS blocking all local-network use?" asked after a permission/no-route failure.
pub type BlockedCheck = Arc<
    dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>> + Send + Sync,
>;

/// The real check: loopback works but the gateway cannot be reached either.
pub fn system_blocked_check() -> BlockedCheck {
    Arc::new(|| Box::pin(async { saved_address_blocked(&SystemNetwork).await }))
}

async fn probe_ok<N: Network>(
    net: &N,
    cancel: &CancelToken,
    host: &str,
    port: u16,
    timeout: Duration,
) -> bool {
    !cancel.is_cancelled() && matches!(net.probe(host, port, timeout).await, Ok(Some(_)))
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
    .map_err(|_| ConnError::Probe(ProbeError::TimedOut))?
    .map_err(|e| ConnError::Probe(map_ws_error(e)))?;
    let negotiated = resp
        .headers()
        .get("sec-websocket-protocol")
        .and_then(|v| v.to_str().ok());
    if negotiated != Some(SUBPROTOCOL) {
        return Err(ConnError::Probe(ProbeError::Handshake(
            "invalid Playback protocol negotiation".into(),
        )));
    }
    Ok(ws)
}

#[allow(clippy::too_many_arguments)] // the connection task takes its whole environment
async fn run(
    options: ConnectionOptions,
    shared: Arc<Shared>,
    observer: Option<Observer>,
    mut cmd_rx: mpsc::Receiver<Request>,
    mut stop: watch::Receiver<bool>,
    blocked_check: BlockedCheck,
    network: impl Network + Clone + 'static,
    cancel: CancelToken,
) {
    let epoch = Instant::now();
    let mut host = options
        .host_override
        .clone()
        .unwrap_or_else(|| "127.0.0.1".into());
    let mut last_good: Option<String> = None;
    let mut source: Option<DiscoverySource> = None;
    let mut last_source: Option<DiscoverySource> = None;
    let mut last_scan: Option<Instant> = None;
    while !*stop.borrow() {
        let mut valid_heartbeat = false;
        let result: Result<(), ConnError> = async {
            // Python `_locate`: a manual host is used as is and never falls back to scanning;
            // otherwise the last good address, then loopback, then (auto-scan only, at most once
            // per FULL_SCAN_INTERVAL) the LAN.
            if options.host_override.is_none() {
                let mut found: Option<String> = None;
                if let Some(good) = &last_good {
                    if probe_ok(&network, &cancel, good, options.port, RECONNECT_PROBE_TIMEOUT)
                        .await
                    {
                        found = Some(good.clone());
                        source = last_source;
                    }
                }
                if found.is_none() {
                    let due = options.auto_scan
                        && last_scan.is_none_or(|t| t.elapsed() >= FULL_SCAN_INTERVAL);
                    if due {
                        last_scan = Some(Instant::now());
                    }
                    found = find_playback(
                        &network,
                        None,
                        options.port,
                        due,
                        if due {
                            FULL_SCAN_PROBE_TIMEOUT
                        } else {
                            RECONNECT_PROBE_TIMEOUT
                        },
                        &cancel,
                    )
                    .await
                    .map(|r| {
                        source = Some(r.source);
                        r.host
                    });
                }
                match found {
                    Some(h) => host = h,
                    None => {
                        return Err(ConnError::Closed(
                            "Playback not found; remote connections may be off",
                        ))
                    }
                }
            }
            let mut ws = connect(&host, options.port, options.heartbeat_timeout).await?;
            if options.host_override.is_some() {
                source = Some(DiscoverySource::Manual);
            }
            tracing::info!(target: "playback", host = %host, port = options.port, "playback_connected");
            last_good = Some(host.clone());
            last_source = source;
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
                            Some(Err(e)) => return Err(ConnError::Probe(map_ws_error(e))),
                        };
                        let before = norm.heartbeat_serial;
                        let ts = epoch.elapsed().as_secs_f64();
                        let events = norm.feed(raw.as_str(), ts);
                        if norm.heartbeat_serial != before {
                            last_heartbeat = Instant::now();
                            valid_heartbeat = true;
                            shared.heartbeat_serial.fetch_add(1, Ordering::SeqCst);
                            *shared.last_heartbeat_at.lock().unwrap() = tokio::time::Instant::now();
                        }
                        if *stop.borrow() { return Ok(()); }
                        publish(&shared, &observer, options.history_capacity, PlaybackStatus {
                            connected: valid_heartbeat,
                            host: Some(host.clone()),
                            port: options.port,
                            last_error: None,
                            heartbeat: norm.heartbeat,
                            setlist_id: norm.setlist_id.clone(),
                            heartbeat_serial: shared.heartbeat_serial.load(Ordering::SeqCst),
                            source,
                        }, &events);
                    }
                }
            }
        }
        .await;
        // Anything that was in flight belongs to the dead connection.
        *shared.generation.lock().unwrap() += 1;
        let stopping = *stop.borrow();
        if let (Err(err), false) = (&result, stopping) {
            let (class, detail, step) = classify_error(&err.probe_error());
            // One line per failed attempt (Python logs `playback_connect_failed`); without it a
            // client stuck against half-closed sockets looks silent (rig D finding).
            tracing::warn!(
                target: "playback",
                host = %host,
                port = options.port,
                class = ?class,
                step = ?step,
                retry_in_ms = options.reconnect_delay.as_millis() as u64,
                "playback_connect_failed: {detail}"
            );
            // A saved address that fails at once may be the OS refusing all local-network use.
            let blocked = matches!(class, ErrorClass::PermissionDenied | ErrorClass::NoRoute)
                && blocked_check().await;
            publish(
                &shared,
                &observer,
                options.history_capacity,
                PlaybackStatus {
                    connected: false,
                    host: Some(host.clone()),
                    port: options.port,
                    last_error: Some(
                        if blocked {
                            SCAN_BLOCK_DETAIL
                        } else {
                            UNAVAILABLE
                        }
                        .into(),
                    ),
                    heartbeat: None,
                    setlist_id: None,
                    heartbeat_serial: shared.heartbeat_serial.load(Ordering::SeqCst),
                    source,
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

/// What measurement and song-order discovery need from a Playback connection.
/// `PlaybackClient` implements it; tests implement it with a scripted fake and a
/// virtual clock, so no real sockets or sleeping are needed.
pub trait TransportPort: Send + Sync {
    fn status(&self) -> PlaybackStatus;
    /// Identity of the current connection (changes on every reconnect).
    fn generation(&self) -> u64;
    /// Count of valid heartbeats seen so far (monotonic across reconnects).
    fn heartbeat_serial(&self) -> u64;
    /// When the newest valid heartbeat arrived.
    fn last_heartbeat_at(&self) -> tokio::time::Instant;
    /// Receiver that fires after every published status.
    fn subscribe(&self) -> watch::Receiver<u64>;
    fn send(
        &self,
        command: Command,
    ) -> impl std::future::Future<Output = Result<(), ClientError>> + Send;
}

impl TransportPort for PlaybackClient {
    fn status(&self) -> PlaybackStatus {
        PlaybackClient::status(self)
    }
    fn generation(&self) -> u64 {
        *self.shared.generation.lock().unwrap()
    }
    fn heartbeat_serial(&self) -> u64 {
        self.shared.heartbeat_serial.load(Ordering::SeqCst)
    }
    fn last_heartbeat_at(&self) -> tokio::time::Instant {
        *self.shared.last_heartbeat_at.lock().unwrap()
    }
    fn subscribe(&self) -> watch::Receiver<u64> {
        self.shared.changed.subscribe()
    }
    fn send(
        &self,
        command: Command,
    ) -> impl std::future::Future<Output = Result<(), ClientError>> + Send {
        PlaybackClient::send(self, command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_network_gets_the_macos_message_and_everything_else_the_generic_one() {
        assert_eq!(
            unavailable_message(true),
            "macOS is blocking StagePilot's local-network access"
        );
        assert_eq!(
            unavailable_message(false),
            "Playback unavailable; remote connections may be off"
        );
    }

    #[test]
    fn connection_errors_classify_like_python() {
        let cases = [
            (ConnError::Probe(ProbeError::Refused), ErrorClass::Refused),
            (ConnError::Probe(ProbeError::TimedOut), ErrorClass::TimedOut),
            (
                ConnError::Probe(ProbeError::PermissionDenied("x".into())),
                ErrorClass::PermissionDenied,
            ),
            (
                ConnError::Probe(ProbeError::Handshake("x".into())),
                ErrorClass::NotPlayback,
            ),
            (
                ConnError::Closed("Playback heartbeat timed out"),
                ErrorClass::NotPlayback,
            ),
        ];
        for (err, want) in cases {
            assert_eq!(classify_error(&err.probe_error()).0, want, "{err:?}");
        }
    }
}
