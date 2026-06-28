//! Discord service configuration, ported from `discord_config.go`.

use std::collections::BTreeMap;

use url::Url;

use crate::config::{
    ServiceConfig, build_url, encode_query, format_color, insert_if_nonempty, parse_bool,
    parse_color,
};
use crate::error::{Error, Result};
use crate::message::MESSAGE_LEVEL_COUNT;

/// Base URL for Discord webhook endpoints.
pub(crate) const HOOKS_BASE_URL: &str = "https://discord.com/api/webhooks";

// Default per-level colors.
const DEFAULT_COLOR: u32 = 0x50_D9FF;
const DEFAULT_COLOR_ERROR: u32 = 0xD6_0510;
const DEFAULT_COLOR_WARN: u32 = 0xFF_C441;
const DEFAULT_COLOR_INFO: u32 = 0x24_88FF;
const DEFAULT_COLOR_DEBUG: u32 = 0x7B_00AB;

/// Configuration for the Discord service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordConfig {
    /// Webhook ID (URL host).
    pub webhook_id: String,
    /// Webhook token (URL user).
    pub token: String,
    /// Title shown above the first embed (`title`).
    pub title: String,
    /// Override the webhook's default username (`username`).
    pub username: String,
    /// Override the webhook's default avatar URL (`avatar`, `avatarurl`).
    pub avatar: String,
    /// Left-border color for plain messages (`color`).
    pub color: u32,
    /// Left-border color for error messages (`colorError`).
    pub color_error: u32,
    /// Left-border color for warning messages (`colorWarn`).
    pub color_warn: u32,
    /// Left-border color for info messages (`colorInfo`).
    pub color_info: u32,
    /// Left-border color for debug messages (`colorDebug`).
    pub color_debug: u32,
    /// Send each line as a separate embed (`splitLines`).
    pub split_lines: bool,
    /// Treat the message as a raw JSON payload (`json`, or a `/raw` path).
    pub json: bool,
    /// Thread ID to post into (`thread_id`).
    pub thread_id: String,
}

impl Default for DiscordConfig {
    fn default() -> Self {
        Self {
            webhook_id: String::new(),
            token: String::new(),
            title: String::new(),
            username: String::new(),
            avatar: String::new(),
            color: DEFAULT_COLOR,
            color_error: DEFAULT_COLOR_ERROR,
            color_warn: DEFAULT_COLOR_WARN,
            color_info: DEFAULT_COLOR_INFO,
            color_debug: DEFAULT_COLOR_DEBUG,
            split_lines: true,
            json: false,
            thread_id: String::new(),
        }
    }
}

impl DiscordConfig {
    /// Per-level color array indexed by [`MessageLevel`](crate::message::MessageLevel).
    pub(crate) fn level_colors(&self) -> [u32; MESSAGE_LEVEL_COUNT] {
        [
            self.color,
            self.color_error,
            self.color_warn,
            self.color_info,
            self.color_debug,
        ]
    }

    /// Build the webhook POST URL (with an optional `thread_id` query), as in
    /// `CreatePostURLFromConfig`. Returns an empty string for an incomplete config.
    pub(crate) fn post_url(&self) -> String {
        if self.webhook_id.is_empty() || self.token.is_empty() {
            return String::new();
        }
        let webhook_id = self.webhook_id.trim();
        let token = self.token.trim();
        let mut url = format!("{HOOKS_BASE_URL}/{webhook_id}/{token}");
        if !self.thread_id.is_empty() {
            let mut query = BTreeMap::new();
            query.insert("thread_id".to_string(), self.thread_id.trim().to_string());
            url.push('?');
            url.push_str(&encode_query(&query));
        }
        url
    }
}

impl ServiceConfig for DiscordConfig {
    fn scheme() -> &'static str {
        "discord"
    }

    fn set_field(&mut self, key: &str, value: &str) -> Result<()> {
        match key.to_ascii_lowercase().as_str() {
            "title" => self.title = value.to_string(),
            "username" => self.username = value.to_string(),
            "avatar" | "avatarurl" => self.avatar = value.to_string(),
            "color" => self.color = parse_color(value)?,
            "colorerror" => self.color_error = parse_color(value)?,
            "colorwarn" => self.color_warn = parse_color(value)?,
            "colorinfo" => self.color_info = parse_color(value)?,
            "colordebug" => self.color_debug = parse_color(value)?,
            "splitlines" => self.split_lines = parse_bool(value)?,
            "json" => self.json = parse_bool(value)?,
            "thread_id" => self.thread_id = value.trim().to_string(),
            _ => {
                return Err(Error::UnknownConfigKey(format!("discord: {key}")));
            }
        }
        Ok(())
    }

    fn from_url(url: &Url) -> Result<Self> {
        let mut config = DiscordConfig {
            webhook_id: url.host_str().unwrap_or_default().to_string(),
            token: url.username().to_string(),
            ..Default::default()
        };

        match url.path() {
            "" | "/" => {}
            "/raw" => config.json = true,
            other => {
                return Err(Error::InvalidConfig(format!(
                    "illegal discord URL path: {other}"
                )));
            }
        }

        if config.webhook_id.is_empty() {
            return Err(Error::InvalidConfig(
                "missing discord webhook ID".to_string(),
            ));
        }
        if config.token.is_empty() {
            return Err(Error::InvalidConfig("missing discord token".to_string()));
        }

        config.apply_query_pairs(url)?;

        Ok(config)
    }

    fn to_url(&self) -> Result<Url> {
        let mut query = BTreeMap::new();
        insert_if_nonempty(&mut query, "title", &self.title);
        insert_if_nonempty(&mut query, "username", &self.username);
        insert_if_nonempty(&mut query, "avatar", &self.avatar);
        if self.color != DEFAULT_COLOR {
            query.insert("color".to_string(), format_color(self.color));
        }
        if self.color_error != DEFAULT_COLOR_ERROR {
            query.insert("colorError".to_string(), format_color(self.color_error));
        }
        if self.color_warn != DEFAULT_COLOR_WARN {
            query.insert("colorWarn".to_string(), format_color(self.color_warn));
        }
        if self.color_info != DEFAULT_COLOR_INFO {
            query.insert("colorInfo".to_string(), format_color(self.color_info));
        }
        if self.color_debug != DEFAULT_COLOR_DEBUG {
            query.insert("colorDebug".to_string(), format_color(self.color_debug));
        }
        if !self.split_lines {
            // Only reachable when `split_lines` is false, so the rendered value is always "No"
            // (Go's `format.PrintBool`).
            query.insert("splitLines".to_string(), "No".to_string());
        }
        insert_if_nonempty(&mut query, "thread_id", &self.thread_id);

        let path = if self.json { "/raw" } else { "" };
        build_url("discord", &self.token, None, &self.webhook_id, path, &query)
    }
}
