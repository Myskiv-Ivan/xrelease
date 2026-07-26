//! Generic outgoing HTTP webhook sink.
//!
//! POSTs (or PUTs) each [`Event`] to an arbitrary URL. By default the body is
//! the canonical [`EventPayload`] JSON; a `template` turns it into any custom
//! string (e.g. a Slack `{"text": "..."}` block) via the shared
//! [`render_template`] dialect. This one sink unlocks integration with n8n,
//! Zapier, internal APIs, and anything that accepts an HTTP callback — without
//! new code per target.

use std::collections::HashMap;

use hmac::{Hmac, Mac};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE};
use reqwest::Method;
use sha2::Sha256;

use crate::error::NotifyError;
use crate::notify::payload::render_body_or_json;
use crate::notify::{Event, Notifier};

type HmacSha256 = Hmac<Sha256>;

/// Default header carrying the HMAC body signature (GitHub-compatible).
const DEFAULT_SIGNATURE_HEADER: &str = "X-Signature-256";

/// HMAC-SHA256 body signing for outbound webhooks so receivers can verify
/// authenticity. Mirrors the `X-Hub-Signature-256: sha256=<hex>` convention.
#[derive(Clone)]
struct WebhookSigning {
    secret: Vec<u8>,
    header: HeaderName,
}

impl std::fmt::Debug for WebhookSigning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebhookSigning")
            .field("secret", &"<redacted>")
            .field("header", &self.header)
            .finish()
    }
}

impl WebhookSigning {
    /// Compute the `sha256=<hex>` signature for `body`.
    fn sign(&self, body: &[u8]) -> String {
        let mut mac =
            HmacSha256::new_from_slice(&self.secret).expect("HMAC accepts keys of any length");
        mac.update(body);
        let digest = mac.finalize().into_bytes();
        let mut hex = String::with_capacity(7 + digest.len() * 2);
        hex.push_str("sha256=");
        for byte in digest {
            use std::fmt::Write as _;
            let _ = write!(hex, "{byte:02x}");
        }
        hex
    }
}

/// HTTP method allowed for webhook delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebhookMethod {
    /// HTTP `POST` (default).
    Post,
    /// HTTP `PUT`.
    Put,
}

impl WebhookMethod {
    fn as_reqwest(self) -> Method {
        match self {
            Self::Post => Method::POST,
            Self::Put => Method::PUT,
        }
    }
}

/// Sink that delivers events to a single HTTP endpoint.
#[derive(Debug, Clone)]
pub struct WebhookNotifier {
    client: reqwest::Client,
    url: String,
    method: WebhookMethod,
    headers: HeaderMap,
    content_type: String,
    /// Optional body template; `None` = canonical JSON payload.
    template: Option<String>,
    /// Optional HMAC body signing for receiver-side authenticity checks.
    signing: Option<WebhookSigning>,
    /// Label for diagnostics when several webhooks are configured.
    name: String,
}

impl WebhookNotifier {
    /// Construct a webhook sink, pre-parsing static headers into a [`HeaderMap`].
    ///
    /// # Errors
    /// Returns [`NotifyError::Misconfigured`] when the URL is empty or any
    /// configured header name/value is not a valid HTTP header.
    pub fn new(
        client: reqwest::Client,
        name: impl Into<String>,
        url: impl Into<String>,
        method: WebhookMethod,
        headers: &HashMap<String, String>,
        content_type: Option<&str>,
        template: Option<String>,
    ) -> Result<Self, NotifyError> {
        let url = url.into();
        if url.trim().is_empty() {
            return Err(NotifyError::Misconfigured(
                "webhook `url` must not be empty".to_owned(),
            ));
        }

        let mut header_map = HeaderMap::with_capacity(headers.len());
        for (key, value) in headers {
            let name = HeaderName::try_from(key.as_str()).map_err(|_| {
                NotifyError::Misconfigured(format!("invalid webhook header name `{key}`"))
            })?;
            let val = HeaderValue::try_from(value.as_str()).map_err(|_| {
                NotifyError::Misconfigured(format!("invalid webhook header value for `{key}`"))
            })?;
            header_map.insert(name, val);
        }

        Ok(Self {
            client,
            url,
            method,
            headers: header_map,
            content_type: content_type.unwrap_or("application/json").to_owned(),
            template,
            signing: None,
            name: name.into(),
        })
    }

