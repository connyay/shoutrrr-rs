# shoutrrr-rs

Send a notification to a destination described entirely by a URL. A Rust port of
[`nicholas-fedor/shoutrrr`](https://github.com/nicholas-fedor/shoutrrr).

```rust
shoutrrr::send(&http, "discord://token@webhookid", "Hello, world!").await?;
```

It runs anywhere `async`/`await` does, including `wasm32-unknown-unknown` on Cloudflare Workers. The
core has no `tokio` and no native-TLS, and the HTTP transport is a trait you plug in, so the same
code runs on a server or in a Worker.

So far it implements Slack, Discord, and a generic webhook target; the rest of the shoutrrr services
follow the same pattern.

## Install

```toml
[dependencies]
# Native (default): Slack + Discord + a reqwest transport + concurrent fan-out.
shoutrrr = "0.1"

# WASM / Cloudflare Workers: drop reqwest, use the Workers fetch transport.
shoutrrr = { version = "0.1", default-features = false, features = ["slack", "discord", "worker", "fanout"] }
```

To track unreleased changes, point at the repository instead:

```toml
shoutrrr = { git = "https://github.com/connyay/shoutrrr-rs" }
```

## Usage

### Native (reqwest)

```rust
use shoutrrr::transport::ReqwestClient;

let http = ReqwestClient::new();
shoutrrr::send(&http, "slack://hook:AAAAAAAAA-BBBBBBBBB-123456789123456789123456@webhook", "Deploy finished").await?;
```

### Fan out to many destinations

```rust
use shoutrrr::{Params, Sender};

let sender = Sender::from_urls([
    "slack://hook:AAAAAAAAA-BBBBBBBBB-123456789123456789123456@webhook",
    "discord://token@webhookid",
])?;
let results = sender.send(&http, "Build passed", &Params::new()).await; // Vec<Result<()>>
```

### Per-message overrides

```rust
let params = Params::new().with_title("Alert").set("color", "ff0000");
shoutrrr::send_with_params(&http, url, "Disk almost full", &params).await?;
```

### Cloudflare Workers

Enable the `worker` feature and use the bundled `WorkerClient`:

```rust
use shoutrrr::transport::WorkerClient;

let http = WorkerClient::new();
shoutrrr::send(&http, "discord://token@webhookid", "From the edge").await?;
```

Or implement the transport yourself over any HTTP client. That's all the core needs:

```rust
use async_trait::async_trait;
use shoutrrr::transport::{HttpClient, HttpRequest, HttpResponse};

struct MyClient;

#[async_trait(?Send)]
impl HttpClient for MyClient {
    async fn execute(&self, req: HttpRequest) -> shoutrrr::Result<HttpResponse> {
        // ...drive req.method/url/headers/body through your client, return the response...
        # todo!()
    }
}
```

## Supported services & URL formats

| Service | URL format |
| --- | --- |
| Slack (webhook) | `slack://hook:TOKEN_A-TOKEN_B-TOKEN_C@webhook` |
| Slack (API) | `slack://xoxb:TOKEN_A-TOKEN_B-TOKEN_C@CHANNEL` |
| Discord | `discord://token@webhookid` |
| Generic webhook | `generic://host/path?template=json` (or the `generic+https://host/path` shortcut) |

Common query parameters. Slack: `botname`, `icon`, `color`, `title`, `thread_ts`. Discord:
`title`, `username`, `avatar`, `color`/`colorError`/`colorWarn`/`colorInfo`/`colorDebug`,
`splitLines`, `json` (or a `/raw` path), `thread_id`. Generic: `template` (`json`/`JSON`, or a
custom name), `contenttype`, `method`, `disabletls`, `titlekey`, `messagekey`, plus `@Header=value`
custom headers and `$key=value` extra JSON fields.

### Generic webhook payloads

The generic service adapts to endpoints shoutrrr doesn't model directly. Without a `template` the
message is POSTed verbatim as `text/plain`; with `template=json` the params (title/message, keyed by
`titlekey`/`messagekey`) plus any `$extra` fields are marshaled to a flat JSON object:

```rust
let params = shoutrrr::Params::new().with_title("System Alert");
shoutrrr::send_with_params(
    &http,
    "generic://api.example.com/webhook?template=json&@Authorization=Bearer%20token",
    "Disk almost full",
    &params,
).await?; // POST {"message":"Disk almost full","title":"System Alert"} with an Authorization header
```

For full Go `text/template` payloads (`{{.message}}` etc.), enable the `generic-template` feature and
register a template by name on a directly-constructed service:

```rust
use shoutrrr::Service;
use shoutrrr::services::generic::GenericService;

let mut service = GenericService::from_url(&url::Url::parse("generic://host/hook?template=news")?)?;
service.set_template_string("news", "{{.title}} ==> {{.message}}")?;
service.send(&http, "it's today!", &params).await?;
```

## Feature flags

| Feature | Default | Purpose |
| --- | --- | --- |
| `slack` | ✓ | Slack service |
| `discord` | ✓ | Discord service |
| `generic` | ✓ | Generic webhook service (plain + JSON payloads) |
| `generic-template` | | Custom Go `text/template` payloads for the generic service (pulls in `gtmpl`; native-leaning) |
| `reqwest` | ✓ | `ReqwestClient` transport (native only) |
| `fanout` | ✓ | `Sender` for concurrent multi-destination delivery |
| `worker` | | `WorkerClient` transport for Cloudflare Workers (wasm32 only) |

## Testing

`MockTransport` (always available) records requests and returns a canned response, so you can assert
on the exact HTTP a service produces without a network or an async runtime:

```rust
use shoutrrr::transport::MockTransport;

let mock = MockTransport::new(200, "ok");
pollster::block_on(shoutrrr::send(&mock, "discord://token@123", "hi")).unwrap();
assert_eq!(mock.last_request().unwrap().url, "https://discord.com/api/webhooks/123/token");
```

See `examples/send.rs` (`cargo run --example send`) for a runnable end-to-end demo.

Discord retries transient failures (`429` honoring `Retry-After`, and `5xx`) with exponential
backoff. The wait is awaited through the transport's `sleep` (so it stays runtime-agnostic); the
bundled `ReqwestClient` and `WorkerClient` implement it, and `ReqwestClient` defaults to a 30s
request timeout.

## Roadmap

- Discord file attachments (multipart), and transport-level retry of dropped connections.
- A derive macro to cut per-service config boilerplate.
- The remaining shoutrrr services (Telegram, Matrix, Gotify, SMTP, Pushover, ...).

## License

MIT.
