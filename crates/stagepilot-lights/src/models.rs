//! Lighting API models: `core/lights.py` and the Lighting parts of `models/api.py`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use stagepilot_core::config::{
    LightingCue, LightingMethod, LightingPad, OscArgument, OscLightingCue,
};
use stagepilot_core::state::ConnectionStatus;
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightingOutputResponse {
    pub name: String,
    pub ambiguous: bool,
    pub selected: bool,
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightsStatusResponse {
    pub enabled: bool,
    pub method: LightingMethod,
    pub osc_host: String,
    pub osc_port: u16,
    pub output_name: Option<String>,
    pub channel: u8,
    pub pulse_ms: u16,
    pub connection_status: ConnectionStatus,
    pub detail: Option<String>,
    pub outputs: Vec<LightingOutputResponse>,
    pub last_cue: Option<LightingCue>,
    pub last_osc_cue: Option<OscLightingCue>,
    pub last_cue_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightsOperationResponse {
    pub accepted: bool,
    pub message: String,
    pub lights: LightsStatusResponse,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightingCaptureResponse {
    pub accepted: bool,
    pub message: String,
    pub lights: LightsStatusResponse,
    #[ts(as = "Option<String>")]
    pub cue_id: Option<Uuid>,
    pub at_seconds: Option<f64>,
}

/// `LightsSettingsRequest`. A request without a method is a pre-OSC client and means MIDI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightsSettingsRequest {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "midi")]
    pub method: LightingMethod,
    #[serde(default)]
    pub osc_host: Option<String>,
    #[serde(default)]
    pub osc_port: Option<u16>,
    #[serde(default)]
    pub output_name: Option<String>,
    #[serde(default = "one")]
    pub channel: u8,
    #[serde(default = "hundred")]
    pub pulse_ms: u16,
}

fn yes() -> bool {
    true
}
fn midi() -> LightingMethod {
    LightingMethod::Midi
}
fn one() -> u8 {
    1
}
fn hundred() -> u16 {
    100
}
fn v127() -> u8 {
    127
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightingCueTestRequest {
    pub note: u8,
    #[serde(default = "v127")]
    pub velocity: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightingOscCueTestRequest {
    pub address: String,
    #[serde(default)]
    pub args: Vec<OscArgument>,
}

/// `LightingCueMapRequest`: a song cue map that replaces only `method`'s cue list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightingCueMapRequest {
    pub song_key: String,
    pub song_title: String,
    #[serde(default)]
    pub cues: Vec<LightingCue>,
    #[serde(default)]
    pub osc_cues: Vec<OscLightingCue>,
    #[serde(default = "midi")]
    pub method: LightingMethod,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightingPadsRequest {
    #[serde(default)]
    pub pads: Vec<LightingPad>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightingCaptureRequest {
    #[ts(as = "String")]
    pub pad_id: Uuid,
    #[serde(default)]
    pub song_key: Option<String>,
    #[serde(default)]
    pub song_title: Option<String>,
    #[serde(default)]
    pub at_seconds: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LightingCaptureUndoRequest {
    pub song_key: String,
    #[ts(as = "String")]
    pub cue_id: Uuid,
}
