//! Apprise HTTP sink.
//!
//! Delivers notifications to a running [Apprise API server][api]
//! (`caronc/apprise`). Two modes:
//!
//! - **Stateless** — POST `/notify` with the target `urls` in the body. The URLs
//!   (which contain secrets) live in xrelease config, nothing is stored in
//!   Apprise.
//! - **Config key** — POST `/notify/<key>` against a persistent configuration
//!   previously registered in Apprise via `/add/<key>`. Secrets live in Apprise.
//!
//! [api]: https://github.com/caronc/apprise-api

use serde::Serialize;

use crate::error::NotifyError;
use crate::notify::{Event, Notifier};

/// Notifier that POSTs to an Apprise API server.
#[derive(Debug, Clone)]
pub struct AppriseNotifier {
    client: reqwest::Client,
    endpoint: String,
    target: Target,
    tag: Option<String>,
    format: String,
}

#[derive(Debug, Clone)]
enum Target {
    /// Stateless `/notify` with a comma-joined list of Apprise URLs.
    Stateless { urls: String },
    /// Persistent `/notify/<key>` against a stored Apprise config.
    Config { key: String },
}

impl AppriseNotifier {
    /// Construct a notifier.
    ///
    /// Exactly one of `config_key` or a non-empty `urls` must be provided; if
    /// both are present `config_key` wins.
    pub fn new(
        client: reqwest::Client,
        endpoint: &str,
        urls: &[String],
        config_key: Option<&str>,
        tag: Option<&str>,
        format: &str,
    ) -> Result<Self, NotifyError> {
        let target = if let Some(key) = config_key {
            Target::Config {
                key: key.to_owned(),
            }
        } else if urls.is_empty() {
            return Err(NotifyError::Misconfigured(
                "set either `apprise.urls` or `apprise.config_key`".to_owned(),
            ));
        } else {
            Target::Stateless {
                urls: urls.join(","),
            }
        };

        Ok(Self {
            client,
            endpoint: endpoint.trim_end_matches('/').to_owned(),
            target,
            tag: tag.map(str::to_owned),
            format: format.to_owned(),
        })
    }

    /// Probe whether the Apprise API endpoint responds (readiness check).
    pub async fn health_check(&self) -> bool {
        let url = format!("{}/", self.endpoint);
        self.client
            .get(&url)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
    }
}

/// Request body accepted by the Apprise `/notify` endpoints.
#[derive(Debug, Serialize)]
struct Payload<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    urls: Option<String>,
    title: &'a str,
    body: &'a str,
    /// One of `info` | `success` | `warning` | `failure`.
    #[serde(rename = "type")]
    notification_type: &'a str,
    format: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    tag: Option<&'a str>,
}

impl Notifier for AppriseNotifier {
    fn notify(
        &self,
        event: &Event,
    ) -> impl std::future::Future<Output = Result<(), NotifyError>> + Send {
        let (url, urls) = match &self.target {
            Target::Stateless { urls } => (format!("{}/notify", self.endpoint), Some(urls.clone())),
            Target::Config { key } => (format!("{}/notify/{}", self.endpoint, key), None),
        };
        let effective_tag = event.routing_tag.clone().or_else(|| self.tag.clone());
        let title = event.title.clone();
        let body = event.body.clone();
        let format = self.format.clone();
        let client = self.client.clone();

        async move {
            let payload = Payload {
                urls,
                title: &title,
                body: &body,
                notification_type: "success",
                format: &format,
                tag: effective_tag.as_deref(),
            };

            let response = client.post(&url).json(&payload).send().await?;
            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(NotifyError::Rejected {
                    backend: "apprise",
                    status: status.as_u16(),
                    body,
                });
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_event() -> Event {
        Event {
            source_id: "github:tokio-rs/tokio".into(),
            source_kind: "GitHub".into(),
            title: "tokio-rs/tokio: new release v1.0.0".into(),
            body: "body".into(),
            url: Some("https://example.test".into()),
            routing_tag: None,
        }
    }

    #[test]
    fn new_should_reject_empty_target() {
        let client = reqwest::Client::new();
        let result =
            AppriseNotifier::new(client, "http://apprise:8000", &[], None, None, "markdown");
        assert!(result.is_err());
    }

    #[test]
    fn stateless_payload_should_include_joined_urls() {
        let urls = ["tgram://a/b".to_string(), "mailto://c".to_string()];
        let payload = Payload {
            urls: Some(urls.join(",")),
            title: "t",
            body: "b",
            notification_type: "success",
            format: "markdown",
            tag: None,
        };
        let value = serde_json::to_value(&payload).expect("serialize");
        assert_eq!(value["urls"], "tgram://a/b,mailto://c");
    }

    #[test]
    fn config_target_payload_should_omit_urls() {
        let payload = Payload {
            urls: None,
            title: "t",
            body: "b",
            notification_type: "success",
            format: "markdown",
            tag: Some("all"),
        };
        let value = serde_json::to_value(&payload).expect("serialize");
        assert!(value.get("urls").is_none());
        assert_eq!(value["type"], "success");
    }

    #[test]
    fn event_is_constructible() {
        // Guards against the sample helper rotting if `Event` fields change.
        assert_eq!(sample_event().source_kind, "GitHub");
    }
}
