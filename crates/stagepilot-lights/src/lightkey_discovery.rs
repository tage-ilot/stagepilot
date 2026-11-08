//! Finding the Lighting computer by itself.
//!
//! While nothing is connected, StagePilot looks for the Lightkey helper (`lightkey-api`) on the
//! networks this computer can reach, asking each candidate one question, `GET /health`. This file
//! holds the whole feature except the glue to the plugin:
//!
//! * the pure parts: the candidate list (`build_targets`), the classification of one answer
//!   (`classify`), the settings a found computer changes (`apply_change`) and the decision table
//!   (`decide`, a pure function of the mode, the link, the last scan and the dismissed offers);
//! * the probe (`HttpProber`) and the scan (`scan`), both behind a trait so tests can count and
//!   delay them;
//! * `Discovery`, the small engine that decides WHEN to scan: never while the link is connected,
//!   never when Lighting is off or MIDI, about 5 s after start or after a setting change, then
//!   every 60 s, and at once when the operator presses Search again.
//!
//! What is probed: `127.0.0.1`, the private IPv4 networks this computer is attached to, and the
//! /24 around the saved Lighting address, the saved Replay address and the last found computer.
//! Only RFC 1918 addresses (10/8, 172.16/12, 192.168/16) are ever contacted, with `GET /health`
//! and nothing else. Public, link-local and other loopback addresses never are.

use crate::lightkey::effective_api_url;
use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use stagepilot_core::config::{clean_lightkey_api_url, LightsSettings, DEFAULT_LIGHTKEY_API_PORT};
use std::collections::BTreeSet;
use std::future::Future;
use std::net::Ipv4Addr;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{Notify, Semaphore};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::Instant;
use ts_rs::TS;

/// The most addresses one scan asks.
pub const MAX_TARGETS: usize = 1100;
/// The most probes in flight at once.
pub const MAX_IN_FLIGHT: usize = 64;
/// One probe: one attempt, this long in total (connect, answer and body).
pub const PROBE_TIMEOUT: Duration = Duration::from_millis(600);
/// A `/health` answer larger than this is not a helper.
pub const MAX_BODY_BYTES: usize = 64 * 1024;
/// About this long after StagePilot starts, or after a setting changes, before the first scan:
/// the saved address gets its chance first.
pub const SETTLE: Duration = Duration::from_secs(5);
/// While still not connected, a new scan this often.
pub const SCAN_EVERY: Duration = Duration::from_secs(60);
/// How long "Found a Lighting computer at ... and connected." stays on screen.
pub const MESSAGE_SECONDS: u64 = 300;
/// The engine looks at the link this often.
const TICK: Duration = Duration::from_millis(250);

/// What the helper says about a computer that refuses us (`do_GET` of `lightkey_api.py`).
const REFUSING_TEXT: &str = "not available on this interface";

// ------------------------------------------------------------------------------ API types

/// `LightkeyDiscovery.state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DiscoveryState {
    /// Not looking: Lighting is off, set to MIDI, or this is a build without the search.
    Off,
    /// A search is running or about to run.
    Searching,
    /// A Lighting computer answers; nothing is searched.
    Connected,
    /// Automatic address, several computers found: the operator picks one.
    Choose,
    /// A chosen address is down and another computer answered: the operator decides.
    Offer,
    /// The search ran and found no computer. An ordinary state.
    #[serde(rename = "none")]
    Nothing,
}

/// One Lighting computer a search found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FoundLighting {
    /// `10.30.0.211`, or `10.30.0.211:9000` when the port is not the usual one.
    pub address: String,
    /// The helper's version; absent on a helper older than 0.5.0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<String>")]
    pub version: Option<String>,
    /// Lightkey is running on it.
    pub lightkey_running: bool,
    /// The Lightkey project (base name) its helper has read, when it says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<String>")]
    pub project: Option<String>,
}

/// `LightkeyStatus.discovery`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightkeyDiscovery {
    pub state: DiscoveryState,
    /// `choose`: every computer found. `offer`: the ones not silenced, the first is offered.
    pub found: Vec<FoundLighting>,
    /// Computers that answered "not available on this interface".
    pub refusing: Vec<String>,
    /// The address the operator chose (`lightkey.api_url`); absent in Automatic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<String>")]
    pub chosen: Option<String>,
    /// How long nothing has been connected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<u32>")]
    pub chosen_down_for_s: Option<u32>,
    /// When the last search finished (RFC 3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<String>")]
    pub last_scan_at: Option<String>,
    /// `Found a Lighting computer at <ip> and connected.`, while it is news.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<String>")]
    pub message: Option<String>,
    /// `Capture is from <ip>; Replay sends to <host>.` when the two differ.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<String>")]
    pub note: Option<String>,
}

impl LightkeyDiscovery {
    /// Not searching (also what a build without the search reports).
    pub fn off() -> Self {
        Self {
            state: DiscoveryState::Off,
            found: Vec::new(),
            refusing: Vec::new(),
            chosen: None,
            chosen_down_for_s: None,
            last_scan_at: None,
            message: None,
            note: None,
        }
    }
}

// ------------------------------------------------------------------------------ addresses

/// How an address is written to the operator: the IP, plus the port only when it is not the usual.
pub fn display_address(ip: Ipv4Addr, port: u16) -> String {
    if port == DEFAULT_LIGHTKEY_API_PORT {
        ip.to_string()
    } else {
        format!("{ip}:{port}")
    }
}

/// The saved form: `http://10.30.0.211:8770`.
pub fn address_url(ip: Ipv4Addr, port: u16) -> String {
    format!("http://{ip}:{port}")
}

/// Host and optional port of a saved address (`10.30.0.211`, `10.30.0.211:8770`,
/// `http://10.30.0.211:8770`). The host is whatever precedes the port.
pub fn split_address(text: &str) -> Option<(String, Option<u16>)> {
    let text = text.trim();
    let rest = text.split_once("://").map_or(text, |(_, rest)| rest);
    let rest = rest.split('/').next().unwrap_or(rest);
    if rest.is_empty() {
        return None;
    }
    match rest.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => Some((host.to_string(), port.parse().ok())),
        _ => Some((rest.to_string(), None)),
    }
}

fn ipv4_of(text: &str) -> Option<Ipv4Addr> {
    split_address(text)?.0.parse().ok()
}

/// `display_address` of a saved address; `10.30.0.211:8770` reads `10.30.0.211`.
pub fn display_saved(text: &str) -> String {
    match split_address(text) {
        Some((host, Some(port))) if port == DEFAULT_LIGHTKEY_API_PORT => host,
        Some((host, Some(port))) => format!("{host}:{port}"),
        Some((host, None)) => host,
        None => text.trim().to_string(),
    }
}

// ------------------------------------------------------------------------------ candidates

/// One address of this computer on an attached network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Net {
    pub ip: Ipv4Addr,
    pub prefix: u8,
}

/// What `build_targets` needs to know.
#[derive(Debug, Clone, Default)]
pub struct TargetInputs {
    /// Non-loopback, up interfaces' IPv4 addresses with their real prefix.
    pub interfaces: Vec<Net>,
    /// Every IPv4 address of this computer: never probed.
    pub own: Vec<Ipv4Addr>,
    /// The host of the saved Lighting address, of the saved Replay address and of the last found
    /// computer (each only when it is an IPv4 address).
    pub saved: Option<Ipv4Addr>,
    pub replay: Option<Ipv4Addr>,
    pub found: Option<Ipv4Addr>,
}

