//! Generic service configuration, ported from `generic_config.go` and the `format` package's
//! query-escaping (`format_query.go`).

use std::collections::BTreeMap;

use url::{Position, Url};

use super::query::{EXTRA_PREFIX, HEADER_PREFIX, strip_custom_query_values};
use crate::config::{ServiceConfig, encode_query, parse_bool};
use crate::error::{Error, Result};

/// Prefix used to escape a webhook query key that collides with a config property key, so it
/// survives a round-trip through the service URL instead of being consumed as config. Go's
/// `format.KeyPrefix`.
const KEY_PREFIX: &str = "__";

/// The config property keys, in sorted order (matching Go's `PropKeyResolver.QueryFields`, which
/// sorts its keys). Used to drive both parsing and round-trip rendering.
const QUERY_KEYS: [&str; 7] = [
    "contenttype",
    "disabletls",
    "messagekey",
    "method",
    "template",
    "title",
    "titlekey",
];

const DEFAULT_CONTENT_TYPE: &str = "application/json";
const DEFAULT_TITLE_KEY: &str = "title";
const DEFAULT_MESSAGE_KEY: &str = "message";
const DEFAULT_METHOD: &str = "POST";

/// Configuration for the generic webhook service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericConfig {
    /// The webhook target. Stored with a placeholder scheme; [`webhook_url`](Self::webhook_url)
    /// resolves the real scheme from [`disable_tls`](Self::disable_tls). Its query holds the
    /// leftover (non-config, non-`@`/`$`) parameters that pass through to the endpoint.
    webhook_url: Url,
    /// Custom HTTP headers parsed from `@name` query keys.
    headers: BTreeMap<String, String>,
    /// Extra data parsed from `$name` query keys, merged into JSON payloads.
    extra_data: BTreeMap<String, String>,
    /// `Content-Type`/`Accept` header value for templated payloads (`contenttype`).
    pub content_type: String,
    /// Use `http` instead of `https` for the webhook (`disabletls`).
    pub disable_tls: bool,
    /// Payload template: empty (plain message), `json`/`JSON`, or a registered custom name
    /// (`template`).
    pub template: String,
    /// Title config property (`title`). Retained for URL round-tripping; the payload title comes
    /// from the per-message `title` param, keyed by [`title_key`](Self::title_key).
    pub title: String,
    /// Payload key that carries the title (`titlekey`).
    pub title_key: String,
    /// Payload key that carries the message (`messagekey`).
    pub message_key: String,
    /// HTTP method (`method`).
    pub request_method: String,
}

impl Default for GenericConfig {
    fn default() -> Self {
        Self {
            // Overwritten by `store_webhook` during parsing; only a transient placeholder.
            webhook_url: Url::parse("https://localhost").expect("placeholder URL is valid"),
            headers: BTreeMap::new(),
            extra_data: BTreeMap::new(),
            content_type: DEFAULT_CONTENT_TYPE.to_string(),
            disable_tls: false,
            template: String::new(),
            title: String::new(),
            title_key: DEFAULT_TITLE_KEY.to_string(),
            message_key: DEFAULT_MESSAGE_KEY.to_string(),
            request_method: DEFAULT_METHOD.to_string(),
        }
    }
}

impl GenericConfig {
    /// The webhook POST target, with its scheme resolved from [`disable_tls`](Self::disable_tls)
    /// (`http` when TLS is disabled, otherwise `https`). Port of Go's `WebhookURL`.
    pub fn webhook_url(&self) -> Result<Url> {
        let mut url = self.webhook_url.clone();
        let scheme = if self.disable_tls { "http" } else { "https" };
        url.set_scheme(scheme)
            .map_err(|()| Error::InvalidConfig("invalid generic webhook scheme".to_string()))?;
        Ok(url)
    }

    /// Custom headers to attach to the request.
    pub(super) fn headers(&self) -> &BTreeMap<String, String> {
        &self.headers
    }

