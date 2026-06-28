//! Scheme-based dispatch and multi-destination fan-out.

use url::Url;

use crate::error::{Error, Result};
use crate::service::Service;

/// Construct a boxed [`Service`] from a raw service URL by dispatching on its scheme.
///
/// A `scheme+suffix` form (e.g. `slack+https`) dispatches on the leading scheme, like Go's router.
/// Returns [`Error::UnsupportedScheme`] if no enabled service matches.
pub fn service_from_url(raw_url: &str) -> Result<Box<dyn Service>> {
    let url = Url::parse(raw_url)?;
    let scheme = url.scheme();
    let base_scheme = scheme.split('+').next().unwrap_or(scheme);

    match base_scheme {
        #[cfg(feature = "slack")]
        "slack" => Ok(Box::new(crate::services::slack::SlackService::from_url(
            &url,
        )?)),
        #[cfg(feature = "discord")]
        "discord" => Ok(Box::new(
            crate::services::discord::DiscordService::from_url(&url)?,
        )),
        // The generic service also answers the `generic+<scheme>` shortcut form (e.g.
        // `generic+https://host/hook`), which `base_scheme` reduces to `generic`.
        #[cfg(feature = "generic")]
        "generic" => Ok(Box::new(
            crate::services::generic::GenericService::from_url(&url)?,
        )),
        other => Err(Error::UnsupportedScheme(other.to_string())),
    }
}

/// A reusable collection of services that delivers one message to many destinations.
#[cfg(feature = "fanout")]
pub struct Sender {
    services: Vec<Box<dyn Service>>,
}

#[cfg(feature = "fanout")]
impl Sender {
    /// Build a sender from a list of service URLs. Fails if any URL is invalid/unsupported.
    pub fn from_urls<I, S>(urls: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let services = urls
            .into_iter()
            .map(|u| service_from_url(u.as_ref()))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { services })
    }

    /// Number of destinations this sender will deliver to.
    pub fn len(&self) -> usize {
        self.services.len()
    }

    /// Whether the sender has no destinations.
    pub fn is_empty(&self) -> bool {
        self.services.is_empty()
    }

    /// Deliver `message` to every destination concurrently, returning a result per destination
    /// (in the order the URLs were supplied).
    pub async fn send(
        &self,
        http: &dyn crate::transport::HttpClient,
        message: &str,
        params: &crate::params::Params,
    ) -> Vec<Result<()>> {
        let futures = self
            .services
            .iter()
            .map(|service| service.send(http, message, params));
        futures_util::future::join_all(futures).await
    }
}
