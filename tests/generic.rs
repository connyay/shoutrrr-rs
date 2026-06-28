//! Generic webhook service tests: URL round-trips, the `__`-key escaping, `@header`/`$extra`
//! handling, payload shapes (plain / JSON / custom template), and the send path.

use shoutrrr::Params;
use shoutrrr::config::ServiceConfig;
use shoutrrr::services::generic::GenericConfig;
use shoutrrr::transport::MockTransport;

mod common;
use common::parse;

// --- URL parsing & round-tripping -------------------------------------------------------------

#[test]
fn url_round_trips_with_customs_and_escaping() {
    // A `__title` escaped key, an `@header`, a `$extra`, and several config props must all survive
    // a parse -> render round-trip. Only the `$`/`@`/`/` characters gain percent-encoding.
    let raw = "generic://user:pass@host.tld/api/v1/webhook?$context=inside-joke&@Authorization=frend&__title=w&contenttype=a%2Fb&template=f&title=t";
    let expected = "generic://user:pass@host.tld/api/v1/webhook?%24context=inside-joke&%40Authorization=frend&__title=w&contenttype=a%2Fb&template=f&title=t";

    let config = GenericConfig::from_url(&parse(raw)).unwrap();
    assert_eq!(config.content_type, "a/b");
    assert_eq!(config.template, "f");
    assert_eq!(config.title, "t");

    assert_eq!(config.to_url().unwrap().as_str(), expected);
}

#[test]
fn escaped_key_is_not_consumed_as_config() {
    // `__template` is the escaped form of `template`; it must pass through to the webhook query
    // rather than set the config field, and round-trip back to `__template`.
    let service_url = "generic://example.com/?__template=passed";
    let config = GenericConfig::from_url(&parse(service_url)).unwrap();

    assert_ne!(config.template, "passed");
    assert_eq!(
        config.webhook_url().unwrap().as_str(),
        "https://example.com/?template=passed"
    );
    assert_eq!(config.to_url().unwrap().as_str(), service_url);
}

#[test]
fn handles_both_escaped_and_property_keys() {
    let config = GenericConfig::from_url(&parse(
        "generic://example.com/?__template=passed&template=captured",
    ))
    .unwrap();

    // The bare `template` sets the config; the escaped `__template` passes through to the webhook.
    assert_eq!(config.template, "captured");
    assert_eq!(
        config.webhook_url().unwrap().as_str(),
        "https://example.com/?template=passed"
    );
}

#[test]
fn unknown_query_keys_pass_through_to_webhook() {
    // Unlike Slack/Discord (which reject unknown keys), the generic service keeps non-config query
    // params on the webhook URL.
    let config = GenericConfig::from_url(&parse("generic://example.com/path?foo=bar")).unwrap();
    assert_eq!(
        config.webhook_url().unwrap().as_str(),
        "https://example.com/path?foo=bar"
    );
}

#[test]
fn disabletls_selects_http_scheme() {
    let off = GenericConfig::from_url(&parse("generic://test.tld?disabletls=yes")).unwrap();
    assert!(off.disable_tls);
    assert_eq!(off.webhook_url().unwrap().scheme(), "http");

    let on = GenericConfig::from_url(&parse("generic://test.tld")).unwrap();
    assert!(!on.disable_tls);
    assert_eq!(on.webhook_url().unwrap().scheme(), "https");
}

#[test]
fn shortcut_form_uses_embedded_url_as_webhook() {
    // `generic+https://host` is a shortcut: the embedded URL is the webhook target. (The url crate
    // normalizes the empty path to "/", a cosmetic divergence from Go's "https://test.tld".)
    let config = GenericConfig::from_url(&parse("generic+https://test.tld")).unwrap();
    assert_eq!(config.webhook_url().unwrap().as_str(), "https://test.tld/");
    assert!(!config.disable_tls);

    // The embedded scheme decides TLS: `generic+http` disables it.
    let http = GenericConfig::from_url(&parse("generic+http://test.tld")).unwrap();
    assert!(http.disable_tls);
    assert_eq!(http.webhook_url().unwrap().scheme(), "http");
}

