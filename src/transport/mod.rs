//! Pluggable HTTP transport.
//!
//! The core library never talks to a concrete HTTP client directly. Instead it builds an
//! [`HttpRequest`] value and hands it to an [`HttpClient`] implementation. This keeps the crate
//! free of `tokio`/native-TLS and lets it run anywhere `async`/`await` does, including
//! `wasm32-unknown-unknown` on Cloudflare Workers, where you implement [`HttpClient`] over the
//! Workers `Fetch` API.
//!
//! Three implementations ship with the crate:
//! - [`MockTransport`] records requests and returns a canned response. It's always available, so
//!   it works for tests on any target.
//! - [`ReqwestClient`] is a native `reqwest`-backed client, behind the `reqwest` feature.
//! - `WorkerClient` wraps the Cloudflare Workers `fetch` API, behind the `worker` feature on
//!   `wasm32` targets. Because it only compiles for `wasm32`, it is absent from the docs.rs API
//!   pages (which build for a native target); see the crate README for its usage.

use std::time::Duration;

use crate::error::{Error, Result};
use async_trait::async_trait;

mod mock;
pub use mock::MockTransport;

#[cfg(feature = "reqwest")]
mod reqwest;
#[cfg(feature = "reqwest")]
pub use self::reqwest::ReqwestClient;

#[cfg(all(feature = "worker", target_arch = "wasm32"))]
mod worker;
#[cfg(all(feature = "worker", target_arch = "wasm32"))]
pub use self::worker::WorkerClient;

/// HTTP method. Covers the standard verbs; most services only GET or POST, but the generic
/// webhook service honors a configurable method (`?method=PUT`, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// HTTP GET.
    Get,
    /// HTTP POST.
    Post,
    /// HTTP PUT.
    Put,
    /// HTTP PATCH.
    Patch,
    /// HTTP DELETE.
    Delete,
    /// HTTP HEAD.
    Head,
    /// HTTP OPTIONS.
    Options,
}

impl Method {
    /// The uppercase method name.
    pub fn as_str(&self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
            Method::Head => "HEAD",
            Method::Options => "OPTIONS",
        }
    }

    /// Parse a method name case-insensitively. Returns [`Error::InvalidConfig`] for a verb outside
    /// the standard set (Go's generic service forwards an arbitrary method string; this port limits
    /// it to the standard verbs, which covers every real webhook).
    pub fn parse(name: &str) -> Result<Self> {
        match name.to_ascii_uppercase().as_str() {
            "GET" => Ok(Method::Get),
            "POST" => Ok(Method::Post),
            "PUT" => Ok(Method::Put),
            "PATCH" => Ok(Method::Patch),
            "DELETE" => Ok(Method::Delete),
            "HEAD" => Ok(Method::Head),
            "OPTIONS" => Ok(Method::Options),
            other => Err(Error::InvalidConfig(format!(
                "unsupported HTTP method: {other:?}"
            ))),
        }
    }
}

/// A transport-agnostic HTTP request.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    /// Request method.
    pub method: Method,
    /// Fully-qualified request URL.
    pub url: String,
    /// Header name/value pairs.
    pub headers: Vec<(String, String)>,
    /// Raw request body.
    pub body: Vec<u8>,
}

impl HttpRequest {
    /// Start a request with the given `method` and `url`, an empty body, and no headers.
    pub fn new(method: Method, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    /// Start a POST request to `url` with an empty body and no headers.
    pub fn post(url: impl Into<String>) -> Self {
        Self::new(Method::Post, url)
    }

    /// Add a header (builder style).
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Set the raw request body without adding a `Content-Type` header (builder style); the caller
    /// sets the content type explicitly. Contrast [`json_body`](HttpRequest::json_body).
    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }

    /// Set a JSON body and the `Content-Type: application/json` header (builder style).
    pub fn json_body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self.headers
            .push(("Content-Type".into(), "application/json".into()));
        self
    }
}

/// A transport-agnostic HTTP response.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Header name/value pairs.
    pub headers: Vec<(String, String)>,
    /// Raw response body.
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// The response body decoded as lossy UTF-8.
    pub fn body_string(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// Whether the status code is in the 2xx range.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// Look up a header value, case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// An async HTTP client that executes a single [`HttpRequest`].
///
/// The `?Send` bound is deliberate. A Workers `fetch` future holds JavaScript values that aren't
/// `Send`, and even [`MockTransport`] borrows a `RefCell`, so requiring `Send` would force every
/// impl to add synchronization it doesn't need.
///
/// Dropping it costs nothing here. [`Sender`](crate::Sender) still overlaps requests to many
/// services with `join_all` on one thread: while one service waits on its response, the others get
/// sent and awaited too, so every request is in flight at once, which is all an I/O-bound send
/// needs. These futures can't cross threads, though, so on a multi-threaded native runtime you
/// `.await` a send inline (or `spawn_local` it) rather than `tokio::spawn`.
#[async_trait(?Send)]
pub trait HttpClient {
    /// Execute `request` and return the response, or a transport/HTTP error.
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse>;

    /// Suspend for `duration` before resolving. Retrying services (e.g. Discord on `429`/`5xx`) call
    /// this to back off between attempts, so it must yield to the runtime rather than block — there
    /// is no shared timer the core can rely on, hence it lives on the transport (the one component
    /// that already knows the runtime).
    ///
    /// The default returns immediately, so a custom transport that never overrides it makes retries
    /// fire back-to-back (still bounded by the retry cap). The bundled transports override it with a
    /// real timer ([`ReqwestClient`] via `tokio::time::sleep`, the Workers client via `Delay`); a
    /// transport used with a retrying service should do the same.
    async fn sleep(&self, duration: Duration) {
        let _ = duration;
    }
}