/// The host addresses of the network `net.ip` sits on. A prefix shorter than /22 would be more
/// than 1,022 hosts, so the /24 around the address is used instead. /31 and /32 have no hosts.
pub fn attached_hosts(net: Net) -> Vec<Ipv4Addr> {
    let prefix = if net.prefix < 22 { 24 } else { net.prefix };
    if prefix >= 31 {
        return Vec::new();
    }
    let mask = u32::MAX << (32 - u32::from(prefix));
    let address = u32::from(net.ip);
    let network = address & mask;
    let broadcast = network | !mask;
    (network + 1..broadcast).map(Ipv4Addr::from).collect()
}

/// The 254 hosts of the /24 around `ip`.
fn around(ip: Ipv4Addr) -> Vec<Ipv4Addr> {
    attached_hosts(Net { ip, prefix: 24 })
}

/// The addresses to ask, in order, at most [`MAX_TARGETS`], no duplicates, never one of this
/// computer's own and never anything but `127.0.0.1` and RFC 1918 addresses:
///
/// 1. `127.0.0.1` (the helper on this very computer);
/// 2. the saved Lighting address, the saved Replay address and the last found computer, then the
///    /24 around each of them (this is what finds a Lighting Mac behind a router);
/// 3. every directly attached private network.
pub fn build_targets(inputs: &TargetInputs) -> Vec<Ipv4Addr> {
    let own: BTreeSet<Ipv4Addr> = inputs.own.iter().copied().collect();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    let loopback = Ipv4Addr::LOCALHOST;
    seen.insert(loopback);
    out.push(loopback);
    let mut push = |ip: Ipv4Addr| {
        if out.len() < MAX_TARGETS && ip.is_private() && !own.contains(&ip) && seen.insert(ip) {
            out.push(ip);
        }
    };
    let hints = [inputs.saved, inputs.replay, inputs.found];
    for hint in hints.into_iter().flatten() {
        push(hint);
    }
    for hint in hints.into_iter().flatten() {
        if hint.is_private() {
            around(hint).into_iter().for_each(&mut push);
        }
    }
    for net in &inputs.interfaces {
        if net.ip.is_private() {
            attached_hosts(*net).into_iter().for_each(&mut push);
        }
    }
    out
}

/// This computer's IPv4 addresses: the attached networks of the up, non-loopback interfaces, and
/// every address it owns. An error reading them leaves the lists empty (only the hints and
/// `127.0.0.1` are asked then).
pub fn local_networks() -> (Vec<Net>, Vec<Ipv4Addr>) {
    let mut nets = Vec::new();
    let mut own = Vec::new();
    for interface in if_addrs::get_if_addrs().unwrap_or_default() {
        let if_addrs::IfAddr::V4(v4) = &interface.addr else {
            continue;
        };
        own.push(v4.ip);
        if !interface.is_loopback() && interface.is_oper_up() {
            nets.push(Net {
                ip: v4.ip,
                prefix: v4.prefixlen,
            });
        }
    }
    (nets, own)
}

// ------------------------------------------------------------------------------ one probe

/// A computer that answered like the helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperAnswer {
    pub version: Option<String>,
    pub lightkey_running: bool,
    pub project: Option<String>,
}

/// What one probe made of the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probed {
    Helper(HelperAnswer),
    /// The helper is there but does not allow StagePilot's network.
    Refusing,
    /// No answer, or not a helper.
    Ignore,
}

/// Classify one `GET /health` answer. `body` is `None` when it was larger than
/// [`MAX_BODY_BYTES`] or could not be read.
pub fn classify(status: u16, body: Option<&[u8]>) -> Probed {
    let Some(body) = body.filter(|b| b.len() <= MAX_BODY_BYTES) else {
        return Probed::Ignore;
    };
    let Ok(Value::Object(map)) = serde_json::from_slice::<Value>(body) else {
        return Probed::Ignore;
    };
    match status {
        200 => {
            let running = map.get("lightkey").and_then(Value::as_bool);
            match (map.get("ok"), running) {
                (Some(Value::Bool(true)), Some(lightkey_running)) => Probed::Helper(HelperAnswer {
                    version: map
                        .get("version")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|v| !v.is_empty())
                        .map(str::to_string),
                    lightkey_running,
                    project: map
                        .get("fixture_map")
                        .and_then(|m| m.get("project"))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|p| !p.is_empty())
                        .map(str::to_string),
                }),
                _ => Probed::Ignore,
            }
        }
        403 if map.get("error").and_then(Value::as_str) == Some(REFUSING_TEXT) => Probed::Refusing,
        _ => Probed::Ignore,
    }
}

/// Asks one address. A trait so tests can count, delay and fake the answers.
#[async_trait]
pub trait Prober: Send + Sync {
    async fn probe(&self, ip: Ipv4Addr, port: u16) -> Probed;
}

/// The real probe: plain HTTP, one `GET /health`, [`PROBE_TIMEOUT`] in total, no redirects, no
/// proxy, the body capped at [`MAX_BODY_BYTES`]. Nothing else is ever sent.
pub struct HttpProber {
    http: reqwest::Client,
}

impl HttpProber {
    pub fn new() -> Self {
        // Cargo unifies reqwest features across the workspace: a TLS provider must be installed
        // before any Client is built (see the Lightkey link).
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .timeout(PROBE_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .pool_max_idle_per_host(0)
            .build()
            .unwrap_or_default();
        Self { http }
    }
}

impl Default for HttpProber {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Prober for HttpProber {
    async fn probe(&self, ip: Ipv4Addr, port: u16) -> Probed {
        let Ok(mut response) = self
            .http
            .get(format!("http://{ip}:{port}/health"))
            .send()
            .await
        else {
            return Probed::Ignore;
        };
        let status = response.status().as_u16();
        // Only these two statuses mean anything; a redirect, a 404 or a 500 is not the helper.
        if status != 200 && status != 403 {
            return Probed::Ignore;
        }
        if response
            .content_length()
            .is_some_and(|n| n > MAX_BODY_BYTES as u64)
        {
            return Probed::Ignore;
        }
        let mut body = Vec::new();
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    body.extend_from_slice(&chunk);
                    if body.len() > MAX_BODY_BYTES {
                        return Probed::Ignore;
                    }
                }
                Ok(None) => break,
                Err(_) => return Probed::Ignore,
            }
        }
        classify(status, Some(&body))
    }
}

// ------------------------------------------------------------------------------ one scan

/// A helper a scan found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub ip: Ipv4Addr,
    pub port: u16,
    pub answer: HelperAnswer,
}

impl Found {
    pub fn address(&self) -> String {
        display_address(self.ip, self.port)
    }

    pub fn url(&self) -> String {
        address_url(self.ip, self.port)
    }

    fn view(&self) -> FoundLighting {
        FoundLighting {
            address: self.address(),
            version: self.answer.version.clone(),
            lightkey_running: self.answer.lightkey_running,
            project: self.answer.project.clone(),
        }
    }
}

/// What one scan learned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanResult {
    /// Lightkey running first, then by address.
    pub found: Vec<Found>,
    pub refusing: Vec<Ipv4Addr>,
    pub port: u16,
    pub at: DateTime<Utc>,
}

