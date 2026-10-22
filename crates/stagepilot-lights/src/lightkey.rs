//! Lightkey live-state recording and replay: models and pure logic (no I/O, no clock, no network).
//! Function-for-function port of `backend/src/stagepilot/core/lightkey.py`.
//!
//! Lightkey sends nothing back over OSC or MIDI, so StagePilot reads which cues are active from the
//! separate `lightkey-api` agent on the Lightkey Mac. While a song's countdown runs and recording
//! is armed, every change of the active-cue set is stored as an *edge* at a song time; on a later
//! run of the same song the edges are replayed as OSC `activate` / `deactivate` commands. Nothing
//! here uses `tokio`, so every rule is tested without a runtime against
//! `backend/tests/golden/lightkey_golden.json`, the file Python is tested with.

use chrono::{DateTime, Timelike, Utc};
use serde::{Deserialize, Serialize};
use stagepilot_core::config::{LightkeySettings, DEFAULT_LIGHTKEY_API_PORT};
use stagepilot_core::state::ConnectionStatus;
use std::collections::{BTreeSet, HashMap};
use ts_rs::TS;

/// A take is kept only if the recording covered at least this share of the song.
pub const MIN_TAKE_FRACTION: f64 = 0.5;
/// Seconds before a recorded edge that the replay command is sent (OSC + fade latency).
pub const REPLAY_LEAD_SECONDS: f64 = 0.04;
/// How long an operator's own change to a replayed cue is respected before replay may touch it.
pub const OPERATOR_HOLD_SECONDS: f64 = 3.0;
/// A cue change seen this soon after replay sent a command for that cue is replay's own echo.
pub const ECHO_WINDOW_SECONDS: f64 = 2.5;

/// Largest song time / duration a take accepts (`le=86_399`).
const MAX_SECONDS: f64 = 86_399.0;
const MAX_NAMES: usize = 512;
const MAX_EDGES: usize = 20_000;

/// `Verb`: the idempotent OSC verbs. `toggle` is never used: it is not idempotent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verb {
    Activate,
    Deactivate,
}

impl Verb {
    pub fn as_str(self) -> &'static str {
        match self {
            Verb::Activate => "activate",
            Verb::Deactivate => "deactivate",
        }
    }
}

/// Python `round(value, 3)` for the finite values a song time can take.
fn round_ms(value: f64) -> f64 {
    (value * 1000.0).round_ties_even() / 1000.0
}

/// A take's validation failure (the Python `ValidationError` text is not part of any contract:
/// the callers only need to know that the data is not a valid take).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakeError(pub String);

impl std::fmt::Display for TakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TakeError {}

/// One change of the active-cue set, `at_seconds` into the song.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LightkeyEdge {
    pub at_seconds: f64,
    #[serde(default)]
    pub added: Vec<String>,
    #[serde(default)]
    pub removed: Vec<String>,
}

impl LightkeyEdge {
    /// `LightkeyEdge(...)`: times are rounded to 1 ms; an edge must change something and cannot
    /// add and remove the same cue.
    pub fn new(
        at_seconds: f64,
        added: Vec<String>,
        removed: Vec<String>,
    ) -> Result<Self, TakeError> {
        let edge = Self {
            at_seconds: round_ms(at_seconds),
            added,
            removed,
        };
        edge.validate()?;
        Ok(edge)
    }

    pub fn validate(&self) -> Result<(), TakeError> {
        let err = |m: &str| Err(TakeError(m.to_string()));
        if !(self.at_seconds.is_finite() && (0.0..=MAX_SECONDS).contains(&self.at_seconds)) {
            return err("An edge time must be between 0 and 86399 seconds.");
        }
        if self.added.len() > MAX_NAMES || self.removed.len() > MAX_NAMES {
            return err("An edge can name at most 512 cues.");
        }
        if self.added.is_empty() && self.removed.is_empty() {
            return err("An edge must add or remove at least one cue.");
        }
        if self.added.iter().any(|a| self.removed.contains(a)) {
            return err("A cue cannot be added and removed by the same edge.");
        }
        Ok(())
    }
}

/// Everything captured for one song: the starting set plus the edges after it.
///
/// `initial` is the active-cue set at song time 0 (edges at 0.0 are folded into it by the
/// recorder). Replaying the take drives only cues named somewhere in it (`managed_cues`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LightkeyTake {
    pub song_key: String,
    pub song_title: String,
    #[serde(with = "py_datetime")]
    pub recorded_at: DateTime<Utc>,
    pub duration_seconds: f64,
    #[serde(default)]
    pub initial: Vec<String>,
    #[serde(default)]
    pub edges: Vec<LightkeyEdge>,
    /// Fixture state captured from the `lightkey-api` relay (colour, intensity, pan/tilt...).
    /// Absent in takes recorded before the relay existed, and left out of the file when empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixtures: Option<crate::lightkey_fixtures::FixtureTrack>,
}