#[test]
fn from_webhook_url_extracts_config_and_leftover_query() {
    let webhook = parse(
        "https://example.com/webhook?template=json&contenttype=application/json&method=POST&titlekey=customtitle&messagekey=custommessage&extra=param",
    );
    let config = GenericConfig::from_webhook_url(&webhook).unwrap();

    assert_eq!(config.template, "json");
    assert_eq!(config.content_type, "application/json");
    assert_eq!(config.request_method, "POST");
    assert_eq!(config.title_key, "customtitle");
    assert_eq!(config.message_key, "custommessage");
    // The non-config `extra=param` stays on the webhook URL.
    assert_eq!(config.webhook_url().unwrap().query(), Some("extra=param"));
}

#[test]
fn from_webhook_url_captures_headers_and_extra_data() {
    let webhook = parse(
        "https://example.com/webhook?@Authorization=Bearer%20token&$extraKey=extraValue&template=json",
    );
    let config = GenericConfig::from_webhook_url(&webhook).unwrap();

    assert_eq!(config.template, "json");
    // All `@`/`$` and config keys consumed — nothing left on the webhook query.
    assert_eq!(config.webhook_url().unwrap().query(), None);
}

#[test]
fn query_keys_are_case_insensitive() {
    let config =
        GenericConfig::from_url(&parse("generic://host.tld/hook?TitleKey=Header&Method=put"))
            .unwrap();
    assert_eq!(config.title_key, "Header");
    assert_eq!(config.request_method, "put");
}

// --- Payload shapes (asserted through the send path) ------------------------------------------

#[test]
fn no_template_sends_message_as_plain_text() {
    let mock = MockTransport::new(200, "");
    pollster::block_on(shoutrrr::send(
        &mock,
        "generic://host.tld/webhook",
        "test message",
    ))
    .unwrap();

    let request = mock.last_request().unwrap();
    assert_eq!(request.url, "https://host.tld/webhook");
    assert_eq!(request.method, shoutrrr::Method::Post);
    assert_eq!(String::from_utf8(request.body).unwrap(), "test message");
    assert!(
        request
            .headers
            .iter()
            .any(|(k, v)| k == "Content-Type" && v == "text/plain")
    );
}

#[test]
fn json_template_builds_object_with_title_and_message() {
    let mock = MockTransport::new(200, "");
    let params = Params::new().with_title("test title");
    pollster::block_on(shoutrrr::send_with_params(
        &mock,
        "generic://host.tld/webhook?template=JSON",
        "test message",
        &params,
    ))
    .unwrap();

    let body: serde_json::Value =
        serde_json::from_slice(&mock.last_request().unwrap().body).unwrap();
    assert_eq!(body["title"], "test title");
    assert_eq!(body["message"], "test message");
}

#[test]
fn json_template_honors_alternate_keys() {
    let mock = MockTransport::new(200, "");
    let params = Params::new().with_title("test title");
    pollster::block_on(shoutrrr::send_with_params(
        &mock,
        "generic://host.tld/webhook?template=JSON&messagekey=body&titlekey=header",
        "test message",
        &params,
    ))
    .unwrap();

    let body: serde_json::Value =
        serde_json::from_slice(&mock.last_request().unwrap().body).unwrap();
    assert_eq!(body["header"], "test title");
    assert_eq!(body["body"], "test message");
    assert!(body.get("title").is_none());
}

#[test]
fn json_template_includes_extra_data() {
    let mock = MockTransport::new(200, "");
    pollster::block_on(shoutrrr::send(
        &mock,
        "generic://host.tld/webhook?template=json&$context=inside+joke",
        "Message",
    ))
    .unwrap();

    let body: serde_json::Value =
        serde_json::from_slice(&mock.last_request().unwrap().body).unwrap();
    assert_eq!(body["message"], "Message");
    assert_eq!(body["context"], "inside joke");
}

