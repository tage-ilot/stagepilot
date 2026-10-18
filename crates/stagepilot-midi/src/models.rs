//! Wire and domain models for the MIDI routes (`models/api.py`, `core/midi.py`).
//! TypeScript for the response/request types is generated into `generated/ts/`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use stagepilot_core::events::ActionName;
use stagepilot_core::state::ApplicationState;
use ts_rs::TS;

pub use stagepilot_core::api::{MidiCueName, MidiInputResponse, MidiInputsResponse};

/// `MidiMessage.type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum NoteType {
    NoteOn,
    NoteOff,
}

/// `MidiMessage`: a validated note message with an operator-facing, one-based channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidiMessage {
    #[serde(rename = "type")]
    pub kind: NoteType,
    pub channel: u8,
    pub note: u8,
    pub velocity: u8,
}

/// `MidiMessageDisposition`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MidiMessageDisposition {
    Dispatched,
    ActionRejected,
    Duplicate,
    Unmapped,
    WrongChannel,
    NoteRelease,
    QueueFull,
    Error,
}

/// `MidiMonitorMessageResponse` (also the in-memory monitor entry).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MidiMonitorMessage {
    pub timestamp: DateTime<Utc>,
    #[serde(default)]
    pub input_name: Option<String>,
    pub message_type: NoteType,
    pub channel: u8,
    pub note: u8,
    pub note_name: String,
    pub velocity: u8,
    pub disposition: MidiMessageDisposition,
    pub detail: String,
    #[serde(default)]
    pub action: Option<ActionName>,
    #[serde(default)]
    pub simulated: bool,
}

/// `MidiMonitorResponse`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MidiMonitorResponse {
    pub messages: Vec<MidiMonitorMessage>,
}

/// `MidiInputSelectionRequest` (`input_id` is required but may be null).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MidiInputSelectionRequest {
    pub input_id: Option<String>,
}

/// `MidiInputSelectionResponse`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MidiInputSelectionResponse {
    pub accepted: bool,
    pub message: String,
    pub midi: MidiInputsResponse,
}

/// `MidiCueSimulationRequest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MidiCueSimulationRequest {
    pub cue: MidiCueName,
}

/// `MidiCueSimulationResponse`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MidiCueSimulationResponse {
    pub cue: MidiCueName,
    pub action: ActionName,
    pub accepted: bool,
    pub message: String,
    pub state: ApplicationState,
}

pub fn cue_name_str(cue: MidiCueName) -> &'static str {
    match cue {
        MidiCueName::StartNext => "start_next",
        MidiCueName::RestartCurrent => "restart_current",
        MidiCueName::Previous => "previous",
        MidiCueName::Next => "next",
        MidiCueName::ReloadPlan => "reload_plan",
        MidiCueName::StopTimer => "stop_timer",
    }
}

pub fn cue_from_str(name: &str) -> Option<MidiCueName> {
    [
        MidiCueName::StartNext,
        MidiCueName::RestartCurrent,
        MidiCueName::Previous,
        MidiCueName::Next,
        MidiCueName::ReloadPlan,
        MidiCueName::StopTimer,
    ]
    .into_iter()
    .find(|c| cue_name_str(*c) == name)
}

/// `MidiCueName.action`.
pub fn cue_action(cue: MidiCueName) -> ActionName {
    ActionName::parse(cue_name_str(cue)).unwrap_or(ActionName::StartNext)
}

/// Playback labels MIDI note 0 as C-2. MIDI octave labels are not standardised, so the monitor
/// deliberately follows Playback's UI.
pub fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[(note % 12) as usize],
        i32::from(note / 12) - 2
    )
}

/// SHA-256 hex of the port name: the opaque, stable id the dashboard selects by.
pub fn input_id(name: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(name.as_bytes()))
}