impl LightkeyTake {
    /// `LightkeyTake(...)`: validates every field and sorts the edges by time (stable).
    pub fn new(
        song_key: String,
        song_title: String,
        recorded_at: DateTime<Utc>,
        duration_seconds: f64,
        initial: Vec<String>,
        edges: Vec<LightkeyEdge>,
    ) -> Result<Self, TakeError> {
        let mut take = Self {
            song_key,
            song_title,
            recorded_at,
            duration_seconds,
            initial,
            edges,
            fixtures: None,
        };
        take.normalise_and_validate()?;
        Ok(take)
    }

    /// Validate as pydantic does and apply the after-validators (edge rounding, edge sorting).
    pub fn normalise_and_validate(&mut self) -> Result<(), TakeError> {
        let err = |m: &str| Err(TakeError(m.to_string()));
        let key_len = self.song_key.chars().count();
        if !(1..=256).contains(&key_len) {
            return err("song_key must be 1 to 256 characters.");
        }
        let title_len = self.song_title.chars().count();
        if !(1..=500).contains(&title_len) {
            return err("song_title must be 1 to 500 characters.");
        }
        if !(self.duration_seconds.is_finite()
            && (0.0..=MAX_SECONDS).contains(&self.duration_seconds))
        {
            return err("duration_seconds must be between 0 and 86399.");
        }
        if self.initial.len() > MAX_NAMES {
            return err("initial can name at most 512 cues.");
        }
        if self.edges.len() > MAX_EDGES {
            return err("A take can hold at most 20000 edges.");
        }
        for edge in &mut self.edges {
            edge.at_seconds = round_ms(edge.at_seconds);
            edge.validate()?;
        }
        self.edges
            .sort_by(|a, b| a.at_seconds.total_cmp(&b.at_seconds));
        if let Some(track) = self.fixtures.as_mut() {
            track.normalise_and_validate().map_err(|e| TakeError(e.0))?;
        }
        Ok(())
    }
}

/// `datetime` the way pydantic writes an aware UTC value: `2026-10-20T20:00:00Z`, with six
/// fractional digits only when there are microseconds.
mod py_datetime {
    use chrono::{DateTime, SecondsFormat, Timelike, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
        let micros = value.nanosecond() / 1_000 % 1_000_000;
        let text = if micros == 0 {
            value.to_rfc3339_opts(SecondsFormat::Secs, true)
        } else {
            value.to_rfc3339_opts(SecondsFormat::Micros, true)
        };
        s.serialize_str(&text)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DateTime<Utc>, D::Error> {
        let text = String::deserialize(d)?;
        DateTime::parse_from_rfc3339(&text)
            .map(|d| d.with_timezone(&Utc))
            .map_err(serde::de::Error::custom)
    }
}

/// A wall-clock instant truncated to microseconds (Python `datetime` resolution).
pub fn truncate_micros(value: DateTime<Utc>) -> DateTime<Utc> {
    value
        .with_nanosecond(value.nanosecond() / 1_000 * 1_000)
        .unwrap_or(value)
}

/// `LightkeyTakeSummary`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightkeyTakeSummary {
    pub song_key: String,
    pub song_title: String,
    #[serde(with = "py_datetime")]
    #[ts(as = "String")]
    pub recorded_at: DateTime<Utc>,
    pub duration_seconds: f64,
    pub edge_count: u32,
    pub cue_count: u32,
    /// Fixtures the capture drives and how many fixture changes it holds. Left out for takes
    /// without a fixture capture, so those serialise exactly as before the relay existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub fixture_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub fixture_event_count: Option<u32>,
}

/// `LightkeyStatus.link`: off = Lighting disabled; connected = API answered and Lightkey is
/// running; lightkey_closed = API answered, Lightkey is not running; unreachable = no answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LightkeyLink {
    Off,
    Connected,
    LightkeyClosed,
    Unreachable,
}

/// State of the fixture stream from the `lightkey-api` OSC relay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FixtureLink {
    /// Subscribed; nothing received yet.
    Waiting,
    /// Packets are arriving and the fixture state is current.
    Live,
    /// No packets for a few seconds, or the relay refused us.
    Lost,
}

/// The fixture stream, as the strip shows it. `LightkeyStatus.fixtures` is left out until the
/// relay has answered once, so a Lightkey API older than 0.4 changes nothing on screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FixtureStatus {
    pub link: FixtureLink,
    pub detail: Option<String>,
    /// Fixtures the Lightkey project reports.
    pub fixtures: u32,
    /// Packets lost since the link started.
    pub lost_packets: u32,
    /// This capture records fixture changes against the song.
    pub recording: bool,
}

