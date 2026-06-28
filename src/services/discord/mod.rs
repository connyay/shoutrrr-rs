//! Discord notification service.
//!
//! Sends to a Discord webhook (`discord://token@webhookid`). Plain messages become a `content`
//! field or a set of embeds; a `/raw` path (or `?json=Yes`) sends the message body verbatim.

mod config;
mod payload;

pub use config::DiscordConfig;

use std::time::Duration;

use async_trait::async_trait;
use url::Url;

use crate::config::ServiceConfig;
use crate::error::{Error, Result};
use crate::message::{MessageItem, MessageLimit, message_items_from_lines, partition_message};
use crate::params::Params;
use crate::service::Service;
use crate::transport::{HttpClient, HttpRequest, HttpResponse};
use config::HOOKS_BASE_URL;
use payload::create_payload_from_items;

// Chunking limits.
const CHUNK_SIZE: usize = 2000;
const TOTAL_CHUNK_SIZE: usize = 6000;
const CHUNK_COUNT: usize = 10;
const MAX_SEARCH_RUNES: usize = 100;

const LIMITS: MessageLimit = MessageLimit {
    chunk_size: CHUNK_SIZE,
    total_chunk_size: TOTAL_CHUNK_SIZE,
    chunk_count: CHUNK_COUNT,
};

// Retry policy for transient Discord failures (rate limits and server errors). Ports the essential
// behavior of Go's `sendWithRetry`: honor `Retry-After` on `429`, otherwise exponential backoff
// capped at `MAX_BACKOFF`. Go's wall-clock total-timeout guard is dropped because `Instant::now()`
// panics on `wasm32`; the attempt cap bounds the total wait instead.
const MAX_RETRIES: u32 = 5;
const BASE_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(64);

/// A configured Discord service instance.
pub struct DiscordService {
    config: DiscordConfig,
}

impl DiscordService {
    /// Construct a service from a parsed `discord://` URL.
    pub fn from_url(url: &Url) -> Result<Self> {
        Ok(Self {
            config: DiscordConfig::from_url(url)?,
        })
    }
}

#[async_trait(?Send)]
impl Service for DiscordService {
    fn id(&self) -> &'static str {
        "discord"
    }

    async fn send(&self, http: &dyn HttpClient, message: &str, params: &Params) -> Result<()> {
        if message.is_empty() {
            return Err(Error::InvalidConfig("empty discord message".to_string()));
        }

        // Raw JSON mode: post the message body verbatim, with no param overrides.
        if self.config.json {
            let post_url = self.config.post_url();
            return do_send(http, message.as_bytes(), &post_url).await;
        }

        let mut config = self.config.clone();
        config.apply_params(params)?;

        let batches = create_items_from_plain(message, config.split_lines);

        let mut first_err: Option<Error> = None;
        for batch in batches {
            if let Err(err) = send_items(http, &config, &batch).await
                && first_err.is_none()
            {
                first_err = Some(err);
            }
        }

        first_err.map_or(Ok(()), Err)
    }
}

/// Build a JSON payload for one batch of items and POST it.
async fn send_items(
    http: &dyn HttpClient,
    config: &DiscordConfig,
    items: &[MessageItem],
) -> Result<()> {
    let mut payload = create_payload_from_items(items, &config.title, config.level_colors())?;
    payload.username = config.username.clone();
    payload.avatar_url = config.avatar.clone();

    let body = serde_json::to_vec(&payload)?;
    let post_url = config.post_url();
    do_send(http, &body, &post_url).await
}