/// Ask every target (at most [`MAX_IN_FLIGHT`] at a time). `keep_going` is asked before each
/// probe starts: once it says no, the rest are skipped.
pub async fn scan(
    prober: Arc<dyn Prober>,
    targets: Vec<Ipv4Addr>,
    port: u16,
    keep_going: Arc<dyn Fn() -> bool + Send + Sync>,
) -> ScanResult {
    let gate = Arc::new(Semaphore::new(MAX_IN_FLIGHT));
    let mut tasks = JoinSet::new();
    for ip in targets.into_iter().take(MAX_TARGETS) {
        let (prober, gate, keep_going) = (prober.clone(), gate.clone(), keep_going.clone());
        tasks.spawn(async move {
            let Ok(_permit) = gate.acquire_owned().await else {
                return (ip, Probed::Ignore);
            };
            if !keep_going() {
                return (ip, Probed::Ignore);
            }
            (ip, prober.probe(ip, port).await)
        });
    }
    let mut found = Vec::new();
    let mut refusing = Vec::new();
    while let Some(done) = tasks.join_next().await {
        match done {
            Ok((ip, Probed::Helper(answer))) => found.push(Found { ip, port, answer }),
            Ok((ip, Probed::Refusing)) => refusing.push(ip),
            _ => {}
        }
    }
    found.sort_by_key(|f| (!f.answer.lightkey_running, f.ip));
    refusing.sort();
    ScanResult {
        found,
        refusing,
        port,
        at: Utc::now(),
    }
}

// ------------------------------------------------------------------------------ the decision

/// Whether anything answers at the address in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// The helper answers (Lightkey running or not).
    Up,
    /// Nothing answers.
    Down,
    /// The link has not tried yet.
    Pending,
}

/// What the operator typed, or nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// `lightkey.api_url` is blank.
    Automatic,
    /// `lightkey.api_url` is set; the display form of it.
    Chosen(String),
}

/// What `decide` is asked about.
#[derive(Debug, Clone)]
pub struct DecideInput<'a> {
    /// Lighting is on and set to OSC (the only case that searches).
    pub active: bool,
    pub mode: Mode,
    pub reach: Reach,
    pub scanning: bool,
    pub scan: Option<&'a ScanResult>,
    /// Addresses whose offer the operator silenced (`Keep waiting`).
    pub dismissed: &'a BTreeSet<String>,
    /// The display form of the address in use now; never offered back.
    pub in_use: &'a str,
}

/// What follows from the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Nothing,
    /// Automatic with exactly one computer found: connect to it.
    Connect(Found),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub state: DiscoveryState,
    pub found: Vec<FoundLighting>,
    pub refusing: Vec<String>,
    pub step: Step,
}

/// The whole table. Pure: no clock, no network, no settings.
///
/// | mode      | reach            | found (other than the address in use) | result              |
/// |-----------|------------------|----------------------------------------|---------------------|
/// | any       | Lighting off/MIDI | -                                     | `off`               |
/// | any       | up               | -                                      | `connected`, no search |
/// | any       | down, no result yet or scanning | -                       | `searching`         |
/// | automatic | down             | none                                   | `none`              |
/// | automatic | down             | one                                    | `searching` + connect it |
/// | automatic | down             | several                                | `choose`            |
/// | chosen    | down             | none, or all silenced                  | `none`              |
/// | chosen    | down             | some not silenced                      | `offer` (never switches) |
pub fn decide(input: &DecideInput<'_>) -> Decision {
    let nothing = |state| Decision {
        state,
        found: Vec::new(),
        refusing: Vec::new(),
        step: Step::Nothing,
    };
    if !input.active {
        return nothing(DiscoveryState::Off);
    }
    match input.reach {
        Reach::Up => return nothing(DiscoveryState::Connected),
        Reach::Pending => return nothing(DiscoveryState::Searching),
        Reach::Down => {}
    }
    let Some(scan) = input.scan.filter(|_| !input.scanning) else {
        return nothing(DiscoveryState::Searching);
    };
    let refusing: Vec<String> = scan
        .refusing
        .iter()
        .map(|ip| display_address(*ip, scan.port))
        .filter(|address| address != input.in_use)
        .collect();
    let others: Vec<&Found> = scan
        .found
        .iter()
        .filter(|f| f.address() != input.in_use)
        .collect();
    let (state, shown, step) = match &input.mode {
        Mode::Automatic => match others.as_slice() {
            [] => (DiscoveryState::Nothing, Vec::new(), Step::Nothing),
            [one] => (
                DiscoveryState::Searching,
                Vec::new(),
                Step::Connect((*one).clone()),
            ),
            many => (
                DiscoveryState::Choose,
                many.iter().map(|f| f.view()).collect(),
                Step::Nothing,
            ),
        },
        Mode::Chosen(_) => {
            let offered: Vec<FoundLighting> = others
                .iter()
                .filter(|f| !input.dismissed.contains(&f.address()))
                .map(|f| f.view())
                .collect();
            let state = if offered.is_empty() {
                DiscoveryState::Nothing
            } else {
                DiscoveryState::Offer
            };
            (state, offered, Step::Nothing)
        }
    };
    Decision {
        state,
        found: shown,
        refusing,
        step,
    }
}

/// Whether a scan may start now. Search runs only when nothing is connected, and only with
/// Lighting on and set to OSC.
pub fn may_scan(active: bool, reach: Reach) -> bool {
    active && reach == Reach::Down
}

// ------------------------------------------------------------------------------ the change

/// Which saved address a chosen computer becomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeTarget {
    /// Automatic stays automatic: `found_url` is set.
    Found,
    /// The operator confirmed an offer: it becomes the chosen `api_url`.
    Chosen,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub ip: Ipv4Addr,
    pub port: u16,
    pub target: ChangeTarget,
}

impl Change {
    pub fn address(&self) -> String {
        display_address(self.ip, self.port)
    }

    pub fn url(&self) -> String {
        address_url(self.ip, self.port)
    }
}

/// What a change did besides the address.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Applied {
    /// `Capture is from <ip>; Replay sends to <host>.` when the Replay host was left alone and differs.
    pub note: Option<String>,
    /// The Replay host was blank and now is the found computer.
    pub replay_filled: bool,
}

/// Write a found computer into the settings. `found_url` is always set; `api_url` only for
/// [`ChangeTarget::Chosen`]; the Replay host only when it is blank, otherwise it is left alone and
/// the note says so if it differs.
pub fn apply_change(lights: &mut LightsSettings, change: &Change) -> Applied {
    lights.lightkey.found_url = change.url();
    if change.target == ChangeTarget::Chosen {
        lights.lightkey.api_url = change.url();
    }
    let host = lights.osc_host.trim().to_string();
    let mut applied = Applied::default();
    if host.is_empty() {
        lights.osc_host = change.ip.to_string();
        applied.replay_filled = true;
    } else if host != change.ip.to_string() {
        applied.note = Some(format!(
            "Capture is from {}; Replay sends to {host}.",
            change.address()
        ));
    }
    applied
}

// ------------------------------------------------------------------------------ the engine

/// What the engine reads from the plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostInputs {
    /// Lighting is on and set to OSC.
    pub active: bool,
    pub reach: Reach,
    /// `lightkey.api_url` as saved (blank in Automatic).
    pub api_url: String,
    pub found_url: String,
    pub osc_host: String,
    /// The address the link uses now (`effective_api_url`).
    pub effective_url: String,
}

