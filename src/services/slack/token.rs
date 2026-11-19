//! Slack token parsing and normalization, ported from Go shoutrrr's `slack_token.go`.

use crate::error::{Error, Result};

const WEBHOOK_BASE: &str = "https://hooks.slack.com/services/";
const HOOK_TOKEN_IDENTIFIER: &str = "hook";
/// Length of the type identifier (e.g. `xoxb`, `hook`).
const TYPE_IDENTIFIER_LENGTH: usize = 4;
/// Offset past the type identifier and its `:` separator (e.g. `xoxb:`).
const TYPE_IDENTIFIER_OFFSET: usize = 5;
const MIN_TOKEN_LENGTH: usize = 3;
/// Minimum length of each of the first two token parts.
const PART_MIN_LENGTH: usize = 9;
/// Minimum length of the third token part.
const SECRET_MIN_LENGTH: usize = 24;

/// The pieces of a token located by [`find_token`].
struct TokenParts<'a> {
    /// The type identifier (`xoxb`, `hook`, ...), when the token carries one.
    type_id: Option<&'a str>,
    p1: &'a str,
    sep1: u8,
    p2: &'a str,
    sep2: u8,
    p3: &'a str,
}

/// Find the leftmost token in `input`.
///
/// This is a hand-written matcher for the Go `tokenPattern`, kept out of a regex engine because
/// that engine dominated the WASM bundle size:
///
/// ```text
/// (?:(?P<type>xox.|hook)[-:]|:?)(?P<p1>[A-Z0-9]{9,})(?P<s1>[-/,])(?P<p2>[A-Z0-9]{9,})(?P<s2>[-/,])(?P<p3>[A-Za-z0-9]{24,})
/// ```
///
/// Like the pattern, it is unanchored: text before and after the token is ignored. The scan is
/// linear in the length of `input`.
fn find_token(input: &str) -> Option<TokenParts<'_>> {
    let bytes = input.as_bytes();
    (0..bytes.len())
        .filter(|&start| {
            // A start inside a part-1 run sees the same run end as the run's first byte, only
            // with fewer characters, so it can't match where that one failed. Skipping these is
            // what keeps a long run from being rescanned from each of its positions.
            let mid_run = start > 0 && is_part_byte(bytes[start]) && is_part_byte(bytes[start - 1]);
            !mid_run && input.is_char_boundary(start)
        })
        .find_map(|start| match_at(&input[start..]))
}

/// Match a token at the very start of `s`: a typed token first, then an untyped one.
///
/// The pattern's optional `:` before an untyped token is not handled here: it captures nothing,
/// so skipping it is the same as finding the token one character later.
fn match_at(s: &str) -> Option<TokenParts<'_>> {
    if let Some((type_id, rest)) = split_type(s)
        && let Some(parts) = match_parts(rest)
    {
        return Some(TokenParts {
            type_id: Some(type_id),
            ..parts
        });
    }
    match_parts(s)
}

/// `(xox.|hook)[-:]`: split a leading type identifier and its separator off `s`.
fn split_type(s: &str) -> Option<(&str, &str)> {
    let len = if let Some(rest) = s.strip_prefix("xox") {
        // `.` is any character except a newline.
        let any = rest.chars().next().filter(|&c| c != '\n')?;
        "xox".len() + any.len_utf8()
    } else if s.starts_with(HOOK_TOKEN_IDENTIFIER) {
        HOOK_TOKEN_IDENTIFIER.len()
    } else {
        return None;
    };
    let (type_id, rest) = s.split_at(len);
    Some((type_id, rest.strip_prefix(['-', ':'])?))
}

/// `[A-Z0-9]{9,} [-/,] [A-Z0-9]{9,} [-/,] [A-Za-z0-9]{24,}` at the start of `s`.
fn match_parts(s: &str) -> Option<TokenParts<'_>> {
    let (p1, s) = take_run(s, PART_MIN_LENGTH, is_part_byte)?;
    let (sep1, s) = take_separator(s)?;
    let (p2, s) = take_run(s, PART_MIN_LENGTH, is_part_byte)?;
    let (sep2, s) = take_separator(s)?;
    let (p3, _) = take_run(s, SECRET_MIN_LENGTH, |b| b.is_ascii_alphanumeric())?;
    Some(TokenParts {
        type_id: None,
        p1,
        sep1,
        p2,
        sep2,
        p3,
    })
}

