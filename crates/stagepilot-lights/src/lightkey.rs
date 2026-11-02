//! Lightkey fixture recording and replay: models and pure logic (no I/O, no clock, no network).
//!
//! While a song's countdown runs and recording is armed, the fixture changes the `lightkey-api`
//! relay reports (sACN DMX read on the Lightkey Mac) are stored against song time. On a later run
//! of the same song they are replayed as fixture overrides. Cues are neither recorded nor
//! replayed: a take holds fixture data only. Nothing here uses `tokio`.

use chrono::{DateTime, Timelike, Utc};
use serde::{Deserialize, Serialize};
use stagepilot_core::config::{LightkeySettings, DEFAULT_LIGHTKEY_API_PORT};
use stagepilot_core::state::ConnectionStatus;
use ts_rs::TS;

/// A take is kept only if the recording covered at least this share of the song.
pub const MIN_TAKE_FRACTION: f64 = 0.5;
/// Largest song time / duration a take accepts (`le=86_399`).
const MAX_SECONDS: f64 = 86_399.0;

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

/// Everything captured for one song: the fixture track.
///
/// Takes recorded before the cue-free replay also carry `initial` / `edges` (cue data). They are
/// still read, so an old `lightkey-takes.json` loads, but nothing uses them and they are not
/// written back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LightkeyTake {
    pub song_key: String,
    pub song_title: String,
    #[serde(with = "py_datetime")]
    pub recorded_at: DateTime<Utc>,
    pub duration_seconds: f64,
    /// Legacy cue data, ignored.
    #[serde(default, skip_serializing)]
    pub initial: serde_json::Value,
    /// Legacy cue data, ignored.
    #[serde(default, skip_serializing)]
    pub edges: serde_json::Value,
    /// Fixture state captured from the `lightkey-api` relay (colour, intensity, pan/tilt...).
    /// Absent in takes recorded before the relay existed, and left out of the file when empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixtures: Option<crate::lightkey_fixtures::FixtureTrack>,
}

impl LightkeyTake {
    /// A take with no fixture track yet.
    pub fn new(
        song_key: String,
        song_title: String,
        recorded_at: DateTime<Utc>,
        duration_seconds: f64,
    ) -> Result<Self, TakeError> {
        let mut take = Self {
            song_key,
            song_title,
            recorded_at,
            duration_seconds,
            initial: serde_json::Value::Null,
            edges: serde_json::Value::Null,
            fixtures: None,
        };
        take.normalise_and_validate()?;
        Ok(take)
    }

    /// Validate the fields and repair the fixture track (see `FixtureTrack`).
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
    /// Fixtures the capture drives and how many fixture changes it holds. Left out for takes
    /// without a fixture capture ("no fixture capture": such a take replays nothing).
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
