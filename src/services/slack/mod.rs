//! Slack notification service.
//!
//! Supports both webhook tokens (`slack://hook:A-B-C@webhook`) and API tokens
//! (`slack://xoxb-A-B-C@channel`). See [`SlackConfig`] for the configurable fields.

mod config;
mod payload;
mod token;

pub use config::SlackConfig;
pub use token::Token;

use async_trait::async_trait;
use url::Url;

use crate::config::ServiceConfig;
use crate::error::{Error, Result};
use crate::params::Params;
use crate::service::Service;
use crate::transport::HttpClient;
use payload::{ApiResponse, MessagePayload, create_json_payload};

/// The Slack API endpoint used in API-token mode.
const API_POST_MESSAGE: &str = "https://slack.com/api/chat.postMessage";

/// A configured Slack service instance.
pub struct SlackService {
    config: SlackConfig,
}

impl SlackService {
    /// Construct a service from a parsed `slack://` URL.
    pub fn from_url(url: &Url) -> Result<Self> {
        Ok(Self {
            config: SlackConfig::from_url(url)?,
        })
    }
}

#[async_trait(?Send)]
impl Service for SlackService {
    fn id(&self) -> &'static str {
        "slack"
    }

    async fn send(&self, http: &dyn HttpClient, message: &str, params: &Params) -> Result<()> {
        let mut config = self.config.clone();
        config.apply_params(params)?;

        let payload = create_json_payload(&config, message);

        if config.token.is_api_token() {
            send_api(http, &config, &payload).await
        } else {
            send_webhook(http, &config, &payload).await
        }
    }
}

/// Send via the Slack Web API (`chat.postMessage`) using a Bearer token.
async fn send_api(
    http: &dyn HttpClient,
    config: &SlackConfig,
    payload: &MessagePayload,
) -> Result<()> {
    let body = serde_json::to_vec(payload)?;
    let request = crate::transport::HttpRequest::post(API_POST_MESSAGE)
        .header("Authorization", config.token.authorization())
        .json_body(body);

    let response = http.execute(request).await?;
    if !response.is_success() {
        return Err(Error::Http {
            status: response.status,
            body: response.body_string(),
        });
    }

    let api: ApiResponse = serde_json::from_slice(&response.body)?;
    if !api.ok {
        if !api.error.is_empty() {
            return Err(Error::ServiceResponse(format!(
                "Slack API error: {}",
                api.error
            )));
        }
        return Err(Error::ServiceResponse(
            "unknown Slack API error".to_string(),
        ));
    }
    // `api.warning` is intentionally ignored here (the Go version only logs it).
    let _ = api.warning;

    Ok(())
}

/// Send via a Slack incoming webhook.
async fn send_webhook(
    http: &dyn HttpClient,
    config: &SlackConfig,
    payload: &MessagePayload,
) -> Result<()> {
    let body = serde_json::to_vec(payload)?;
    let request = crate::transport::HttpRequest::post(config.token.webhook_url()).json_body(body);

    let response = http.execute(request).await?;
    match response.body_string().as_str() {
        "ok" => Ok(()),
        "" if response.is_success() => Ok(()),
        "" => Err(Error::Http {
            status: response.status,
            body: String::new(),
        }),
        other => Err(Error::ServiceResponse(format!(
            "unexpected webhook response: {other}"
        ))),
    }
}
