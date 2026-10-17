//! LAN discovery: mirrors `playback_api/find.py`.
//!
//! Strategy (identical to Python): a typed host is ONE direct probe and never scans; otherwise
//! this computer (127.0.0.1) first, then every host of every attached private network (never
//! larger than a /24, never smaller than the interface's own subnet), 32 probes at a time, in
//! chunks of 64 hosts, with a 3 s per-probe timeout. Probes only listen: no commands are sent.
//!
//! Everything that touches the outside world sits behind [`Network`] (probe, interface list,
//! gateway, self-tests, name lookup). Time is `tokio::time`, so tests run on a paused virtual
//! clock (`#[tokio::test(start_paused = true)]`) and never sleep for real.

use crate::normalizer::{parse_heartbeat, parse_message, Heartbeat};
use futures_util::{stream, StreamExt};
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::future::Future;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

/// Probes in flight at once (`ThreadPoolExecutor(max_workers=32)`).
pub const MAX_WORKERS: usize = 32;
/// Hosts per progress step in the scan (`chunk = 64`).
pub const SCAN_CHUNK: usize = 64;
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);
pub const MAX_LOGGED_ERRORS: usize = 5;
pub const SCAN_BLOCK_DETAIL: &str = "macOS is blocking StagePilot's local-network access";
/// A blocked path fails in milliseconds; a genuinely unreachable host takes seconds to time out.
pub const BLOCKED_MIN_SHARE: f64 = 0.95;
pub const FAST_FAIL: Duration = Duration::from_secs(1);

// ---------------------------------------------------------------------------------------------
// Cancellation
// ---------------------------------------------------------------------------------------------

/// Shared cancel flag (Python `threading.Event` / `cancelled()` callable).
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

// ---------------------------------------------------------------------------------------------
// Error classification
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClass {
    PermissionDenied,
    NoRoute,
    Refused,
    TimedOut,
    NotPlayback,
    InvalidAddress,
    UnexpectedError,
}

impl ErrorClass {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorClass::PermissionDenied => "permission_denied",
            ErrorClass::NoRoute => "no_route",
            ErrorClass::Refused => "refused",
            ErrorClass::TimedOut => "timed_out",
            ErrorClass::NotPlayback => "not_playback",
            ErrorClass::InvalidAddress => "invalid_address",
            ErrorClass::UnexpectedError => "unexpected_error",
        }
    }
}

const REFUSED_DETAIL: &str = "The computer answered but nothing is listening on the Playback port";
const TIMED_OUT_DETAIL: &str = "No answer before the time limit";

/// What a probe can fail with: the Rust spelling of the exceptions `classify_exception` maps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeError {
    /// `ProbeFailure`: already classified.
    Failure {
        class: ErrorClass,
        detail: String,
        step: Option<String>,
    },
    /// `WSHandshakeError`
    Handshake(String),
    /// `WSClosed`
    Closed(String),
    /// `socket.timeout` / `TimeoutError`
    TimedOut,
    /// `ConnectionRefusedError`
    Refused,
    /// `PermissionError`
    PermissionDenied(String),
    /// `socket.gaierror`
    Unresolved,
    /// Any other `OSError`, with its errno when known.
    Os { errno: Option<i32>, message: String },
    /// `ValueError`
    InvalidInput,
    /// Anything else (`type name`, `message`).
    Other { kind: String, message: String },
}

impl ProbeError {
    pub fn os(errno: i32) -> Self {
        ProbeError::Os {
            errno: Some(errno),
            message: format!("[Errno {errno}]"),
        }
    }
}

impl From<std::io::Error> for ProbeError {
    fn from(e: std::io::Error) -> Self {
        use std::io::ErrorKind::*;
        match e.kind() {
            TimedOut => ProbeError::TimedOut,
            ConnectionRefused => ProbeError::Refused,
            PermissionDenied => ProbeError::PermissionDenied(e.to_string()),
            _ => ProbeError::Os {
                errno: e.raw_os_error(),
                message: e.to_string(),
            },
        }
    }
}

/// errno values per platform (Python's `errno` module differs between Linux, macOS and Windows).
mod errnos {
    #[cfg(target_os = "linux")]
    pub const BLOCKED: &[i32] = &[1, 13];
    #[cfg(target_os = "linux")]
    pub const NO_ROUTE: &[i32] = &[113, 101, 112, 100];
    #[cfg(target_os = "linux")]
    pub const REFUSED: i32 = 111;
    #[cfg(target_os = "linux")]
    pub const TIMED_OUT: i32 = 110;

    #[cfg(target_os = "macos")]
    pub const BLOCKED: &[i32] = &[1, 13];
    #[cfg(target_os = "macos")]
    pub const NO_ROUTE: &[i32] = &[65, 51, 64, 50];
    #[cfg(target_os = "macos")]
    pub const REFUSED: i32 = 61;
    #[cfg(target_os = "macos")]
    pub const TIMED_OUT: i32 = 60;