    /// Enable HMAC-SHA256 body signing.
    ///
    /// When `secret` is non-empty, every request gains a signature header
    /// (`header` or [`DEFAULT_SIGNATURE_HEADER`]) of the form `sha256=<hex>`.
    /// An empty `secret` is a no-op so callers can pass resolved config blindly.
    ///
    /// # Errors
    /// Returns [`NotifyError::Misconfigured`] when `header` is not a valid HTTP
    /// header name.
    pub fn with_signing(mut self, secret: &str, header: Option<&str>) -> Result<Self, NotifyError> {
        if secret.trim().is_empty() {
            return Ok(self);
        }
        let header_name = header
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(DEFAULT_SIGNATURE_HEADER);
        let header = HeaderName::try_from(header_name).map_err(|_| {
            NotifyError::Misconfigured(format!("invalid webhook signature header `{header_name}`"))
        })?;
        self.signing = Some(WebhookSigning {
            secret: secret.as_bytes().to_vec(),
            header,
        });
        Ok(self)
    }

    fn render_body(&self, event: &Event) -> String {
        render_body_or_json(self.template.as_deref(), event)
    }
}

impl Notifier for WebhookNotifier {
    fn notify(
        &self,
        event: &Event,
    ) -> impl std::future::Future<Output = Result<(), NotifyError>> + Send {
        let client = self.client.clone();
        let url = self.url.clone();
        let method = self.method.as_reqwest();
        let mut headers = self.headers.clone();
        let content_type = self.content_type.clone();
        let body = self.render_body(event);

        // Sign synchronously while the rendered body is still in scope.
        if let Some(signing) = &self.signing {
            if let Ok(value) = HeaderValue::try_from(signing.sign(body.as_bytes())) {
                headers.insert(signing.header.clone(), value);
            }
        }

        async move {
            if !headers.contains_key(CONTENT_TYPE) {
                if let Ok(value) = HeaderValue::try_from(content_type.as_str()) {
                    headers.insert(CONTENT_TYPE, value);
                }
            }

            let response = client
                .request(method, &url)
                .headers(headers)
                .body(body)
                .send()
                .await?;

            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(NotifyError::Rejected {
                    backend: "webhook",
                    status: status.as_u16(),
                    body,
                });
            }
            Ok(())
        }
    }
}

impl WebhookNotifier {
    /// Diagnostic label for logs.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> Event {
        Event {
            source_id: "github:org/app".into(),
            source_kind: "GitHub".into(),
            title: "app: v2.0.0".into(),
            body: "notes".into(),
            url: Some("https://example.test/r".into()),
            routing_tag: None,
        }
    }

    #[test]
    fn new_should_reject_empty_url() {
        let client = reqwest::Client::new();
        let result = WebhookNotifier::new(
            client,
            "w",
            "",
            WebhookMethod::Post,
            &HashMap::new(),
            None,
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn new_should_reject_invalid_header() {
        let client = reqwest::Client::new();
        let mut headers = HashMap::new();
        headers.insert("Bad Header".to_owned(), "v".to_owned());
        let result = WebhookNotifier::new(
            client,
            "w",
            "https://example.test",
            WebhookMethod::Post,
            &headers,
            None,
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn render_body_should_default_to_json_payload() {
        let client = reqwest::Client::new();
        let notifier = WebhookNotifier::new(
            client,
            "w",
            "https://example.test",
            WebhookMethod::Post,
            &HashMap::new(),
            None,
            None,
        )
        .expect("notifier");
        let body = notifier.render_body(&event());
        let value: serde_json::Value = serde_json::from_str(&body).expect("json");
        assert_eq!(value["source_id"], "github:org/app");
        assert_eq!(value["title"], "app: v2.0.0");
    }

    #[test]
    fn with_signing_should_be_noop_for_empty_secret() {
        let notifier = WebhookNotifier::new(
            reqwest::Client::new(),
            "w",
            "https://example.test",
            WebhookMethod::Post,
            &HashMap::new(),
            None,
            None,
        )
        .expect("notifier")
        .with_signing("   ", None)
        .expect("signing");
        assert!(notifier.signing.is_none());
    }

    #[test]
    fn with_signing_should_reject_invalid_header() {
        let result = WebhookNotifier::new(
            reqwest::Client::new(),
            "w",
            "https://example.test",
            WebhookMethod::Post,
            &HashMap::new(),
            None,
            None,
        )
        .expect("notifier")
        .with_signing("secret", Some("Bad Header"));
        assert!(result.is_err());
    }

    #[test]
    fn render_body_should_use_template_when_set() {
        let client = reqwest::Client::new();
        let notifier = WebhookNotifier::new(
            client,
            "w",
            "https://example.test",
            WebhookMethod::Post,
            &HashMap::new(),
            Some("text/plain"),
            Some("{{source_kind}}: {{title}}".to_owned()),
        )
        .expect("notifier");
        assert_eq!(notifier.render_body(&event()), "GitHub: app: v2.0.0");
    }
}
