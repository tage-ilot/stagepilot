//! Errors raised by the ProPresenter integration boundary (`errors.py`).

/// The Python hierarchy is `ProPresenterError` > {Connection, Response, TimerNotFound, TimerType,
/// LookNotFound}; `ValueError` is a separate family. [`PpError::is_propresenter_error`] is the
/// `except ProPresenterError` test that drives the stale-timer retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PpError {
    /// The API cannot be reached.
    Connection(String),
    /// ProPresenter rejected the request or returned something unusable.
    Response(String),
    /// The configured timer cannot be found uniquely.
    TimerNotFound(String),
    /// The configured timer is not a countdown timer.
    TimerType(String),
    /// A saved audience Look cannot be found or verified.
    LookNotFound(String),
    /// Python `ValueError`/`RuntimeError`: a caller or state problem, message is public.
    Value(String),
}

impl PpError {
    pub fn message(&self) -> &str {
        match self {
            PpError::Connection(m)
            | PpError::Response(m)
            | PpError::TimerNotFound(m)
            | PpError::TimerType(m)
            | PpError::LookNotFound(m)
            | PpError::Value(m) => m,
        }
    }

    pub fn is_propresenter_error(&self) -> bool {
        !matches!(self, PpError::Value(_))
    }

    pub fn is_connection(&self) -> bool {
        matches!(self, PpError::Connection(_))
    }
}

impl std::fmt::Display for PpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for PpError {}
