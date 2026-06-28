//! Generic webhook notification service.
//!
//! Sends to any webhook endpoint described by a `generic://host/path?...` URL (or the
//! `generic+https://host/path` shortcut). The payload format is selected by the `template` query
//! parameter:
//!
//! - With no `template`, the message is sent verbatim as `text/plain`.
//! - With `template=json` (or `JSON`), the params plus any `$extra` data are marshaled to a flat
//!   JSON object.
//! - With a custom template name, a Go-style template registered via
//!   [`set_template_string`](GenericService::set_template_string) is rendered (requires the
//!   `generic-template` feature).
//!
//! `@name` query keys become HTTP headers; `$name` keys become extra JSON data. See [`GenericConfig`]
//! for the configurable fields.

mod config;
mod query;

pub use config::GenericConfig;

use std::collections::BTreeMap;

use async_trait::async_trait;
use url::Url;

use crate::config::ServiceConfig;
use crate::error::{Error, Result};
use crate::params::Params;
use crate::service::Service;
use crate::transport::{HttpClient, HttpRequest, Method};

/// A configured generic webhook service instance.
pub struct GenericService {
    config: GenericConfig,
    /// Custom payload templates registered by name (see
    /// [`set_template_string`](Self::set_template_string)).
    #[cfg(feature = "generic-template")]
    templates: std::collections::HashMap<String, String>,
}

impl GenericService {
    /// Construct a service from a parsed `generic://` (or `generic+<scheme>://`) URL.
    pub fn from_url(url: &Url) -> Result<Self> {
        Ok(Self {
            config: GenericConfig::from_url(url)?,
            #[cfg(feature = "generic-template")]
            templates: std::collections::HashMap::new(),
        })
    }

    /// The parsed configuration.
    pub fn config(&self) -> &GenericConfig {
        &self.config
    }

    /// Register a custom Go-style template under `name`, referenced by `?template=<name>`.
    ///
    /// The template receives the send params (message under `messagekey`, title under `titlekey`,
    /// plus any other params) as its data context and uses [Go `text/template`] syntax, e.g.
    /// `{{.title}} ==> {{.message}}`. A malformed template surfaces its error when a message is
    /// sent.
    ///
    /// [Go `text/template`]: https://pkg.go.dev/text/template
    #[cfg(feature = "generic-template")]
    pub fn set_template_string(
        &mut self,
        name: impl Into<String>,
        template: impl Into<String>,
    ) -> Result<()> {
        self.templates.insert(name.into(), template.into());
        Ok(())
    }

    /// Build the request body for the configured template, given the prepared send params.
    fn build_payload(
        &self,
        config: &GenericConfig,
        send_params: &BTreeMap<String, String>,
    ) -> Result<Vec<u8>> {
        match config.template.as_str() {
            // No template: the message (stored under `message_key`) is the raw body.
            "" => Ok(send_params
                .get(&config.message_key)
                .cloned()
                .unwrap_or_default()
                .into_bytes()),
            // JSON template: marshal the params plus `$extra` data to a flat object.
            "json" | "JSON" => {
                let mut data = send_params.clone();
                for (key, value) in config.extra_data() {
                    data.insert(key.clone(), value.clone());
                }
                Ok(serde_json::to_vec(&data)?)
            }
            // Custom registered template.
            name => self.render_custom_template(name, send_params),
        }
    }

    #[cfg(feature = "generic-template")]
    fn render_custom_template(
        &self,
        name: &str,
        send_params: &BTreeMap<String, String>,
    ) -> Result<Vec<u8>> {
        let source = self.templates.get(name).ok_or_else(|| {
            Error::InvalidConfig(format!("template has not been loaded: {name:?}"))
        })?;
        // `gtmpl` accepts a `HashMap<String, String>` directly as its data context.
        let context: std::collections::HashMap<String, String> = send_params
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        let rendered = gtmpl::template(source, context)
            .map_err(|e| Error::InvalidConfig(format!("executing template {name:?}: {e}")))?;
        Ok(rendered.into_bytes())
    }

    #[cfg(not(feature = "generic-template"))]
    fn render_custom_template(
        &self,
        name: &str,
        _send_params: &BTreeMap<String, String>,
    ) -> Result<Vec<u8>> {
        Err(Error::InvalidConfig(format!(
            "template has not been loaded: {name:?} (enable the `generic-template` feature for custom templates)"
        )))
    }
}

#[async_trait(?Send)]
impl Service for GenericService {
    fn id(&self) -> &'static str {
        "generic"
    }

    async fn send(&self, http: &dyn HttpClient, message: &str, params: &Params) -> Result<()> {
        // Per-message params can override config props (e.g. `titlekey`); unknown keys are ignored.
        let mut config = self.config.clone();
        config.apply_params(params)?;

        let send_params = create_send_params(&config, params, message);
        let body = self.build_payload(&config, &send_params)?;

        // Go forces `text/plain` for the no-template (raw message) case, otherwise the configured
        // content type applies to both `Content-Type` and `Accept`.
        let content_type = if config.template.is_empty() {
            "text/plain"
        } else {
            config.content_type.as_str()
        };

        let url = config.webhook_url()?.to_string();
        let method = Method::parse(&config.request_method)?;

        let mut request = HttpRequest::new(method, url)
            .header("Content-Type", content_type)
            .header("Accept", content_type)
            .body(body);
        for (name, value) in config.headers() {
            request = request.header(name, value);
        }

        let response = http.execute(request).await?;
        // Go treats any status < 400 as success — including 3xx — unlike the 2xx-only services.
        if response.status >= 400 {
            return Err(Error::Http {
                status: response.status,
                body: response.body_string(),
            });
        }
        Ok(())
    }
}

/// Prepare the params sent to the payload builder: copy the caller's params, renaming the `title`
/// key to the configured [`title_key`](GenericConfig::title_key), then add the message under
/// [`message_key`](GenericConfig::message_key). Port of `createSendParams`.
fn create_send_params(
    config: &GenericConfig,
    params: &Params,
    message: &str,
) -> BTreeMap<String, String> {
    let mut send_params = BTreeMap::new();
    for (key, value) in params.iter() {
        let key = if key == "title" {
            config.title_key.clone()
        } else {
            key.to_string()
        };
        send_params.insert(key, value.to_string());
    }
    send_params.insert(config.message_key.clone(), message.to_string());
    send_params
}
