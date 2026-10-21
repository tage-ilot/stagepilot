//! Typed events exchanged by services and plugins: port of `backend/src/stagepilot/core/events.py`.

use crate::state::{
    ConnectionStatus, PluginStatus, ServiceLoadStatus, ServicePlan, ServicePlanCandidate,
    SkippedServiceItem, Song,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum EventType {
    #[serde(rename = "application.started")]
    ApplicationStarted,
    #[serde(rename = "application.stopping")]
    ApplicationStopping,
    #[serde(rename = "action.requested")]
    ActionRequested,
    #[serde(rename = "service.reload_requested")]
    ServiceReloadRequested,
    #[serde(rename = "service.plan_selection_requested")]
    ServicePlanSelectionRequested,
    #[serde(rename = "service.load_changed")]
    ServiceLoadChanged,
    #[serde(rename = "service.loaded")]
    ServiceLoaded,
    #[serde(rename = "song.selected")]
    SongSelected,
    #[serde(rename = "song.started")]
    SongStarted,
    #[serde(rename = "song.restarted")]
    SongRestarted,
    #[serde(rename = "midi.note_received")]
    MidiNoteReceived,
    #[serde(rename = "timer.stop_requested")]
    TimerStopRequested,
    #[serde(rename = "timer.reset_requested")]
    TimerResetRequested,
    #[serde(rename = "timer.started")]
    TimerStarted,
    #[serde(rename = "timer.resumed")]
    TimerResumed,
    #[serde(rename = "timer.seeked")]
    TimerSeeked,
    #[serde(rename = "timer.stopped")]
    TimerStopped,
    #[serde(rename = "timer.failed")]
    TimerFailed,
    #[serde(rename = "connection.changed")]
    ConnectionChanged,
    #[serde(rename = "plugin.status_changed")]
    PluginStatusChanged,
    #[serde(rename = "plugin.failed")]
    PluginFailed,
}

impl EventType {
    /// The dotted wire name (`"timer.started"`), identical to the Python `StrEnum` value.
    pub fn as_str(self) -> String {
        match serde_json::to_value(self) {
            Ok(serde_json::Value::String(s)) => s,
            _ => String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ActionName {
    StartNext,
    RestartCurrent,
    Previous,
    Next,
    StopTimer,
    ReloadPlan,
    ResetPosition,
}

impl ActionName {
    pub const ALL: [ActionName; 7] = [
        ActionName::StartNext,
        ActionName::RestartCurrent,
        ActionName::Previous,
        ActionName::Next,
        ActionName::StopTimer,
        ActionName::ReloadPlan,
        ActionName::ResetPosition,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ActionName::StartNext => "start_next",
            ActionName::RestartCurrent => "restart_current",
            ActionName::Previous => "previous",
            ActionName::Next => "next",
            ActionName::StopTimer => "stop_timer",
            ActionName::ReloadPlan => "reload_plan",
            ActionName::ResetPosition => "reset_position",
        }
    }

    pub fn parse(value: &str) -> Option<ActionName> {
        Self::ALL.into_iter().find(|a| a.as_str() == value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Integration {
    PlanningCenter,
    Midi,
    Propresenter,
    Lights,
}

impl Integration {
    pub fn as_str(self) -> &'static str {
        match self {
            Integration::PlanningCenter => "planning_center",
            Integration::Midi => "midi",
            Integration::Propresenter => "propresenter",
            Integration::Lights => "lights",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum EventPayload {
    Empty,
    Action {
        action: ActionName,
        #[ts(as = "String")]
        request_id: Uuid,
    },
    Service {
        plan: ServicePlan,
    },
    ServicePlanSelection {
        plan_id: String,
    },
    ServiceLoad {
        status: ServiceLoadStatus,
        #[serde(default)]
        #[ts(as = "Option<String>")]
        target_date: Option<NaiveDate>,
        #[serde(default)]
        candidates: Vec<ServicePlanCandidate>,
        #[serde(default)]
        skipped_items: Vec<SkippedServiceItem>,
        #[serde(default)]
        message: Option<String>,
        #[serde(default)]
        is_stale: bool,
    },
    Song {
        song: Song,
        index: usize,
    },
    MidiNote {
        channel: u8,
        note: u8,
        velocity: u8,
        action: ActionName,
        #[serde(default)]
        song_position: Option<u8>,
        #[serde(default)]
        #[ts(as = "Option<String>")]
        connection_id: Option<Uuid>,
        #[serde(default)]
        simulated: bool,
    },
    Timer(TimerPayload),
    Connection {
        integration: Integration,
        status: ConnectionStatus,
        #[serde(default)]
        detail: Option<String>,
    },
    Plugin {
        name: String,
        version: String,
        status: PluginStatus,
        #[serde(default)]
        error: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TimerPayload {
    pub duration_seconds: u32,
    #[serde(default)]
    pub song: Option<Song>,
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub message: Option<String>,
    /// Song position (seconds) the countdown starts from (resumed/seeked) or was frozen at.
    #[serde(default)]
    pub position_seconds: Option<f64>,
    #[serde(default)]
    pub remaining_seconds: Option<u32>,
    /// `timer.seeked` only: whether the countdown keeps running after the seek.
    #[serde(default)]
    pub running: bool,
}

/// Immutable event envelope with a typed, discriminated payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StagePilotEvent {
    #[ts(as = "String")]
    pub id: Uuid,
    #[serde(rename = "type")]
    #[ts(rename = "type")]
    pub event_type: EventType,
    pub timestamp: DateTime<Utc>,
    pub source: String,
    pub payload: EventPayload,
}

pub fn new_event(event_type: EventType, source: &str, payload: EventPayload) -> StagePilotEvent {
    StagePilotEvent {
        id: Uuid::new_v4(),
        event_type,
        timestamp: Utc::now(),
        source: source.to_string(),
        payload,
    }
}

pub fn empty_event(event_type: EventType, source: &str) -> StagePilotEvent {
    new_event(event_type, source, EventPayload::Empty)
}
