//! Observable snapshot and route models for the ProPresenter integration (`core/propresenter.py`,
//! `models/api.py`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use stagepilot_core::state::ConnectionStatus;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProPresenterTimerResponse {
    pub id: String,
    pub name: String,
    pub index: u32,
    pub is_countdown: bool,
    pub state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProPresenterLookResponse {
    pub id: String,
    pub name: String,
    pub index: u32,
}

/// `ProPresenterStatusResponse` (the snapshot has identical fields).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProPresenterStatusResponse {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub timer_name: String,
    pub look_id: Option<String>,
    pub request_timeout_seconds: f64,
    pub connection_status: ConnectionStatus,
    pub detail: Option<String>,
    pub timers: Vec<ProPresenterTimerResponse>,
    pub selected_timer_id: Option<String>,
    pub timer_found: bool,
    pub looks: Vec<ProPresenterLookResponse>,
    pub current_look_id: Option<String>,
    pub look_found: bool,
    pub last_checked_at: Option<DateTime<Utc>>,
}

/// `ProPresenterSettingsRequest`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProPresenterSettingsRequest {
    pub host: String,
    pub port: u16,
    pub timer_name: String,
    #[serde(default)]
    pub look_id: Option<String>,
    pub request_timeout_seconds: f64,
}

/// `ProPresenterOperationResponse`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProPresenterOperationResponse {
    pub accepted: bool,
    pub message: String,
    pub propresenter: ProPresenterStatusResponse,
}
