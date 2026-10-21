//! Observable application state models: port of `backend/src/stagepilot/models/state.py`.
//!
//! JSON field names and enum values are identical to the Python (pydantic) models. Constraints that
//! pydantic enforces at construction (`ge`, `min_length`, the `ServiceLoadState` consistency rules)
//! are enforced by the `validate`/`new` functions here, because serde alone cannot express them.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ApplicationStatus {
    #[default]
    Starting,
    Running,
    Stopping,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ConnectionStatus {
    Connecting,
    Connected,
    #[default]
    Disconnected,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TimerStatus {
    #[default]
    Idle,
    Running,
    Stopped,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PluginStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ServiceLoadStatus {
    #[default]
    Idle,
    Loading,
    Loaded,
    NotFound,
    Ambiguous,
    Error,
}

/// A validation failure with a pydantic-style location and message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{loc}: {msg}")]
pub struct ValidationError {
    pub loc: String,
    pub msg: String,
}

impl ValidationError {
    pub fn new(loc: impl Into<String>, msg: impl Into<String>) -> Self {
        Self {
            loc: loc.into(),
            msg: msg.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Song {
    pub id: String,
    pub title: String,
    pub duration_seconds: Option<u32>,
    pub order: u32,
    #[serde(default)]
    pub service_sequence: Option<u32>,
    #[serde(default)]
    pub is_generic: bool,
    #[serde(default)]
    pub source_song_id: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Computed from `duration_seconds` (`m:ss`, or `h:mm:ss`); present on every serialised song.
    #[serde(default)]
    pub formatted_duration: Option<String>,
}

pub fn format_duration(duration_seconds: u32) -> String {
    let (hours, remainder) = (duration_seconds / 3600, duration_seconds % 3600);
    let (minutes, seconds) = (remainder / 60, remainder % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

impl Song {
    pub fn new(id: &str, title: &str, duration_seconds: Option<u32>, order: u32) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            duration_seconds,
            order,
            service_sequence: None,
            is_generic: false,
            source_song_id: None,
            description: None,
            formatted_duration: duration_seconds.map(format_duration),
        }
    }

    /// Enforce the pydantic field constraints and refresh the computed field.
    pub fn validated(mut self) -> Result<Self, ValidationError> {
        if self.title.is_empty() {
            return Err(ValidationError::new(
                "title",
                "String should have at least 1 character",
            ));
        }
        if self.order < 1 {
            return Err(ValidationError::new(
                "order",
                "Input should be greater than or equal to 1",
            ));
        }
        if self
            .description
            .as_deref()
            .is_some_and(|d| d.len() > 10_000)
        {
            return Err(ValidationError::new(
                "description",
                "String should have at most 10000 characters",
            ));
        }
        self.formatted_duration = self.duration_seconds.map(format_duration);
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ServicePlan {
    pub id: String,
    pub title: String,
    #[ts(as = "String")]
    pub date: NaiveDate,
    pub service_type: String,
    #[serde(default)]
    pub service_type_id: Option<String>,
    #[serde(default)]
    pub service_times: Vec<String>,
    #[serde(default = "default_duration_source")]
    pub duration_source: String,
    #[serde(default)]
    pub songs: Vec<Song>,
}

fn default_duration_source() -> String {
    "Scheduled item length".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ServicePlanCandidate {
    pub id: String,
    pub title: String,
    pub service_type_id: String,
    pub service_type_name: String,
    #[ts(as = "String")]
    pub target_date: NaiveDate,
    pub service_times: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SkippedServiceItem {
    pub item_id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    pub item_type: String,
    pub sequence: u32,
    #[serde(default)]
    pub duration_seconds: Option<u32>,
    pub reason: String,
}

/// Validated projection of the current plan-loading outcome.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ServiceLoadState {
    #[serde(default)]
    pub status: ServiceLoadStatus,
    #[serde(default)]
    #[ts(as = "Option<String>")]
    pub target_date: Option<NaiveDate>,
    #[serde(default)]
    pub candidates: Vec<ServicePlanCandidate>,
    #[serde(default)]
    pub skipped_items: Vec<SkippedServiceItem>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub is_stale: bool,
    #[serde(default)]
    pub last_attempt_at: Option<DateTime<Utc>>,
}

impl ServiceLoadState {
    /// The `model_validator(mode="after")` of the Python model, with the same messages.
    pub fn validate(&self) -> Result<(), String> {
        use ServiceLoadStatus::*;
        if self.status == Idle {
            if self.target_date.is_some() {
                return Err("An idle service-load state cannot have a target date.".into());
            }
        } else if self.target_date.is_none() {
            return Err("A non-idle service-load state requires a target date.".into());
        }
        if self.status == Ambiguous {
            if self.candidates.len() < 2 {
                return Err(
                    "An ambiguous service-load state requires at least two candidates.".into(),
                );
            }
            if self
                .candidates
                .iter()
                .any(|c| Some(c.target_date) != self.target_date)
            {
                return Err(
                    "Every service-plan candidate must match the service-load target date.".into(),
                );
            }
        } else if !self.candidates.is_empty() {
            return Err(
                "Service-plan candidates are only valid for an ambiguous load state.".into(),
            );
        }
        if self.is_stale && !matches!(self.status, Loading | NotFound | Ambiguous | Error) {
            return Err(
                "An idle or successfully loaded service cannot retain a stale plan.".into(),
            );
        }
        Ok(())
    }

    pub fn validated(self) -> Result<Self, String> {
        self.validate()?;
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TimerState {
    #[serde(default)]
    pub status: TimerStatus,
    #[serde(default)]
    pub duration_seconds: Option<u32>,
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,
    /// Position in the song (seconds) when the countdown was last started or frozen.
    #[serde(default)]
    pub position_seconds: Option<f64>,
    /// Seconds left on the countdown at the moment it was stopped.
    #[serde(default)]
    pub remaining_seconds: Option<u32>,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PluginHealth {
    pub name: String,
    pub version: String,
    pub status: PluginStatus,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub last_activity_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EventSummary {
    #[ts(as = "String")]
    pub id: Uuid,
    #[serde(rename = "type")]
    #[ts(rename = "type")]
    pub event_type: String,
    pub timestamp: DateTime<Utc>,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ErrorSummary {
    pub timestamp: DateTime<Utc>,
    pub component: String,
    pub message: String,
    #[serde(default)]
    #[ts(as = "Option<String>")]
    pub event_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ApplicationState {
    pub revision: u64,
    pub updated_at: DateTime<Utc>,
    pub application_status: ApplicationStatus,
    pub plan: Option<ServicePlan>,
    /// The navigation CURSOR: Previous / Next move it without starting anything.
    pub current_song: Option<Song>,
    pub next_song: Option<Song>,
    pub current_song_index: Option<usize>,
    /// What the screen says: the song actually started, and the song Start next will play.
    pub playing_song: Option<Song>,
    pub playing_song_index: Option<usize>,
    pub up_next_song: Option<Song>,
    pub planning_center_status: ConnectionStatus,
    pub midi_status: ConnectionStatus,
    pub propresenter_status: ConnectionStatus,
    pub lights_status: ConnectionStatus,
    pub service_load: ServiceLoadState,
    pub timer: TimerState,
    pub plugins: std::collections::BTreeMap<String, PluginHealth>,
    pub recent_events: Vec<EventSummary>,
    pub recent_errors: Vec<ErrorSummary>,
    pub last_successful_plan_reload_at: Option<DateTime<Utc>>,
    pub last_action: Option<String>,
}

impl Default for ApplicationState {
    fn default() -> Self {
        Self {
            revision: 0,
            updated_at: Utc::now(),
            application_status: ApplicationStatus::Starting,
            plan: None,
            current_song: None,
            next_song: None,
            current_song_index: None,
            playing_song: None,
            playing_song_index: None,
            up_next_song: None,
            planning_center_status: ConnectionStatus::Disconnected,
            midi_status: ConnectionStatus::Disconnected,
            propresenter_status: ConnectionStatus::Disconnected,
            lights_status: ConnectionStatus::Disconnected,
            service_load: ServiceLoadState::default(),
            timer: TimerState::default(),
            plugins: Default::default(),
            recent_events: Vec::new(),
            recent_errors: Vec::new(),
            last_successful_plan_reload_at: None,
            last_action: None,
        }
    }
}
