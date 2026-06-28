//! Per-message parameter overrides.

use std::collections::BTreeMap;

/// String key/value overrides applied to a service's configuration for a single send.
///
/// Mirrors Go shoutrrr's `types.Params`. Keys correspond to a service's query-parameter keys
/// (e.g. `title`, `color`, `botname`). `BTreeMap` gives deterministic iteration order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Params(BTreeMap<String, String>);

impl Params {
    /// An empty parameter set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a key/value pair, returning `self` (builder style).
    pub fn set(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.0.insert(key.into(), value.into());
        self
    }

    /// Insert a key/value pair in place.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.0.insert(key.into(), value.into());
    }

    /// Look up a value by key.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    /// Shorthand for setting the `title` override (builder style).
    pub fn with_title(self, title: impl Into<String>) -> Self {
        self.set("title", title)
    }

    /// Whether no overrides are present.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterate over the key/value pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}