/// Validate the webhook URL and POST the payload, retrying transient failures.
///
/// A `2xx` succeeds; a `429` or `5xx` is retried (up to [`MAX_RETRIES`]) after backing off; any
/// other status fails immediately. The backoff is awaited via [`HttpClient::sleep`] so it stays
/// runtime-agnostic.
async fn do_send(http: &dyn HttpClient, payload: &[u8], post_url: &str) -> Result<()> {
    validate_webhook_url(post_url)?;

    let mut attempt: u32 = 0;
    loop {
        let request = HttpRequest::post(post_url)
            .header("User-Agent", "shoutrrr")
            .json_body(payload.to_vec());

        let response = http.execute(request).await?;
        if response.is_success() {
            return Ok(());
        }

        let status = response.status;
        let retryable = status == 429 || status >= 500;
        if !retryable || attempt >= MAX_RETRIES {
            return Err(Error::Http {
                status,
                body: response.body_string(),
            });
        }

        // On a rate limit, honor `Retry-After` if present; otherwise fall back to exponential
        // backoff (also used for 5xx).
        let wait = if status == 429 {
            retry_after(&response).unwrap_or_else(|| backoff(attempt))
        } else {
            backoff(attempt)
        };
        http.sleep(wait).await;
        attempt += 1;
    }
}

/// Exponential backoff for a zero-based `attempt`: `min(2^attempt * BASE_BACKOFF, MAX_BACKOFF)`.
fn backoff(attempt: u32) -> Duration {
    // Cap the shift so the multiplier can't overflow; 2^6 already exceeds MAX_BACKOFF/BASE_BACKOFF.
    let factor = 1u32 << attempt.min(6);
    BASE_BACKOFF.saturating_mul(factor).min(MAX_BACKOFF)
}

/// Parse a `Retry-After` header as a non-negative number of seconds, capped at [`MAX_BACKOFF`].
///
/// Matches Go, which reads `Retry-After` as float seconds; a missing/unparseable/HTTP-date value
/// yields `None` so the caller falls back to exponential backoff.
fn retry_after(response: &HttpResponse) -> Option<Duration> {
    let seconds: f64 = response.header("Retry-After")?.trim().parse().ok()?;
    if seconds.is_finite() && seconds >= 0.0 {
        Some(Duration::from_secs_f64(seconds).min(MAX_BACKOFF))
    } else {
        None
    }
}

/// Split plain text into batches of items, by line or by chunk. Port of `CreateItemsFromPlain`.
fn create_items_from_plain(plain: &str, split_lines: bool) -> Vec<Vec<MessageItem>> {
    if split_lines {
        return message_items_from_lines(plain, LIMITS);
    }

    // Byte offset of each rune (plus the end), so a rune index slices the original `&str` in O(1)
    // instead of re-collecting the remaining text into a fresh String on every iteration.
    let mut offsets: Vec<usize> = plain.char_indices().map(|(byte, _)| byte).collect();
    offsets.push(plain.len());
    let rune_count = offsets.len() - 1;

    let mut batches = Vec::new();
    let mut start = 0usize;

    loop {
        let (items, omitted) =
            partition_message(&plain[offsets[start]..], LIMITS, MAX_SEARCH_RUNES);
        batches.push(items);
        if omitted == 0 {
            break;
        }
        start = rune_count - omitted;
    }

    batches
}

/// Validate that `post_url` is a well-formed Discord webhook endpoint. Port of
/// `validateDiscordWebhookURL`.
fn validate_webhook_url(post_url: &str) -> Result<()> {
    if post_url.is_empty() {
        return Err(Error::InvalidConfig(
            "empty discord webhook URL".to_string(),
        ));
    }

    let parsed = Url::parse(post_url)
        .map_err(|e| Error::InvalidConfig(format!("invalid discord webhook URL: {e}")))?;

    if parsed.scheme() != "https" {
        return Err(Error::InvalidConfig(
            "discord webhook URL must use https".to_string(),
        ));
    }
    if parsed.host_str() != Some("discord.com") {
        return Err(Error::InvalidConfig(
            "discord webhook URL host must be discord.com".to_string(),
        ));
    }
    if !parsed.path().starts_with("/api/webhooks/") {
        return Err(Error::InvalidConfig(
            "discord webhook URL path must start with /api/webhooks/".to_string(),
        ));
    }

    let remainder = post_url
        .strip_prefix(HOOKS_BASE_URL)
        .and_then(|r| r.strip_prefix('/'))
        .unwrap_or_default();
    // Strip any query before counting path segments.
    let remainder = remainder.split('?').next().unwrap_or_default();
    let parts: Vec<&str> = remainder.split('/').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err(Error::InvalidConfig(
            "malformed discord webhook URL".to_string(),
        ));
    }

    Ok(())
}
