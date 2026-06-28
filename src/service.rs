//! The object-safe [`Service`] trait implemented by every notification destination.

use async_trait::async_trait;

use crate::error::Result;
use crate::params::Params;
use crate::transport::HttpClient;

/// A notification service: knows how to turn a message into HTTP request(s) for one destination.
///
/// Object-safe so a [`Sender`](crate::router::Sender) can hold a heterogeneous
/// `Vec<Box<dyn Service>>`. `?Send` matches the transport model (WASM `fetch` futures are `!Send`).
#[async_trait(?Send)]
pub trait Service {
    /// The service's scheme identifier (e.g. `"slack"`).
    fn id(&self) -> &'static str;

    /// Deliver `message` to this service's destination, applying any `params` overrides.
    async fn send(&self, http: &dyn HttpClient, message: &str, params: &Params) -> Result<()>;
}
