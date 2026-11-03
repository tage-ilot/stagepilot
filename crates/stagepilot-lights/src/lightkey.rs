//! Lightkey fixture recording and replay: models and pure logic (no I/O, no clock, no network).
//!
//! While a song's countdown runs and recording is armed, the fixture changes the `lightkey-api`
//! relay reports (sACN DMX read on the Lightkey Mac) are kept against song time. At the end they
//! are thinned into look rows on the song's timeline (`crate::lightkey_looks`), which the
//! timeline then replays like any other row. Nothing here uses `tokio`.

use serde::{Deserialize, Serialize};
use stagepilot_core::config::{LightkeyDetail, LightkeySettings, DEFAULT_LIGHTKEY_API_PORT};
use stagepilot_core::state::ConnectionStatus;
use ts_rs::TS;

/// A recording is saved only if it covered at least this share of the song.
pub const MIN_RECORDED_FRACTION: f64 = 0.5;

/// The looks last saved to a song's timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightkeyCaptured {
    pub song_title: String,
    /// Captured look rows now on that song's timeline.
    pub looks: u32,
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
    /// How finely recordings are kept (the saved Detail setting), left out while `normal`.
    #[serde(default, skip_serializing_if = "LightkeyDetail::is_normal")]
    #[ts(optional, as = "Option<LightkeyDetail>")]
    pub detail_level: LightkeyDetail,
    /// The song most recently saved to the timeline, left out until one has been.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub captured: Option<LightkeyCaptured>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub fixtures: Option<FixtureStatus>,
    /// Fixtures the replay holds with overrides, left out when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub holding_fixtures: Option<u32>,
    /// Fixtures the replay gave to the operator, left out when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub yielded_fixtures: Option<u32>,
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

/// `LightkeySimplifyResponse`: what "Simplify this song" would do (`applied = false`) or did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightkeySimplifyResponse {
    /// The request was valid and the song has captured rows to simplify.
    pub accepted: bool,
    /// The simplified rows were saved (always false for a preview).
    pub applied: bool,
    pub message: String,
    pub rows_before: u32,
    pub rows_after: u32,
    pub messages_before: u32,
    pub messages_after: u32,
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

/// True when the recording covered enough of the song to be saved to its timeline.
pub fn recording_is_worth_keeping(covered_seconds: f64, duration_seconds: f64) -> bool {
    duration_seconds > 0.0 && covered_seconds >= duration_seconds * MIN_RECORDED_FRACTION
}