    /// Extra `$name` data merged into JSON payloads.
    pub(super) fn extra_data(&self) -> &BTreeMap<String, String> {
        &self.extra_data
    }

    /// Build a config from a webhook URL (the `generic+<scheme>://...` shortcut form, or for direct
    /// use). The embedded scheme decides TLS. Port of `ConfigFromWebhookURL`.
    pub fn from_webhook_url(webhook: &Url) -> Result<Self> {
        let mut config = Self::default();
        config.parse_query(webhook)?;
        // The webhook's own scheme wins over any `disabletls` query value.
        config.disable_tls = webhook.scheme() == "http";
        Ok(config)
    }

    /// Strip `@`/`$` customs and config properties out of `source`'s query, leaving the remainder as
    /// the webhook query. Shared by both URL forms.
    fn parse_query(&mut self, source: &Url) -> Result<()> {
        let mut query = query_map(source);
        let (headers, extra) = strip_custom_query_values(&mut query);
        self.headers = headers;
        self.extra_data = extra;
        let remaining = self.set_config_props_from_query(query)?;
        self.store_webhook(source, &remaining)
    }

    /// Consume the config-property keys out of `query`, set the matching fields, and return the
    /// leftover query. An escaped `__prop` key is unescaped back to `prop` so it passes through to
    /// the webhook. Port of `format.SetConfigPropsFromQuery`.
    ///
    /// Config keys are matched case-insensitively (so a Go-generated lowercase URL and a
    /// hand-written `?TitleKey=...` both work), mirroring the convention the other services follow.
    fn set_config_props_from_query(
        &mut self,
        query: BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, String>> {
        let mut remaining = BTreeMap::new();
        // Pull out config-property keys; everything else is a literal webhook query param.
        for (key, value) in query {
            if is_config_key(&key) {
                self.set_field(&key, &value)?;
            } else {
                remaining.insert(key, value);
            }
        }

        // Unescape `__prop` -> `prop` (now free, since the real `prop` was consumed above).
        let escaped: Vec<String> = remaining
            .keys()
            .filter(|key| key.strip_prefix(KEY_PREFIX).is_some_and(is_config_key))
            .cloned()
            .collect();
        for key in escaped {
            let value = remaining.remove(&key).expect("key just collected");
            let unescaped = key
                .strip_prefix(KEY_PREFIX)
                .expect("filtered for prefix")
                .to_ascii_lowercase();
            remaining.insert(unescaped, value);
        }

        Ok(remaining)
    }

    /// Store the webhook URL: authority + path taken from `source`, query set to `remaining`, with a
    /// placeholder scheme (resolved later by [`webhook_url`](Self::webhook_url)).
    fn store_webhook(&mut self, source: &Url, remaining: &BTreeMap<String, String>) -> Result<()> {
        let auth_path = &source[Position::BeforeUsername..Position::AfterPath];
        let mut webhook = Url::parse(&format!("https://{auth_path}"))
            .map_err(|e| Error::InvalidConfig(format!("invalid generic webhook URL: {e}")))?;
        let query = encode_query(remaining);
        webhook.set_query((!query.is_empty()).then_some(query.as_str()));
        self.webhook_url = webhook;
        Ok(())
    }

    /// Render the round-trip query: escape colliding webhook keys, add non-default config props,
    /// then append `@`/`$` customs. Port of `format.BuildQueryWithCustomFields` plus
    /// `appendCustomQueryValues`.
    fn build_query(&self) -> BTreeMap<String, String> {
        let mut query = query_map(&self.webhook_url);
        // Go skips escaping entirely when the webhook carried no query of its own.
        let skip_escape = query.is_empty();

        for key in QUERY_KEYS {
            if !skip_escape && let Some(value) = query.remove(key) {
                query.insert(format!("{KEY_PREFIX}{key}"), value);
            }

            let value = self.get_prop(key);
            if value == self.default_prop(key) {
                continue;
            }
            query.insert(key.to_string(), value);
        }

        for (key, value) in &self.headers {
            query.insert(format!("{HEADER_PREFIX}{key}"), value.clone());
        }
        for (key, value) in &self.extra_data {
            query.insert(format!("{EXTRA_PREFIX}{key}"), value.clone());
        }

        query
    }

    /// The current value of a config property, formatted as it appears in a URL.
    fn get_prop(&self, key: &str) -> String {
        match key {
            "contenttype" => self.content_type.clone(),
            "disabletls" => print_bool(self.disable_tls),
            "messagekey" => self.message_key.clone(),
            "method" => self.request_method.clone(),
            "template" => self.template.clone(),
            "title" => self.title.clone(),
            "titlekey" => self.title_key.clone(),
            _ => String::new(),
        }
    }

    /// The default value of a config property (omitted from the rendered URL when it matches).
    fn default_prop(&self, key: &str) -> &'static str {
        match key {
            "contenttype" => DEFAULT_CONTENT_TYPE,
            "disabletls" => "No",
            "messagekey" => DEFAULT_MESSAGE_KEY,
            "method" => DEFAULT_METHOD,
            "titlekey" => DEFAULT_TITLE_KEY,
            // `template` and `title` default to empty.
            _ => "",
        }
    }
}