/// `[A-Z0-9]`, the alphabet of the first two token parts.
fn is_part_byte(byte: u8) -> bool {
    byte.is_ascii_uppercase() || byte.is_ascii_digit()
}

/// Split the longest leading run of `class` bytes off `s`, if it is at least `min` long.
fn take_run(s: &str, min: usize, class: impl Fn(u8) -> bool) -> Option<(&str, &str)> {
    let len = s.bytes().take_while(|&b| class(b)).count();
    (len >= min).then(|| s.split_at(len))
}

/// Split a leading `-`, `/` or `,` off `s`.
fn take_separator(s: &str) -> Option<(u8, &str)> {
    let (&sep, _) = s.as_bytes().split_first()?;
    matches!(sep, b'-' | b'/' | b',').then(|| (sep, &s[1..]))
}

/// A Slack API token or webhook token, stored in normalized `type:p1-p2-p3` form.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Token {
    raw: String,
}

impl Token {
    /// Parse and normalize a token string.
    pub fn parse(input: &str) -> Result<Self> {
        let mut token = Token::default();
        token.set_from_prop(input)?;
        Ok(token)
    }

    /// Set the token from a raw property value, validating and normalizing it.
    pub fn set_from_prop(&mut self, prop: &str) -> Result<()> {
        if prop.len() < MIN_TOKEN_LENGTH {
            return Err(Error::InvalidToken(format!("token too short: {prop:?}")));
        }

        let parts = find_token(prop).ok_or_else(|| {
            Error::InvalidToken(format!("token does not match expected format: {prop:?}"))
        })?;

        let type_id = parts.type_id.unwrap_or(HOOK_TOKEN_IDENTIFIER);

        // Normalize to `type:p1-p2-p3`. (`raw` is set even on the separator-mismatch error,
        // matching the Go behavior.)
        self.raw = format!("{type_id}:{}-{}-{}", parts.p1, parts.p2, parts.p3);

        if parts.sep1 != parts.sep2 {
            return Err(Error::InvalidToken(
                "mismatched token separators".to_string(),
            ));
        }

        Ok(())
    }

    /// Bypass validation and store a raw value directly (mirrors the Go `dummy` escape hatch).
    pub(crate) fn set_raw(&mut self, raw: String) {
        self.raw = raw;
    }

    /// The `type:` prefix (`xoxb`, `hook`, ...), or `""` for an unset/too-short token.
    ///
    /// A parsed token is always normalized to `type:p1-p2-p3`, so this is non-empty after
    /// [`set_from_prop`](Token::set_from_prop). The guard keeps the accessors total for a default
    /// or raw-set [`Token`], where `raw` may be shorter than the prefix — slicing would otherwise
    /// panic. (Go's equivalents slice unconditionally and panic in the same situation.)
    fn type_part(&self) -> &str {
        self.raw.get(..TYPE_IDENTIFIER_LENGTH).unwrap_or_default()
    }

    /// The token body after the `type:` prefix, or `""` for an unset/too-short token.
    fn body_part(&self) -> &str {
        self.raw.get(TYPE_IDENTIFIER_OFFSET..).unwrap_or_default()
    }

    /// The 4-character type identifier (`xoxb`, `xoxp`, `hook`, ...); `""` if unset.
    pub fn type_identifier(&self) -> &str {
        self.type_part()
    }

    /// Whether this is an API token (anything other than a `hook` webhook token).
    pub fn is_api_token(&self) -> bool {
        self.type_identifier() != HOOK_TOKEN_IDENTIFIER
    }

    /// The `Authorization` header value, e.g. `Bearer xoxb-AAA-BBB-CCC`.
    pub fn authorization(&self) -> String {
        format!("Bearer {}-{}", self.type_part(), self.body_part())
    }

    /// The webhook URL for this token (dashes become path separators).
    pub fn webhook_url(&self) -> String {
        let mut url = String::from(WEBHOOK_BASE);
        for ch in self.body_part().chars() {
            url.push(if ch == '-' { '/' } else { ch });
        }
        url
    }

    /// The `(user, password)` userinfo pair used to render the token into a URL.
    pub fn user_info(&self) -> (&str, &str) {
        (self.type_part(), self.body_part())
    }

    /// The normalized token string (`type:p1-p2-p3`).
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}