    // Windows: Winsock error numbers (what `OSError.errno` carries for socket failures there).
    #[cfg(windows)]
    pub const BLOCKED: &[i32] = &[10013];
    #[cfg(windows)]
    pub const NO_ROUTE: &[i32] = &[10065, 10051, 10064, 10050];
    #[cfg(windows)]
    pub const REFUSED: i32 = 10061;
    #[cfg(windows)]
    pub const TIMED_OUT: i32 = 10060;

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    pub const BLOCKED: &[i32] = &[1, 13];
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    pub const NO_ROUTE: &[i32] = &[];
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    pub const REFUSED: i32 = 111;
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    pub const TIMED_OUT: i32 = 110;
}

/// Map a probe error to (class, plain detail, failed step). Never panics. Port of
/// `classify_exception`, including the errno table (EPERM/EACCES blocked; EHOSTUNREACH,
/// ENETUNREACH, EHOSTDOWN, ENETDOWN no route; ECONNREFUSED; ETIMEDOUT).
pub fn classify_error(err: &ProbeError) -> (ErrorClass, String, Option<String>) {
    let step = |s: &str| Some(s.to_string());
    match err {
        ProbeError::Failure {
            class,
            detail,
            step,
        } => (*class, detail.clone(), step.clone()),
        ProbeError::Handshake(m) => (
            ErrorClass::NotPlayback,
            format!("Connected, but it did not answer as Playback ({m})"),
            step("handshake"),
        ),
        ProbeError::Closed(m) => (
            ErrorClass::NotPlayback,
            format!("Connected, but the connection closed early ({m})"),
            step("stream"),
        ),
        ProbeError::TimedOut => (
            ErrorClass::TimedOut,
            TIMED_OUT_DETAIL.into(),
            step("connect"),
        ),
        ProbeError::Refused => (ErrorClass::Refused, REFUSED_DETAIL.into(), step("connect")),
        ProbeError::PermissionDenied(m) => (
            ErrorClass::PermissionDenied,
            format!("The operating system blocked the connection ({m})"),
            step("connect"),
        ),
        ProbeError::Unresolved => (
            ErrorClass::InvalidAddress,
            "That name or address could not be resolved".into(),
            step("resolve"),
        ),
        ProbeError::Os { errno, message } => match errno {
            Some(c) if errnos::BLOCKED.contains(c) => (
                ErrorClass::PermissionDenied,
                format!("The operating system blocked the connection ({message})"),
                step("connect"),
            ),
            Some(c) if errnos::NO_ROUTE.contains(c) => (
                ErrorClass::NoRoute,
                "No route to that address".into(),
                step("connect"),
            ),
            Some(c) if *c == errnos::REFUSED => {
                (ErrorClass::Refused, REFUSED_DETAIL.into(), step("connect"))
            }
            Some(c) if *c == errnos::TIMED_OUT => (
                ErrorClass::TimedOut,
                TIMED_OUT_DETAIL.into(),
                step("connect"),
            ),
            _ => (
                ErrorClass::UnexpectedError,
                format!("OSError: {message}"),
                step("connect"),
            ),
        },
        ProbeError::InvalidInput => (
            ErrorClass::InvalidAddress,
            "That is not a valid address".into(),
            step("validate"),
        ),
        ProbeError::Other { kind, message } => (
            ErrorClass::UnexpectedError,
            format!("{kind}: {message}"),
            None,
        ),
    }
}

// ---------------------------------------------------------------------------------------------
// Networks
// ---------------------------------------------------------------------------------------------

/// `ipaddress.IPv4Network(strict=False)` for the few operations discovery needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ipv4Net {
    base: u32,
    prefix: u8,
}

impl Ipv4Net {
    pub fn new(addr: Ipv4Addr, prefix: u8) -> Option<Self> {
        if prefix > 32 {
            return None;
        }
        let mask = Self::mask(prefix);
        Some(Ipv4Net {
            base: u32::from(addr) & mask,
            prefix,
        })
    }
    fn mask(prefix: u8) -> u32 {
        if prefix == 0 {
            0
        } else {
            u32::MAX << (32 - prefix as u32)
        }
    }
    pub fn prefix(&self) -> u8 {
        self.prefix
    }
    pub fn contains(&self, addr: Ipv4Addr) -> bool {
        u32::from(addr) & Self::mask(self.prefix) == self.base
    }
    /// Usable host addresses (`network.hosts()`): excludes network and broadcast below /31.
    pub fn hosts(&self) -> Vec<Ipv4Addr> {
        let size: u64 = 1u64 << (32 - self.prefix as u32);
        let first = self.base as u64;
        let last = first + size - 1;
        let (lo, hi) = match self.prefix {
            32 => (first, first),
            31 => (first, last),
            _ => (first + 1, last - 1),
        };
        (lo..=hi).map(|v| Ipv4Addr::from(v as u32)).collect()
    }
}

impl std::fmt::Display for Ipv4Net {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", Ipv4Addr::from(self.base), self.prefix)
    }
}

