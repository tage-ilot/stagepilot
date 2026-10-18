//! Rust mirror of the Playback API response models
//! (`backend/src/stagepilot/api/playback.py`, `plugins/playback_api/arbiter.py`).
//!
//! The routes in `stagepilot-server` build these and serialise them, so the JSON the server
//! sends and the TypeScript in `generated/ts/` come from one definition. Timestamps are held as
//! the strings Python's pydantic would send. `tests/api_contract.rs` checks field names and the
//! required/optional split against `contract/openapi.json`.

use crate::normalizer::PlaybackEvent;
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct SourceStatus {
    pub connected: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct SourcesStatus {
    pub midi: SourceStatus,
    pub playback_api: SourceStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct PlaybackConnectionResponse {
    pub active_source: String,
    pub connected: bool,
    pub reason: String,
    pub sources: SourcesStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct PlaybackMonitorEntry {
    pub event: PlaybackEvent,
    pub discovery: bool,
    pub at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct PlaybackEventsResponse {
    pub events: Vec<PlaybackMonitorEntry>,
    pub capacity: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct ScanCandidateModel {
    pub name: String,
    pub host: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct ScanResult {
    pub state: String,
    pub candidates: Vec<ScanCandidateModel>,
    pub reason: Option<String>,
    pub current: u32,
    pub total: u32,
    pub phase: Option<String>,
    pub error_class: Option<String>,
    pub typed: bool,
    pub elapsed: f64,
    pub networks: Vec<String>,
    pub hosts_probed: u32,
    pub hosts_total: u32,
    pub details: Option<String>,
    pub settings_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct PlaybackPlanSong {
    pub id: String,
    pub title: String,
    pub duration_seconds: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct PlaybackSettingsRequest {
    pub enabled: bool,
    pub host: Option<String>,
    pub port: u16,
    pub auto_scan: bool,
    pub fast_transport: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct LengthItemResult {
    pub item_id: String,
    pub title: String,
    pub status: String,
    pub old_length: Option<i64>,
    pub new_length: Option<i64>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct LengthUpdateResult {
    pub operation: String,
    pub status: String,
    pub message: String,
    pub items: Vec<LengthItemResult>,
    pub reload: String,
}

impl LengthUpdateResult {
    pub fn idle() -> Self {
        Self {
            operation: "update".into(),
            status: "idle".into(),
            message: String::new(),
            items: vec![],
            reload: "not_needed".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct PlaybackStatusResponse {
    pub planning_center_lengths: LengthUpdateResult,
    pub planning_center_undo_available: bool,
    pub planning_center_connected: bool,
    pub plan_title: Option<String>,
    pub plan_date: Option<String>,
    pub scan: ScanResult,
    pub selected: bool,
    pub enabled: bool,
    pub connected: bool,
    pub active_source: String,
    pub sources: SourcesStatus,
    pub reason: String,
    pub host: Option<String>,
    pub port: u16,
    pub source: Option<String>,
    pub last_error: Option<String>,
    pub playing: bool,
    pub setlist_cloud_version: Option<i64>,
    pub discovery: String,
    pub progress: u32,
    pub discovery_song: u32,
    pub discovery_total: u32,
    pub song_order: Vec<i64>,
    pub song_lengths: Vec<Option<f64>>,
    pub skipped_song_ids: Vec<i64>,
    pub skipped_plan_song_ids: Vec<String>,
    pub arrangement_current: bool,
    pub discovery_duration_seconds: Option<f64>,
    pub planning_center_update_service_type_id: Option<String>,
    pub planning_center_update_reason: Option<String>,
    pub lengths_measured_at: Option<String>,
    pub plan_songs: Vec<PlaybackPlanSong>,
    pub song_count: u32,
    pub plan_song_count: u32,
    pub captured_version: Option<i64>,
    pub captured_at: Option<String>,
    pub stale: bool,
    pub plan_changed: bool,
    pub lengths_applied: bool,
    pub settings: PlaybackSettingsRequest,
}
