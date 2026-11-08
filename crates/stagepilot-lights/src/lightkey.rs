//! Lightkey fixture recording and replay: models and pure logic (no I/O, no clock, no network).
//!
//! While a song's countdown runs and recording is armed, the fixture changes the `lightkey-api`
//! relay reports (sACN DMX read on the Lightkey Mac) are kept against song time. At the end they
//! are thinned into look rows on the song's timeline (`crate::lightkey_looks`), which the
//! timeline then replays like any other row. Nothing here uses `tokio`.

use serde::{Deserialize, Serialize};
use stagepilot_core::config::{
    LightkeyDetail, LightkeyMode, LightkeySettings, DEFAULT_LIGHTKEY_API_PORT,
};
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

/// Auto's decision for the current (or next) song, with the reason the strip shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightkeyAuto {
    /// The song captures (otherwise it replays).
    pub capturing: bool,
    /// For example "Auto: capturing this song".
    pub reason: String,
    /// The song the decision is about, when one is known.
    pub song: Option<String>,
}

/// The oldest `lightkey-api` that has the project watcher, `POST /fixtures/refresh` and the
/// `version` key in `GET /health`. A helper that reports no version is older than this.
pub const MIN_HELPER_VERSION: &str = "0.5.0";

/// What `GET /health` of the `lightkey-api` helper says besides "it is up" (`LightkeyStatus.helper`).
/// Present once the helper answered `/health` after the last connect; the numbers describe the
/// helper's own fixture map and project watcher, not the live relay stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightkeyHelper {
    /// The helper's version; absent on a helper older than 0.5.0 (it does not report one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<String>")]
    pub version: Option<String>,
    /// The helper is older than [`MIN_HELPER_VERSION`]: capture and replay work, Read fixtures
    /// and the project watcher do not exist.
    pub outdated: bool,
    /// Project base name of the helper's fixture map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<String>")]
    pub project: Option<String>,
    /// Fixtures in the helper's fixture map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<u32>")]
    pub fixtures: Option<u32>,
    /// The fixture map id (the number the relay hello ends with).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<f64>")]
    pub map: Option<i64>,
    /// `ok`, `changed`, `lightkey_closed`, `no_project`, `permission_denied` or `unreadable`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<String>")]
    pub watch_state: Option<String>,
    /// The helper's plain-language sentence about the watch state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<String>")]
    pub watch_detail: Option<String>,
}

/// The current song was captured under another Lightkey project than the one open now
/// (`LightkeyStatus.project_mismatch`). Only ever a notice: replay is not held or stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightkeyProjectMismatch {
    pub song: String,
    pub captured: String,
    pub current: String,
    /// "Captured with X. Lightkey has Y open."
    pub message: String,
}

/// `POST /api/v1/lights/lightkey/read-fixtures`: the helper's answer in plain words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightkeyReadFixtures {
    pub ok: bool,
    /// "Read 66 fixtures from Show.lightkeyproj", "No change: 66 fixtures", or the reason.
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<String>")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<u32>")]
    pub count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<bool>")]
    pub changed: Option<bool>,
    /// The helper's code (`lightkey_closed`, `no_project`, `busy`, `permission_denied`,
    /// `unreadable`) or ours (`old_helper`, `unreachable`, `lighting_off`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<String>")]
    pub code: Option<String>,
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
    /// Auto, or Replay / Record pinned for every song (Record is the armed Record switch).
    pub mode: LightkeyMode,
    /// In Auto: what the link decided (or would decide) for the current/next song.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub auto: Option<LightkeyAuto>,
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
    /// Fixtures the saved looks can override, left out when none. After a restart the held set
    /// is empty but Lightkey may still hold overrides: Release lights clears all of these.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub known_fixtures: Option<u32>,
    /// The Lightkey project (base name) that is open, from the relay hello, else from the
    /// helper's health answer. Left out until one of them says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub project: Option<String>,
    /// What the helper's `GET /health` said (version, fixture map, watch state).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub helper: Option<LightkeyHelper>,
    /// The current song was captured under another project than the one open now. Never
    /// changes what replay does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub project_mismatch: Option<LightkeyProjectMismatch>,
    /// The search for the Lighting computer (`crate::lightkey_discovery`). Set by the plugin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub discovery: Option<crate::lightkey_discovery::LightkeyDiscovery>,
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

