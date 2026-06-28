//! A recording HTTP client for tests, available on every target.

use std::cell::RefCell;
use std::time::Duration;

use async_trait::async_trait;

use super::{HttpClient, HttpRequest, HttpResponse};
use crate::error::Result;

/// An [`HttpClient`] that records every request it receives and replies with canned responses.
///
/// Single-threaded by design (`RefCell`), matching the `?Send` transport model and the
/// single-threaded WASM runtime. Drive it in tests with `pollster::block_on`.
///
/// Responses are returned in order; once the list is exhausted the last one repeats, so a
/// single-response mock answers every request identically. [`sleep`](HttpClient::sleep) returns
/// immediately but records the requested duration (see [`slept`](MockTransport::slept)), so retry
/// tests run instantly while still asserting on the computed backoff.
pub struct MockTransport {
    requests: RefCell<Vec<HttpRequest>>,
    responses: Vec<HttpResponse>,
    next: RefCell<usize>,
    slept: RefCell<Vec<Duration>>,
}

impl MockTransport {
    /// A mock that always returns `status` with the given body.
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self::with_responses(vec![HttpResponse {
            status,
            headers: Vec::new(),
            body: body.into(),
        }])
    }

    /// A mock that returns the given responses in order, repeating the last once exhausted.
    ///
    /// An empty list is treated as a single `200 OK`.
    pub fn with_responses(responses: Vec<HttpResponse>) -> Self {
        let responses = if responses.is_empty() {
            vec![HttpResponse {
                status: 200,
                headers: Vec::new(),
                body: Vec::new(),
            }]
        } else {
            responses
        };
        Self {
            requests: RefCell::new(Vec::new()),
            responses,
            next: RefCell::new(0),
            slept: RefCell::new(Vec::new()),
        }
    }

    /// A mock that returns each `(status, body)` in order, repeating the last once exhausted.
    pub fn sequence<I, B>(responses: I) -> Self
    where
        I: IntoIterator<Item = (u16, B)>,
        B: Into<Vec<u8>>,
    {
        Self::with_responses(
            responses
                .into_iter()
                .map(|(status, body)| HttpResponse {
                    status,
                    headers: Vec::new(),
                    body: body.into(),
                })
                .collect(),
        )
    }

    /// A mock that always returns `200 OK` with an empty body.
    pub fn ok() -> Self {
        Self::new(200, Vec::new())
    }

    /// A clone of the most recently received request, if any.
    pub fn last_request(&self) -> Option<HttpRequest> {
        self.requests.borrow().last().cloned()
    }

    /// All requests received so far, cloned.
    pub fn requests(&self) -> Vec<HttpRequest> {
        self.requests.borrow().clone()
    }

    /// How many requests have been received.
    pub fn request_count(&self) -> usize {
        self.requests.borrow().len()
    }

    /// The durations passed to [`sleep`](HttpClient::sleep), in order (one per backoff).
    pub fn slept(&self) -> Vec<Duration> {
        self.slept.borrow().clone()
    }
}

#[async_trait(?Send)]
impl HttpClient for MockTransport {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse> {
        self.requests.borrow_mut().push(request);
        let mut next = self.next.borrow_mut();
        let index = (*next).min(self.responses.len() - 1);
        *next += 1;
        Ok(self.responses[index].clone())
    }

    async fn sleep(&self, duration: Duration) {
        self.slept.borrow_mut().push(duration);
    }
}