impl HostInputs {
    pub fn from_settings(
        enabled: bool,
        osc: bool,
        reach: Reach,
        osc_host: &str,
        settings: &stagepilot_core::config::LightkeySettings,
    ) -> Self {
        Self {
            active: enabled && osc,
            reach,
            api_url: settings.api_url.clone(),
            found_url: settings.found_url.clone(),
            osc_host: osc_host.trim().to_string(),
            effective_url: effective_api_url(settings, osc_host),
        }
    }

    fn mode(&self) -> Mode {
        if self.api_url.trim().is_empty() {
            Mode::Automatic
        } else {
            Mode::Chosen(display_saved(&self.api_url))
        }
    }

    /// What a setting change is judged by.
    fn key(&self) -> (bool, String, String, String) {
        (
            self.active,
            self.api_url.clone(),
            self.found_url.clone(),
            self.osc_host.clone(),
        )
    }
}

/// The plugin, as the engine sees it.
#[async_trait]
pub trait DiscoveryHost: Send + Sync {
    fn inputs(&self) -> HostInputs;
    /// Save the change (settings file and the running plugin).
    async fn apply(&self, change: Change) -> Result<Applied, String>;
}

/// Writes a change to the saved settings and returns them. The server takes its cue-map lock and
/// settings file; without one the plugin changes only its own copy.
pub type DiscoveryPersist = Arc<
    dyn Fn(
            Change,
        )
            -> Pin<Box<dyn Future<Output = Result<(LightsSettings, Applied), String>> + Send>>
        + Send
        + Sync,
>;

/// The addresses to ask for the given inputs. Tests replace it to aim at loopback fakes.
pub type TargetsFn = Arc<dyn Fn(&HostInputs) -> Vec<Ipv4Addr> + Send + Sync>;

/// How the search is built and timed. `enabled: false` (the default) never starts a task.
#[derive(Clone)]
pub struct DiscoveryOptions {
    pub enabled: bool,
    pub settle: Duration,
    pub every: Duration,
    pub prober: Option<Arc<dyn Prober>>,
    pub targets: Option<TargetsFn>,
    pub persist: Option<DiscoveryPersist>,
}

impl Default for DiscoveryOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            settle: SETTLE,
            every: SCAN_EVERY,
            prober: None,
            targets: None,
            persist: None,
        }
    }
}

impl DiscoveryOptions {
    /// The real search: the real network and the usual timing.
    pub fn real() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }
}

/// The default list of targets for the inputs.
pub fn default_targets(inputs: &HostInputs) -> Vec<Ipv4Addr> {
    let (interfaces, own) = local_networks();
    build_targets(&TargetInputs {
        interfaces,
        own,
        saved: ipv4_of(&inputs.api_url),
        replay: ipv4_of(&inputs.osc_host),
        found: ipv4_of(&inputs.found_url),
    })
}

/// The port to ask: the one of the saved Lighting address, else 8770.
pub fn scan_port(inputs: &HostInputs) -> u16 {
    split_address(&inputs.api_url)
        .and_then(|(_, port)| port)
        .unwrap_or(DEFAULT_LIGHTKEY_API_PORT)
}

#[derive(Default)]
struct EngineState {
    scanning: bool,
    last: Option<ScanResult>,
    dismissed: BTreeSet<String>,
    /// The next scan starts at this time (or `force`).
    due: Option<Instant>,
    force: bool,
    down_since: Option<Instant>,
    message: Option<(String, Option<String>, Instant)>,
    key: Option<(bool, String, String, String)>,
    stopped: bool,
}

struct Core {
    options: DiscoveryOptions,
    prober: Arc<dyn Prober>,
    state: Mutex<EngineState>,
    wake: Notify,
}

/// Decides when to search and what to do with the answer.
#[derive(Clone)]
pub struct Discovery {
    core: Arc<Core>,
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
}

fn lock(m: &Mutex<EngineState>) -> std::sync::MutexGuard<'_, EngineState> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Discovery {
    pub fn new(options: DiscoveryOptions) -> Self {
        let prober = options
            .prober
            .clone()
            .unwrap_or_else(|| Arc::new(HttpProber::new()));
        Self {
            core: Arc::new(Core {
                options,
                prober,
                state: Mutex::new(EngineState::default()),
                wake: Notify::new(),
            }),
            task: Arc::new(Mutex::new(None)),
        }
    }

    pub fn enabled(&self) -> bool {
        self.core.options.enabled
    }

    pub fn persist(&self) -> Option<DiscoveryPersist> {
        self.core.options.persist.clone()
    }

    /// Start the task (a build without the search starts none).
    pub fn start(&self, host: Arc<dyn DiscoveryHost>) {
        if !self.core.options.enabled {
            return;
        }
        {
            let mut st = lock(&self.core.state);
            st.stopped = false;
            st.due = Some(Instant::now() + self.core.options.settle);
        }
        let core = self.core.clone();
        let handle = tokio::spawn(run(core, host));
        let old = self
            .task
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .replace(handle);
        if let Some(old) = old {
            old.abort();
        }
    }

    pub fn stop(&self) {
        lock(&self.core.state).stopped = true;
        self.core.wake.notify_one();
        if let Some(task) = self.task.lock().unwrap_or_else(|p| p.into_inner()).take() {
            task.abort();
        }
    }

    /// What the strip shows.
    pub fn status(&self, inputs: &HostInputs) -> LightkeyDiscovery {
        if !self.core.options.enabled {
            return LightkeyDiscovery::off();
        }
        let st = lock(&self.core.state);
        let mode = inputs.mode();
        let in_use = display_saved(&inputs.effective_url);
        let decision = decide(&DecideInput {
            active: inputs.active,
            mode: mode.clone(),
            reach: inputs.reach,
            scanning: st.scanning,
            scan: st.last.as_ref(),
            dismissed: &st.dismissed,
            in_use: &in_use,
        });
        let fresh = |at: Instant| at.elapsed().as_secs() < MESSAGE_SECONDS;
        let (message, note) = match &st.message {
            Some((m, n, at)) if inputs.reach == Reach::Up && fresh(*at) => {
                (Some(m.clone()), n.clone())
            }
            _ => (None, None),
        };
        let down_for = (inputs.reach == Reach::Down)
            .then(|| st.down_since.map(|at| at.elapsed().as_secs()))
            .flatten()
            .map(|s| u32::try_from(s).unwrap_or(u32::MAX));
        LightkeyDiscovery {
            state: decision.state,
            found: decision.found,
            refusing: decision.refusing,
            chosen: match mode {
                Mode::Chosen(text) => Some(text),
                Mode::Automatic => None,
            },
            chosen_down_for_s: down_for,
            last_scan_at: st
                .last
                .as_ref()
                .filter(|_| inputs.active && inputs.reach == Reach::Down)
                .map(|s| s.at.to_rfc3339_opts(SecondsFormat::Secs, true)),
            message,
            note,
        }
    }

    /// "Search again": scan now. `Err` is why not.
    pub fn scan_now(&self, inputs: &HostInputs) -> Result<String, String> {
        if !self.core.options.enabled || !inputs.active {
            return Err("Turn Lighting on first.".into());
        }
        if inputs.reach == Reach::Up {
            return Err("A Lighting computer is already connected.".into());
        }
        let mut st = lock(&self.core.state);
        if st.scanning {
            return Ok("Already looking for a Lighting computer.".into());
        }
        st.force = true;
        drop(st);
        self.core.wake.notify_one();
        Ok("Looking for a Lighting computer on this network\u{2026}".into())
    }

    /// "Keep waiting": silence the offer of `address` until StagePilot restarts or the chosen
    /// address changes.
    pub fn dismiss(&self, address: &str) {
        lock(&self.core.state)
            .dismissed
            .insert(address.trim().to_string());
    }

    /// "Use <ip>" / "Use this one": the address must be one the last search found.
    pub async fn use_address(
        &self,
        host: &dyn DiscoveryHost,
        address: &str,
    ) -> Result<(String, Option<String>), String> {
        let address = address.trim();
        let found = lock(&self.core.state)
            .last
            .as_ref()
            .and_then(|scan| scan.found.iter().find(|f| f.address() == address).cloned())
            .ok_or_else(|| {
                "That Lighting computer was not found by the last search. Press Search again."
                    .to_string()
            })?;
        let inputs = host.inputs();
        let target = match inputs.mode() {
            Mode::Automatic => ChangeTarget::Found,
            Mode::Chosen(_) => ChangeTarget::Chosen,
        };
        let applied = host
            .apply(Change {
                ip: found.ip,
                port: found.port,
                target,
            })
            .await?;
        let message = format!("Using the Lighting computer at {}.", found.address());
        let mut st = lock(&self.core.state);
        st.last = None;
        st.dismissed.clear();
        st.message = Some((message.clone(), applied.note.clone(), Instant::now()));
        st.key = Some(host.inputs().key());
        st.due = Some(Instant::now() + self.core.options.settle);
        Ok((message, applied.note))
    }
}

