//! Slack service tests: token normalization, URL round-trips, and send behavior via MockTransport.

use shoutrrr::Params;
use shoutrrr::config::ServiceConfig;
use shoutrrr::services::slack::{SlackConfig, Token};
use shoutrrr::transport::MockTransport;

mod common;
use common::parse;

const WEBHOOK_TOKEN: &str = "AAAAAAAAA-BBBBBBBBB-123456789123456789123456";

#[test]
fn token_normalizes_webhook_form() {
    let token = Token::parse("AAAAAAAAA/BBBBBBBBB/123456789123456789123456").unwrap();
    assert_eq!(
        token.as_str(),
        "hook:AAAAAAAAA-BBBBBBBBB-123456789123456789123456"
    );
    assert!(!token.is_api_token());
    assert_eq!(
        token.webhook_url(),
        "https://hooks.slack.com/services/AAAAAAAAA/BBBBBBBBB/123456789123456789123456"
    );
}

#[test]
fn token_normalizes_api_form() {
    let token = Token::parse("xoxb:AAAAAAAAA-BBBBBBBBB-123456789123456789123456").unwrap();
    assert!(token.is_api_token());
    assert_eq!(
        token.authorization(),
        "Bearer xoxb-AAAAAAAAA-BBBBBBBBB-123456789123456789123456"
    );
}

#[test]
fn token_rejects_invalid() {
    assert!(Token::parse("xoxb").is_err());
    // Separators must be consistent (`-` vs `/`).
    assert!(Token::parse("AAAAAAAAA-BBBBBBBBB/123456789123456789123456").is_err());
}

#[test]
fn url_round_trips() {
    let raw = "slack://hook:AAAAAAAAA-BBBBBBBBB-123456789123456789123456@webhook\
               ?botname=testbot&color=3f00fe&title=Test+title";
    let config = SlackConfig::from_url(&parse(raw)).unwrap();

    assert_eq!(config.bot_name, "testbot");
    assert_eq!(config.color, "3f00fe");
    assert_eq!(config.title, "Test title");
    assert_eq!(config.channel, "webhook");
    assert_eq!(config.to_url().unwrap().as_str(), raw);
}

#[test]
fn legacy_url_normalizes() {
    let old = "slack://testbot@AAAAAAAAA/BBBBBBBBB/123456789123456789123456\
               ?color=3f00fe&title=Test+title";
    let new = "slack://hook:AAAAAAAAA-BBBBBBBBB-123456789123456789123456@webhook\
               ?botname=testbot&color=3f00fe&title=Test+title";
    let config = SlackConfig::from_url(&parse(old)).unwrap();
    assert_eq!(config.to_url().unwrap().as_str(), new);
}

#[test]
fn webhook_send_builds_request() {
    let mock = MockTransport::new(200, "ok");
    let url = format!("slack://hook:{WEBHOOK_TOKEN}@webhook");

    pollster::block_on(shoutrrr::send(&mock, &url, "Hello\nWorld")).unwrap();

    let request = mock.last_request().unwrap();
    assert_eq!(
        request.url,
        "https://hooks.slack.com/services/AAAAAAAAA/BBBBBBBBB/123456789123456789123456"
    );

    let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(body["text"], "");
    assert_eq!(body["attachments"][0]["text"], "Hello");
    assert_eq!(body["attachments"][1]["text"], "World");
}

