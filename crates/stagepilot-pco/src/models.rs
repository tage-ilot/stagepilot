//! Planning Center JSON:API transport models, domain models and API response models: port of
//! `plugins/planning_center/models.py` plus the Planning Center parts of `models/api.py`.
//!
//! Pydantic's field constraints (`min_length`, `max_length`, `pattern`, `ge`) are enforced by the
//! `valid()` methods, because serde alone cannot express them. A response that fails them is a
//! [`crate::errors::PcoError::Response`], exactly like Python's `ValidationError` translation.

use chrono::{DateTime, FixedOffset, NaiveDate};
use serde::{Deserialize, Serialize};
use stagepilot_core::config::ConnectionMethod;
use stagepilot_core::state::{ConnectionStatus, ServicePlan};
use ts_rs::TS;

/// Characters pydantic counts for `max_length` / `min_length`.
pub fn chars(value: &str) -> usize {
    value.chars().count()
}

/// `^[A-Za-z0-9_-]+$` (the resource-id pattern) combined with a length bound.
pub fn is_safe_identifier(value: &str, max: usize) -> bool {
    let n = chars(value);
    (1..=max).contains(&n)
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

// ---- secrets -------------------------------------------------------------------------------------

/// A credential that never prints: `Debug` and `Display` are redacted (`SecretStr`).
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Secret(value.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(**********)")
    }
}

// ---- runtime settings ------------------------------------------------------------------------------

/// `StagePilot/<version> (<project url>)`, with the build-injected application version.
pub fn default_user_agent() -> String {
    format!(
        "StagePilot/{} (https://github.com/tage-ilot/stagepilot)",
        stagepilot_core::settings::APP_VERSION
    )
}

/// `PlanningCenterSettings`: the runtime settings the client and plugin work from.
#[derive(Clone, Debug, PartialEq)]
pub struct PcoSettings {
    pub app_id: Option<Secret>,
    pub secret: Option<Secret>,
    pub connection_method: ConnectionMethod,
    pub access_token: Option<Secret>,
    pub service_type_id: Option<String>,
    pub plan_title_preference: Option<String>,
    pub preferred_service_time: Option<String>,
    pub upcoming_lookahead_days: u16,
    pub request_timeout_seconds: f64,
    pub user_agent: String,
}

impl Default for PcoSettings {
    fn default() -> Self {
        Self {
            app_id: None,
            secret: None,
            connection_method: ConnectionMethod::Manual,
            access_token: None,
            service_type_id: None,
            plan_title_preference: None,
            preferred_service_time: None,
            upcoming_lookahead_days: 30,
            request_timeout_seconds: 10.0,
            user_agent: default_user_agent(),
        }
    }
}

impl PcoSettings {
    /// A credential-presence flag that does not reveal either value.
    pub fn is_configured(&self) -> bool {
        match self.connection_method {
            ConnectionMethod::Oauth => self.access_token.is_some(),
            ConnectionMethod::Manual => self.app_id.is_some() && self.secret.is_some(),
        }
    }
}

// ---- JSON:API transport models --------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct NextPage {
    pub offset: u64,
}

#[derive(Debug, Default, Deserialize)]
pub struct PaginationMeta {
    #[serde(default)]
    pub total_count: Option<u64>,
    #[serde(default)]
    pub count: Option<u64>,
    #[serde(default)]
    pub next: Option<NextPage>,
}