/// What the StagePilot Lightkey strip shows (`LightkeyStatus`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightkeyStatus {
    pub api_url: String,
    pub link: LightkeyLink,
    pub detail: Option<String>,
    pub active_count: Option<u32>,
    pub recording: bool,
    pub recording_song: Option<String>,
    pub replay: bool,
    pub replay_blocked: Option<String>,
    pub replaying_song: Option<String>,
    pub message: Option<String>,
    pub takes: Vec<LightkeyTakeSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub fixtures: Option<FixtureStatus>,
}

impl LightkeyStatus {
    pub fn as_connection(&self) -> ConnectionStatus {
        if self.link == LightkeyLink::Connected {
            ConnectionStatus::Connected
        } else {
            ConnectionStatus::Disconnected
        }
    }
}

/// `LightkeyActionResponse`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightkeyActionResponse {
    pub accepted: bool,
    pub message: String,
    pub lightkey: LightkeyStatus,
}

// ------------------------------------------------------------------ pure logic

/// `effective_api_url`: the saved address, else the OSC host on the default API port.
pub fn effective_api_url(settings: &LightkeySettings, osc_host: &str) -> String {
    if settings.api_url.is_empty() {
        return format!("http://{osc_host}:{DEFAULT_LIGHTKEY_API_PORT}");
    }
    let text = &settings.api_url;
    let host_part = text
        .split_once("://")
        .map_or(text.as_str(), |(_, rest)| rest);
    if host_part.contains(':') {
        text.clone()
    } else {
        format!("{text}:{DEFAULT_LIGHTKEY_API_PORT}")
    }
}

/// `diff`: `(added, removed)` going from `previous` to `current`, each sorted.
pub fn diff<'a>(
    previous: impl IntoIterator<Item = &'a String>,
    current: impl IntoIterator<Item = &'a String>,
) -> (Vec<String>, Vec<String>) {
    let before: BTreeSet<&String> = previous.into_iter().collect();
    let now: BTreeSet<&String> = current.into_iter().collect();
    (
        now.difference(&before).map(|s| (*s).clone()).collect(),
        before.difference(&now).map(|s| (*s).clone()).collect(),
    )
}

/// `state_at`: the captured active set at `position` seconds (an edge at `position` is applied).
pub fn state_at(take: &LightkeyTake, position: f64) -> BTreeSet<String> {
    let mut active: BTreeSet<String> = take.initial.iter().cloned().collect();
    for edge in &take.edges {
        if edge.at_seconds > position {
            break;
        }
        for name in &edge.removed {
            active.remove(name);
        }
        active.extend(edge.added.iter().cloned());
    }
    active
}

/// `managed_cues`: every cue the take ever mentions. Replay never touches a cue outside this set.
pub fn managed_cues(take: &LightkeyTake) -> BTreeSet<String> {
    let mut cues: BTreeSet<String> = take.initial.iter().cloned().collect();
    for edge in &take.edges {
        cues.extend(edge.added.iter().cloned());
        cues.extend(edge.removed.iter().cloned());
    }
    cues
}

/// `osc_address`: Lightkey's idempotent cue address. `*` matches any control panel.
pub fn osc_address(cue: &str, verb: Verb) -> String {
    format!("/live/*/cue/{cue}/{}", verb.as_str())
}

/// `is_addressable`: a cue name containing `/` or a control character cannot be put in an OSC
/// address.
pub fn is_addressable(cue: &str) -> bool {
    !cue.is_empty() && !cue.contains('/') && !cue.chars().any(|c| (c as u32) < 32)
}

/// `ReplayCommand`.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayCommand {
    pub at_seconds: f64,
    pub cue: String,
    pub verb: Verb,
}

/// `replay_plan`: `(sync_now, later)` for starting or repositioning playback at `position`.
///
/// `sync_now` forces every managed cue to its captured state at `position` and is sent
/// immediately, sorted by name. `later` holds one command per cue change after `position` at its
/// edge time, in time order, removals before additions within one edge. Unaddressable names are
/// dropped.
pub fn replay_plan(take: &LightkeyTake, position: f64) -> (Vec<ReplayCommand>, Vec<ReplayCommand>) {
    let on = state_at(take, position);
    let sync_now = managed_cues(take)
        .into_iter()
        .filter(|cue| is_addressable(cue))
        .map(|cue| ReplayCommand {
            at_seconds: position,
            verb: if on.contains(&cue) {
                Verb::Activate
            } else {
                Verb::Deactivate
            },
            cue,
        })
        .collect();
    let mut later = Vec::new();
    for edge in &take.edges {
        if edge.at_seconds <= position {
            continue;
        }
        for (names, verb) in [
            (&edge.removed, Verb::Deactivate),
            (&edge.added, Verb::Activate),
        ] {
            for cue in names.iter().filter(|c| is_addressable(c)) {
                later.push(ReplayCommand {
                    at_seconds: edge.at_seconds,
                    cue: cue.clone(),
                    verb,
                });
            }
        }
    }
    (sync_now, later)
}

