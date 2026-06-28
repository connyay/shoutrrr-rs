//! Custom query-value handling, ported from `custom_query.go`.
//!
//! The generic service overloads the URL query string with two prefixed namespaces: `@name` keys
//! become HTTP headers and `$name` keys become extra data merged into JSON payloads. Everything
//! else is either a config property or a literal webhook query parameter.

use std::collections::BTreeMap;

/// Prefix marking a query key as an HTTP header.
pub(super) const HEADER_PREFIX: char = '@';
/// Prefix marking a query key as extra JSON payload data.
pub(super) const EXTRA_PREFIX: char = '$';

/// Convert a header key to HTTP header casing (e.g. `ContentType` -> `Content-Type`,
/// `authorization` -> `Authorization`). Port of Go's `normalizedHeaderKey`.
///
/// Header keys are treated as ASCII (HTTP header names always are); each byte is handled in turn.
pub(super) fn normalized_header_key(key: &str) -> String {
    let bytes = key.as_bytes();
    let mut out = String::with_capacity(key.len() * 2);

    for (i, &c) in bytes.iter().enumerate() {
        let mut ch = c;
        if c.is_ascii_uppercase() {
            // Insert a dash before an interior uppercase letter not already preceded by one.
            if i > 0 && bytes[i - 1] != b'-' {
                out.push('-');
            }
        } else if (i == 0 || bytes[i - 1] == b'-') && c.is_ascii_lowercase() {
            // The first letter, and any letter following a dash, is upper-cased.
            ch = c.to_ascii_uppercase();
        }
        out.push(ch as char);
    }

    out
}

/// Extract `@header` and `$extra` entries from `query`, removing them from it.
///
/// Returns `(headers, extra_data)`. Keys of length <= 1 (a bare `@`/`$`) are left in place, matching
/// Go's guard against malformed entries. Port of `stripCustomQueryValues`.
pub(super) fn strip_custom_query_values(
    query: &mut BTreeMap<String, String>,
) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
    let mut headers = BTreeMap::new();
    let mut extra = BTreeMap::new();
    let mut consumed = Vec::new();

    for (key, value) in query.iter() {
        if key.len() <= 1 {
            continue;
        }
        match key.chars().next() {
            Some(HEADER_PREFIX) => {
                headers.insert(normalized_header_key(&key[1..]), value.clone());
                consumed.push(key.clone());
            }
            Some(EXTRA_PREFIX) => {
                extra.insert(key[1..].to_string(), value.clone());
                consumed.push(key.clone());
            }
            _ => {}
        }
    }

    for key in consumed {
        query.remove(&key);
    }

    (headers, extra)
}
