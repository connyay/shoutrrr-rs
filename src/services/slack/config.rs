//! Slack service configuration, ported from `slack_config.go`.

use std::collections::BTreeMap;

use url::Url;

use super::token::Token;
use crate::config::{ServiceConfig, build_url, insert_if_nonempty};
use crate::error::{Error, Result};

/// Configuration for the Slack service.
#[derive(Debug, Clone, Default)]
pub struct SlackConfig {
    /// Override bot name (`botname`, `username`).
    pub bot_name: String,
    /// Emoji or image URL icon (`icon`, `icon_emoji`, `icon_url`).
    pub icon: String,
    /// API/webhook token (URL user:pass).
    pub token: Token,
    /// Left-border color (`color`).
    pub color: String,
    /// Title prepended above the message (`title`).
    pub title: String,
    /// Target channel (URL host); `"webhook"` for webhook tokens.
    pub channel: String,
    /// Parent message `ts` to reply in a thread (`thread_ts`).
    pub thread_ts: String,
}

const DUMMY_URL: &str = "slack://dummy@dummy.com";

impl ServiceConfig for SlackConfig {
    fn scheme() -> &'static str {
        "slack"
    }

    fn set_field(&mut self, key: &str, value: &str) -> Result<()> {
        match key.to_ascii_lowercase().as_str() {
            "botname" | "username" => self.bot_name = value.to_string(),
            "icon" | "icon_emoji" | "icon_url" => self.icon = value.to_string(),
            "color" => self.color = value.to_string(),
            "title" => self.title = value.to_string(),
            "thread_ts" => self.thread_ts = value.to_string(),
            _ => {
                return Err(Error::UnknownConfigKey(format!("slack: {key}")));
            }
        }
        Ok(())
    }

    fn from_url(url: &Url) -> Result<Self> {
        let mut config = SlackConfig::default();

        let host = url.host_str().unwrap_or_default();
        let path = url.path();

        let token = if path.len() > 1 {
            // Legacy URL format: `slack://botname@A/B/C`.
            config.channel = "webhook".to_string();
            config.bot_name = url.username().to_string();
            format!("{host}{path}")
        } else {
            config.channel = host.to_string();
            match url.password() {
                Some(pass) => format!("{}:{}", url.username(), pass),
                None => url.username().to_string(),
            }
        };

        if url.as_str() != DUMMY_URL {
            config.token.set_from_prop(&token)?;
        } else {
            config.token.set_raw(token);
        }

        config.apply_query_pairs(url)?;

        Ok(config)
    }

    fn to_url(&self) -> Result<Url> {
        let mut query = BTreeMap::new();
        insert_if_nonempty(&mut query, "botname", &self.bot_name);
        insert_if_nonempty(&mut query, "color", &self.color);
        insert_if_nonempty(&mut query, "icon", &self.icon);
        insert_if_nonempty(&mut query, "thread_ts", &self.thread_ts);
        insert_if_nonempty(&mut query, "title", &self.title);

        let (user, pass) = self.token.user_info();
        build_url("slack", user, Some(pass), &self.channel, "", &query)
    }
}
