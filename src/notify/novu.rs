//! Novu notification workflow trigger.
//!
//! Delivers each [`Event`] with a single call to the
//! [Novu Event API](https://docs.novu.co/api-reference/events/trigger-event):
//!
//! `POST {base}/v1/events/trigger` with `Authorization: ApiKey <secret>` and a
//! JSON body `{ name, to, payload, transactionId }`.
//!
//! Novu owns multi-channel fan-out (email, SMS, Slack, in-app, …), preferences,
//! and digests. xrelease only triggers a workflow identified by
//! [`NovuNotifierOptions::workflow`].
//!
//! Retries from the notification outbox reuse a deterministic
//! `Idempotency-Key` / `transactionId` so Novu does not bill or deliver twice.

use sha2::{Digest, Sha256};

use crate::error::NotifyError;
use crate::notify::payload::render_template;
use crate::notify::{Event, Notifier};

/// Configuration for [`NovuNotifier`].
#[derive(Clone)]
pub struct NovuNotifierOptions {
    /// Diagnostic label for logs.
    pub name: String,
    /// Novu API base URL (Cloud US/EU or self-hosted), no trailing slash.
    pub base_url: String,
    /// Secret API key (`Authorization: ApiKey …`).
    pub api_key: String,
    /// Workflow trigger identifier (`name` in the Novu trigger body).
    pub workflow: String,
    /// Optional topic key template (e.g. `{{tag}}` or a fixed key).
    /// When set (non-empty after render), takes precedence over [`Self::subscriber_id`].
    pub topic_key: Option<String>,
    /// Optional subscriber id template when not targeting a topic.
    pub subscriber_id: Option<String>,
}

/// Sink that triggers a Novu workflow for each release event.
#[derive(Clone)]
pub struct NovuNotifier {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    workflow: String,
    topic_key: Option<String>,
    subscriber_id: Option<String>,
    name: String,
}

impl NovuNotifier {
    /// Construct a Novu Event API sink.
    ///
    /// # Errors
    /// Returns [`NotifyError::Misconfigured`] when a required field is empty or
    /// neither `topic_key` nor `subscriber_id` is configured.
    pub fn new(
        client: reqwest::Client,
        options: &NovuNotifierOptions,
    ) -> Result<Self, NotifyError> {
        let require = |field: &str, value: &str| -> Result<(), NotifyError> {
            if value.trim().is_empty() {
                Err(NotifyError::Misconfigured(format!(
                    "novu `{field}` must not be empty"
                )))
            } else {
                Ok(())
            }
        };
        require("base_url", &options.base_url)?;
        require("api_key", &options.api_key)?;
        require("workflow", &options.workflow)?;

        let topic = options
            .topic_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let subscriber = options
            .subscriber_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if topic.is_none() && subscriber.is_none() {
            return Err(NotifyError::Misconfigured(
                "novu requires `topic_key` or `subscriber_id`".to_owned(),
            ));
        }

        Ok(Self {
            client,
            base_url: options.base_url.trim_end_matches('/').to_owned(),
            api_key: options.api_key.trim().to_owned(),
            workflow: options.workflow.trim().to_owned(),
            topic_key: topic.map(str::to_owned),
            subscriber_id: subscriber.map(str::to_owned),
            name: options.name.clone(),
        })
    }

    /// Diagnostic label for logs.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Stable id for Novu `transactionId` + `Idempotency-Key` (retries safe).
    fn transaction_id(event: &Event) -> String {
        let mut hasher = Sha256::new();
        hasher.update(event.source_id.as_bytes());
        hasher.update(b"\0");
        hasher.update(event.title.as_bytes());
        hasher.update(b"\0");
        if let Some(url) = event.url.as_deref() {
            hasher.update(url.as_bytes());
        }
        format!("xr-{}", hex::encode(hasher.finalize()))
    }

    fn trigger_payload(&self, event: &Event) -> Result<serde_json::Value, NotifyError> {
        let to = if let Some(template) = &self.topic_key {
            let topic_key = render_template(template, event);
            if topic_key.trim().is_empty() {
                return Err(NotifyError::Misconfigured(
                    "novu `topic_key` rendered empty (set a fixed key or ensure the event has a routing tag for {{tag}})"
                        .to_owned(),
                ));
            }
            serde_json::json!({
                "type": "Topic",
                "topicKey": topic_key,
            })
        } else if let Some(template) = &self.subscriber_id {
            let subscriber_id = render_template(template, event);
            if subscriber_id.trim().is_empty() {
                return Err(NotifyError::Misconfigured(
                    "novu `subscriber_id` rendered empty".to_owned(),
                ));
            }
            serde_json::json!({ "subscriberId": subscriber_id })
        } else {
            return Err(NotifyError::Misconfigured(
                "novu requires `topic_key` or `subscriber_id`".to_owned(),
            ));
        };

        let mut payload = serde_json::Map::new();
        payload.insert("source_id".into(), serde_json::json!(event.source_id));
        payload.insert("kind".into(), serde_json::json!(event.source_kind));
        payload.insert("source_kind".into(), serde_json::json!(event.source_kind));
        payload.insert("title".into(), serde_json::json!(event.title));
        payload.insert("body".into(), serde_json::json!(event.body));
        if let Some(url) = event.url.as_deref() {
            payload.insert("url".into(), serde_json::json!(url));
        }
        if let Some(tag) = event.routing_tag.as_deref() {
            payload.insert("tag".into(), serde_json::json!(tag));
        }

        Ok(serde_json::json!({
            "name": self.workflow,
            "to": to,
            "payload": payload,
            "transactionId": Self::transaction_id(event),
        }))
    }
}