fn mask_to_prefix(mask: &str) -> Option<u8> {
    let value: u32 = if let Some(hex) = mask.strip_prefix("0x").or_else(|| mask.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).ok()?
    } else if !mask.is_empty() && mask.bytes().all(|b| b.is_ascii_digit()) {
        // A bare number is a prefix length, as in `IPv4Network("0.0.0.0/24")`.
        let n: u8 = mask.parse().ok()?;
        return (n <= 32).then_some(n);
    } else {
        u32::from(mask.parse::<Ipv4Addr>().ok()?)
    };
    // Netmask first, then hostmask (what ipaddress accepts).
    if value.leading_ones() + value.trailing_zeros() == 32 || value == 0 {
        return Some(value.leading_ones() as u8);
    }
    let inv = !value;
    if inv.leading_ones() + inv.trailing_zeros() == 32 || inv == 0 {
        return Some(inv.leading_ones() as u8);
    }
    None
}

/// `(address, prefix)` pairs from `ip -o -4 addr` or `ifconfig` text.
pub fn parse_interfaces(text: &str) -> Vec<(String, u32)> {
    let slash = Regex::new(r"inet\s+([0-9]+\.[0-9]+\.[0-9]+\.[0-9]+)/([0-9]+)").unwrap();
    let mask =
        Regex::new(r"inet\s+([0-9]+\.[0-9]+\.[0-9]+\.[0-9]+)\s+(?:-->\s+\S+\s+)?netmask\s+(\S+)")
            .unwrap();
    let mut out = Vec::new();
    for c in slash.captures_iter(text) {
        out.push((c[1].to_string(), c[2].parse().unwrap_or(u32::MAX)));
    }
    for c in mask.captures_iter(text) {
        if let Some(p) = mask_to_prefix(&c[2]) {
            out.push((c[1].to_string(), p as u32));
        }
    }
    out
}

/// Private networks worth scanning: skip loopback, link-local, multicast, unspecified and
/// prefixes outside 1..=30; never wider than the attached /24.
pub fn usable_networks(interfaces: &[(String, u32)]) -> Vec<Ipv4Net> {
    let mut out: Vec<Ipv4Net> = Vec::new();
    for (host, prefix) in interfaces {
        let Ok(addr) = host.parse::<Ipv4Addr>() else {
            continue;
        };
        let o = addr.octets();
        let link_local = o[0] == 169 && o[1] == 254;
        if addr.is_loopback()
            || link_local
            || addr.is_multicast()
            || addr.is_unspecified()
            || !(*prefix > 0 && *prefix < 31)
        {
            continue;
        }
        if let Some(net) = Ipv4Net::new(addr, (*prefix).max(24) as u8) {
            if !out.contains(&net) {
                out.push(net);
            }
        }
    }
    out
}

/// Best-effort ip -> interface name from `ifconfig` / `ip -o addr` output.
pub fn interface_names(text: &str) -> Vec<(String, String)> {
    let head = Regex::new(r"^(\S+?):\s").unwrap();
    let ip_o = Regex::new(r"^[0-9]+:\s+(\S+)\s+inet\s+([0-9]+\.[0-9]+\.[0-9]+\.[0-9]+)").unwrap();
    let inet = Regex::new(r"^\s+inet\s+([0-9]+\.[0-9]+\.[0-9]+\.[0-9]+)\b").unwrap();
    let mut names: Vec<(String, String)> = Vec::new();
    let mut put = |ip: &str, name: &str| {
        if let Some(e) = names.iter_mut().find(|(i, _)| i == ip) {
            e.1 = name.to_string();
        } else {
            names.push((ip.to_string(), name.to_string()));
        }
    };
    let mut current = String::new();
    for line in text.lines() {
        if !line.starts_with([' ', '\t']) {
            if let Some(h) = head.captures(line) {
                current = h[1].to_string();
            }
        }
        if let Some(m) = ip_o.captures(line) {
            put(&m[2], &m[1]);
        }
        if let Some(m) = inet.captures(line) {
            if !current.is_empty() {
                put(&m[1], &current);
            }
        }
    }
    names
}

pub fn valid_typed_host(host: &str) -> bool {
    !host.is_empty() && host.len() <= 253 && !host.chars().any(|c| "\r\n /?#\\".contains(c))
}

// ---------------------------------------------------------------------------------------------
// Network port
// ---------------------------------------------------------------------------------------------

/// Result of the "can this process reach the LAN at all" connect test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LanCheck {
    Ok,
    Blocked,
    Unknown,
    Skipped,
}