async fn run(core: Arc<Core>, host: Arc<dyn DiscoveryHost>) {
    loop {
        if lock(&core.state).stopped {
            return;
        }
        tick(&core, &host).await;
        tokio::select! {
            () = core.wake.notified() => {}
            () = tokio::time::sleep(TICK) => {}
        }
    }
}

async fn tick(core: &Arc<Core>, host: &Arc<dyn DiscoveryHost>) {
    let inputs = host.inputs();
    let now = Instant::now();
    {
        let mut st = lock(&core.state);
        // A setting changed: the saved address gets its chance first, and what was found or
        // silenced for the old address no longer applies.
        let key = inputs.key();
        if let Some(old) = &st.key {
            if *old != key {
                if old.1 != key.1 {
                    st.dismissed.clear();
                }
                st.last = None;
                st.message = None;
                st.due = Some(now + core.options.settle);
            }
        }
        st.key = Some(key);
        if !inputs.active {
            st.last = None;
            st.due = Some(now + core.options.settle);
            st.down_since = None;
            return;
        }
        match inputs.reach {
            Reach::Up => {
                st.down_since = None;
                st.last = None;
                st.due = Some(now + core.options.settle);
                return;
            }
            Reach::Pending => return,
            Reach::Down => {
                if st.down_since.is_none() {
                    st.down_since = Some(now);
                    // Nothing was connected a moment ago: give the saved address its chance.
                    st.due = Some(
                        st.due
                            .filter(|d| *d > now)
                            .unwrap_or(now + core.options.settle),
                    );
                }
            }
        }
        let due = st.force || st.due.is_some_and(|d| now >= d);
        if !due || st.scanning {
            return;
        }
        st.force = false;
        st.scanning = true;
    }
    run_scan(core, host, &inputs).await;
}

async fn run_scan(core: &Arc<Core>, host: &Arc<dyn DiscoveryHost>, started: &HostInputs) {
    let targets = match &core.options.targets {
        Some(targets) => targets(started),
        None => default_targets(started),
    };
    let key = started.key();
    let keep_going: Arc<dyn Fn() -> bool + Send + Sync> = {
        let (host, core, key) = (host.clone(), core.clone(), key.clone());
        Arc::new(move || {
            let now = host.inputs();
            !lock(&core.state).stopped && may_scan(now.active, now.reach) && now.key() == key
        })
    };
    let result = scan(core.prober.clone(), targets, scan_port(started), keep_going).await;
    let now = host.inputs();
    let step = {
        let mut st = lock(&core.state);
        st.scanning = false;
        // The link connected, a setting changed or StagePilot is stopping while we looked: this
        // answer is about something else now.
        if st.stopped || !may_scan(now.active, now.reach) || now.key() != key {
            return;
        }
        st.due = Some(Instant::now() + core.options.every);
        let in_use = display_saved(&now.effective_url);
        let decision = decide(&DecideInput {
            active: now.active,
            mode: now.mode(),
            reach: now.reach,
            scanning: false,
            scan: Some(&result),
            dismissed: &st.dismissed,
            in_use: &in_use,
        });
        st.last = Some(result);
        decision.step
    };
    if let Step::Connect(found) = step {
        let change = Change {
            ip: found.ip,
            port: found.port,
            target: ChangeTarget::Found,
        };
        let outcome = host.apply(change).await;
        let mut st = lock(&core.state);
        st.last = None;
        match outcome {
            Ok(applied) => {
                st.message = Some((
                    format!(
                        "Found a Lighting computer at {} and connected.",
                        found.address()
                    ),
                    applied.note,
                    Instant::now(),
                ));
                st.key = Some(host.inputs().key());
                st.due = Some(Instant::now() + core.options.settle);
            }
            Err(error) => {
                tracing::warn!(%error, "lightkey_discovery_not_saved");
            }
        }
    }
}

/// Used by tests: an `AtomicBool`-backed `keep_going`.
pub fn flag_keep_going(flag: Arc<AtomicBool>) -> Arc<dyn Fn() -> bool + Send + Sync> {
    Arc::new(move || flag.load(Ordering::SeqCst))
}

