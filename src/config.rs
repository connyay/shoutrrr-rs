//! Service configuration parsing.
//!
//! Where Go shoutrrr derives config from a URL via struct-tag reflection, this port uses an
//! explicit [`ServiceConfig`] implementation per service. Each impl owns the mapping between URL
//! parts/query params and its fields.

#[cfg(any(feature = "slack", feature = "discord", feature = "generic"))]
use std::collections::BTreeMap;

use url::Url;

use crate::error::{Error, Result};
use crate::params::Params;

/// A service's configuration, convertible to/from its URL representation.
pub trait ServiceConfig: Sized {
    /// The URL scheme this config is associated with (e.g. `"slack"`).
    fn scheme() -> &'static str;

    /// Parse a config from a service URL.
    fn from_url(url: &Url) -> Result<Self>;

    /// Render the config back to its canonical service URL (round-trips with [`from_url`]).
    ///
    /// Returns [`Error::InvalidConfig`] if the fields can't form a valid
    /// URL — e.g. a host or token holding URL-illegal characters. A config produced by [`from_url`]
    /// always round-trips cleanly; only hand-built configs can fail here.
    ///
    /// [`from_url`]: ServiceConfig::from_url
    fn to_url(&self) -> Result<Url>;

    /// Set a single config field from a query/param key.
    ///
    /// Keys are matched case-insensitively, mirroring Go's `PropKeyResolver` (which lowercases key
    /// tags), so URLs produced by the Go tool (whose keys are emitted lowercase) parse here too.
    /// Returns [`Error::UnknownConfigKey`] for unrecognized keys.
    ///
    /// This is the single per-service hook the shared [`apply_params`](ServiceConfig::apply_params)
    /// and [`apply_query_pairs`](ServiceConfig::apply_query_pairs) loops drive.
    fn set_field(&mut self, key: &str, value: &str) -> Result<()>;

    /// Set every field named by a URL's query string. Used by `from_url` implementations.
    fn apply_query_pairs(&mut self, url: &Url) -> Result<()> {
        for (key, value) in url.query_pairs() {
            self.set_field(&key, &value)?;
        }
        Ok(())
    }

    /// Apply per-message [`Params`] overrides on top of the parsed config.
    ///
    /// Keys this service doesn't recognize are ignored — a fan-out passes one `Params` set to every
    /// destination, so another service's keys legitimately appear here. But a bad *value* on a key
    /// we do own (e.g. `color=notacolor`) is a real mistake and propagates, matching Go's `Send`,
    /// which fails on any `UpdateConfigFromParams` error.
    fn apply_params(&mut self, params: &Params) -> Result<()> {
        for (key, value) in params.iter() {
            match self.set_field(key, value) {
                Ok(()) | Err(Error::UnknownConfigKey(_)) => {}
                Err(err) => return Err(err),
            }
        }
        Ok(())
    }
}

/// Encode query parameters as `k=v&...`, percent-encoding values (spaces as `+`), with keys in
/// sorted order, like Go's `url.Values.Encode()`.
#[cfg(any(feature = "slack", feature = "discord", feature = "generic"))]
pub(crate) fn encode_query(pairs: &BTreeMap<String, String>) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in pairs {
        serializer.append_pair(key, value);
    }
    serializer.finish()
}

/// Insert `key => value` into a `to_url` query map, but only when `value` is non-empty. Skipping
/// empty values keeps the rendered URL free of blank `?key=` params, matching how each service's
/// `to_url` omits unset string fields.
#[cfg(any(feature = "slack", feature = "discord"))]
pub(crate) fn insert_if_nonempty(query: &mut BTreeMap<String, String>, key: &str, value: &str) {
    if !value.is_empty() {
        query.insert(key.to_string(), value.to_string());
    }
}

/// Parse a color, accepting `#RRGGBB`, `0xRRGGBB`, or bare hex (all base-16). Mirrors Go's
/// `StripNumberPrefix` plus base-16 field parsing.
#[cfg(feature = "discord")]
pub(crate) fn parse_color(input: &str) -> Result<u32> {
    // All accepted forms are base-16; only the optional prefix (`#`, `0x`, `0X`) is stripped.
    let digits = input
        .strip_prefix('#')
        .or_else(|| input.strip_prefix("0x"))
        .or_else(|| input.strip_prefix("0X"))
        .unwrap_or(input);

    u32::from_str_radix(digits, 16)
        .map_err(|e| crate::error::Error::InvalidConfig(format!("invalid color {input:?}: {e}")))
}

/// Format a color as `0x` plus lowercase hex, like Go's base-16 uint serialization
/// (`"0x" + FormatUint(v, 16)`).
#[cfg(feature = "discord")]
pub(crate) fn format_color(value: u32) -> String {
    format!("0x{value:x}")
}

/// Parse a boolean accepting `true/1/yes/y` and `false/0/no/n` (case-insensitive), matching Go's
/// `format.ParseBool`.
#[cfg(any(feature = "discord", feature = "generic"))]
pub(crate) fn parse_bool(value: &str) -> Result<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "y" => Ok(true),
        "false" | "0" | "no" | "n" => Ok(false),
        other => Err(crate::error::Error::InvalidConfig(format!(
            "invalid boolean value: {other:?}"
        ))),
    }
}

/// Build a [`Url`] from components, percent-encoding the query. `path` should be empty or begin
/// with `/`. Used by services' `to_url` implementations.
///
/// Returns [`Error::InvalidConfig`] if the assembled string isn't a valid URL (e.g. a host/userinfo
/// holding URL-illegal characters), rather than panicking.
#[cfg(any(feature = "slack", feature = "discord"))]
pub(crate) fn build_url(
    scheme: &str,
    user: &str,
    password: Option<&str>,
    host: &str,
    path: &str,
    query: &BTreeMap<String, String>,
) -> Result<Url> {
    let mut raw = format!("{scheme}://");
    if !user.is_empty() || password.is_some() {
        raw.push_str(user);
        if let Some(pass) = password {
            raw.push(':');
            raw.push_str(pass);
        }
        raw.push('@');
    }
    raw.push_str(host);
    raw.push_str(path);
    let query = encode_query(query);
    if !query.is_empty() {
        raw.push('?');
        raw.push_str(&query);
    }
    Url::parse(&raw)
        .map_err(|e| crate::error::Error::InvalidConfig(format!("cannot render {scheme} URL: {e}")))
}