/// One observation while recording: `(song_seconds, added, removed)`.
pub type Observation = (f64, Vec<String>, Vec<String>);

/// `build_take_edges`: fold timed observations into `(initial, edges)`.
///
/// `observed` is in arrival order. Times are clamped into `[0, duration]`; an observation at 0
/// becomes part of `initial`; adding a cue that is already on, or removing one that is already
/// off, is dropped so the stored set never lies.
pub fn build_take_edges<'a>(
    baseline: impl IntoIterator<Item = &'a String>,
    observed: &[Observation],
    duration: f64,
) -> Result<(Vec<String>, Vec<LightkeyEdge>), TakeError> {
    let mut active: BTreeSet<String> = baseline.into_iter().cloned().collect();
    let mut initial = active.clone();
    let mut edges = Vec::new();
    let mut ordered: Vec<&Observation> = observed.iter().collect();
    ordered.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (raw_time, added, removed) in ordered {
        let time = raw_time.max(0.0).min(duration);
        let really_removed: BTreeSet<String> = removed
            .iter()
            .filter(|c| active.contains(*c))
            .cloned()
            .collect();
        let really_added: BTreeSet<String> = added
            .iter()
            .filter(|c| !active.contains(*c))
            .cloned()
            .collect();
        if really_added.is_empty() && really_removed.is_empty() {
            continue;
        }
        for name in &really_removed {
            active.remove(name);
        }
        active.extend(really_added.iter().cloned());
        if time <= 0.0 {
            initial = active.clone();
            continue;
        }
        edges.push(LightkeyEdge::new(
            time,
            really_added.into_iter().collect(),
            really_removed.into_iter().collect(),
        )?);
    }
    Ok((initial.into_iter().collect(), edges))
}

/// `take_is_worth_keeping`: true when the recording covered enough of the song to replace an
/// earlier take.
pub fn take_is_worth_keeping(covered_seconds: f64, duration_seconds: f64) -> bool {
    duration_seconds > 0.0 && covered_seconds >= duration_seconds * MIN_TAKE_FRACTION
}

/// `summarize`.
pub fn summarize(take: &LightkeyTake) -> LightkeyTakeSummary {
    LightkeyTakeSummary {
        song_key: take.song_key.clone(),
        song_title: take.song_title.clone(),
        recorded_at: take.recorded_at,
        duration_seconds: take.duration_seconds,
        edge_count: u32::try_from(take.edges.len()).unwrap_or(u32::MAX),
        cue_count: u32::try_from(managed_cues(take).len()).unwrap_or(u32::MAX),
        fixture_count: take
            .fixtures
            .as_ref()
            .map(|t| u32::try_from(t.fixtures().len()).unwrap_or(u32::MAX)),
        fixture_event_count: take
            .fixtures
            .as_ref()
            .map(|t| u32::try_from(t.events.len()).unwrap_or(u32::MAX)),
    }
}

/// Tracks which replayed cues the live operator has touched, so replay stands back.
///
/// Time is passed in (monotonic seconds) so this stays pure. `note_sent` records that replay just
/// drove a cue; `note_change` is called for every change the Lightkey API reports. A change to a
/// cue soon after replay sent it is replay's own echo; any other change is the operator's, and
/// that cue is skipped by replay until `OPERATOR_HOLD_SECONDS` later.
#[derive(Debug, Default, Clone)]
pub struct OperatorHold {
    sent: HashMap<String, f64>,
    held_until: HashMap<String, f64>,
}

impl OperatorHold {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn note_sent(&mut self, cue: &str, now: f64) {
        self.sent.insert(cue.to_string(), now);
    }

    /// Returns true when this change was the operator's (and starts or extends a hold).
    pub fn note_change(&mut self, cue: &str, now: f64) -> bool {
        if let Some(sent_at) = self.sent.get(cue) {
            if now - sent_at <= ECHO_WINDOW_SECONDS {
                return false;
            }
        }
        self.held_until
            .insert(cue.to_string(), now + OPERATOR_HOLD_SECONDS);
        true
    }

    pub fn is_held(&self, cue: &str, now: f64) -> bool {
        self.held_until.get(cue).is_some_and(|until| *until > now)
    }

    pub fn held_cues(&self, now: f64) -> Vec<String> {
        let mut cues: Vec<String> = self
            .held_until
            .iter()
            .filter(|(_, until)| **until > now)
            .map(|(cue, _)| cue.clone())
            .collect();
        cues.sort();
        cues
    }
}
