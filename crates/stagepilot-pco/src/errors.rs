//! Safe, typed failures raised by the Planning Center API boundary: port of
//! `plugins/planning_center/errors.py`.
//!
//! The `Display` text of every variant is exactly the message Python surfaces (never a response
//! body, URL, header or credential), because the routes return it as the HTTP `detail`.

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PcoError {
    /// Required configuration is absent or invalid (HTTP 409).
    #[error("{0}")]
    Configuration(String),
    /// Planning Center rejected the configured credentials (HTTP 401).
    #[error("{0}")]
    Authentication(String),
    /// The authenticated user lacks access to the resource (HTTP 403).
    #[error("{0}")]
    Permission(String),
    #[error("{0}")]
    Timeout(String),
    #[error("{0}")]
    Transport(String),
    /// Planning Center returned data that did not match the documented contract.
    #[error("{0}")]
    Response(String),
    /// A requested plan is not one of the nearest-date candidates.
    #[error("{0}")]
    PlanSelection(String),
    /// The item changed while a rate-limited write was waiting.
    #[error("{0}")]
    LengthConflict(String),
    /// Planning Center returned an unsuccessful HTTP response.
    #[error("Planning Center returned HTTP {status}.")]
    Api { status: u16 },
    /// Planning Center asked the client to pause before making more requests (HTTP 429).
    #[error("Planning Center returned HTTP 429.")]
    RateLimit { retry_after_seconds: Option<u64> },
    /// The base `PlanningCenterError`.
    #[error("{0}")]
    Generic(String),
    /// A Python-side `pydantic.ValidationError` / bug (not a `PlanningCenterError`).
    #[error("{0}")]
    Unexpected(String),
}

impl PcoError {
    pub fn configuration(message: &str) -> Self {
        PcoError::Configuration(message.to_string())
    }

    pub fn response(message: impl Into<String>) -> Self {
        PcoError::Response(message.into())
    }

    /// The HTTP status carried by an API error, if any (`PlanningCenterApiError.status_code`).
    pub fn http_status(&self) -> Option<u16> {
        match self {
            PcoError::Api { status } => Some(*status),
            PcoError::RateLimit { .. } => Some(429),
            _ => None,
        }
    }

    /// `isinstance(exc, PlanningCenterError)`: everything except an unexpected failure.
    pub fn is_planning_center_error(&self) -> bool {
        !matches!(self, PcoError::Unexpected(_))
    }

    /// The type name Python logs as `exception_type` (never a message).
    pub fn type_name(&self) -> &'static str {
        match self {
            PcoError::Configuration(_) => "PlanningCenterConfigurationError",
            PcoError::Authentication(_) => "PlanningCenterAuthenticationError",
            PcoError::Permission(_) => "PlanningCenterPermissionError",
            PcoError::Timeout(_) => "PlanningCenterTimeoutError",
            PcoError::Transport(_) => "PlanningCenterTransportError",
            PcoError::Response(_) => "PlanningCenterResponseError",
            PcoError::PlanSelection(_) => "PlanningCenterPlanSelectionError",
            PcoError::LengthConflict(_) => "PlanningCenterLengthConflictError",
            PcoError::Api { .. } => "PlanningCenterApiError",
            PcoError::RateLimit { .. } => "PlanningCenterRateLimitError",
            PcoError::Generic(_) => "PlanningCenterError",
            PcoError::Unexpected(_) => "Exception",
        }
    }
}