impl Notifier for NovuNotifier {
    fn notify(
        &self,
        event: &Event,
    ) -> impl std::future::Future<Output = Result<(), NotifyError>> + Send {
        let client = self.client.clone();
        let url = format!("{}/v1/events/trigger", self.base_url);
        let api_key = self.api_key.clone();
        let idempotency = Self::transaction_id(event);
        let payload_result = self.trigger_payload(event);

        async move {
            let payload = payload_result?;
            let response = client
                .post(&url)
                .header("Authorization", format!("ApiKey {api_key}"))
                .header("Idempotency-Key", &idempotency)
                .json(&payload)
                .send()
                .await?;
            let status = response.status();
            // 409: duplicate Idempotency-Key while the first request is still
            // processing (Novu docs) — treat as success so outbox retries do not
            // loop forever on an in-flight trigger.
            if status.as_u16() == 409 || status.is_success() {
                return Ok(());
            }
            let body = response.text().await.unwrap_or_default();
            Err(NotifyError::Rejected {
                backend: "novu",
                status: status.as_u16(),
                body: format!("POST /v1/events/trigger failed: {body}"),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> NovuNotifierOptions {
        NovuNotifierOptions {
            name: "novu".into(),
            base_url: "https://api.novu.co/".into(),
            api_key: "nv_test_key".into(),
            workflow: "xrelease-new-release".into(),
            topic_key: Some("{{tag}}".into()),
            subscriber_id: None,
        }
    }

    fn event() -> Event {
        Event {
            source_id: "github:org/app".into(),
            source_kind: "GitHub".into(),
            title: "v1.2.3".into(),
            body: "notes".into(),
            url: Some("https://example.com/r".into()),
            routing_tag: Some("platform-team".into()),
        }
    }

    #[test]
    fn new_should_reject_empty_api_key() {
        let mut opts = options();
        opts.api_key.clear();
        assert!(NovuNotifier::new(reqwest::Client::new(), &opts).is_err());
    }

    #[test]
    fn new_should_reject_missing_target() {
        let mut opts = options();
        opts.topic_key = None;
        opts.subscriber_id = None;
        assert!(NovuNotifier::new(reqwest::Client::new(), &opts).is_err());
    }

    #[test]
    fn new_should_trim_base_url_slash() {
        let notifier = NovuNotifier::new(reqwest::Client::new(), &options()).expect("notifier");
        assert_eq!(notifier.base_url, "https://api.novu.co");
    }

    #[test]
    fn trigger_payload_should_prefer_topic_over_subscriber() {
        let mut opts = options();
        opts.subscriber_id = Some("ops".into());
        let notifier = NovuNotifier::new(reqwest::Client::new(), &opts).expect("notifier");
        let payload = notifier.trigger_payload(&event()).expect("payload");
        assert_eq!(payload["name"], "xrelease-new-release");
        assert_eq!(payload["to"]["type"], "Topic");
        assert_eq!(payload["to"]["topicKey"], "platform-team");
        assert_eq!(payload["payload"]["title"], "v1.2.3");
        assert!(payload["transactionId"]
            .as_str()
            .unwrap()
            .starts_with("xr-"));
    }

    #[test]
    fn trigger_payload_should_use_subscriber_when_no_topic() {
        let mut opts = options();
        opts.topic_key = None;
        opts.subscriber_id = Some("ops-bot".into());
        let notifier = NovuNotifier::new(reqwest::Client::new(), &opts).expect("notifier");
        let payload = notifier.trigger_payload(&event()).expect("payload");
        assert_eq!(payload["to"]["subscriberId"], "ops-bot");
    }

    #[test]
    fn trigger_payload_should_fail_when_topic_template_empty() {
        let notifier = NovuNotifier::new(reqwest::Client::new(), &options()).expect("notifier");
        let mut ev = event();
        ev.routing_tag = None;
        let err = notifier.trigger_payload(&ev).expect_err("empty tag");
        assert!(err.to_string().contains("topic_key"));
    }

    #[test]
    fn transaction_id_should_be_stable_for_same_event() {
        let a = NovuNotifier::transaction_id(&event());
        let b = NovuNotifier::transaction_id(&event());
        assert_eq!(a, b);
        let mut other = event();
        other.title = "v9".into();
        assert_ne!(a, NovuNotifier::transaction_id(&other));
    }
}