impl ServiceConfig for GenericConfig {
    fn scheme() -> &'static str {
        "generic"
    }

    fn set_field(&mut self, key: &str, value: &str) -> Result<()> {
        match key.to_ascii_lowercase().as_str() {
            "contenttype" => self.content_type = value.to_string(),
            "disabletls" => self.disable_tls = parse_bool(value)?,
            "template" => self.template = value.to_string(),
            "title" => self.title = value.to_string(),
            "titlekey" => self.title_key = value.to_string(),
            "messagekey" => self.message_key = value.to_string(),
            "method" => self.request_method = value.to_string(),
            _ => return Err(Error::UnknownConfigKey(format!("generic: {key}"))),
        }
        Ok(())
    }

    fn from_url(url: &Url) -> Result<Self> {
        // Shortcut form `generic+<scheme>://host/hook`: the embedded URL is the webhook target.
        if url.scheme().strip_prefix("generic+").is_some() {
            let webhook_str = url
                .as_str()
                .strip_prefix("generic+")
                .expect("scheme prefix implies string prefix");
            let webhook = Url::parse(webhook_str)?;
            return Self::from_webhook_url(&webhook);
        }

        // Standard form `generic://host/hook?...`: TLS comes from the `disabletls` query value only.
        let mut config = Self::default();
        config.parse_query(url)?;
        Ok(config)
    }

    fn to_url(&self) -> Result<Url> {
        let query = self.build_query();
        let auth_path = &self.webhook_url[Position::BeforeUsername..Position::AfterPath];
        let mut raw = format!("generic://{auth_path}");
        let encoded = encode_query(&query);
        if !encoded.is_empty() {
            raw.push('?');
            raw.push_str(&encoded);
        }
        Url::parse(&raw)
            .map_err(|e| Error::InvalidConfig(format!("cannot render generic URL: {e}")))
    }
}

/// Collect a URL's query into a map, keeping the first value for any duplicated key (matching Go's
/// `url.Values` `Get`/`Set`, which read index 0).
fn query_map(url: &Url) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for (key, value) in url.query_pairs() {
        map.entry(key.into_owned())
            .or_insert_with(|| value.into_owned());
    }
    map
}

/// Whether `key` names a config property (case-insensitively).
fn is_config_key(key: &str) -> bool {
    QUERY_KEYS.contains(&key.to_ascii_lowercase().as_str())
}

/// Format a boolean as Go's `format.PrintBool` does (`Yes`/`No`).
fn print_bool(value: bool) -> String {
    if value { "Yes" } else { "No" }.to_string()
}
