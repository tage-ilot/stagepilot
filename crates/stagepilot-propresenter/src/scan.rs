//! Finding ProPresenter's network API by looking: the same shape and limits as the Playback network scan
//! (`stagepilot_playback::find`), for a different question.
//!
//! * It asks ONE thing of every address: `GET /version`. Nothing else is ever requested, nothing is written, nothing is sent
//!   to the screen. An answer counts only when it is a JSON object with a `host_description`.
//! * It looks only at private addresses (RFC 1918: 10/8, 172.16/12, 192.168/16) and at this computer (127.0.0.1): the
//!   private networks this computer is on (never wider than the attached /24) plus the /24 of the saved host when that is a
//!   private address. A public address is never probed, whatever is asked.
//! * The saved API port is tried first, then 1025 (ProPresenter's usual one). 32 addresses at a time, in chunks of 64, a
//!   short time limit for each one; it can be cancelled at any moment.
//! * Nothing is saved: the person picks a computer and presses Connect.
//! * When this computer answers on loopback but every other private address fails at once with "not allowed" (macOS blocking
//!   the local network), the result says so (`permission_denied`) with the page to open, the way the Playback scan does.
//!
//! Everything that touches the outside world sits behind [`Network`], so tests run on a fake one.

use async_trait::async_trait;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::task::{JoinHandle, JoinSet};
use ts_rs::TS;

/// ProPresenter's usual API port, tried after the saved one.
pub const USUAL_PORT: u16 = 1025;
/// Probes in flight at once.
pub const MAX_WORKERS: usize = 32;
/// Hosts per progress step.
pub const SCAN_CHUNK: usize = 64;
/// The time limit of one probe: a computer on the same network answers in a few milliseconds.
pub const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);
/// What `GET /version` may weigh.
const MAX_ANSWER: usize = 16 * 1024;
/// Macs ask the person to allow local-network access here.
pub const LOCAL_NETWORK_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork";
/// A blocked path fails at once for (nearly) every address.
const BLOCKED_MIN_SHARE: f64 = 0.95;

const NOT_FOUND_REASON: &str = "No ProPresenter answered on this network.";
const BLOCKED_REASON: &str = "macOS is blocking StagePilot's local-network access.";
const NO_NETWORK_REASON: &str = "No usable network was found on this computer.";

// ---- what the page reads ------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "ProPresenterScanState")]
pub enum ScanState {
    Idle,
    Scanning,
    Found,
    NotFound,
}

/// Why a scan found nothing, when it is more than "nobody answered".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "ProPresenterScanErrorClass")]
pub enum ScanErrorClass {
    PermissionDenied,
    NoRoute,
}

/// One ProPresenter that answered `GET /version`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "ProPresenterScanCandidate")]
pub struct Candidate {
    pub host: String,
    pub port: u16,
    /// The computer's name as ProPresenter gives it (the answer's `name`); the address when it gives none.
    pub name: String,
    /// What ProPresenter calls itself (the answer's `host_description`, for example "ProPresenter 19.0.1").
    pub version: String,
}

/// `GET /api/v1/propresenter/scan`: where the scan is.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export, rename = "ProPresenterScan")]
pub struct ScanSnapshot {
    pub state: ScanState,
    pub candidates: Vec<Candidate>,
    /// One plain sentence when the scan ended without a candidate, or was cancelled.
    pub reason: Option<String>,
    /// Addresses looked at so far / to look at.
    pub current: u32,
    pub total: u32,
    pub phase: Option<String>,
    pub error_class: Option<ScanErrorClass>,
    pub elapsed: f64,
    /// The private networks looked at (`192.168.4.0/24`).
    pub networks: Vec<String>,
    pub hosts_probed: u32,
    pub hosts_total: u32,
    /// What the last scan did, as JSON text, for "Copy diagnostic details". Holds no secret.
    pub details: Option<String>,
    /// The page to open when macOS blocks the local network.
    pub settings_url: Option<String>,
}

