//! RabbitMQ (AMQP 0-9-1) publish sink (feature `rabbitmq`).
//!
//! Publishes the canonical [`EventPayload`] JSON (or a `template`d body) to an
//! exchange with a templated routing key (e.g. `{{kind}}`). The connection and
//! channel are opened lazily and re-established when the channel drops, so a
//! broker restart is recovered on the next delivery (failed sends meanwhile go
//! through the notification outbox retry loop).

use std::sync::Arc;
use std::time::Duration;

use lapin::options::BasicPublishOptions;
use lapin::{BasicProperties, Channel, Connection, ConnectionProperties};
use tokio::sync::Mutex;

use crate::error::NotifyError;
use crate::notify::payload::{render_body_or_json, render_template};
use crate::notify::{Event, Notifier};

/// Bound on a single connect or publish-confirm so a stalled broker can't hang
/// delivery indefinitely (mirrors the Kafka producer send timeout).
const SEND_TIMEOUT: Duration = Duration::from_secs(30);

/// Sink that publishes events to a RabbitMQ exchange.
#[derive(Clone)]
pub struct RabbitMqNotifier {
    url: String,
    exchange: String,
    /// Routing key template (e.g. `{{kind}}`).
    routing_key: String,
    /// Optional body template; `None` = canonical JSON payload.
    template: Option<String>,
    name: String,
    // Keep the connection alive alongside the channel; dropping the connection
    // closes the channel.
    conn: Arc<Mutex<Option<(Connection, Channel)>>>,
}

fn backend_err(message: impl ToString) -> NotifyError {
    NotifyError::Backend {
        backend: "rabbitmq",
        message: message.to_string(),
    }
}

impl RabbitMqNotifier {
    /// Construct an AMQP sink (no connection is opened yet).
    ///
    /// # Errors
    /// Returns [`NotifyError::Misconfigured`] when `url` or `routing_key` is empty.
    pub fn new(
        name: impl Into<String>,
        url: impl Into<String>,
        exchange: impl Into<String>,
        routing_key: impl Into<String>,
        template: Option<String>,
    ) -> Result<Self, NotifyError> {
        let url = url.into();
        let routing_key = routing_key.into();
        if url.trim().is_empty() {
            return Err(NotifyError::Misconfigured(
                "rabbitmq `url` must not be empty".into(),
            ));
        }
        if routing_key.trim().is_empty() {
            return Err(NotifyError::Misconfigured(
                "rabbitmq `routing_key` must not be empty".into(),
            ));
        }
        Ok(Self {
            url,
            exchange: exchange.into(),
            routing_key,
            template,
            name: name.into(),
            conn: Arc::new(Mutex::new(None)),
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

async fn open_channel(url: &str) -> Result<(Connection, Channel), NotifyError> {
    let conn = Connection::connect(url, ConnectionProperties::default())
        .await
        .map_err(backend_err)?;
    let channel = conn.create_channel().await.map_err(backend_err)?;
    Ok((conn, channel))
}

impl Notifier for RabbitMqNotifier {
    fn notify(
        &self,
        event: &Event,
    ) -> impl std::future::Future<Output = Result<(), NotifyError>> + Send {
        let slot = Arc::clone(&self.conn);
        let url = self.url.clone();
        let exchange = self.exchange.clone();
        let routing_key = render_template(&self.routing_key, event);
        let payload = self.payload(event);

        async move {
            // Hold the lock only to (re)establish and clone the channel — not
            // across the publish. `lapin::Channel` is a cheap `Arc` handle, so
            // concurrent deliveries through this sink no longer serialize on the
            // mutex and a stalled confirm cannot block the slot.
            let channel = {
                let mut guard = slot.lock().await;
                let healthy = guard
                    .as_ref()
                    .is_some_and(|(_, channel)| channel.status().connected());
                if !healthy {
                    let opened = tokio::time::timeout(SEND_TIMEOUT, open_channel(&url))
                        .await
                        .map_err(|_| backend_err("rabbitmq connect timed out"))??;
                    *guard = Some(opened);
                }
                // Channel was just opened above when the guard was empty/unhealthy.
                let channel = guard
                    .as_ref()
                    .ok_or_else(|| backend_err("rabbitmq channel missing"))?
                    .1
                    .clone();
                channel
            };

            let publish = async {
                channel
                    .basic_publish(
                        &exchange,
                        &routing_key,
                        BasicPublishOptions::default(),
                        payload.as_bytes(),
                        BasicProperties::default(),
                    )
                    .await
                    .map_err(backend_err)?
                    .await
                    .map_err(backend_err)?;
                Ok::<(), NotifyError>(())
            };
            tokio::time::timeout(SEND_TIMEOUT, publish)
                .await
                .map_err(|_| backend_err("rabbitmq delivery timed out"))?
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_should_reject_empty_routing_key() {
        let result = RabbitMqNotifier::new("r", "amqp://localhost:5672/%2f", "ex", "", None);
        assert!(result.is_err());
    }
}
