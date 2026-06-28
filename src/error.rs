//! Error types for the library.

use thiserror::Error;

/// The single error type returned across the crate.
#[derive(Debug, Error)]
pub enum Error {
    /// The provided string could not be parsed as a URL.
    #[error("failed to parse URL: {0}")]
    UrlParse(#[from] url::ParseError),

    /// No service is registered for the URL scheme (or its feature is disabled).
    #[error("unsupported service scheme: {0}")]
    UnsupportedScheme(String),

    /// The URL was valid but did not satisfy the service's configuration requirements.
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    /// A configuration key was not recognized by the service.
    ///
    /// Surfaced as an error when parsing a URL (unknown query keys are rejected), but ignored
    /// when applying per-message [`Params`](crate::Params) overlays, where foreign keys (e.g.
    /// another service's keys during a fan-out) are expected and skipped.
    #[error("unknown configuration key: {0}")]
    UnknownConfigKey(String),

    /// A credential/token embedded in the URL was malformed.
    #[error("invalid token: {0}")]
    InvalidToken(String),

    /// The underlying HTTP transport failed (DNS, TLS, connection, etc.).
    #[error("transport error: {0}")]
    Transport(String),

    /// The remote endpoint returned a non-success HTTP status.
    #[error("HTTP {status}: {body}")]
    Http {
        /// HTTP status code.
        status: u16,
        /// Response body (best-effort, lossy UTF-8).
        body: String,
    },

    /// The remote endpoint returned a success status but an application-level error.
    #[error("service returned an error: {0}")]
    ServiceResponse(String),

    /// (De)serialization of a request or response payload failed.
    #[error("serialization error: {0}")]
    Serialization(String),
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Serialization(err.to_string())
    }
}

/// Convenience alias for `Result<T, shoutrrr::Error>`.
pub type Result<T> = std::result::Result<T, Error>;