impl ScanSnapshot {
    fn idle() -> Self {
        ScanSnapshot {
            state: ScanState::Idle,
            candidates: vec![],
            reason: None,
            current: 0,
            total: 0,
            phase: None,
            error_class: None,
            elapsed: 0.0,
            networks: vec![],
            hosts_probed: 0,
            hosts_total: 0,
            details: None,
            settings_url: None,
        }
    }
}

// ---- the outside world --------------------------------------------------------------------------------------------

/// What a probe that did not get an answer ran into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProbeError {
    TimedOut,
    Refused,
    /// The operating system did not allow the connection (macOS local-network privacy, a firewall).
    Denied,
    NoRoute,
    Other,
}

/// What `GET /version` said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub name: Option<String>,
    pub version: String,
}

#[async_trait]
pub trait Network: Send + Sync {
    /// `GET /version` on `host:port`. `Ok(None)`: something answered, but not as ProPresenter.
    async fn probe(
        &self,
        host: Ipv4Addr,
        port: u16,
        timeout: Duration,
    ) -> Result<Option<Answer>, ProbeError>;
    /// The private networks this computer is on (each at most the attached /24).
    async fn networks(&self) -> Vec<Ipv4Net>;
    /// This computer can talk to itself: tells "blocked" from "nobody there".
    async fn loopback_works(&self) -> bool;
}

/// A private IPv4 network, never smaller than a /24 host range of 254 addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ipv4Net {
    base: u32,
    prefix: u8,
}

impl Ipv4Net {
    /// The network's first address.
    pub fn network(&self) -> Ipv4Addr {
        Ipv4Addr::from(self.base)
    }

    pub fn new(addr: Ipv4Addr, prefix: u8) -> Option<Self> {
        if prefix > 32 {
            return None;
        }
        let mask = if prefix == 0 {
            0
        } else {
            u32::MAX << (32 - prefix as u32)
        };
        Some(Ipv4Net {
            base: u32::from(addr) & mask,
            prefix,
        })
    }
    /// Usable host addresses: no network and no broadcast address.
    pub fn hosts(&self) -> Vec<Ipv4Addr> {
        let size: u64 = 1u64 << (32 - self.prefix as u32);
        let first = self.base as u64;
        let last = first + size - 1;
        let (lo, hi) = if self.prefix >= 31 {
            (first, last)
        } else {
            (first + 1, last - 1)
        };
        (lo..=hi).map(|v| Ipv4Addr::from(v as u32)).collect()
    }
}

impl std::fmt::Display for Ipv4Net {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", Ipv4Addr::from(self.base), self.prefix)
    }
}

/// RFC 1918. The ONLY addresses (besides this computer) a scan ever asks.
pub fn is_private(addr: Ipv4Addr) -> bool {
    addr.is_private()
}

/// This computer or a private address: the scan's whole world.
pub fn may_probe(addr: Ipv4Addr) -> bool {
    addr.is_loopback() || is_private(addr)
}

/// `(address, prefix)` pairs out of `ip -o -4 addr` or `ifconfig` text.
pub fn parse_interfaces(text: &str) -> Vec<(Ipv4Addr, u8)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        let Some(at) = words.iter().position(|w| *w == "inet") else {
            continue;
        };
        let Some(first) = words.get(at + 1) else {
            continue;
        };
        if let Some((addr, prefix)) = first.split_once('/') {
            if let (Ok(addr), Ok(prefix)) = (addr.parse::<Ipv4Addr>(), prefix.parse::<u8>()) {
                out.push((addr, prefix));
            }
        } else if let Ok(addr) = first.parse::<Ipv4Addr>() {
            let mask = words
                .iter()
                .position(|w| *w == "netmask")
                .and_then(|i| words.get(i + 1))
                .and_then(|m| mask_to_prefix(m));
            if let Some(prefix) = mask {
                out.push((addr, prefix));
            }
        }
    }
    out
}

fn mask_to_prefix(mask: &str) -> Option<u8> {
    let value: u32 = if let Some(hex) = mask.strip_prefix("0x").or_else(|| mask.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        u32::from(mask.parse::<Ipv4Addr>().ok()?)
    };
    (value.leading_ones() + value.trailing_zeros() == 32).then(|| value.leading_ones() as u8)
}

