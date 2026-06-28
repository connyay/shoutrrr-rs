//! Discord service tests: URL round-trips, payload shape, chunking, and raw-JSON mode.

use shoutrrr::Params;
use shoutrrr::config::ServiceConfig;
use shoutrrr::services::discord::DiscordConfig;
use shoutrrr::transport::MockTransport;

mod common;
use common::parse;

#[test]
fn url_round_trips() {
    let raw = "discord://test-token@123456789?color=0xff0000&splitLines=No&title=Hi&username=bot";
    let config = DiscordConfig::from_url(&parse(raw)).unwrap();

    assert_eq!(config.webhook_id, "123456789");
    assert_eq!(config.token, "test-token");
    assert_eq!(config.color, 0xff_0000);
    assert!(!config.split_lines);
    assert_eq!(config.title, "Hi");
    assert_eq!(config.username, "bot");

    // Exact string round-trip and structural round-trip.
    assert_eq!(config.to_url().unwrap().as_str(), raw);
    assert_eq!(
        DiscordConfig::from_url(&config.to_url().unwrap()).unwrap(),
        config
    );
}

#[test]
fn raw_path_sets_json_mode() {
    let config = DiscordConfig::from_url(&parse("discord://tok@123456789/raw")).unwrap();
    assert!(config.json);
    assert_eq!(
        config.to_url().unwrap().as_str(),
        "discord://tok@123456789/raw"
    );
}

#[test]
fn missing_parts_are_rejected() {
    assert!(DiscordConfig::from_url(&parse("discord://@123456789")).is_err()); // no token
    assert!(DiscordConfig::from_url(&parse("discord://tok@123456789/bogus")).is_err());
    // bad path
}

#[test]
fn plain_single_line_uses_content() {
    let mock = MockTransport::new(204, "");
    pollster::block_on(shoutrrr::send(
        &mock,
        "discord://tok@123456789",
        "hello world",
    ))
    .unwrap();

    let request = mock.last_request().unwrap();
    assert_eq!(
        request.url,
        "https://discord.com/api/webhooks/123456789/tok"
    );

    let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(body["content"], "hello world");
    assert!(body.get("embeds").is_none());
}

#[test]
fn multiline_uses_embeds_with_default_color() {
    let mock = MockTransport::new(204, "");
    pollster::block_on(shoutrrr::send(
        &mock,
        "discord://tok@123456789",
        "line1\nline2",
    ))
    .unwrap();

    let body: serde_json::Value =
        serde_json::from_slice(&mock.last_request().unwrap().body).unwrap();
    let embeds = body["embeds"].as_array().unwrap();
    assert_eq!(embeds.len(), 2);
    assert_eq!(embeds[0]["description"], "line1");
    assert_eq!(embeds[1]["description"], "line2");
    assert_eq!(embeds[0]["color"].as_u64().unwrap(), 0x50_D9FF);
}

#[test]
fn raw_json_mode_sends_verbatim() {
    let mock = MockTransport::new(204, "");
    let payload = r#"{"content":"hi"}"#;
    pollster::block_on(shoutrrr::send(
        &mock,
        "discord://tok@123456789/raw",
        payload,
    ))
    .unwrap();

    let request = mock.last_request().unwrap();
    assert_eq!(
        request.url,
        "https://discord.com/api/webhooks/123456789/tok"
    );
    assert_eq!(String::from_utf8(request.body).unwrap(), payload);
}

#[test]
fn thread_id_is_appended_to_url() {
    let mock = MockTransport::new(204, "");
    pollster::block_on(shoutrrr::send(
        &mock,
        "discord://tok@123456789?thread_id=987",
        "hi",
    ))
    .unwrap();

    assert_eq!(
        mock.last_request().unwrap().url,
        "https://discord.com/api/webhooks/123456789/tok?thread_id=987"
    );
}

#[test]
fn large_message_splits_into_multiple_requests() {
    // splitLines=No forces chunk-based partitioning; >6000 runes forces a second batch.
    let big = "a".repeat(6100);
    let mock = MockTransport::new(204, "");
    pollster::block_on(shoutrrr::send(
        &mock,
        "discord://tok@123456789?splitLines=No",
        &big,
    ))
    .unwrap();

    assert_eq!(mock.request_count(), 2);
}

#[test]
fn non_success_status_is_an_error() {
    let mock = MockTransport::new(404, "not found");
    let err =
        pollster::block_on(shoutrrr::send(&mock, "discord://tok@123456789", "hi")).unwrap_err();
    assert!(err.to_string().contains("404"));
}

#[test]
fn query_keys_are_case_insensitive() {
    // Go lowercases its config key tags, so its generated URLs use lowercase keys; the port must
    // accept any case.
    let config =
        DiscordConfig::from_url(&parse("discord://tok@123456789?ColorError=0x111111")).unwrap();
    assert_eq!(config.color_error, 0x11_1111);
}

