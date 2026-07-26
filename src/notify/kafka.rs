//! Kafka publish sink (feature `kafka`).
//!
//! Produces the canonical [`EventPayload`] JSON (or a `template`d body) to a
//! Kafka topic via `rdkafka`'s `FutureProducer`. An optional templated `key`
//! controls partitioning (e.g. `{{source_id}}` keeps a source's events ordered).
//! The producer is created up front (no broker I/O) and cloned cheaply.

use std::time::Duration;

use rdkafka::config::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};

use crate::error::NotifyError;
use crate::notify::payload::{render_body_or_json, render_template};
use crate::notify::{Event, Notifier};

/// How long to wait for the producer queue before failing a send.
const SEND_TIMEOUT: Duration = Duration::from_secs(30);

/// Sink that produces events to a Kafka topic.
#[derive(Clone)]
pub struct KafkaNotifier {
    producer: FutureProducer,
    topic: String,
    /// Optional partition key template.
    key: Option<String>,
    /// Optional body template; `None` = canonical JSON payload.
    template: Option<String>,
    name: String,
}

fn backend_err(message: impl ToString) -> NotifyError {
    NotifyError::Backend {
        backend: "kafka",
        message: message.to_string(),
    }
}

impl KafkaNotifier {
    /// Build a Kafka producer for `brokers`.
    ///
    /// # Errors
    /// Returns [`NotifyError::Misconfigured`] when `brokers`/`topic` are empty,
    /// or [`NotifyError::Backend`] when the producer cannot be created.
    pub fn new(
        name: impl Into<String>,
        brokers: &[String],
        topic: impl Into<String>,
        key: Option<String>,
        template: Option<String>,
    ) -> Result<Self, NotifyError> {
        let topic = topic.into();
        if brokers.is_empty() {
            return Err(NotifyError::Misconfigured(
                "kafka `brokers` must not be empty".into(),
            ));
        }
        if topic.trim().is_empty() {
            return Err(NotifyError::Misconfigured(
                "kafka `topic` must not be empty".into(),
            ));
        }
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", brokers.join(","))
            .create()
            .map_err(backend_err)?;
        Ok(Self {
            producer,
            topic,
            key,
            template,
            name: name.into(),
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

impl Notifier for KafkaNotifier {
    fn notify(
        &self,
        event: &Event,
    ) -> impl std::future::Future<Output = Result<(), NotifyError>> + Send {
        let producer = self.producer.clone();
        let topic = self.topic.clone();
        let key = self.key.as_deref().map(|tpl| render_template(tpl, event));
        let payload = self.payload(event);

        async move {
            let mut record = FutureRecord::to(&topic).payload(&payload);
            if let Some(key) = &key {
                record = record.key(key);
            }
            producer
                .send(record, SEND_TIMEOUT)
                .await
                .map_err(|(err, _msg)| backend_err(err))?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_should_reject_empty_brokers() {
        let result = KafkaNotifier::new("k", &[], "topic", None, None);
        assert!(result.is_err());
    }
}