/// The private networks worth looking at: skips loopback, link-local, public addresses and prefixes outside 1..=30; never wider
/// than the attached /24.
pub fn usable_networks(interfaces: &[(Ipv4Addr, u8)]) -> Vec<Ipv4Net> {
    let mut out: Vec<Ipv4Net> = Vec::new();
    for (addr, prefix) in interfaces {
        if !is_private(*addr) || !(*prefix > 0 && *prefix < 31) {
            continue;
        }
        if let Some(net) = Ipv4Net::new(*addr, (*prefix).max(24)) {
            if !out.contains(&net) {
                out.push(net);
            }
        }
    }
    out
}

/// The real network: `reqwest` for the one request, the system's own tools for the list of networks.
pub struct SystemNetwork {
    http: reqwest::Client,
}

impl Default for SystemNetwork {
    fn default() -> Self {
        // Cargo unifies reqwest features across the workspace; a TLS provider must exist before any Client is built.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .connect_timeout(PROBE_TIMEOUT)
            .timeout(PROBE_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .pool_max_idle_per_host(0)
            .build()
            .unwrap_or_default();
        SystemNetwork { http }
    }
}

fn classify_io(error: &std::io::Error) -> ProbeError {
    use std::io::ErrorKind::*;
    match error.kind() {
        PermissionDenied => return ProbeError::Denied,
        ConnectionRefused => return ProbeError::Refused,
        TimedOut => return ProbeError::TimedOut,
        HostUnreachable | NetworkUnreachable | NetworkDown => return ProbeError::NoRoute,
        _ => {}
    }
    match error.raw_os_error() {
        // EPERM, EACCES, WSAEACCES.
        Some(1 | 13 | 10013) => ProbeError::Denied,
        // EHOSTUNREACH, ENETUNREACH, EHOSTDOWN, ENETDOWN (Linux, then macOS), WSAEHOSTUNREACH, WSAENETUNREACH.
        Some(113 | 101 | 112 | 100 | 65 | 51 | 64 | 50 | 10065 | 10051) => ProbeError::NoRoute,
        _ => ProbeError::Other,
    }
}

fn classify_request(error: &reqwest::Error) -> Option<ProbeError> {
    if error.is_timeout() {
        return Some(ProbeError::TimedOut);
    }
    let mut source: Option<&(dyn std::error::Error + 'static)> = std::error::Error::source(error);
    while let Some(cause) = source {
        if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            return Some(classify_io(io));
        }
        source = cause.source();
    }
    error.is_connect().then_some(ProbeError::Other)
}

/// The answer is ProPresenter's when it is a JSON object with a text `host_description`.
pub fn read_answer(body: &[u8]) -> Option<Answer> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let text = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| t.chars().take(120).collect::<String>())
    };
    Some(Answer {
        name: text("name"),
        version: text("host_description")?,
    })
}

async fn run_command(program: &str, args: &[&str]) -> Option<String> {
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null());
    let out = tokio::time::timeout(Duration::from_secs(5), command.output())
        .await
        .ok()?
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[async_trait]
impl Network for SystemNetwork {
    async fn probe(
        &self,
        host: Ipv4Addr,
        port: u16,
        timeout: Duration,
    ) -> Result<Option<Answer>, ProbeError> {
        if !may_probe(host) {
            // Never a public address, whoever asks.
            return Err(ProbeError::Other);
        }
        // The one request this module makes.
        let response = tokio::time::timeout(
            timeout,
            self.http
                .get(format!("http://{host}:{port}/version"))
                .send(),
        )
        .await
        .map_err(|_| ProbeError::TimedOut)?
        .map_err(|error| classify_request(&error).unwrap_or(ProbeError::Other));
        let response = match response {
            Ok(response) => response,
            // Connected but it did not talk HTTP: somebody else's service.
            Err(ProbeError::Other) => return Ok(None),
            Err(error) => return Err(error),
        };
        if response.status().as_u16() != 200
            || response
                .content_length()
                .is_some_and(|n| n as usize > MAX_ANSWER)
        {
            return Ok(None);
        }
        let body = tokio::time::timeout(timeout, response.bytes())
            .await
            .map_err(|_| ProbeError::TimedOut)?
            .map_err(|_| ProbeError::Other);
        match body {
            Ok(bytes) if bytes.len() <= MAX_ANSWER => Ok(read_answer(&bytes)),
            _ => Ok(None),
        }
    }