#[test]
fn param_with_invalid_value_is_rejected() {
    // A bad value on a key this service owns is a real mistake and must surface, not be swallowed.
    let mock = MockTransport::new(204, "");
    let params = Params::new().set("color", "notacolor");
    let err = pollster::block_on(shoutrrr::send_with_params(
        &mock,
        "discord://tok@123456789",
        "hi",
        &params,
    ))
    .unwrap_err();

    assert!(err.to_string().contains("color"));
    assert_eq!(mock.request_count(), 0); // failed before any HTTP
}

#[test]
fn param_with_unknown_key_is_ignored() {
    // A foreign key (e.g. another service's key during a fan-out) is skipped, not an error.
    let mock = MockTransport::new(204, "");
    let mut params = Params::new();
    params.insert("thread_ts", "123"); // a Slack key, meaningless to Discord
    pollster::block_on(shoutrrr::send_with_params(
        &mock,
        "discord://tok@123456789",
        "hi",
        &params,
    ))
    .unwrap();

    assert_eq!(mock.request_count(), 1);
}

#[test]
fn failing_batch_does_not_short_circuit() {
    // A >6000-rune splitLines=No message splits into two batches. The first failing must not stop
    // the second from being attempted; the first error is then returned. Each batch exhausts its
    // retries (1 initial + 5) against the always-500 mock, so both batches => 12 requests.
    let big = "a".repeat(6100);
    let mock = MockTransport::new(500, "boom");
    let err = pollster::block_on(shoutrrr::send(
        &mock,
        "discord://tok@123456789?splitLines=No",
        &big,
    ))
    .unwrap_err();

    assert_eq!(mock.request_count(), 12);
    assert!(err.to_string().contains("500"));
}

#[test]
fn retries_server_error_then_succeeds() {
    // 5xx is transient: back off and retry, then succeed on the 204. No error surfaces.
    let mock = MockTransport::sequence([(500, "boom"), (503, "boom"), (204, "")]);
    pollster::block_on(shoutrrr::send(&mock, "discord://tok@123456789", "hi")).unwrap();

    assert_eq!(mock.request_count(), 3);
    // Backed off twice (after the two failures), exponentially: 1s then 2s.
    assert_eq!(
        mock.slept(),
        vec![
            std::time::Duration::from_secs(1),
            std::time::Duration::from_secs(2)
        ]
    );
}

#[test]
fn gives_up_after_max_retries() {
    // Persistent 5xx: 1 initial attempt + 5 retries, then the last error is returned.
    let mock = MockTransport::new(500, "boom");
    let err =
        pollster::block_on(shoutrrr::send(&mock, "discord://tok@123456789", "hi")).unwrap_err();

    assert_eq!(mock.request_count(), 6);
    assert_eq!(mock.slept().len(), 5);
    assert!(err.to_string().contains("500"));
}

#[test]
fn honors_retry_after_header_on_rate_limit() {
    use shoutrrr::HttpResponse;

    // A 429 carrying Retry-After should wait exactly that long (not the exponential fallback).
    let mock = MockTransport::with_responses(vec![
        HttpResponse {
            status: 429,
            headers: vec![("Retry-After".to_string(), "1.5".to_string())],
            body: b"rate limited".to_vec(),
        },
        HttpResponse {
            status: 204,
            headers: Vec::new(),
            body: Vec::new(),
        },
    ]);
    pollster::block_on(shoutrrr::send(&mock, "discord://tok@123456789", "hi")).unwrap();

    assert_eq!(mock.request_count(), 2);
    assert_eq!(mock.slept(), vec![std::time::Duration::from_secs_f64(1.5)]);
}

#[test]
fn does_not_retry_client_error() {
    // 4xx (other than 429) is not transient: fail immediately with no backoff.
    let mock = MockTransport::new(403, "forbidden");
    let err =
        pollster::block_on(shoutrrr::send(&mock, "discord://tok@123456789", "hi")).unwrap_err();

    assert_eq!(mock.request_count(), 1);
    assert!(mock.slept().is_empty());
    assert!(err.to_string().contains("403"));
}

#[test]
fn multibyte_message_chunks_without_corruption() {
    // 6100 two-byte chars, chunked by rune. Go's `plain[len(plain)-omitted:]` slices a rune count
    // as a byte offset and corrupts multibyte input; this port maps rune index to byte offset, so
    // the concatenated chunks must reconstruct the original exactly.
    let big = "é".repeat(6100);
    let mock = MockTransport::new(204, "");
    pollster::block_on(shoutrrr::send(
        &mock,
        "discord://tok@123456789?splitLines=No",
        &big,
    ))
    .unwrap();

    assert_eq!(mock.request_count(), 2);

    let mut reconstructed = String::new();
    for request in mock.requests() {
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        if let Some(embeds) = body["embeds"].as_array() {
            for embed in embeds {
                reconstructed.push_str(embed["description"].as_str().unwrap());
            }
        } else {
            reconstructed.push_str(body["content"].as_str().unwrap());
        }
    }
    assert_eq!(reconstructed, big);
}
