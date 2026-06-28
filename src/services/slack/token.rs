//! Slack token parsing and normalization, ported from Go shoutrrr's `slack_token.go`.

use std::sync::OnceLock;

use regex::Regex;

use crate::error::{Error, Result};

const WEBHOOK_BASE: &str = "https://hooks.slack.com/services/";
const HOOK_TOKEN_IDENTIFIER: &str = "hook";
/// Length of the type identifier (e.g. `xoxb`, `hook`).
const TYPE_IDENTIFIER_LENGTH: usize = 4;
/// Offset past the type identifier and its `:` separator (e.g. `xoxb:`).
const TYPE_IDENTIFIER_OFFSET: usize = 5;
const MIN_TOKEN_LENGTH: usize = 3;

/// The token-matching pattern, identical to the Go `tokenPattern` (parts of length 9/9/24+).
fn token_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?:(?P<type>xox.|hook)[-:]|:?)(?P<p1>[A-Z0-9]{9,})(?P<s1>[-/,])(?P<p2>[A-Z0-9]{9,})(?P<s2>[-/,])(?P<p3>[A-Za-z0-9]{24,})",
        )
        .expect("token pattern is a valid regex")
    })
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

        let caps = token_pattern().captures(prop).ok_or_else(|| {
            Error::InvalidToken(format!("token does not match expected format: {prop:?}"))
        })?;

        let type_id = caps
            .name("type")
            .map(|m| m.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(HOOK_TOKEN_IDENTIFIER);

        // Normalize to `type:p1-p2-p3`. (`raw` is set even on the separator-mismatch error,
        // matching the Go behavior.)
        self.raw = format!("{type_id}:{}-{}-{}", &caps["p1"], &caps["p2"], &caps["p3"]);

        let (sep1, sep2) = (&caps["s1"], &caps["s2"]);
        if sep1 != sep2 {
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