#[derive(Debug, Default, Deserialize)]
pub struct CollectionLinks {
    #[serde(default)]
    pub next: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CollectionDocument<T> {
    pub data: Vec<T>,
    #[serde(default)]
    pub meta: PaginationMeta,
    #[serde(default)]
    pub links: CollectionLinks,
}

/// A transport resource whose constraints can be checked after deserialisation.
pub trait Resource {
    fn resource_id(&self) -> &str;
    fn valid(&self) -> bool;
}

#[derive(Debug, Deserialize)]
pub struct ServiceTypeAttributes {
    pub name: String,
    #[serde(default)]
    pub sequence: i64,
    #[serde(default)]
    pub archived_at: Option<DateTime<FixedOffset>>,
    #[serde(default)]
    pub deleted_at: Option<DateTime<FixedOffset>>,
}

#[derive(Debug, Deserialize)]
pub struct ServiceTypeResource {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub attributes: ServiceTypeAttributes,
}

impl Resource for ServiceTypeResource {
    fn resource_id(&self) -> &str {
        &self.id
    }
    fn valid(&self) -> bool {
        self.kind == "ServiceType"
            && is_safe_identifier(&self.id, 128)
            && (1..=500).contains(&chars(&self.attributes.name))
            && self.attributes.sequence >= 0
    }
}

#[derive(Debug, Deserialize)]
pub struct PlanAttributes {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub dates: String,
}

#[derive(Debug, Deserialize)]
pub struct PlanResource {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub attributes: PlanAttributes,
}

impl Resource for PlanResource {
    fn resource_id(&self) -> &str {
        &self.id
    }
    fn valid(&self) -> bool {
        self.kind == "Plan"
            && is_safe_identifier(&self.id, 128)
            && chars(&self.attributes.title) <= 500
            && chars(&self.attributes.dates) <= 500
    }
}

#[derive(Debug, Deserialize)]
pub struct PlanTimeAttributes {
    pub starts_at: DateTime<FixedOffset>,
    #[serde(default)]
    pub ends_at: Option<DateTime<FixedOffset>>,
    #[serde(default)]
    pub name: String,
    pub time_type: String,
}

#[derive(Debug, Deserialize)]
pub struct PlanTimeResource {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub attributes: PlanTimeAttributes,
}

impl Resource for PlanTimeResource {
    fn resource_id(&self) -> &str {
        &self.id
    }
    fn valid(&self) -> bool {
        self.kind == "PlanTime"
            && (1..=128).contains(&chars(&self.id))
            && chars(&self.attributes.name) <= 500
            && (1..=32).contains(&chars(&self.attributes.time_type))
    }
}

#[derive(Debug, Deserialize)]
pub struct SongIdentifier {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct SongRelationship {
    #[serde(default)]
    pub data: Option<SongIdentifier>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ItemRelationships {
    #[serde(default)]
    pub song: SongRelationship,
}

#[derive(Debug, Deserialize)]
pub struct ItemAttributes {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    pub item_type: String,
    #[serde(default)]
    pub length: Option<i64>,
    pub sequence: i64,
    #[serde(default)]
    pub service_position: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ItemResource {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub attributes: ItemAttributes,
    #[serde(default)]
    pub relationships: ItemRelationships,
}

impl Resource for ItemResource {
    fn resource_id(&self) -> &str {
        &self.id
    }
    fn valid(&self) -> bool {
        let a = &self.attributes;
        self.kind == "Item"
            && is_safe_identifier(&self.id, 128)
            && chars(&a.title) <= 500
            && a.description.as_deref().is_none_or(|d| chars(d) <= 10_000)
            && (1..=32).contains(&chars(&a.item_type))
            && a.length.is_none_or(|l| l >= 0)
            && a.sequence >= 0
            && a.service_position.as_deref().is_none_or(|p| chars(p) <= 32)
            && self
                .relationships
                .song
                .data
                .as_ref()
                .is_none_or(|s| s.kind == "Song" && (1..=128).contains(&chars(&s.id)))
    }
}

/// The single-resource document `{"data": {...}}` returned by `GET/PATCH .../items/{id}`.
#[derive(Debug, Deserialize)]
pub struct ItemDocument {
    pub data: ItemResource,
}

// ---- domain models -------------------------------------------------------------------------------

/// `PlanningCenterServiceType`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceType {
    pub id: String,
    pub name: String,
    pub sequence: u32,
    #[serde(default)]
    pub archived: bool,
}

/// `PlanningCenterPlanCandidate`: a plan whose service times match the nearest service date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanCandidate {
    pub id: String,
    pub title: String,
    pub service_type_id: String,
    pub service_type_name: String,
    pub target_date: NaiveDate,
    /// Service times in the configured zone (their offset is kept), earliest first.
    pub service_times: Vec<DateTime<FixedOffset>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkippedItemReason {
    Header,
    Media,
    NotSong,
    MissingTitle,
}

impl SkippedItemReason {
    pub fn as_str(self) -> &'static str {
        match self {
            SkippedItemReason::Header => "header",
            SkippedItemReason::Media => "media",
            SkippedItemReason::NotSong => "not_song",
            SkippedItemReason::MissingTitle => "missing_title",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedPlanItem {
    pub item_id: String,
    pub title: String,
    pub description: Option<String>,
    pub item_type: String,
    pub sequence: u32,
    pub duration_seconds: Option<u32>,
    pub reason: SkippedItemReason,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum PlanDiscoveryResult {
    NotFound {
        service_type: Option<ServiceType>,
        target_date: NaiveDate,
    },
    Ambiguous {
        service_type: Option<ServiceType>,
        target_date: NaiveDate,
        candidates: Vec<PlanCandidate>,
    },
    Loaded {
        candidate: PlanCandidate,
        plan: ServicePlan,
        skipped_items: Vec<SkippedPlanItem>,
    },
}

// ---- API request / response models (TypeScript is generated from these) ---------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlanningCenterStatusResponse {
    pub connection_status: ConnectionStatus,
    pub configured: bool,
    pub app_id: Option<String>,
    pub service_type_id: Option<String>,
    pub planning_center_secret_saved: bool,
    pub connection_method: ConnectionMethod,
    pub oauth_connected: bool,
    pub oauth_needs_reconnect: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlanningCenterOAuthStartResponse {
    pub authorize_url: String,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlanningCenterOAuthCallbackRequest {
    pub state: String,
    pub code: String,
    pub redirect_uri: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlanningCenterOAuthStatusResponse {
    pub connection_method: ConnectionMethod,
    pub connected: bool,
    pub needs_reconnect: bool,
    pub expires_at: Option<f64>,
    pub scope: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlanningCenterServiceTypeResponse {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlanningCenterTestResponse {
    pub authenticated: bool,
    pub message: String,
    pub service_types: Vec<PlanningCenterServiceTypeResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlanSelectionRequest {
    pub plan_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PendingPlanSelectionResponse {
    pub pending: bool,
    pub target_date: Option<String>,
    pub candidates: Vec<stagepilot_core::state::ServicePlanCandidate>,
    pub message: Option<String>,
}