#[test]
fn api_send_sets_bearer_and_channel() {
    let mock = MockTransport::new(200, r#"{"ok":true}"#);
    let url = format!("slack://xoxb:{WEBHOOK_TOKEN}@C123");

    pollster::block_on(shoutrrr::send(&mock, &url, "Hi")).unwrap();

    let request = mock.last_request().unwrap();
    assert_eq!(request.url, "https://slack.com/api/chat.postMessage");

    let auth = request
        .headers
        .iter()
        .find(|(k, _)| k == "Authorization")
        .map(|(_, v)| v.as_str())
        .unwrap();
    assert_eq!(
        auth,
        "Bearer xoxb-AAAAAAAAA-BBBBBBBBB-123456789123456789123456"
    );

    let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(body["channel"], "C123");
}

#[test]
fn api_error_response_is_reported() {
    let mock = MockTransport::new(200, r#"{"ok":false,"error":"invalid_auth"}"#);
    let url = format!("slack://xoxb:{WEBHOOK_TOKEN}@C123");

    let err = pollster::block_on(shoutrrr::send(&mock, &url, "Hi")).unwrap_err();
    assert!(err.to_string().contains("invalid_auth"));
}

#[test]
fn api_warning_is_ignored() {
    // ok:true with a warning still succeeds (the Go version only logs the warning).
    let mock = MockTransport::new(200, r#"{"ok":true,"warning":"missing_charset"}"#);
    let url = format!("slack://xoxb:{WEBHOOK_TOKEN}@C123");

    pollster::block_on(shoutrrr::send(&mock, &url, "Hi")).unwrap();
}

fn webhook_body(message: &str) -> serde_json::Value {
    let mock = MockTransport::new(200, "ok");
    let url = format!("slack://hook:{WEBHOOK_TOKEN}@webhook");
    pollster::block_on(shoutrrr::send(&mock, &url, message)).unwrap();
    serde_json::from_slice(&mock.last_request().unwrap().body).unwrap()
}

#[test]
fn icon_url_vs_emoji_routing() {
    let mock = MockTransport::new(200, "ok");
    let url = format!("slack://hook:{WEBHOOK_TOKEN}@webhook?icon=https://example.com/i.png");
    pollster::block_on(shoutrrr::send(&mock, &url, "hi")).unwrap();
    let body: serde_json::Value =
        serde_json::from_slice(&mock.last_request().unwrap().body).unwrap();
    assert_eq!(body["icon_url"], "https://example.com/i.png");
    assert!(body.get("icon_emoji").is_none());

    let mock = MockTransport::new(200, "ok");
    let url = format!("slack://hook:{WEBHOOK_TOKEN}@webhook?icon=rocket");
    pollster::block_on(shoutrrr::send(&mock, &url, "hi")).unwrap();
    let body: serde_json::Value =
        serde_json::from_slice(&mock.last_request().unwrap().body).unwrap();
    assert_eq!(body["icon_emoji"], "rocket");
    assert!(body.get("icon_url").is_none());
}

#[test]
fn overflow_lines_merge_into_last_attachment() {
    // 105 lines: the first 100 each get their own attachment; lines 101+ are appended onto the
    // 100th, matching Go's MaxAttachments handling.
    let message = (0..105)
        .map(|i| format!("line{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let body = webhook_body(&message);
    let attachments = body["attachments"].as_array().unwrap();

    assert_eq!(attachments.len(), 100);
    assert_eq!(attachments[0]["text"], "line0");
    assert_eq!(
        attachments[99]["text"],
        "line99\nline100\nline101\nline102\nline103\nline104"
    );
}

#[test]
fn trailing_newline_drops_empty_attachment() {
    let body = webhook_body("One\nTwo\nThree\n");
    let attachments = body["attachments"].as_array().unwrap();

    assert_eq!(attachments.len(), 3);
    for attachment in attachments {
        assert_ne!(attachment["text"], "");
    }
}

#[test]
fn empty_message_yields_no_attachments() {
    let body = webhook_body("");
    assert!(body.get("attachments").is_none());
}

#[test]
fn query_keys_are_case_insensitive() {
    let raw = format!("slack://hook:{WEBHOOK_TOKEN}@webhook?BotName=bob&TITLE=Hi");
    let config = SlackConfig::from_url(&parse(&raw)).unwrap();
    assert_eq!(config.bot_name, "bob");
    assert_eq!(config.title, "Hi");
}

#[test]
fn params_apply_known_keys_and_ignore_unknown() {
    let mut params = Params::new().with_title("Override");
    params.insert("not_a_slack_key", "whatever"); // a foreign key (e.g. from a fan-out) is ignored

    let mock = MockTransport::new(200, "ok");
    let url = format!("slack://hook:{WEBHOOK_TOKEN}@webhook");
    pollster::block_on(shoutrrr::send_with_params(&mock, &url, "hi", &params)).unwrap();

    let body: serde_json::Value =
        serde_json::from_slice(&mock.last_request().unwrap().body).unwrap();
    assert_eq!(body["text"], "Override");
}
