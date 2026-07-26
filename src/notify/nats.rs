//! NATS publish sink (feature `nats`).
//!
//! Publishes the canonical [`EventPayload`] JSON (or a `template`d body) to a
//! NATS subject. The subject itself is templated, so `releases.{{kind}}` fans
//! events out to per-kind subjects. The connection is established lazily on the
//! first delivery and cached; `async-nats` handles reconnection internally.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::OnceCell;

use crate::error::NotifyError;
use crate::notify::payload::{render_body_or_json, render_template};
use crate::notify::{Event, Notifier};

/// Bound on a single connect+publish+flush so a stalled broker can't hang
/// delivery indefinitely (mirrors the Kafka producer send timeout).
const SEND_TIMEOUT: Duration = Duration::from_secs(30);

/// Sink that publishes events to a NATS server.
#[derive(Clone)]
pub struct NatsNotifier {
    url: String,
    /// Subject template (e.g. `releases.{{kind}}`).
    subject: String,
    /// Optional body template; `None` = canonical JSON payload.
    template: Option<String>,
    name: String,
    client: Arc<OnceCell<async_nats::Client>>,
}

fn backend_err(message: impl ToString) -> NotifyError {
    NotifyError::Backend {
        backend: "nats",
        message: message.to_string(),
    }
}

impl NatsNotifier {
    /// Construct a NATS sink (no connection is opened yet).
    ///
    /// # Errors
    /// Returns [`NotifyError::Misconfigured`] when `url` or `subject` is empty.
    pub fn new(
        name: impl Into<String>,
        url: impl Into<String>,
        subject: impl Into<String>,
        template: Option<String>,
    ) -> Result<Self, NotifyError> {
        let url = url.into();
        let subject = subject.into();
        if url.trim().is_empty() {
            return Err(NotifyError::Misconfigured(
                "nats `url` must not be empty".into(),
            ));
        }
        if subject.trim().is_empty() {
            return Err(NotifyError::Misconfigured(
                "nats `subject` must not be empty".into(),
            ));
        }
        Ok(Self {
            url,
            subject,
            template,
            name: name.into(),
            client: Arc::new(OnceCell::new()),
        })
    }

    fn payload(&self, event: &Event) -> String {
        render_body_or_json(self.template.as_deref(), event)
    }

    /// Diagnostic label for logs.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Notifier for NatsNotifier {
    fn notify(
        &self,
        event: &Event,
    ) -> impl std::future::Future<Output = Result<(), NotifyError>> + Send {
        let cell = Arc::clone(&self.client);
        let url = self.url.clone();
        let subject = render_template(&self.subject, event);
        let payload = self.payload(event);

        async move {
            let deliver = async move {
                let client = cell
                    .get_or_try_init(|| async {
                        async_nats::connect(&url).await.map_err(backend_err)
                    })
                    .await?;
                client
                    .publish(subject, payload.into())
                    .await
                    .map_err(backend_err)?;
                // Deliberate per-publish flush: `publish` only enqueues into the
                // client-side buffer, and the outbox marks this notification
                // `sent` as soon as we return Ok. Without the flush a crash
                // could drop a buffered message that the ledger already counted
                // as delivered — at-least-once beats batching throughput here.
                client.flush().await.map_err(backend_err)?;
                Ok::<(), NotifyError>(())
            };
            tokio::time::timeout(SEND_TIMEOUT, deliver)
                .await
                .map_err(|_| backend_err("nats delivery timed out"))?
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_should_reject_empty_subject() {
        let result = NatsNotifier::new("n", "nats://localhost:4222", "", None);
        assert!(result.is_err());
    }
}
