//! Builds and sends notifications through a mock transport, so it needs no real network or
//! credentials.
//!
//! Run with: `cargo run --example send`
//!
//! In a real native app you'd swap `MockTransport` for `shoutrrr::transport::ReqwestClient`; on
//! Cloudflare Workers, for `shoutrrr::transport::WorkerClient` (or your own `HttpClient`).

use shoutrrr::transport::MockTransport;
use shoutrrr::{Params, Sender};

fn main() {
    // A mock transport records every request and returns a canned 200/"ok" response.
    let mock = MockTransport::new(200, "ok");

    let slack_url =
        "slack://hook:AAAAAAAAA-BBBBBBBBB-123456789123456789123456@webhook?color=36a64f";
    let discord_url = "discord://token@123456789";
    // A generic webhook posting a JSON body, with a custom auth header.
    let generic_url = "generic://example.com/webhook?template=json&@Authorization=Bearer%20token";

    // 1. Send to a single destination.
    pollster::block_on(shoutrrr::send(&mock, slack_url, "Deploy finished :rocket:"))
        .expect("slack send");

    // 2. Fan out to several destinations concurrently.
    let sender = Sender::from_urls([slack_url, discord_url, generic_url]).expect("valid URLs");
    let results = pollster::block_on(sender.send(&mock, "Build #42 passed", &Params::new()));

    println!("fan-out to {} destination(s):", results.len());
    for (i, result) in results.iter().enumerate() {
        match result {
            Ok(()) => println!("  [{i}] ok"),
            Err(e) => println!("  [{i}] error: {e}"),
        }
    }

    println!("\ncaptured {} request(s):", mock.request_count());
    for request in mock.requests() {
        println!("  {} {}", request.method.as_str(), request.url);
        println!("    body: {}", String::from_utf8_lossy(&request.body));
    }
}