    async fn networks(&self) -> Vec<Ipv4Net> {
        for (program, args) in [
            ("ip", &["-o", "-4", "addr", "show"][..]),
            ("/sbin/ifconfig", &[][..]),
            ("ifconfig", &[][..]),
        ] {
            if let Some(text) = run_command(program, args).await {
                let found = parse_interfaces(&text);
                if !found.is_empty() {
                    return usable_networks(&found);
                }
            }
        }
        let script = "Get-NetIPAddress -AddressFamily IPv4 | ForEach-Object { 'inet ' + $_.IPAddress + '/' + $_.PrefixLength }";
        match run_command(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command", script],
        )
        .await
        {
            Some(text) => usable_networks(&parse_interfaces(&text)),
            None => vec![],
        }
    }

    async fn loopback_works(&self) -> bool {
        let Ok(listener) = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await else {
            return false;
        };
        let Ok(addr) = listener.local_addr() else {
            return false;
        };
        matches!(
            tokio::time::timeout(Duration::from_secs(1), tokio::net::TcpStream::connect(addr))
                .await,
            Ok(Ok(_))
        )
    }
}

// ---- the scan -----------------------------------------------------------------------------------------------------

/// Shared cancel flag.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// How one address went.
enum HostOutcome {
    Found(Candidate),
    /// Nobody there or nothing like ProPresenter; the strongest thing that went wrong, if anything.
    Missed(Option<ProbeError>),
}

/// The ports to try, the saved one first.
fn ports_for(saved: u16) -> Vec<u16> {
    if saved == 0 || saved == USUAL_PORT {
        vec![USUAL_PORT]
    } else {
        vec![saved, USUAL_PORT]
    }
}

async fn probe_host(
    net: &dyn Network,
    host: Ipv4Addr,
    ports: &[u16],
    cancel: &CancelToken,
) -> HostOutcome {
    let mut worst: Option<ProbeError> = None;
    for port in ports {
        if cancel.is_cancelled() {
            break;
        }
        match net.probe(host, *port, PROBE_TIMEOUT).await {
            Ok(Some(answer)) => {
                return HostOutcome::Found(Candidate {
                    host: host.to_string(),
                    port: *port,
                    name: answer.name.unwrap_or_else(|| host.to_string()),
                    version: answer.version,
                })
            }
            Ok(None) => {}
            // Something is there but not on this port: the next port gets its turn.
            Err(ProbeError::Refused) => {}
            // Nobody there, or the system will not let us: the second port would only wait again.
            Err(error) => {
                worst = Some(error);
                break;
            }
        }
    }
    HostOutcome::Missed(worst)
}

/// What the scan looks at, in order: this computer, the saved host's own address, the attached private networks, then the
/// saved host's /24 when it is not one of them. Only addresses a scan may ask, each once.
pub fn plan_hosts(networks: &[Ipv4Net], saved_host: &str) -> (Vec<Ipv4Addr>, Vec<String>) {
    let saved = saved_host
        .trim()
        .parse::<Ipv4Addr>()
        .ok()
        .filter(|a| is_private(*a));
    // A network that is not private is never looked at, whoever lists it.
    let mut nets: Vec<Ipv4Net> = networks
        .iter()
        .copied()
        .filter(|n| is_private(n.network()))
        .collect();
    if let Some(addr) = saved {
        if let Some(net) = Ipv4Net::new(addr, 24) {
            if !nets.contains(&net) {
                nets.push(net);
            }
        }
    }
    let mut hosts: Vec<Ipv4Addr> = Vec::new();
    let push = |hosts: &mut Vec<Ipv4Addr>, addr: Ipv4Addr| {
        if may_probe(addr) && !hosts.contains(&addr) {
            hosts.push(addr);
        }
    };
    if let Some(addr) = saved {
        push(&mut hosts, addr);
    }
    for net in &nets {
        for addr in net.hosts() {
            push(&mut hosts, addr);
        }
    }
    (hosts, nets.iter().map(|n| n.to_string()).collect())
}

