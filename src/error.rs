//! Error types for the library.

use thiserror::Error;

/// The single error type returned across the crate.
///
/// New variants can be added without a major version bump, so a `match` on it needs a `_` arm.
#[derive(Debug, Error)]
#[non_exhaustive]
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

    /// The server answered with a redirect (`301`, `302`, `303`, `307` or `308`) and the transport
    /// was built with [`RedirectPolicy::Reject`](crate::transport::RedirectPolicy::Reject), so the
    /// request wasn't sent on.
    #[error(
        "redirect not followed: HTTP {status} to {} (the transport rejects redirects)",
        .location.as_deref().unwrap_or("no Location header")
    )]
    RedirectRejected {
        /// The redirect status code.
        status: u16,
        /// The response's `Location` header as sent, which may be a relative URL, or `None` if it
        /// had none.
        location: Option<String>,
    },

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