/// Everything discovery needs from the outside world; replace it in tests.
pub trait Network: Send + Sync {
    /// `Ok(None)` is "nothing answered as Playback" (a plain miss, counted as a timeout).
    fn probe(
        &self,
        host: &str,
        port: u16,
        timeout: Duration,
    ) -> impl Future<Output = Result<Option<Heartbeat>, ProbeError>> + Send;
    fn networks(&self) -> impl Future<Output = Vec<Ipv4Net>> + Send;
    fn resolve_name(&self, host: &str) -> impl Future<Output = String> + Send;
    fn gateway(&self) -> impl Future<Output = Option<String>> + Send;
    fn selftest_loopback(&self) -> impl Future<Output = bool> + Send;
    fn selftest_connect(
        &self,
        host: &str,
        port: u16,
        timeout: Duration,
    ) -> impl Future<Output = LanCheck> + Send;
    fn interface_names(&self) -> impl Future<Output = Vec<(String, String)>> + Send;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverySource {
    Manual,
    Loopback,
    Lan,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveryResult {
    pub host: String,
    pub port: u16,
    pub source: DiscoverySource,
    pub heartbeat: Heartbeat,
}

/// `Probe` with the Python `probe()` semantics: any failure is a miss.
async fn probe_or_miss<N: Network>(
    net: &N,
    cancel: &CancelToken,
    host: &str,
    port: u16,
    timeout: Duration,
) -> Option<Heartbeat> {
    if cancel.is_cancelled() {
        return None;
    }
    net.probe(host, port, timeout).await.ok().flatten()
}

/// First Playback found: manual host (never falls back), loopback, then the LAN.
/// `cancel` is polled before every probe, so a stop ends a scan within one probe timeout.
pub async fn find_playback<N: Network>(
    net: &N,
    host_override: Option<&str>,
    port: u16,
    auto_scan: bool,
    timeout: Duration,
    cancel: &CancelToken,
) -> Option<DiscoveryResult> {
    if let Some(host) = host_override {
        let hb = probe_or_miss(net, cancel, host, port, timeout).await?;
        return Some(DiscoveryResult {
            host: host.to_string(),
            port,
            source: DiscoverySource::Manual,
            heartbeat: hb,
        });
    }
    if let Some(hb) = probe_or_miss(net, cancel, "127.0.0.1", port, timeout).await {
        return Some(DiscoveryResult {
            host: "127.0.0.1".into(),
            port,
            source: DiscoverySource::Loopback,
            heartbeat: hb,
        });
    }
    if !auto_scan {
        return None;
    }
    for network in net.networks().await {
        let hosts: Vec<String> = network.hosts().iter().map(|h| h.to_string()).collect();
        let mut results = stream::iter(hosts.iter())
            .map(|h| async move { (h, probe_or_miss(net, cancel, h, port, timeout).await) })
            .buffered(MAX_WORKERS);
        while let Some((host, hb)) = results.next().await {
            if let Some(hb) = hb {
                return Some(DiscoveryResult {
                    host: host.clone(),
                    port,
                    source: DiscoverySource::Lan,
                    heartbeat: hb,
                });
            }
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScanCandidate {
    pub name: String,
    pub host: String,
    pub source: DiscoverySource,
}

/// Progress callback: (done, total).
pub type Progress = Arc<dyn Fn(usize, usize) + Send + Sync>;
/// Phase callback: (label, done, total).
pub type PhaseFn = Arc<dyn Fn(&str, usize, usize) + Send + Sync>;

/// The simple scan (`scan_candidates`): this computer first, then every Playback on the LAN.
pub async fn scan_candidates<N: Network>(
    net: &N,
    host_override: Option<&str>,
    port: u16,
    auto_scan: bool,
    timeout: Duration,
    cancel: &CancelToken,
    progress: Option<Progress>,
) -> Vec<ScanCandidate> {
    let miss = |host: String| async move { net.probe(&host, port, timeout).await.ok().flatten() };
    if let Some(host) = host_override {
        return if miss(host.to_string()).await.is_some() {
            vec![ScanCandidate {
                name: net.resolve_name(host).await,
                host: host.into(),
                source: DiscoverySource::Manual,
            }]
        } else {
            vec![]
        };
    }
    if miss("127.0.0.1".into()).await.is_some() {
        return vec![ScanCandidate {
            name: net.resolve_name("127.0.0.1").await,
            host: "127.0.0.1".into(),
            source: DiscoverySource::Loopback,
        }];
    }
    if !auto_scan || cancel.is_cancelled() {
        return vec![];
    }
    let mut hosts = Vec::new();
    for n in net.networks().await {
        hosts.extend(n.hosts().iter().map(|h| h.to_string()));
    }
    let mut found = Vec::new();
    for (i, batch) in hosts.chunks(SCAN_CHUNK).enumerate() {
        if cancel.is_cancelled() {
            break;
        }
        let results: Vec<_> = stream::iter(batch.iter())
            .map(|h| miss(h.clone()))
            .buffered(MAX_WORKERS)
            .collect()
            .await;
        for (host, hit) in batch.iter().zip(results) {
            if hit.is_some() {
                found.push(ScanCandidate {
                    name: net.resolve_name(host).await,
                    host: host.clone(),
                    source: DiscoverySource::Lan,
                });
            }
        }
        if let Some(p) = &progress {
            p(((i + 1) * SCAN_CHUNK).min(hosts.len()), hosts.len());
        }
    }
    found
}

// ---------------------------------------------------------------------------------------------
// Diagnosed scan
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanOutcome {
    Found,
    NotFound,
    Error,
    Cancelled,
}

/// Everything a scan learned, safe to log, show and put in a diagnostics bundle.
#[derive(Debug, Clone)]
pub struct ScanReport {
    pub candidates: Vec<ScanCandidate>,
    pub outcome: ScanOutcome,
    pub error_class: Option<ErrorClass>,
    pub message: Option<String>,
    pub typed_host: Option<String>,
    pub networks: Vec<String>,
    pub interfaces: Vec<String>,
    pub hosts_total: usize,
    pub hosts_probed: usize,
    pub error_counts: BTreeMap<ErrorClass, usize>,
    pub sample_errors: Vec<String>,
    pub loopback_ok: Option<bool>,
    pub lan_check: LanCheck,
    pub duration: Duration,
    pub phases: Vec<Value>,
    pub classification: Option<ErrorClass>,
    pub classification_inputs: Value,
}

impl ScanReport {
    fn new(typed_host: Option<String>) -> Self {
        ScanReport {
            candidates: vec![],
            outcome: ScanOutcome::NotFound,
            error_class: None,
            message: None,
            typed_host,
            networks: vec![],
            interfaces: vec![],
            hosts_total: 0,
            hosts_probed: 0,
            error_counts: BTreeMap::new(),
            sample_errors: vec![],
            loopback_ok: None,
            lan_check: LanCheck::Skipped,
            duration: Duration::ZERO,
            phases: vec![],
            classification: None,
            classification_inputs: json!({}),
        }
    }

    /// Privacy-safe summary: addresses only for the typed host the operator entered.
    pub fn summary(&self) -> Value {
        let counts: BTreeMap<&str, usize> = self
            .error_counts
            .iter()
            .map(|(k, v)| (k.as_str(), *v))
            .collect();
        json!({
            "outcome": self.outcome,
            "error_class": self.error_class,
            "typed_host": self.typed_host,
            "networks": self.networks,
            "interfaces": self.interfaces,
            "hosts_total": self.hosts_total,
            "hosts_probed": self.hosts_probed,
            "found": self.candidates.len(),
            "errors": counts,
            "sample_errors": self.sample_errors,
            "loopback_ok": self.loopback_ok,
            "lan_check": self.lan_check,
            "duration_s": (self.duration.as_secs_f64() * 100.0).round() / 100.0,
            "phases": self.phases,
            "classification": self.classification,
            "classification_inputs": self.classification_inputs,
        })
    }

    fn tally(&mut self, class: ErrorClass, detail: &str) {
        *self.error_counts.entry(class).or_insert(0) += 1;
        if class != ErrorClass::TimedOut && self.sample_errors.len() < MAX_LOGGED_ERRORS {
            self.sample_errors
                .push(format!("{}: {detail}", class.as_str()));
        }
    }
}

/// macOS Local Network block signature: loopback works and every other private address fails at
/// once with no_route/permission_denied (none timed out). This computer's own addresses must
/// already be excluded from `non_self_counts`.
pub fn classify_scan_block(
    non_self_counts: &BTreeMap<ErrorClass, usize>,
    loopback_ok: Option<bool>,
) -> (bool, Value) {
    let total: usize = non_self_counts.values().sum();
    let get = |c| non_self_counts.get(&c).copied().unwrap_or(0);
    let denied = get(ErrorClass::PermissionDenied) + get(ErrorClass::NoRoute);
    let timed_out = get(ErrorClass::TimedOut);
    let share = if total > 0 {
        (denied as f64 / total as f64 * 1000.0).round() / 1000.0
    } else {
        0.0
    };
    let inputs = json!({
        "loopback_ok": loopback_ok,
        "others_total": total,
        "others_denied": denied,
        "others_timed_out": timed_out,
        "denied_share": share,
    });
    let blocked =
        loopback_ok == Some(true) && total > 0 && timed_out == 0 && share >= BLOCKED_MIN_SHARE;
    (blocked, inputs)
}

struct ProbeOne {
    heartbeat: Option<Heartbeat>,
    class: Option<ErrorClass>,
    detail: String,
    step: Option<String>,
}

async fn probe_one<N: Network>(net: &N, host: &str, port: u16, timeout: Duration) -> ProbeOne {
    match net.probe(host, port, timeout).await {
        Err(e) => {
            let (class, detail, step) = classify_error(&e);
            ProbeOne {
                heartbeat: None,
                class: Some(class),
                detail,
                step,
            }
        }
        Ok(None) => ProbeOne {
            heartbeat: None,
            class: Some(ErrorClass::TimedOut),
            detail: "No Playback answered".into(),
            step: None,
        },
        Ok(Some(hb)) => ProbeOne {
            heartbeat: Some(hb),
            class: None,
            detail: String::new(),
            step: None,
        },
    }
}

#[derive(Clone, Default)]
pub struct ScanHooks {
    pub cancel: CancelToken,
    pub progress: Option<Progress>,
    pub on_phase: Option<PhaseFn>,
}

impl ScanHooks {
    fn phase(&self, name: &str, cur: usize, total: usize) {
        if let Some(f) = &self.on_phase {
            f(name, cur, total);
        }
    }
}

/// Blocking-free diagnosed scan. A typed host is ONE direct probe and never scans the LAN.
pub async fn scan_network<N: Network>(
    net: &N,
    host_override: Option<&str>,
    port: u16,
    auto_scan: bool,
    timeout: Duration,
    hooks: &ScanHooks,
) -> ScanReport {
    let started = Instant::now();
    let mut report = ScanReport::new(host_override.map(str::to_string));
    let finish = |mut r: ScanReport| {
        r.duration = started.elapsed();
        r
    };

    if let Some(host) = host_override {
        hooks.phase("Connecting", 0, 1);
        if !valid_typed_host(host) {
            report.outcome = ScanOutcome::Error;
            report.error_class = Some(ErrorClass::InvalidAddress);
            report.message = Some("That is not a valid address.".into());
            return finish(report);
        }
        report.hosts_total = 1;
        let one = probe_one(net, host, port, timeout).await;
        report.hosts_probed = 1;
        report
            .phases
            .push(json!({"phase": "typed", "probed": 1, "found": one.heartbeat.is_some() as u8}));
        if one.heartbeat.is_some() {
            report.candidates.push(ScanCandidate {
                name: net.resolve_name(host).await,
                host: host.into(),
                source: DiscoverySource::Manual,
            });
            report.outcome = ScanOutcome::Found;
        } else {
            let mut class = one.class.expect("a miss always has a class");
            let mut detail = one.detail;
            let mut step = one.step;
            let elapsed = started.elapsed();
            if matches!(class, ErrorClass::PermissionDenied | ErrorClass::NoRoute)
                && net.selftest_loopback().await
            {
                report.loopback_ok = Some(true);
                let gate = net.gateway().await;
                let mut other_blocked = false;
                if let Some(g) = gate.as_deref().filter(|g| !g.is_empty() && *g != host) {
                    report.lan_check = net.selftest_connect(g, 80, Duration::from_secs(1)).await;
                    other_blocked = report.lan_check == LanCheck::Blocked;
                }
                let fast = elapsed <= FAST_FAIL;
                report.classification_inputs = json!({
                    "loopback_ok": true,
                    "typed_error": class,
                    "typed_elapsed_s": (elapsed.as_secs_f64() * 100.0).round() / 100.0,
                    "second_address_blocked": other_blocked,
                    "gateway_known": gate.as_deref().is_some_and(|g| !g.is_empty()),
                });
                if class == ErrorClass::PermissionDenied || (fast && other_blocked) {
                    class = ErrorClass::PermissionDenied;
                    detail = SCAN_BLOCK_DETAIL.into();
                    step = None;
                    report.classification = Some(ErrorClass::PermissionDenied);
                }
            }
            report.outcome = ScanOutcome::Error;
            report.error_class = Some(class);
            report.message = Some(match &step {
                Some(s) => format!("{detail} (step: {s})"),
                None => detail.clone(),
            });
            report.tally(class, &detail);
        }
        hooks.phase("Connecting", 1, 1);
        return finish(report);
    }

    // Phase 1: this computer. Self-test loopback first, then probe Playback on it.
    hooks.phase("This computer", 0, 1);
    report.loopback_ok = Some(net.selftest_loopback().await);
    let local = probe_one(net, "127.0.0.1", port, timeout).await;
    report.phases.push(
        json!({"phase": "this_computer", "probed": 1, "found": local.heartbeat.is_some() as u8}),
    );
    report.hosts_probed += 1;
    report.hosts_total += 1;
    if local.heartbeat.is_some() {
        report.candidates.push(ScanCandidate {
            name: net.resolve_name("127.0.0.1").await,
            host: "127.0.0.1".into(),
            source: DiscoverySource::Loopback,
        });
        report.outcome = ScanOutcome::Found;
        return finish(report);
    }
    if let Some(c) = local.class {
        if !matches!(c, ErrorClass::TimedOut | ErrorClass::Refused) {
            report.tally(c, &local.detail);
        }
    }
    if !auto_scan || hooks.cancel.is_cancelled() {
        report.outcome = if hooks.cancel.is_cancelled() {
            ScanOutcome::Cancelled
        } else {
            ScanOutcome::NotFound
        };
        return finish(report);
    }

    let nets = net.networks().await;
    report.networks = nets.iter().map(|n| n.to_string()).collect();
    let names = if nets.is_empty() {
        vec![]
    } else {
        net.interface_names().await
    };
    for (ip, name) in &names {
        let inside = ip
            .parse::<Ipv4Addr>()
            .map(|a| nets.iter().any(|n| n.contains(a)))
            .unwrap_or(false);
        if inside && !report.interfaces.contains(name) {
            report.interfaces.push(name.clone());
        }
    }

    // Self-test the LAN path before the sweep so "blocked" is distinguishable.
    if let Some(g) = net.gateway().await.filter(|g| !g.is_empty()) {
        report.lan_check = net.selftest_connect(&g, 80, Duration::from_secs(1)).await;
    }

    let mut hosts: Vec<String> = Vec::new();
    let mut owner: BTreeMap<String, String> = BTreeMap::new();
    for n in &nets {
        for h in n.hosts() {
            hosts.push(h.to_string());
            owner.insert(h.to_string(), n.to_string());
        }
    }
    report.hosts_total += hosts.len();
    let mut done = 0;
    let mut lan_found = 0;
    let mut per_network: Vec<(String, usize)> = nets.iter().map(|n| (n.to_string(), 0)).collect();
    let self_ips: Vec<&String> = names.iter().map(|(ip, _)| ip).collect();
    let mut non_self: BTreeMap<ErrorClass, usize> = BTreeMap::new();
    for (i, batch) in hosts.chunks(SCAN_CHUNK).enumerate() {
        if hooks.cancel.is_cancelled() {
            report.outcome = ScanOutcome::Cancelled;
            return finish(report);
        }
        let results: Vec<ProbeOne> = stream::iter(batch.iter())
            .map(|h| probe_one(net, h, port, timeout))
            .buffered(MAX_WORKERS)
            .collect()
            .await;
        for (host, r) in batch.iter().zip(results) {
            report.hosts_probed += 1;
            if let Some(entry) = per_network.iter_mut().find(|(n, _)| n == &owner[host]) {
                entry.1 += 1;
            }
            if r.heartbeat.is_some() {
                lan_found += 1;
                report.candidates.push(ScanCandidate {
                    name: net.resolve_name(host).await,
                    host: host.clone(),
                    source: DiscoverySource::Lan,
                });
            } else if let Some(c) = r.class {
                report.tally(c, &r.detail);
                if !self_ips.contains(&host) {
                    *non_self.entry(c).or_insert(0) += 1;
                }
            }
        }
        done = ((i + 1) * SCAN_CHUNK).min(hosts.len());
        if let Some(p) = &hooks.progress {
            p(done, hosts.len());
        }
        hooks.phase(
            &format!(
                "Network {}",
                if report.networks.len() == 1 {
                    report.networks[0].as_str()
                } else {
                    "scan"
                }
            ),
            done,
            hosts.len(),
        );
    }
    for (n, count) in per_network {
        report
            .phases
            .push(json!({"phase": format!("network {n}"), "probed": count, "found": 0}));
    }
    report
        .phases
        .push(json!({"phase": "lan_total", "probed": done, "found": lan_found}));

    if !report.candidates.is_empty() {
        report.outcome = ScanOutcome::Found;
        return finish(report);
    }
    let (block_signature, mut inputs) = classify_scan_block(&non_self, report.loopback_ok);
    inputs["self_addresses_excluded"] = json!(self_ips.len());
    inputs["lan_check"] = json!(report.lan_check);
    report.classification_inputs = inputs;
    let unexpected = report
        .error_counts
        .get(&ErrorClass::UnexpectedError)
        .copied()
        .unwrap_or(0);
    if report.loopback_ok == Some(true)
        && (report.lan_check == LanCheck::Blocked || block_signature)
    {
        report.outcome = ScanOutcome::Error;
        report.error_class = Some(ErrorClass::PermissionDenied);
        report.classification = Some(ErrorClass::PermissionDenied);
        report.message = Some(SCAN_BLOCK_DETAIL.into());
    } else if nets.is_empty() {
        report.outcome = ScanOutcome::Error;
        report.error_class = Some(ErrorClass::NoRoute);
        report.message = Some("No usable network was found on this computer.".into());
    } else if unexpected > 0 && unexpected >= hosts.len() / 2 {
        report.outcome = ScanOutcome::Error;
        report.error_class = Some(ErrorClass::UnexpectedError);
        report.message = Some(
            report
                .sample_errors
                .first()
                .cloned()
                .unwrap_or_else(|| "Unexpected error".into()),
        );
    } else {
        report.outcome = ScanOutcome::NotFound;
    }
    finish(report)
}

/// A saved address just failed immediately: is the OS blocking all local-network use?
pub async fn saved_address_blocked<N: Network>(net: &N) -> bool {
    if !net.selftest_loopback().await {
        return false;
    }
    match net.gateway().await.filter(|g| !g.is_empty()) {
        Some(g) => net.selftest_connect(&g, 80, Duration::from_secs(1)).await == LanCheck::Blocked,
        None => false,
    }
}

/// TCP connect only (no payload). Refused/connected both prove the OS let packets out.
pub fn lan_check_from(result: Result<(), ProbeError>) -> LanCheck {
    match result {
        Ok(()) | Err(ProbeError::Refused) => LanCheck::Ok,
        Err(e) => match classify_error(&e).0 {
            ErrorClass::PermissionDenied | ErrorClass::NoRoute => LanCheck::Blocked,
            _ => LanCheck::Unknown,
        },
    }
}

// ---------------------------------------------------------------------------------------------
// The real network
// ---------------------------------------------------------------------------------------------

/// The real thing: sockets, `ip`/`ifconfig`/`route`/PowerShell.
#[derive(Debug, Clone, Default)]
pub struct SystemNetwork;

async fn run_command(program: &str, args: &[&str]) -> Option<String> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null());
    let out = tokio::time::timeout(Duration::from_secs(5), cmd.output())
        .await
        .ok()?
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

async fn interface_text() -> Option<String> {
    for (prog, args) in [
        ("ip", &["-o", "-4", "addr", "show"][..]),
        ("/sbin/ifconfig", &[][..]),
        ("ifconfig", &[][..]),
    ] {
        if let Some(text) = run_command(prog, args).await {
            if !parse_interfaces(&text).is_empty() || !interface_names(&text).is_empty() {
                return Some(text);
            }
        }
    }
    None
}

impl Network for SystemNetwork {
    async fn probe(
        &self,
        host: &str,
        port: u16,
        timeout: Duration,
    ) -> Result<Option<Heartbeat>, ProbeError> {
        probe_checked(host, port, timeout).await.map(Some)
    }

    async fn networks(&self) -> Vec<Ipv4Net> {
        if let Some(text) = interface_text().await {
            let found = parse_interfaces(&text);
            if !found.is_empty() {
                return usable_networks(&found);
            }
        }
        // Windows has neither: ask PowerShell for address/prefix pairs.
        let script = "Get-NetIPAddress -AddressFamily IPv4 | ForEach-Object { \
                      'inet ' + $_.IPAddress + '/' + $_.PrefixLength }";
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

    async fn resolve_name(&self, host: &str) -> String {
        // Python uses reverse DNS (`gethostbyaddr`); std has no reverse lookup, so everything
        // but this computer is named by its address. Documented difference.
        if host == "127.0.0.1" {
            "This computer".into()
        } else {
            host.to_string()
        }
    }

    async fn gateway(&self) -> Option<String> {
        let table: [(&str, &[&str], &str); 2] = [
            (
                "/sbin/route",
                &["-n", "get", "default"],
                r"gateway:\s*([0-9]+\.[0-9]+\.[0-9]+\.[0-9]+)",
            ),
            (
                "ip",
                &["-4", "route", "show", "default"],
                r"default via ([0-9]+\.[0-9]+\.[0-9]+\.[0-9]+)",
            ),
        ];
        for (prog, args, pattern) in table {
            if let Some(text) = run_command(prog, args).await {
                if let Some(c) = Regex::new(pattern).unwrap().captures(&text) {
                    return Some(c[1].to_string());
                }
            }
        }
        None
    }

    async fn selftest_loopback(&self) -> bool {
        let Ok(listener) = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await else {
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

    async fn selftest_connect(&self, host: &str, port: u16, timeout: Duration) -> LanCheck {
        let result =
            match tokio::time::timeout(timeout, tokio::net::TcpStream::connect((host, port))).await
            {
                Err(_) => Err(ProbeError::TimedOut),
                Ok(Err(e)) => Err(ProbeError::from(e)),
                Ok(Ok(_)) => Ok(()),
            };
        lan_check_from(result)
    }

    async fn interface_names(&self) -> Vec<(String, String)> {
        interface_text()
            .await
            .map(|t| interface_names(&t))
            .unwrap_or_default()
    }
}

/// Real probe: negotiate `pr-protocol`, wait for a valid heartbeat, send nothing. Errors are
/// classified, not swallowed (`probe_checked`).
pub async fn probe_checked(
    host: &str,
    port: u16,
    timeout: Duration,
) -> Result<Heartbeat, ProbeError> {
    use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue, Error};
    if !valid_typed_host(host) {
        return Err(ProbeError::InvalidInput);
    }
    let deadline = Instant::now() + timeout;
    let authority = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    let mut req = format!("ws://{authority}:{port}/")
        .into_client_request()
        .map_err(|_| ProbeError::InvalidInput)?;
    req.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        HeaderValue::from_static(crate::client::SUBPROTOCOL),
    );
    let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(crate::client::MAX_MESSAGE))
        .max_frame_size(Some(crate::client::MAX_MESSAGE));
    let map = |e: Error| match e {
        Error::Io(io) => ProbeError::from(io),
        Error::Url(_) => ProbeError::InvalidInput,
        Error::Http(_) | Error::HttpFormat(_) | Error::Protocol(_) => {
            ProbeError::Handshake(e.to_string())
        }
        other => ProbeError::Closed(other.to_string()),
    };
    let (mut ws, resp) = tokio::time::timeout_at(
        deadline,
        tokio_tungstenite::connect_async_with_config(req, Some(config), true),
    )
    .await
    .map_err(|_| ProbeError::TimedOut)?
    .map_err(map)?;
    let negotiated = resp
        .headers()
        .get("sec-websocket-protocol")
        .and_then(|v| v.to_str().ok());
    if negotiated != Some(crate::client::SUBPROTOCOL) {
        return Err(ProbeError::Handshake(
            "invalid Playback protocol negotiation".into(),
        ));
    }
    use futures_util::StreamExt as _;
    use tokio_tungstenite::tungstenite::Message;
    loop {
        let next = match tokio::time::timeout_at(deadline, ws.next()).await {
            Err(_) => break,
            Ok(n) => n,
        };
        match next {
            Some(Ok(Message::Text(t))) => {
                if let Some((kind, body)) = parse_message(t.as_str()) {
                    if kind == "heartbeat" {
                        if let Some(hb) = parse_heartbeat(&body) {
                            return Ok(hb);
                        }
                    }
                }
            }
            Some(Ok(Message::Close(_))) | None => {
                return Err(ProbeError::Closed("Playback sent close".into()))
            }
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(map(e)),
        }
    }
    Err(ProbeError::Failure {
        class: ErrorClass::NotPlayback,
        detail: "Connected and handshook, but no valid heartbeat arrived".into(),
        step: Some("heartbeat".into()),
    })
}
