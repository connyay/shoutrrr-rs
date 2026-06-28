//! A Cloudflare Workers `Fetch`-backed [`HttpClient`].
//!
//! Only compiled for `wasm32` with the `worker` feature. It builds a standard [`http::Request`] and
//! lets the `worker` crate's `http` integration convert it to a JS request, so there's no
//! hand-rolled `Headers`/`RequestInit`/`JsValue` plumbing and the body stays raw bytes.

use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::Full;
use worker::{Delay, Fetch, Request as WorkerRequest};

use super::{HttpClient, HttpRequest, HttpResponse};
use crate::error::{Error, Result};

/// An [`HttpClient`] implemented on top of the Cloudflare Workers `fetch` API.
#[derive(Debug, Default, Clone)]
pub struct WorkerClient;

impl WorkerClient {
    /// Create a new Workers fetch client.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait(?Send)]
impl HttpClient for WorkerClient {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse> {
        let mut builder = http::Request::builder()
            .method(request.method.as_str())
            .uri(request.url.as_str());
        for (name, value) in &request.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        let http_request = builder
            .body(Full::<Bytes>::new(Bytes::from(request.body)))
            .map_err(|e| Error::Transport(e.to_string()))?;

        let worker_request =
            WorkerRequest::try_from(http_request).map_err(|e| Error::Transport(e.to_string()))?;

        let mut response = Fetch::Request(worker_request)
            .send()
            .await
            .map_err(|e| Error::Transport(e.to_string()))?;

        let status = response.status_code();
        let headers = response.headers().entries().collect();
        let body = response
            .bytes()
            .await
            .map_err(|e| Error::Transport(e.to_string()))?;

        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }

    async fn sleep(&self, duration: std::time::Duration) {
        Delay::from(duration).await;
    }
}
