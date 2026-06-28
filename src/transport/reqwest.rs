//! A `reqwest`-backed [`HttpClient`] for native targets.

use async_trait::async_trait;

use super::{HttpClient, HttpRequest, HttpResponse, Method};
use crate::error::{Error, Result};

/// Default per-request timeout. A bare `reqwest::Client` has none, so a stalled connection would
/// hang the send forever; this bounds it. Override via [`ReqwestClient::from_client`].
const DEFAULT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// An [`HttpClient`] implemented on top of [`reqwest::Client`].
///
/// Intended for native (non-WASM) use. On Cloudflare Workers, implement [`HttpClient`] over the
/// Workers `Fetch` API instead.
#[derive(Debug, Clone)]
pub struct ReqwestClient {
    inner: ::reqwest::Client,
}

impl Default for ReqwestClient {
    fn default() -> Self {
        let inner = ::reqwest::Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .build()
            // Mirrors `reqwest::Client::default()`, which also unwraps a builder that only fails if
            // the TLS backend can't initialize.
            .expect("default reqwest client should build");
        Self { inner }
    }
}

impl ReqwestClient {
    /// Create a client with a sensible default `reqwest::Client` (30s request timeout).
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a client wrapping a pre-configured `reqwest::Client` (timeouts, proxies, etc.).
    pub fn from_client(inner: ::reqwest::Client) -> Self {
        Self { inner }
    }
}

#[async_trait(?Send)]
impl HttpClient for ReqwestClient {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse> {
        let method = match request.method {
            Method::Get => ::reqwest::Method::GET,
            Method::Post => ::reqwest::Method::POST,
            Method::Put => ::reqwest::Method::PUT,
            Method::Patch => ::reqwest::Method::PATCH,
            Method::Delete => ::reqwest::Method::DELETE,
            Method::Head => ::reqwest::Method::HEAD,
            Method::Options => ::reqwest::Method::OPTIONS,
        };

        let mut builder = self.inner.request(method, &request.url);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        if !request.body.is_empty() {
            builder = builder.body(request.body);
        }

        let response = builder
            .send()
            .await
            .map_err(|e| Error::Transport(e.to_string()))?;

        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_string(),
                    v.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect();
        let body = response
            .bytes()
            .await
            .map_err(|e| Error::Transport(e.to_string()))?
            .to_vec();

        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }

    async fn sleep(&self, duration: std::time::Duration) {
        tokio::time::sleep(duration).await;
    }
}