struct Shared {
    snapshot: ScanSnapshot,
    started: Option<Instant>,
    cancel: CancelToken,
    task: Option<JoinHandle<()>>,
    /// Each scan has a number, so one that was cancelled never writes over the next.
    run: u64,
}

/// The one scan at a time, with its state for the page to read.
pub struct ScanService {
    net: Arc<dyn Network>,
    shared: Arc<Mutex<Shared>>,
}

fn lock(shared: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

impl ScanService {
    pub fn new(net: Arc<dyn Network>) -> Self {
        ScanService {
            net,
            shared: Arc::new(Mutex::new(Shared {
                snapshot: ScanSnapshot::idle(),
                started: None,
                cancel: CancelToken::default(),
                task: None,
                run: 0,
            })),
        }
    }

    pub fn system() -> Self {
        Self::new(Arc::new(SystemNetwork::default()))
    }

    pub fn snapshot(&self) -> ScanSnapshot {
        let shared = lock(&self.shared);
        let mut snapshot = shared.snapshot.clone();
        if snapshot.state == ScanState::Scanning {
            if let Some(started) = shared.started {
                snapshot.elapsed = started.elapsed().as_secs_f64();
            }
        }
        snapshot.elapsed = (snapshot.elapsed * 10.0).round() / 10.0;
        snapshot
    }

    /// Start looking. A scan that is already running keeps going (a second press is not a second scan).
    pub fn start(&self, saved_host: &str, saved_port: u16) {
        let (run, cancel) = {
            let mut shared = lock(&self.shared);
            if shared.snapshot.state == ScanState::Scanning {
                return;
            }
            shared.run += 1;
            shared.cancel = CancelToken::default();
            shared.started = Some(Instant::now());
            shared.snapshot = ScanSnapshot {
                state: ScanState::Scanning,
                phase: Some("This computer".into()),
                ..ScanSnapshot::idle()
            };
            (shared.run, shared.cancel.clone())
        };
        let net = self.net.clone();
        let shared = self.shared.clone();
        let host = saved_host.to_string();
        let task = tokio::spawn(async move {
            run_scan(net, shared, run, cancel, host, saved_port).await;
        });
        lock(&self.shared).task = Some(task);
    }

    /// Stop. Harmless when nothing runs.
    pub async fn cancel(&self) {
        let task = {
            let mut shared = lock(&self.shared);
            shared.cancel.cancel();
            shared.run += 1;
            if shared.snapshot.state == ScanState::Scanning {
                shared.snapshot = ScanSnapshot {
                    reason: Some("Scan cancelled.".into()),
                    ..ScanSnapshot::idle()
                };
            }
            shared.task.take()
        };
        if let Some(task) = task {
            let _ = task.await;
        }
    }
}

fn update(shared: &Mutex<Shared>, run: u64, change: impl FnOnce(&mut ScanSnapshot)) {
    let mut shared = lock(shared);
    if shared.run == run {
        change(&mut shared.snapshot);
    }
}

async fn run_scan(
    net: Arc<dyn Network>,
    shared: Arc<Mutex<Shared>>,
    run: u64,
    cancel: CancelToken,
    saved_host: String,
    saved_port: u16,
) {
    let started = Instant::now();
    let ports = ports_for(saved_port);
    let loopback_ok = net.loopback_works().await;

    // This computer first: when ProPresenter runs here, that is the answer.
    if let HostOutcome::Found(candidate) =
        probe_host(net.as_ref(), Ipv4Addr::LOCALHOST, &ports, &cancel).await
    {
        let details = details_text(&json!({
            "outcome": "found", "networks": [], "hosts_total": 1, "hosts_probed": 1, "found": 1,
            "duration_s": started.elapsed().as_secs_f64(),
        }));
        update(&shared, run, |s| {
            *s = ScanSnapshot {
                state: ScanState::Found,
                candidates: vec![candidate],
                current: 1,
                total: 1,
                hosts_probed: 1,
                hosts_total: 1,
                elapsed: started.elapsed().as_secs_f64(),
                details: Some(details),
                ..ScanSnapshot::idle()
            };
        });
        return;
    }
    if cancel.is_cancelled() {
        return;
    }

    let attached = net.networks().await;
    let (hosts, networks) = plan_hosts(&attached, &saved_host);
    let total = hosts.len() + 1;
    update(&shared, run, |s| {
        s.phase = Some("Looking for ProPresenter…".into());
        s.networks = networks.clone();
        s.total = total as u32;
        s.hosts_total = total as u32;
        s.current = 1;
        s.hosts_probed = 1;
    });

    let mut found: Vec<Candidate> = Vec::new();
    let mut errors: BTreeMap<ProbeError, usize> = BTreeMap::new();
    let mut probed = 1usize;
    let workers = Arc::new(tokio::sync::Semaphore::new(MAX_WORKERS));
    for chunk in hosts.chunks(SCAN_CHUNK) {
        if cancel.is_cancelled() {
            return;
        }
        let mut set = JoinSet::new();
        for host in chunk.iter().copied() {
            let (net, ports, cancel, workers) =
                (net.clone(), ports.clone(), cancel.clone(), workers.clone());
            set.spawn(async move {
                let _slot = workers.acquire_owned().await.ok();
                (host, probe_host(net.as_ref(), host, &ports, &cancel).await)
            });
        }
        let mut chunk_found: Vec<(Ipv4Addr, Candidate)> = Vec::new();
        while let Some(done) = set.join_next().await {
            let Ok((host, outcome)) = done else { continue };
            match outcome {
                HostOutcome::Found(candidate) => chunk_found.push((host, candidate)),
                HostOutcome::Missed(Some(error)) => *errors.entry(error).or_insert(0) += 1,
                HostOutcome::Missed(None) => {}
            }
        }
        // The addresses in the order they were looked up, not in the order they answered.
        chunk_found.sort_by_key(|(host, _)| u32::from(*host));
        found.extend(chunk_found.into_iter().map(|(_, c)| c));
        probed += chunk.len();
        let candidates = found.clone();
        update(&shared, run, |s| {
            s.current = probed as u32;
            s.hosts_probed = probed as u32;
            s.candidates = candidates;
        });
    }
    if cancel.is_cancelled() {
        return;
    }

    let timed_out = errors.get(&ProbeError::TimedOut).copied().unwrap_or(0);
    let denied = errors.get(&ProbeError::Denied).copied().unwrap_or(0)
        + errors.get(&ProbeError::NoRoute).copied().unwrap_or(0);
    let failed: usize = errors.values().sum();
    let blocked = found.is_empty()
        && loopback_ok
        && failed > 0
        && timed_out == 0
        && denied as f64 / failed as f64 >= BLOCKED_MIN_SHARE;
    let no_network = found.is_empty() && networks.is_empty();
    let details = details_text(&json!({
        "outcome": if !found.is_empty() { "found" } else if blocked { "blocked" } else { "not_found" },
        "networks": networks,
        "hosts_total": total,
        "hosts_probed": probed,
        "found": found.len(),
        "loopback_ok": loopback_ok,
        "errors": errors.iter().map(|(k, v)| (format!("{k:?}").to_lowercase(), *v)).collect::<BTreeMap<_, _>>(),
        "duration_s": (started.elapsed().as_secs_f64() * 100.0).round() / 100.0,
    }));
    update(&shared, run, |s| {
        s.elapsed = started.elapsed().as_secs_f64();
        s.phase = None;
        s.details = Some(details);
        if !found.is_empty() {
            s.state = ScanState::Found;
            s.candidates = found.clone();
        } else if blocked {
            s.state = ScanState::NotFound;
            s.error_class = Some(ScanErrorClass::PermissionDenied);
            s.reason = Some(BLOCKED_REASON.into());
            s.settings_url = Some(LOCAL_NETWORK_SETTINGS_URL.into());
        } else if no_network {
            s.state = ScanState::NotFound;
            s.error_class = Some(ScanErrorClass::NoRoute);
            s.reason = Some(NO_NETWORK_REASON.into());
        } else {
            s.state = ScanState::NotFound;
            s.reason = Some(NOT_FOUND_REASON.into());
        }
    });
}

fn details_text(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interfaces_are_read_from_ip_and_ifconfig_text() {
        let text = "2: eth0    inet 192.168.4.21/22 brd 192.168.7.255 scope global eth0\n\
                    \tinet 10.1.2.3 netmask 0xffffff00 broadcast 10.1.2.255\n\
                    \tinet 172.16.5.9 netmask 255.255.0.0 broadcast 172.16.255.255\n";
        assert_eq!(
            parse_interfaces(text),
            vec![
                (Ipv4Addr::new(192, 168, 4, 21), 22),
                (Ipv4Addr::new(10, 1, 2, 3), 24),
                (Ipv4Addr::new(172, 16, 5, 9), 16),
            ]
        );
    }

    #[test]
    fn only_private_networks_are_used_and_none_is_wider_than_a_24() {
        let nets = usable_networks(&[
            (Ipv4Addr::new(192, 168, 4, 21), 22),
            (Ipv4Addr::new(8, 8, 4, 4), 24),
            (Ipv4Addr::new(127, 0, 0, 1), 8),
            (Ipv4Addr::new(169, 254, 1, 1), 16),
            (Ipv4Addr::new(10, 0, 0, 5), 8),
            (Ipv4Addr::new(172, 32, 0, 1), 24),
        ]);
        let text: Vec<String> = nets.iter().map(ToString::to_string).collect();
        assert_eq!(text, ["192.168.4.0/24", "10.0.0.0/24"]);
    }

    #[test]
    fn a_public_address_is_never_planned_even_when_saved() {
        let nets = [Ipv4Net::new(Ipv4Addr::new(192, 168, 1, 7), 24).unwrap()];
        let (hosts, _) = plan_hosts(&nets, "8.8.8.8");
        assert!(hosts.iter().all(|h| is_private(*h)));
        assert_eq!(hosts.len(), 254);
        let (hosts, names) = plan_hosts(&nets, "10.9.9.20");
        assert_eq!(hosts[0], Ipv4Addr::new(10, 9, 9, 20));
        assert_eq!(names, ["192.168.1.0/24", "10.9.9.0/24"]);
        assert_eq!(hosts.len(), 254 + 254);
        let (named, _) = plan_hosts(&nets, "pro.local");
        assert_eq!(named.len(), 254);
    }

    #[test]
    fn the_saved_port_is_tried_first_then_the_usual_one() {
        assert_eq!(ports_for(11025), [11025, 1025]);
        assert_eq!(ports_for(1025), [1025]);
    }

    #[test]
    fn an_answer_counts_only_with_a_host_description() {
        let ok = read_answer(br#"{"name":"Front MacBook","host_description":"ProPresenter 19.0.1","api_version":"v1"}"#).unwrap();
        assert_eq!(ok.name.as_deref(), Some("Front MacBook"));
        assert_eq!(ok.version, "ProPresenter 19.0.1");
        assert!(read_answer(br#"{"name":"x"}"#).is_none());
        assert!(read_answer(b"<html>").is_none());
        assert!(read_answer(b"[1]").is_none());
    }

    #[test]
    fn io_errors_are_classified_without_the_platform_names() {
        use std::io::{Error, ErrorKind};
        assert_eq!(
            classify_io(&Error::from(ErrorKind::PermissionDenied)),
            ProbeError::Denied
        );
        assert_eq!(
            classify_io(&Error::from(ErrorKind::ConnectionRefused)),
            ProbeError::Refused
        );
        assert_eq!(
            classify_io(&Error::from(ErrorKind::HostUnreachable)),
            ProbeError::NoRoute
        );
        assert_eq!(
            classify_io(&Error::from_raw_os_error(1)),
            ProbeError::Denied
        );
        assert_eq!(
            classify_io(&Error::from(ErrorKind::BrokenPipe)),
            ProbeError::Other
        );
    }
}
