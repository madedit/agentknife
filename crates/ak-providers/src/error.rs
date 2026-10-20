use thiserror::Error;

/// Errors from any provider. The agent maps these to plain-language messages.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProviderError {
    #[error("network error: {0}")]
    Network(String),
    #[error("authentication failed (check the API key)")]
    Auth,
    #[error("rate limited")]
    RateLimited { retry_after_secs: Option<u64> },
    #[error("model not found: {0}")]
    ModelNotFound(String),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("provider error ({status}): {message}")]
    Server { status: u16, message: String },
    #[error("could not decode provider response: {0}")]
    Decode(String),
    #[error("response ended before completion")]
    Incomplete,
    #[error("transport: {0}")]
    Transport(String),
}

impl ProviderError {
    /// Whether retrying the same request later may succeed.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Network(_) | Self::RateLimited { .. } | Self::Server { .. } | Self::Incomplete
        )
    }

    /// Maps a non-success HTTP status and body to an error.
    pub fn from_status(status: u16, retry_after: Option<&str>, body: &str) -> Self {
        let message = body.chars().take(500).collect::<String>();
        match status {
            401 | 403 => Self::Auth,
            404 => Self::ModelNotFound(message),
            429 => Self::RateLimited {
                retry_after_secs: retry_after.and_then(|v| v.trim().parse().ok()),
            },
            400 | 409 | 413 | 422 => Self::InvalidRequest(message),
            _ => Self::Server { status, message },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_mapping() {
        assert_eq!(
            ProviderError::from_status(401, None, ""),
            ProviderError::Auth
        );
        assert_eq!(
            ProviderError::from_status(429, Some("12"), ""),
            ProviderError::RateLimited {
                retry_after_secs: Some(12)
            }
        );
        assert!(matches!(
            ProviderError::from_status(400, None, "bad"),
            ProviderError::InvalidRequest(m) if m == "bad"
        ));
        assert!(ProviderError::from_status(529, None, "overloaded").is_retryable());
        assert!(!ProviderError::from_status(401, None, "").is_retryable());
    }
}
