//! Shared helpers for the integration test crates.

use url::Url;

/// Parse a service URL in a test, panicking with a clear message on failure.
pub fn parse(raw: &str) -> Url {
    Url::parse(raw).expect("test URL should parse")
}