/// A saved found address that cannot be read (hand-edited file) is dropped when settings load.
pub fn clean_found_url(value: &str) -> String {
    clean_lightkey_api_url(value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> Ipv4Addr {
        text.parse().unwrap()
    }

    fn net(text: &str, prefix: u8) -> Net {
        Net {
            ip: ip(text),
            prefix,
        }
    }

    // ---- candidates

    #[test]
    fn loopback_comes_first_and_alone_when_nothing_else_is_known() {
        assert_eq!(
            build_targets(&TargetInputs::default()),
            vec![ip("127.0.0.1")]
        );
    }

    #[test]
    fn an_attached_network_is_scanned_with_its_real_prefix_and_without_this_computer() {
        let inputs = TargetInputs {
            interfaces: vec![net("192.168.1.20", 24)],
            own: vec![ip("192.168.1.20")],
            ..TargetInputs::default()
        };
        let targets = build_targets(&inputs);
        assert_eq!(targets[0], ip("127.0.0.1"));
        assert_eq!(targets.len(), 1 + 253);
        assert!(!targets.contains(&ip("192.168.1.20")));
        assert!(!targets.contains(&ip("192.168.1.0")));
        assert!(!targets.contains(&ip("192.168.1.255")));
        assert!(targets.contains(&ip("192.168.1.1")));
        assert!(targets.contains(&ip("192.168.1.254")));
        // A small network keeps its real size: /28 is 14 hosts.
        let small = build_targets(&TargetInputs {
            interfaces: vec![net("192.168.5.34", 28)],
            ..TargetInputs::default()
        });
        assert_eq!(small.len(), 1 + 14);
        assert!(small.contains(&ip("192.168.5.33")) && small.contains(&ip("192.168.5.46")));
        assert!(!small.contains(&ip("192.168.5.47")));
    }

    #[test]
    fn a_network_wider_than_22_bits_is_cut_to_the_24_around_the_interface() {
        for prefix in [8, 12, 16, 21] {
            let hosts = attached_hosts(net("10.20.7.9", prefix));
            assert_eq!(hosts.len(), 254, "/{prefix}");
            assert!(hosts.contains(&ip("10.20.7.1")) && hosts.contains(&ip("10.20.7.254")));
        }
        assert_eq!(attached_hosts(net("10.20.7.9", 22)).len(), 1022);
        assert_eq!(attached_hosts(net("10.20.7.9", 31)).len(), 0);
        assert_eq!(attached_hosts(net("10.20.7.9", 32)).len(), 0);
    }

    #[test]
    fn the_24_around_the_saved_replay_and_found_addresses_is_scanned() {
        // Hunter's rig: StagePilot on 10.20.0.x, the Lighting Mac on 10.30.0.211 behind a router.
        let inputs = TargetInputs {
            interfaces: vec![net("10.20.0.5", 24)],
            own: vec![ip("10.20.0.5")],
            saved: Some(ip("10.30.0.211")),
            ..TargetInputs::default()
        };
        let targets = build_targets(&inputs);
        assert_eq!(targets[1], ip("10.30.0.211"));
        assert!(targets.contains(&ip("10.30.0.1")) && targets.contains(&ip("10.30.0.254")));
        assert!(targets.contains(&ip("10.20.0.1")));

        let replay = build_targets(&TargetInputs {
            replay: Some(ip("172.16.4.9")),
            ..TargetInputs::default()
        });
        assert!(replay.contains(&ip("172.16.4.200")));
        let found = build_targets(&TargetInputs {
            found: Some(ip("192.168.77.3")),
            ..TargetInputs::default()
        });
        assert!(found.contains(&ip("192.168.77.250")));
        assert_eq!(found.len(), 1 + 254);
    }

    #[test]
    fn nothing_public_loopback_or_link_local_is_ever_a_target() {
        let inputs = TargetInputs {
            interfaces: vec![
                net("8.8.8.8", 24),
                net("169.254.3.4", 24),
                net("127.0.0.2", 24),
                net("100.64.1.1", 24),
                net("172.32.0.1", 24),
            ],
            own: vec![],
            saved: Some(ip("8.8.4.4")),
            replay: Some(ip("169.254.9.9")),
            found: Some(ip("127.0.0.9")),
        };
        assert_eq!(build_targets(&inputs), vec![ip("127.0.0.1")]);
        // The edges of 172.16/12 are inside, 172.15 and 172.32 are not.
        let edge = build_targets(&TargetInputs {
            interfaces: vec![
                net("172.16.0.9", 28),
                net("172.31.255.9", 28),
                net("172.15.0.9", 28),
            ],
            ..TargetInputs::default()
        });
        assert!(edge.contains(&ip("172.16.0.1")) && edge.contains(&ip("172.31.255.1")));
        assert!(!edge.iter().any(|t| t.octets()[..2] == [172, 15]));
    }

    #[test]
    fn this_computers_own_addresses_and_duplicates_are_left_out() {
        let inputs = TargetInputs {
            interfaces: vec![net("192.168.1.20", 24), net("192.168.1.21", 24)],
            own: vec![ip("192.168.1.20"), ip("192.168.1.21"), ip("192.168.1.1")],
            saved: Some(ip("192.168.1.50")),
            replay: Some(ip("192.168.1.50")),
            found: Some(ip("192.168.1.20")),
        };
        let targets = build_targets(&inputs);
        let unique: BTreeSet<_> = targets.iter().collect();
        assert_eq!(unique.len(), targets.len());
        for own in &inputs.own {
            assert!(!targets.contains(own), "{own}");
        }
        assert_eq!(targets.len(), 1 + 254 - 3);
    }

    #[test]
    fn a_scan_asks_at_most_1100_addresses_hints_first() {
        let inputs = TargetInputs {
            interfaces: vec![
                net("10.1.0.5", 22),
                net("10.2.0.5", 22),
                net("10.3.0.5", 22),
                net("192.168.9.5", 22),
            ],
            saved: Some(ip("10.30.0.211")),
            ..TargetInputs::default()
        };
        let targets = build_targets(&inputs);
        assert_eq!(targets.len(), MAX_TARGETS);
        assert_eq!(targets[0], ip("127.0.0.1"));
        // The /24 around the saved address is inside the cap even though the networks overflow it.
        assert!(targets.contains(&ip("10.30.0.254")));
    }

    #[test]
    fn the_port_is_the_saved_lighting_ones_else_8770() {
        let mut inputs = HostInputs {
            active: true,
            reach: Reach::Down,
            api_url: String::new(),
            found_url: String::new(),
            osc_host: "10.30.0.211".into(),
            effective_url: "http://10.30.0.211:8770".into(),
        };
        assert_eq!(scan_port(&inputs), 8770);
        inputs.api_url = "http://10.30.0.211:9000".into();
        assert_eq!(scan_port(&inputs), 9000);
        inputs.api_url = "10.30.0.211".into();
        assert_eq!(scan_port(&inputs), 8770);
    }

    #[test]
    fn addresses_read_and_write_the_way_the_operator_sees_them() {
        assert_eq!(display_address(ip("10.0.0.5"), 8770), "10.0.0.5");
        assert_eq!(display_address(ip("10.0.0.5"), 9000), "10.0.0.5:9000");
        assert_eq!(address_url(ip("10.0.0.5"), 8770), "http://10.0.0.5:8770");
        assert_eq!(display_saved("http://10.0.0.5:8770"), "10.0.0.5");
        assert_eq!(display_saved("http://10.0.0.5:9000"), "10.0.0.5:9000");
        assert_eq!(display_saved("http://lights.local"), "lights.local");
        assert_eq!(ipv4_of("http://10.0.0.5:8770"), Some(ip("10.0.0.5")));
        assert_eq!(ipv4_of("lights.local:8770"), None);
        assert_eq!(ipv4_of(""), None);
    }

    // ---- classification

    fn health(text: &str) -> Probed {
        classify(200, Some(text.as_bytes()))
    }

    #[test]
    fn a_healthy_helper_of_either_age_is_found() {
        // 0.4.x: no version, no fixture map.
        assert_eq!(
            health(r#"{"ok": true, "lightkey": true, "error": null, "dmx_age_ms": 4, "reads": 9}"#),
            Probed::Helper(HelperAnswer {
                version: None,
                lightkey_running: true,
                project: None
            })
        );
        // 0.5.0.
        assert_eq!(
            health(
                r#"{"ok": true, "lightkey": false, "version": "0.5.0",
                    "fixture_map": {"project": "Show.lightkeyproj", "count": 66}}"#
            ),
            Probed::Helper(HelperAnswer {
                version: Some("0.5.0".into()),
                lightkey_running: false,
                project: Some("Show.lightkeyproj".into())
            })
        );
        // A null fixture map is no project.
        assert!(matches!(
            health(r#"{"ok": true, "lightkey": true, "version": "0.5.0", "fixture_map": null}"#),
            Probed::Helper(HelperAnswer { project: None, .. })
        ));
    }

    #[test]
    fn only_a_complete_healthy_answer_is_a_helper() {
        for body in [
            r#"{"ok": false, "lightkey": true}"#,
            r#"{"ok": "yes", "lightkey": true}"#,
            r#"{"ok": 1, "lightkey": true}"#,
            r#"{"lightkey": true}"#,
            r#"{"ok": true}"#,
            r#"{"ok": true, "lightkey": "yes"}"#,
            r#"{"ok": true, "lightkey": null}"#,
            r#"[{"ok": true, "lightkey": true}]"#,
            r#""ok""#,
            "null",
            "",
            "not json {",
            "{\"ok\": true, \"lightkey\": tru",
        ] {
            assert_eq!(health(body), Probed::Ignore, "{body}");
        }
    }

    #[test]
    fn a_403_with_the_helpers_own_words_is_refusing_and_nothing_else_is() {
        assert_eq!(
            classify(
                403,
                Some(br#"{"error": "not available on this interface"}"#)
            ),
            Probed::Refusing
        );
        for (status, body) in [
            (403, r#"{"error": "forbidden"}"#),
            (403, r#"{"error": 5}"#),
            (403, "<html>Forbidden</html>"),
            (403, ""),
            (200, r#"{"error": "not available on this interface"}"#),
            (401, r#"{"error": "not available on this interface"}"#),
        ] {
            assert_eq!(
                classify(status, Some(body.as_bytes())),
                Probed::Ignore,
                "{status} {body}"
            );
        }
    }

    #[test]
    fn other_statuses_an_oversize_body_and_an_unreadable_body_are_ignored() {
        let good = br#"{"ok": true, "lightkey": true}"#;
        for status in [204, 301, 302, 307, 400, 404, 405, 500, 503] {
            assert_eq!(classify(status, Some(good)), Probed::Ignore, "{status}");
        }
        assert_eq!(classify(200, None), Probed::Ignore);
        let mut big = br#"{"ok": true, "lightkey": true, "pad": ""#.to_vec();
        big.resize(MAX_BODY_BYTES + 10, b'x');
        big.extend_from_slice(b"\"}");
        assert_eq!(classify(200, Some(&big)), Probed::Ignore);
    }

    // ---- the settings a found computer changes

    fn lights(api_url: &str, osc_host: &str) -> LightsSettings {
        let mut lights = LightsSettings::default();
        lights.lightkey.api_url = api_url.into();
        lights.osc_host = osc_host.into();
        lights
    }

    #[test]
    fn a_blank_replay_host_is_filled_with_the_found_address_and_the_port_is_untouched() {
        let mut settings = lights("", "");
        settings.osc_port = 21_700;
        let change = Change {
            ip: ip("10.30.0.211"),
            port: 8770,
            target: ChangeTarget::Found,
        };
        let applied = apply_change(&mut settings, &change);
        assert_eq!(settings.osc_host, "10.30.0.211");
        assert_eq!(settings.osc_port, 21_700);
        assert_eq!(settings.lightkey.found_url, "http://10.30.0.211:8770");
        assert_eq!(settings.lightkey.api_url, "", "Automatic stays Automatic");
        assert!(applied.replay_filled && applied.note.is_none());
    }

    #[test]
    fn a_replay_host_that_is_set_is_left_alone_and_a_difference_is_said() {
        let change = Change {
            ip: ip("10.30.0.211"),
            port: 8770,
            target: ChangeTarget::Found,
        };
        let mut other = lights("", "10.30.0.99");
        let applied = apply_change(&mut other, &change);
        assert_eq!(other.osc_host, "10.30.0.99");
        assert_eq!(
            applied.note.as_deref(),
            Some("Capture is from 10.30.0.211; Replay sends to 10.30.0.99.")
        );
        let mut same = lights("", "10.30.0.211");
        let applied = apply_change(&mut same, &change);
        assert_eq!(same.osc_host, "10.30.0.211");
        assert!(applied.note.is_none() && !applied.replay_filled);
    }

    #[test]
    fn using_an_offer_saves_it_as_the_chosen_address() {
        let mut settings = lights("http://10.30.0.50:8770", "10.30.0.50");
        let change = Change {
            ip: ip("10.30.0.211"),
            port: 9000,
            target: ChangeTarget::Chosen,
        };
        apply_change(&mut settings, &change);
        assert_eq!(settings.lightkey.api_url, "http://10.30.0.211:9000");
        assert_eq!(settings.lightkey.found_url, "http://10.30.0.211:9000");
        assert_eq!(
            settings.osc_host, "10.30.0.50",
            "a set Replay host is never changed"
        );
    }

    // ---- the decision table

    fn found(text: &str, running: bool) -> Found {
        Found {
            ip: ip(text),
            port: 8770,
            answer: HelperAnswer {
                version: None,
                lightkey_running: running,
                project: None,
            },
        }
    }

    fn result(found: Vec<Found>, refusing: Vec<&str>) -> ScanResult {
        ScanResult {
            found,
            refusing: refusing.into_iter().map(ip).collect(),
            port: 8770,
            at: Utc::now(),
        }
    }

    fn decide_with(
        mode: Mode,
        reach: Reach,
        scanning: bool,
        scan: Option<&ScanResult>,
        dismissed: &[&str],
    ) -> Decision {
        let dismissed: BTreeSet<String> = dismissed.iter().map(|s| (*s).to_string()).collect();
        decide(&DecideInput {
            active: true,
            mode,
            reach,
            scanning,
            scan,
            dismissed: &dismissed,
            in_use: "10.30.0.50",
        })
    }

    fn chosen() -> Mode {
        Mode::Chosen("10.30.0.50".into())
    }

    #[test]
    fn nothing_is_searched_or_said_while_connected_in_either_mode() {
        let scan = result(vec![found("10.30.0.211", true)], vec!["10.30.0.9"]);
        for mode in [Mode::Automatic, chosen()] {
            let d = decide_with(mode, Reach::Up, false, Some(&scan), &[]);
            assert_eq!(d.state, DiscoveryState::Connected);
            assert!(d.found.is_empty() && d.refusing.is_empty());
            assert_eq!(d.step, Step::Nothing);
        }
    }

    #[test]
    fn lighting_off_or_midi_is_off_whatever_else_is_true() {
        let scan = result(vec![found("10.30.0.211", true)], vec![]);
        let none = BTreeSet::new();
        let d = decide(&DecideInput {
            active: false,
            mode: Mode::Automatic,
            reach: Reach::Down,
            scanning: false,
            scan: Some(&scan),
            dismissed: &none,
            in_use: "x",
        });
        assert_eq!(d.state, DiscoveryState::Off);
        assert_eq!(d.step, Step::Nothing);
        assert!(!may_scan(false, Reach::Down));
    }

    #[test]
    fn search_starts_only_when_nothing_answers() {
        assert!(may_scan(true, Reach::Down));
        assert!(!may_scan(true, Reach::Up), "never while connected");
        assert!(
            !may_scan(true, Reach::Pending),
            "the saved address has not been tried yet"
        );
        assert!(!may_scan(false, Reach::Down));
    }

    #[test]
    fn while_looking_or_before_the_first_answer_the_state_is_searching() {
        let scan = result(vec![found("10.30.0.211", true)], vec![]);
        for (scanning, scan) in [(true, Some(&scan)), (false, None), (true, None)] {
            let d = decide_with(Mode::Automatic, Reach::Down, scanning, scan, &[]);
            assert_eq!(d.state, DiscoveryState::Searching);
            assert_eq!(
                d.step,
                Step::Nothing,
                "a result that is stale while rescanning is not acted on"
            );
        }
        let d = decide_with(Mode::Automatic, Reach::Pending, false, None, &[]);
        assert_eq!(d.state, DiscoveryState::Searching);
    }

    #[test]
    fn automatic_and_nothing_found_is_an_ordinary_none_with_any_refusing_helper() {
        let scan = result(vec![], vec!["10.30.0.9"]);
        let d = decide_with(Mode::Automatic, Reach::Down, false, Some(&scan), &[]);
        assert_eq!(d.state, DiscoveryState::Nothing);
        assert_eq!(d.refusing, vec!["10.30.0.9".to_string()]);
        assert_eq!(d.step, Step::Nothing);
    }

    #[test]
    fn automatic_and_exactly_one_found_connects_to_it() {
        let scan = result(vec![found("10.30.0.211", false)], vec![]);
        let d = decide_with(Mode::Automatic, Reach::Down, false, Some(&scan), &[]);
        assert_eq!(d.step, Step::Connect(found("10.30.0.211", false)));
        assert_eq!(d.state, DiscoveryState::Searching);
    }

    #[test]
    fn automatic_and_several_found_connects_to_none_and_lets_the_operator_choose() {
        let scan = result(
            vec![
                found("10.30.0.211", true),
                found("10.30.0.212", false),
                found("10.30.0.7", false),
            ],
            vec![],
        );
        let d = decide_with(Mode::Automatic, Reach::Down, false, Some(&scan), &[]);
        assert_eq!(d.state, DiscoveryState::Choose);
        assert_eq!(d.step, Step::Nothing);
        assert_eq!(
            d.found
                .iter()
                .map(|f| f.address.as_str())
                .collect::<Vec<_>>(),
            vec!["10.30.0.211", "10.30.0.212", "10.30.0.7"]
        );
        assert!(d.found[0].lightkey_running && !d.found[1].lightkey_running);
    }

    #[test]
    fn a_computer_that_is_the_address_in_use_is_never_offered_back() {
        // It answers /health yet the link is down: the link retries it by itself.
        let scan = result(vec![found("10.30.0.50", true)], vec!["10.30.0.50"]);
        for mode in [Mode::Automatic, chosen()] {
            let d = decide_with(mode, Reach::Down, false, Some(&scan), &[]);
            assert_eq!(d.state, DiscoveryState::Nothing);
            assert_eq!(d.step, Step::Nothing);
            assert!(d.refusing.is_empty());
        }
    }

    #[test]
    fn a_down_chosen_address_with_something_else_found_is_offered_never_switched() {
        let scan = result(vec![found("10.30.0.211", true)], vec![]);
        let d = decide_with(chosen(), Reach::Down, false, Some(&scan), &[]);
        assert_eq!(d.state, DiscoveryState::Offer);
        assert_eq!(d.step, Step::Nothing);
        assert_eq!(d.found.len(), 1);
        assert_eq!(d.found[0].address, "10.30.0.211");
    }

    #[test]
    fn keep_waiting_silences_that_address_only() {
        let scan = result(
            vec![found("10.30.0.211", true), found("10.30.0.212", true)],
            vec![],
        );
        let d = decide_with(chosen(), Reach::Down, false, Some(&scan), &["10.30.0.211"]);
        assert_eq!(d.state, DiscoveryState::Offer);
        assert_eq!(d.found[0].address, "10.30.0.212");
        let d = decide_with(
            chosen(),
            Reach::Down,
            false,
            Some(&scan),
            &["10.30.0.211", "10.30.0.212"],
        );
        assert_eq!(d.state, DiscoveryState::Nothing);
        assert!(d.found.is_empty());
    }

    #[test]
    fn a_chosen_address_with_nothing_found_says_none() {
        let scan = result(vec![], vec![]);
        let d = decide_with(chosen(), Reach::Down, false, Some(&scan), &[]);
        assert_eq!(d.state, DiscoveryState::Nothing);
    }

    #[test]
    fn a_non_standard_port_is_part_of_the_address_everywhere() {
        let mut odd = found("10.30.0.211", true);
        odd.port = 9000;
        assert_eq!(odd.address(), "10.30.0.211:9000");
        assert_eq!(odd.url(), "http://10.30.0.211:9000");
    }

    // ---- the scan

    struct Counting {
        in_flight: std::sync::atomic::AtomicUsize,
        most: std::sync::atomic::AtomicUsize,
        asked: Mutex<Vec<Ipv4Addr>>,
    }

    #[async_trait]
    impl Prober for Counting {
        async fn probe(&self, ip: Ipv4Addr, _port: u16) -> Probed {
            let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(now, Ordering::SeqCst);
            self.asked.lock().unwrap().push(ip);
            tokio::time::sleep(Duration::from_millis(5)).await;
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            if ip.octets()[3] == 211 {
                Probed::Helper(HelperAnswer {
                    version: None,
                    lightkey_running: true,
                    project: None,
                })
            } else if ip.octets()[3] == 9 {
                Probed::Refusing
            } else {
                Probed::Ignore
            }
        }
    }

    fn counting() -> Arc<Counting> {
        Arc::new(Counting {
            in_flight: 0.into(),
            most: 0.into(),
            asked: Mutex::new(Vec::new()),
        })
    }

    #[tokio::test]
    async fn a_scan_never_has_more_than_64_probes_in_flight_and_asks_each_address_once() {
        let prober = counting();
        let targets: Vec<Ipv4Addr> = build_targets(&TargetInputs {
            interfaces: vec![net("10.30.0.5", 22)],
            ..TargetInputs::default()
        });
        let wanted = targets.len();
        let result = scan(
            prober.clone(),
            targets,
            8770,
            flag_keep_going(Arc::new(AtomicBool::new(true))),
        )
        .await;
        assert!(prober.most.load(Ordering::SeqCst) <= MAX_IN_FLIGHT);
        assert!(prober.most.load(Ordering::SeqCst) > 1, "probes do overlap");
        let asked = prober.asked.lock().unwrap();
        assert_eq!(asked.len(), wanted);
        assert_eq!(asked.iter().collect::<BTreeSet<_>>().len(), wanted);
        // .211 in each of the four /24s of the /22, and .9 refuses in each.
        assert_eq!(result.found.len(), 4);
        assert_eq!(result.refusing.len(), 4);
        assert!(result.found.windows(2).all(|w| w[0].ip < w[1].ip));
    }

    #[tokio::test]
    async fn a_scan_that_is_told_to_stop_skips_the_rest() {
        let prober = counting();
        let flag = Arc::new(AtomicBool::new(true));
        let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let keep: Arc<dyn Fn() -> bool + Send + Sync> = {
            let (flag, seen) = (flag.clone(), seen.clone());
            Arc::new(move || {
                if seen.fetch_add(1, Ordering::SeqCst) >= 100 {
                    flag.store(false, Ordering::SeqCst);
                }
                flag.load(Ordering::SeqCst)
            })
        };
        let targets: Vec<Ipv4Addr> = (1..=250).map(|n| Ipv4Addr::new(10, 1, 0, n)).collect();
        scan(prober.clone(), targets, 8770, keep).await;
        let asked = prober.asked.lock().unwrap().len();
        assert!((90..=110).contains(&asked), "asked {asked}");
    }
}