/// `effective_api_url`: the saved (chosen) address; in Automatic (blank) the computer the search
/// last connected to, else the OSC host on the default API port.
pub fn effective_api_url(settings: &LightkeySettings, osc_host: &str) -> String {
    if settings.api_url.is_empty() {
        if !settings.found_url.is_empty() {
            return with_default_port(&settings.found_url);
        }
        return format!("http://{osc_host}:{DEFAULT_LIGHTKEY_API_PORT}");
    }
    with_default_port(&settings.api_url)
}

/// A saved address, with the default API port added when it names none.
fn with_default_port(text: &str) -> String {
    let host_part = text.split_once("://").map_or(text, |(_, rest)| rest);
    if host_part.contains(':') {
        text.to_string()
    } else {
        format!("{text}:{DEFAULT_LIGHTKEY_API_PORT}")
    }
}

/// True when the recording covered enough of the song to be saved to its timeline.
pub fn recording_is_worth_keeping(covered_seconds: f64, duration_seconds: f64) -> bool {
    duration_seconds > 0.0 && covered_seconds >= duration_seconds * MIN_RECORDED_FRACTION
}

/// The project name as the notices use it: the base name, trimmed; empty means "not reported".
pub fn clean_project(name: Option<&str>) -> Option<String> {
    name.map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string)
}

/// `true` when `version` (as `lightkey-api` prints it, `0.5.0`) is older than
/// [`MIN_HELPER_VERSION`]. A missing or unreadable version is older: 0.4.x sends none.
pub fn helper_is_outdated(version: Option<&str>) -> bool {
    fn parts(text: &str) -> Option<Vec<u64>> {
        text.trim()
            .trim_start_matches('v')
            .split(['.', '-', '+'])
            .take(3)
            .map(|p| p.parse().ok())
            .collect()
    }
    match (version.and_then(parts), parts(MIN_HELPER_VERSION)) {
        (Some(have), Some(need)) => have < need,
        _ => true,
    }
}

/// The notice for a song captured under `captured` while `current` is open. Only when BOTH are
/// known and they differ; an old capture (no project) or an unreported project never warns.
pub fn project_mismatch(
    song: &str,
    captured: Option<&str>,
    current: Option<&str>,
) -> Option<LightkeyProjectMismatch> {
    let captured = clean_project(captured)?;
    let current = clean_project(current)?;
    (captured != current).then(|| LightkeyProjectMismatch {
        song: song.to_string(),
        message: format!("Captured with {captured}. Lightkey has {current} open."),
        captured,
        current,
    })
}

/// "66 fixtures", "1 fixture".
pub fn fixtures_word(count: u32) -> String {
    format!("{count} fixture{}", if count == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mismatch_needs_both_projects_and_a_difference() {
        assert!(project_mismatch("s", Some("A"), Some("B")).is_some());
        assert_eq!(
            project_mismatch("s", Some("A"), Some("B")).unwrap().message,
            "Captured with A. Lightkey has B open."
        );
        assert!(project_mismatch("s", Some("A"), Some("A")).is_none());
        assert!(project_mismatch("s", None, Some("B")).is_none());
        assert!(project_mismatch("s", Some("A"), None).is_none());
        assert!(project_mismatch("s", Some("A"), Some("  ")).is_none());
        assert!(project_mismatch("s", Some(""), Some("B")).is_none());
    }

    #[test]
    fn a_helper_without_a_version_is_older_than_0_5_0() {
        assert!(helper_is_outdated(None));
        assert!(helper_is_outdated(Some("0.4.3")));
        assert!(helper_is_outdated(Some("nonsense")));
        assert!(!helper_is_outdated(Some("0.5.0")));
        assert!(!helper_is_outdated(Some("0.5.1")));
        assert!(!helper_is_outdated(Some("0.10.0")));
        assert!(!helper_is_outdated(Some("1.0.0")));
    }
}
