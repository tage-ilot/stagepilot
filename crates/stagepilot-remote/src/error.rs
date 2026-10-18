//! Error kinds. Python distinguishes `ProviderError` (and its two subclasses), `OSError` and
//! `ValueError`; callers `except` on those groups, so the kinds stay separate here.

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RemoteError {
    /// `ProviderError`: only sanitized operational messages.
    #[error("{0}")]
    Provider(String),
    /// `InstallationRevokedError` (a `ProviderError`): the control plane no longer accepts us.
    #[error("{0}")]
    InstallationRevoked(String),
    /// `InstallationPermanentlyRevokedError` (a `ProviderError`): reactivate returned 401/403.
    #[error("{0}")]
    PermanentlyRevoked(String),
    /// `OSError`.
    #[error("{0}")]
    Io(String),
    /// `ValueError` (validation failures).
    #[error("{0}")]
    Value(String),
}

pub type Res<T> = Result<T, RemoteError>;

impl RemoteError {
    pub fn provider(message: impl Into<String>) -> Self {
        Self::Provider(message.into())
    }

    pub fn value(message: impl Into<String>) -> Self {
        Self::Value(message.into())
    }

    pub fn io(message: impl Into<String>) -> Self {
        Self::Io(message.into())
    }

    /// `isinstance(error, ProviderError)`.
    pub fn is_provider(&self) -> bool {
        matches!(
            self,
            Self::Provider(_) | Self::InstallationRevoked(_) | Self::PermanentlyRevoked(_)
        )
    }

    /// `except (OSError, ValueError)`.
    pub fn is_io_or_value(&self) -> bool {
        matches!(self, Self::Io(_) | Self::Value(_))
    }
}

impl From<std::io::Error> for RemoteError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

impl From<serde_json::Error> for RemoteError {
    fn from(error: serde_json::Error) -> Self {
        Self::Value(error.to_string())
    }
}
