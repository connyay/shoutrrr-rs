//! shoutrrr — send a notification to a destination described entirely by a URL.
//!
//! This is a Rust port of the Go library [`nicholas-fedor/shoutrrr`](https://github.com/nicholas-fedor/shoutrrr).
//! It runs anywhere `async`/`await` does, including `wasm32-unknown-unknown` on Cloudflare Workers.
//! The HTTP transport is pluggable (see [`transport::HttpClient`]), so the core carries no `tokio`
//! or native-TLS dependency.
//!
//! # Quick start
//!
//! ```no_run
//! # async fn run() -> shoutrrr::Result<()> {
//! use shoutrrr::transport::ReqwestClient;
//!
//! let http = ReqwestClient::new();
//! shoutrrr::send(&http, "discord://token@webhookid", "Hello, world!").await?;
//! # Ok(())
//! # }
//! ```
//!
//! On Cloudflare Workers, enable the `worker` feature for a ready-made `transport::WorkerClient`.
//! For any other single-threaded WASM runtime, implement [`transport::HttpClient`] over the
//! platform's `fetch` API yourself and pass it to [`send`].

#![forbid(unsafe_code)]

pub mod config;
pub mod error;
pub mod message;
pub mod params;
pub mod router;
pub mod service;
pub mod services;
pub mod transport;

pub use config::ServiceConfig;
pub use error::{Error, Result};
pub use params::Params;
pub use service::Service;
pub use transport::{HttpClient, HttpRequest, HttpResponse, Method};

#[cfg(feature = "fanout")]
pub use router::Sender;

/// Deliver `message` to the single destination described by `raw_url`.
///
/// Parses the URL, constructs the matching service, and sends using the provided transport.
pub async fn send(http: &dyn HttpClient, raw_url: &str, message: &str) -> Result<()> {
    send_with_params(http, raw_url, message, &Params::new()).await
}

/// Like [`send`], but applies per-message [`Params`] overrides (title, color, etc.).
pub async fn send_with_params(
    http: &dyn HttpClient,
    raw_url: &str,
    message: &str,
    params: &Params,
) -> Result<()> {
    let service = router::service_from_url(raw_url)?;
    service.send(http, message, params).await
}
