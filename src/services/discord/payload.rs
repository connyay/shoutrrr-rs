//! Discord webhook payload construction, ported from `discord_json.go`.

use serde::Serialize;

use crate::error::{Error, Result};
use crate::message::{MESSAGE_LEVEL_COUNT, MessageItem, MessageLevel};

/// Maximum number of embeds allowed in a single webhook payload.
const MAX_EMBEDS: usize = 10;

/// The Discord webhook payload.
#[derive(Debug, Default, Serialize)]
pub(crate) struct WebhookPayload {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub embeds: Vec<EmbedItem>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub username: String,
    #[serde(rename = "avatar_url", skip_serializing_if = "String::is_empty")]
    pub avatar_url: String,
}

/// A single Discord embed (only the fields this port populates are modeled).
#[derive(Debug, Default, Serialize)]
pub(crate) struct EmbedItem {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(rename = "description", skip_serializing_if = "String::is_empty")]
    pub content: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub color: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer: Option<EmbedFooter>,
}

/// An embed footer (used to carry the message level name).
#[derive(Debug, Serialize)]
pub(crate) struct EmbedFooter {
    pub text: String,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

/// Build a webhook payload from message items, a title, and the per-level color palette.
///
/// A single plain item (no title, `Unknown` level) becomes a top-level `content` message;
/// otherwise items become embeds (capped at [`MAX_EMBEDS`]) with the title on the first embed.
pub(crate) fn create_payload_from_items(
    items: &[MessageItem],
    title: &str,
    colors: [u32; MESSAGE_LEVEL_COUNT],
) -> Result<WebhookPayload> {
    if items.is_empty() {
        return Err(Error::InvalidConfig("empty discord message".to_string()));
    }

    if items.len() == 1 && title.is_empty() && items[0].level == MessageLevel::Unknown {
        return Ok(WebhookPayload {
            content: items[0].text.clone(),
            ..Default::default()
        });
    }

    let item_count = MAX_EMBEDS.min(items.len());
    let mut embeds = Vec::with_capacity(item_count);

    for item in items.iter().take(item_count) {
        let color = colors.get(item.level as usize).copied().unwrap_or(0);
        let mut embed = EmbedItem {
            content: item.text.clone(),
            color,
            ..Default::default()
        };
        if item.level != MessageLevel::Unknown {
            embed.footer = Some(EmbedFooter {
                text: item.level.as_str().to_string(),
            });
        }
        embeds.push(embed);
    }

    if let Some(first) = embeds.first_mut() {
        first.title = title.to_string();
    }

    Ok(WebhookPayload {
        embeds,
        ..Default::default()
    })
}
