//! Slack JSON payload construction, ported from `slack_json.go`.

use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use super::config::SlackConfig;

/// Maximum number of attachments allowed by the Slack API.
const MAX_ATTACHMENTS: usize = 100;

/// Matches an icon value that should be treated as a URL rather than an emoji.
fn icon_url_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"https?://").expect("icon url pattern is a valid regex"))
}

/// The Slack message payload. Only the fields this port populates are modeled; the omitted ones
/// always serialize empty and are skipped, so the JSON stays byte-identical to the Go version.
#[derive(Debug, Default, Serialize)]
pub(crate) struct MessagePayload {
    pub text: String,
    #[serde(rename = "username", skip_serializing_if = "String::is_empty")]
    pub bot_name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub thread_ts: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub channel: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub icon_emoji: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub icon_url: String,
}

/// A single Slack attachment (one per message line).
#[derive(Debug, Serialize)]
pub(crate) struct Attachment {
    pub text: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub color: String,
}

impl MessagePayload {
    /// Route an icon value into either `icon_url` or `icon_emoji` based on an `http(s)://` prefix.
    fn set_icon(&mut self, icon: &str) {
        self.icon_url.clear();
        self.icon_emoji.clear();
        if !icon.is_empty() {
            if icon_url_pattern().is_match(icon) {
                self.icon_url = icon.to_string();
            } else {
                self.icon_emoji = icon.to_string();
            }
        }
    }
}

/// Build a Slack message payload from the config and message, splitting lines into attachments.
pub(crate) fn create_json_payload(config: &SlackConfig, message: &str) -> MessagePayload {
    let mut attachments: Vec<Attachment> = Vec::new();

    for (i, line) in message.split('\n').enumerate() {
        if i >= MAX_ATTACHMENTS {
            // Append overflow lines to the final attachment.
            let last = &mut attachments[MAX_ATTACHMENTS - 1];
            last.text.push('\n');
            last.text.push_str(line);
            continue;
        }
        attachments.push(Attachment {
            text: line.to_string(),
            color: config.color.clone(),
        });
    }

    // Drop a trailing empty attachment (e.g. from a trailing newline).
    if attachments.last().is_some_and(|a| a.text.is_empty()) {
        attachments.pop();
    }

    let mut payload = MessagePayload {
        text: config.title.clone(),
        bot_name: config.bot_name.clone(),
        attachments,
        thread_ts: config.thread_ts.clone(),
        ..Default::default()
    };

    payload.set_icon(&config.icon);

    if config.channel != "webhook" {
        payload.channel = config.channel.clone();
    }

    payload
}

/// The Slack API response envelope (API-token mode).
#[derive(Debug, Default, Deserialize)]
pub(crate) struct ApiResponse {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub warning: String,
}