#[test]
fn unknown_template_is_an_error() {
    let mock = MockTransport::new(200, "");
    let err = pollster::block_on(shoutrrr::send(
        &mock,
        "generic://host.tld/webhook?template=missing",
        "hi",
    ))
    .unwrap_err();

    assert!(err.to_string().contains("template"));
    assert_eq!(mock.request_count(), 0); // failed before any HTTP
}

// --- Send path --------------------------------------------------------------------------------

#[test]
fn includes_custom_headers() {
    let mock = MockTransport::new(200, "");
    pollster::block_on(shoutrrr::send(
        &mock,
        "generic://host.tld/webhook?@authorization=frend",
        "Message",
    ))
    .unwrap();

    // `@authorization` is normalized to the `Authorization` header key.
    let request = mock.last_request().unwrap();
    assert!(
        request
            .headers
            .iter()
            .any(|(k, v)| k == "Authorization" && v == "frend")
    );
}

#[test]
fn uses_the_configured_method() {
    let mock = MockTransport::new(200, "");
    pollster::block_on(shoutrrr::send(
        &mock,
        "generic://host.tld/webhook?method=GET",
        "Message",
    ))
    .unwrap();

    assert_eq!(mock.last_request().unwrap().method, shoutrrr::Method::Get);
}

#[test]
fn non_success_status_is_an_error() {
    let mock = MockTransport::new(404, "not found");
    let err =
        pollster::block_on(shoutrrr::send(&mock, "generic://host.tld/webhook", "hi")).unwrap_err();
    assert!(err.to_string().contains("404"));
}

#[test]
fn redirect_status_is_treated_as_success() {
    // Go's generic service only errors on status >= 400, so a 3xx is accepted.
    let mock = MockTransport::new(302, "");
    pollster::block_on(shoutrrr::send(&mock, "generic://host.tld/webhook", "hi")).unwrap();
    assert_eq!(mock.request_count(), 1);
}

#[test]
fn does_not_mutate_the_given_params() {
    let mock = MockTransport::new(200, "");
    let params = Params::new().with_title("TITLE");
    pollster::block_on(shoutrrr::send_with_params(
        &mock,
        "generic://host.tld/webhook?method=GET",
        "Message",
        &params,
    ))
    .unwrap();

    assert_eq!(params, Params::new().with_title("TITLE"));
}

// --- Custom Go templates (generic-template feature) -------------------------------------------

#[cfg(feature = "generic-template")]
#[test]
fn custom_template_is_rendered() {
    use shoutrrr::Service;
    use shoutrrr::services::generic::GenericService;

    let mock = MockTransport::new(200, "");
    let mut service =
        GenericService::from_url(&parse("generic://host.tld/webhook?template=news")).unwrap();
    service
        .set_template_string("news", "{{.title}} ==> {{.message}}")
        .unwrap();

    let params = Params::new().with_title("BREAKING NEWS");
    pollster::block_on(service.send(&mock, "it's today!", &params)).unwrap();

    assert_eq!(
        String::from_utf8(mock.last_request().unwrap().body).unwrap(),
        "BREAKING NEWS ==> it's today!"
    );
}

#[cfg(feature = "generic-template")]
#[test]
fn custom_template_with_only_message() {
    use shoutrrr::Service;
    use shoutrrr::services::generic::GenericService;

    let mock = MockTransport::new(200, "");
    let mut service =
        GenericService::from_url(&parse("generic://host.tld/webhook?template=arrows")).unwrap();
    service
        .set_template_string("arrows", "==> {{.message}} <==")
        .unwrap();

    pollster::block_on(service.send(&mock, "LOOK AT ME", &Params::new())).unwrap();

    assert_eq!(
        String::from_utf8(mock.last_request().unwrap().body).unwrap(),
        "==> LOOK AT ME <=="
    );
}
