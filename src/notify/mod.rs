//! Notification delivery.
//!
//! The pipeline depends on the [`Notifier`] trait, not on any single backend, so
//! every sink — Apprise ([`apprise::AppriseNotifier`]), eXpress BotX chat
//! ([`express::ExpressNotifier`]), Novu ([`novu::NovuNotifier`]), Slack
//! ([`slack::SlackNotifier`]), Telegram ([`telegram::TelegramNotifier`]),
//! generic webhooks ([`webhook::WebhookNotifier`]), SMTP, message brokers
//! (Kafka / NATS / RabbitMQ), the dry-run [`LogNotifier`], and test fakes —
//! is interchangeable.
//!
//! Multiple sinks are unified behind a single static-dispatch [`Sink`] enum
//! (mirroring [`crate::sources::Provider`]) and fanned out by
//! [`CompositeNotifier`]. All sinks share one canonical event shape and
//! templating via [`payload`], so adding a backend never re-invents payloads,
//! retries, or the outbox — those live one layer up in the pipeline.

pub mod apprise;
pub mod express;
pub mod novu;
pub mod payload;
pub mod routing;
pub mod slack;
pub mod smtp;
pub mod telegram;
pub mod webhook;
pub mod kafka;
pub mod nats;
pub mod rabbitmq;

pub use apprise::AppriseNotifier;
pub use express::{ExpressNotifier, ExpressNotifierOptions};
pub use novu::{NovuNotifier, NovuNotifierOptions};
pub use slack::{SlackNotifier, SlackNotifierOptions};
pub use smtp::{SmtpNotifier, SmtpNotifierOptions, SmtpTlsMode};
pub use telegram::{TelegramNotifier, TelegramNotifierOptions};
pub use webhook::{WebhookMethod, WebhookNotifier};

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::future::join_all;

use crate::error::NotifyError;

/// Default consecutive failures before [`CompositeNotifier`] trips a sink's
/// circuit breaker, when config omits the knob.
pub const DEFAULT_SINK_BREAKER_FAILURE_THRESHOLD: u32 = 5;

/// Default circuit-breaker cooldown (seconds) when config omits the knob.
pub const DEFAULT_SINK_BREAKER_COOLDOWN_SECS: u32 = 300;

/// A single notification to deliver.
#[derive(Debug, Clone)]
pub struct Event {
    /// Originating source id.
    pub source_id: String,
    /// Human-readable source kind (e.g. `GitHub`).
    pub source_kind: String,
    /// Short notification title.
    pub title: String,
    /// Notification body (Markdown).
    pub body: String,
    /// Canonical link, when available.
    pub url: Option<String>,
    /// Per-source team routing tag.
    ///
    /// Matches notifier `tags`. For Apprise this selects an Apprise tag; for
    /// webhooks and brokers it is exposed as `{tag}` in templates and the
    /// `tag` JSON field. Set from [`crate::pipeline::Watch::routing_tag`].
    pub routing_tag: Option<String>,
}

/// Something that can deliver an [`Event`].
pub trait Notifier: Send + Sync {
    /// Deliver one event. The pipeline persists to the notification outbox before
    /// calling this trait; retries drain the outbox on failure.
    fn notify(
        &self,
        event: &Event,
    ) -> impl std::future::Future<Output = Result<(), NotifyError>> + Send;
}

/// A no-op notifier that only logs what it *would* send. Used by tests.
#[derive(Debug, Clone, Copy)]
pub struct LogNotifier;

impl Notifier for LogNotifier {
    fn notify(
        &self,
        event: &Event,
    ) -> impl std::future::Future<Output = Result<(), NotifyError>> + Send {
        let title = event.title.clone();
        async move {
            tracing::info!(title = %title, "dry-run: would notify");
            Ok(())
        }
    }
}

/// One configured delivery backend. Static dispatch over a closed set of sinks
/// (Best Practices Ch. 6) — the same pattern as [`crate::sources::Provider`].
///
/// Every documented sink kind (including Kafka / NATS / RabbitMQ) is always
/// compiled into the binary — one product surface for local builds, GHCR, and
/// GitHub Release archives.
#[derive(Clone)]
pub enum Sink {
    /// Apprise HTTP API server (fan-out to 100+ channels).
    Apprise(AppriseNotifier),
    /// Generic outgoing HTTP webhook.
    Webhook(WebhookNotifier),
    /// eXpress (BotX) chat notification.
    Express(ExpressNotifier),
    /// Novu workflow trigger (multi-channel notification platform).
    Novu(NovuNotifier),
    /// Slack Incoming Webhook or Bot `chat.postMessage`.
    Slack(SlackNotifier),
    /// Telegram Bot API `sendMessage`.
    Telegram(TelegramNotifier),
    /// Direct SMTP email delivery.
    Smtp(SmtpNotifier),
    /// Kafka topic producer.
    Kafka(kafka::KafkaNotifier),
    /// NATS subject publisher.
    Nats(nats::NatsNotifier),
    /// RabbitMQ (AMQP) exchange publisher.
    RabbitMq(rabbitmq::RabbitMqNotifier),
}

impl Sink {
    /// Machine-readable backend label, used in fan-out error messages and logs.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Apprise(_) => "apprise",
            Self::Webhook(_) => "webhook",
            Self::Express(_) => "express",
            Self::Novu(_) => "novu",
            Self::Slack(_) => "slack",
            Self::Telegram(_) => "telegram",
            Self::Smtp(_) => "smtp",
            Self::Kafka(_) => "kafka",
            Self::Nats(_) => "nats",
            Self::RabbitMq(_) => "rabbitmq",
        }
    }

    /// Operator-facing label (config `name`, else the kind).
    #[must_use]
    pub fn label(&self) -> String {
        let named = match self {
            Self::Apprise(_) => None,
            Self::Webhook(n) => Some(n.name()),
            Self::Express(n) => Some(n.name()),
            Self::Novu(n) => Some(n.name()),
            Self::Slack(n) => Some(n.name()),
            Self::Telegram(n) => Some(n.name()),
            Self::Smtp(n) => Some(n.name()),
            Self::Kafka(n) => Some(n.name()),
            Self::Nats(n) => Some(n.name()),
            Self::RabbitMq(n) => Some(n.name()),
        };
        named
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map_or_else(|| self.kind().to_owned(), ToOwned::to_owned)
    }

    /// Best-effort readiness probe.
    ///
    /// - **Apprise** — live HTTP check against the Apprise endpoint.
    /// - **Other sinks** — report ready by configuration presence. Transient
    ///   broker/SMTP blips must not flap `/ready`; delivery failures stay
    ///   visible via the outbox and `xrelease_outbox_*` metrics.
    pub async fn health_check(&self) -> bool {
        match self {
            Self::Apprise(n) => n.health_check().await,
            Self::Webhook(_)
            | Self::Express(_)
            | Self::Novu(_)
            | Self::Slack(_)
            | Self::Telegram(_)
            | Self::Smtp(_)
            | Self::Kafka(_)
            | Self::Nats(_)
            | Self::RabbitMq(_) => true,
        }
    }
}

impl Notifier for Sink {
    async fn notify(&self, event: &Event) -> Result<(), NotifyError> {
        match self {
            Self::Apprise(n) => n.notify(event).await,
            Self::Webhook(n) => n.notify(event).await,
            Self::Express(n) => n.notify(event).await,
            Self::Novu(n) => n.notify(event).await,
            Self::Slack(n) => n.notify(event).await,
            Self::Telegram(n) => n.notify(event).await,
            Self::Smtp(n) => n.notify(event).await,
            Self::Kafka(n) => n.notify(event).await,
            Self::Nats(n) => n.notify(event).await,
            Self::RabbitMq(n) => n.notify(event).await,
        }
    }
}

/// One delivery backend with an optional team routing tag filter.
///
/// When `tags` is empty the sink receives every event (broadcast). When
/// set, only events whose [`Event::routing_tag`] matches one of the tags are
/// delivered — used to route team chats and tagged Apprise / Novu targets.
#[derive(Clone)]
pub struct RoutedSink {
    inner: Sink,
    tags: Vec<String>,
}

impl RoutedSink {
    /// Wrap a sink with a routing tag filter (`tags` empty = all events).
    #[must_use]
    pub fn new(inner: Sink, tags: Vec<String>) -> Self {
        Self { inner, tags }
    }

    /// Routing tags for this sink (empty = match all).
    #[must_use]
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    /// Whether `event` should be delivered through this sink.
    #[must_use]
    pub fn matches(&self, event: &Event) -> bool {
        routing::event_matches_sink_tags(event, &self.tags)
    }

    /// Backend label (e.g. `apprise`, `express`).
    #[must_use]
    pub fn kind(&self) -> &'static str {
        self.inner.kind()
    }

    /// Operator-facing label (config `name`, else the kind).
    #[must_use]
    pub fn label(&self) -> String {
        self.inner.label()
    }

    /// Best-effort readiness probe.
    pub async fn health_check(&self) -> bool {
        self.inner.health_check().await
    }

    /// Deliver when the routing tag matches.
    pub async fn notify(&self, event: &Event) -> Result<(), NotifyError> {
        self.inner.notify(event).await
    }
}

/// One configured sink as seen by the management API / UI.
#[derive(Debug, Clone)]
pub struct SinkView {
    pub index: usize,
    pub kind: &'static str,
    pub name: String,
    pub tags: Vec<String>,
}

/// Circuit-breaker thresholds shared by every sink.
///
/// Process-local and **not** persisted: a restart re-probes every sink from a
/// clean slate, which is the correct behavior for "is this endpoint currently
/// reachable" — unlike the outbox delivery lease, this is not a
/// cross-process coordination mechanism, so there is no reason to route it
/// through the database clock.
#[derive(Debug, Clone, Copy)]
pub struct BreakerConfig {
    /// Consecutive failures before a sink is skipped (breaker opens).
    pub failure_threshold: u32,
    /// How long a tripped sink is skipped before one trial call is let through.
    pub cooldown: Duration,
}

impl Default for BreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: DEFAULT_SINK_BREAKER_FAILURE_THRESHOLD,
            cooldown: Duration::from_secs(u64::from(DEFAULT_SINK_BREAKER_COOLDOWN_SECS)),
        }
    }
}

/// Per-sink failure tracking for [`CompositeNotifier::notify_partial`].
///
/// Three conceptual states without a separate enum tag: `open_until` is `None`
/// in the closed state; once set, calls are rejected until [`Instant::now`]
/// reaches it (open), at which point exactly one trial call is meant to pass
/// through (half-open) — success clears `open_until`, failure re-arms it for
/// another cooldown. Under concurrent delivery to the same sink index, two
/// callers can both observe the cooldown as elapsed and both proceed as the
/// "one" trial call; the consequence is at most one extra probe against a
/// possibly-still-broken sink, not a correctness issue, so this is accepted
/// rather than solved with a compare-and-swap permit.
#[derive(Debug)]
struct SinkBreaker {
    consecutive_failures: u32,
    open_until: Option<Instant>,
}

impl SinkBreaker {
    fn new() -> Self {
        Self {
            consecutive_failures: 0,
            open_until: None,
        }
    }

    /// Whether a call should be attempted right now.
    fn allows(&self, now: Instant) -> bool {
        match self.open_until {
            None => true,
            Some(until) => now >= until,
        }
    }

    /// Record a call outcome, transitioning state.
    ///
    /// Returns `true` when this call newly opens (or re-opens after a failed
    /// half-open trial) the breaker — used for ops meta-alerts.
    fn record(&mut self, now: Instant, success: bool, config: &BreakerConfig) -> bool {
        if success {
            self.consecutive_failures = 0;
            self.open_until = None;
            return false;
        }
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures >= config.failure_threshold {
            let already_open = self.open_until.is_some_and(|until| now < until);
            self.open_until = Some(now + config.cooldown);
            return !already_open;
        }
        false
    }
}

/// Fan-out notifier with per-sink team tag routing.
///
/// Per-sink delivery is tracked in `notification_sink_delivery` so retries only
/// hit sinks that have not yet acknowledged the event. When all matching sinks
/// succeed, the parent outbox row is marked `sent`. Each sink also carries its
/// own [`SinkBreaker`]: a sink that fails repeatedly is skipped for a cooldown
/// instead of paying a connect/request timeout on every call (see
/// [`Self::notify_partial`]).
#[derive(Clone)]
pub struct CompositeNotifier {
    sinks: Vec<RoutedSink>,
    breakers: Vec<Arc<Mutex<SinkBreaker>>>,
    breaker_config: BreakerConfig,
    /// Labels of sinks whose breakers newly opened; drained by outbox flush.
    newly_opened: Arc<Mutex<Vec<String>>>,
}

impl CompositeNotifier {
    /// Wrap routed sinks with default circuit-breaker thresholds.
    /// An empty list is valid for idle UI-first boot (`source = "api"`).
    #[must_use]
    pub fn new(sinks: Vec<RoutedSink>) -> Self {
        Self::with_breaker_config(sinks, BreakerConfig::default())
    }

    /// Wrap routed sinks with explicit circuit-breaker thresholds.
    /// An empty list is valid for idle UI-first boot (`source = "api"`).
    #[must_use]
    pub fn with_breaker_config(sinks: Vec<RoutedSink>, breaker_config: BreakerConfig) -> Self {
        let breakers = sinks
            .iter()
            .map(|_| Arc::new(Mutex::new(SinkBreaker::new())))
            .collect();
        Self {
            sinks,
            breakers,
            breaker_config,
            newly_opened: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Number of configured sinks.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sinks.len()
    }

    /// Whether no sinks are configured (idle until UI/CI apply).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sinks.is_empty()
    }

    /// Backend labels of all configured sinks (for startup logging).
    #[must_use]
    pub fn kinds(&self) -> Vec<&'static str> {
        self.sinks.iter().map(RoutedSink::kind).collect()
    }

    /// Indices of sinks that should receive `event` (team routing).
    #[must_use]
    pub fn matching_indices(&self, event: &Event) -> Vec<usize> {
        self.sinks
            .iter()
            .enumerate()
            .filter(|(_, sink)| sink.matches(event))
            .map(|(index, _)| index)
            .collect()
    }

    /// Backend label at `index`.
    #[must_use]
    pub fn kind_at(&self, index: usize) -> &'static str {
        self.sinks
            .get(index)
            .map(RoutedSink::kind)
            .unwrap_or("unknown")
    }

    /// Operator-facing sink label at `index` (config `name`, else the kind).
    #[must_use]
    pub fn label_at(&self, index: usize) -> String {
        self.sinks
            .get(index)
            .map(RoutedSink::label)
            .unwrap_or_else(|| "unknown".into())
    }

    /// Live circuit-breaker open/closed view for every configured sink.
    #[must_use]
    pub fn breaker_states(&self) -> Vec<crate::metrics::BreakerOpenView> {
        let now = Instant::now();
        self.sinks
            .iter()
            .enumerate()
            .map(|(index, sink)| {
                let open = self
                    .breakers
                    .get(index)
                    .map(|breaker| {
                        let state = breaker.lock().unwrap_or_else(|e| e.into_inner());
                        !state.allows(now)
                    })
                    .unwrap_or(false);
                crate::metrics::BreakerOpenView {
                    kind: sink.kind(),
                    label: sink.label(),
                    open,
                }
            })
            .collect()
    }

    /// Snapshot of configured sinks for the management UI.
    #[must_use]
    pub fn describe(&self) -> Vec<SinkView> {
        self.sinks
            .iter()
            .enumerate()
            .map(|(index, sink)| SinkView {
                index,
                kind: sink.kind(),
                name: sink.label(),
                tags: sink.tags().to_vec(),
            })
            .collect()
    }

    /// Deliver a test event to one sink, bypassing team-tag filtering and the
    /// circuit breaker — operators need a forced probe after fixing a channel.
    pub async fn test_at(&self, index: usize, event: &Event) -> Result<(), NotifyError> {
        let Some(sink) = self.sinks.get(index) else {
            return Err(NotifyError::Misconfigured(format!(
                "sink index {index} out of range ({} configured)",
                self.sinks.len()
            )));
        };
        let result = sink.notify(event).await;
        if let Some(label) = self.record_breaker_outcome(index, result.is_ok()) {
            self.newly_opened
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(label);
        }
        result
    }

    /// Deliver a test event to every configured sink (forced, see [`Self::test_at`]).
    pub async fn test_all(&self, event: &Event) -> Vec<(usize, Result<(), NotifyError>)> {
        let indices: Vec<_> = (0..self.sinks.len()).collect();
        let deliveries = indices
            .iter()
            .map(|&index| async move { (index, self.test_at(index, event).await) });
        join_all(deliveries).await
    }

    /// Readiness: true only when every sink reports healthy.
    pub async fn health_check(&self) -> bool {
        for sink in &self.sinks {
            if !sink.health_check().await {
                return false;
            }
        }
        true
    }

    /// Deliver to a subset of sinks by index (used by the per-sink outbox ledger).
    ///
    /// Delivery is **concurrent**: sinks are independent, so a slow one must not
    /// delay the others (worst-case latency is the slowest sink, not the sum).
    /// [`join_all`] preserves input order, so the returned `(index, result)`
    /// pairs line up with `sink_indices` and the per-sink ledger is unaffected.
    ///
    /// A sink whose circuit breaker is open is skipped without a network call
    /// — the caller sees [`NotifyError::BreakerOpen`], which flows through the
    /// same per-sink failure/backoff path as a real delivery error.
    pub async fn notify_partial(
        &self,
        event: &Event,
        sink_indices: &[usize],
    ) -> Vec<(usize, Result<(), NotifyError>)> {
        let deliveries = sink_indices.iter().map(|&index| async move {
            let Some(sink) = self.sinks.get(index) else {
                return (
                    index,
                    Err(NotifyError::Misconfigured(format!(
                        "sink index {index} out of range ({} configured)",
                        self.sinks.len()
                    ))),
                );
            };
            if !sink.matches(event) {
                return (index, Ok(()));
            }
            if !self.breaker_allows(index) {
                return (
                    index,
                    Err(NotifyError::BreakerOpen {
                        backend: sink.kind(),
                    }),
                );
            }
            let result = sink.notify(event).await;
            if let Some(label) = self.record_breaker_outcome(index, result.is_ok()) {
                self.newly_opened
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(label);
            }
            (index, result)
        });
        join_all(deliveries).await
    }

    /// Whether sink `index`'s circuit breaker currently allows a call.
    /// Never holds the lock across an `.await` point — decide, release, call.
    fn breaker_allows(&self, index: usize) -> bool {
        let Some(breaker) = self.breakers.get(index) else {
            return true;
        };
        let state = breaker.lock().unwrap_or_else(|e| e.into_inner());
        state.allows(Instant::now())
    }

    /// Record a sink call outcome into its breaker, transitioning state.
    ///
    /// Returns the sink label when the breaker newly opens (for ops alerts).
    fn record_breaker_outcome(&self, index: usize, success: bool) -> Option<String> {
        let breaker = self.breakers.get(index)?;
        let mut state = breaker.lock().unwrap_or_else(|e| e.into_inner());
        let newly_opened = state.record(Instant::now(), success, &self.breaker_config);
        newly_opened.then(|| self.label_at(index))
    }

    /// Drain labels of sinks whose breakers newly opened since the last drain.
    ///
    /// Populated by [`Self::notify_partial`] / [`Self::test_at`] so callers
    /// (outbox flush) can emit ops meta-alerts without changing return types.
    #[must_use]
    pub fn drain_newly_opened_breakers(&self) -> Vec<String> {
        std::mem::take(&mut *self.newly_opened.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

impl Notifier for CompositeNotifier {
    /// Broadcast to every matching sink. Implemented on top of
    /// [`Self::notify_partial`] so there is a single, concurrent fan-out path —
    /// the aggregate result and the per-sink ledger can never diverge.
    async fn notify(&self, event: &Event) -> Result<(), NotifyError> {
        let matching = self.matching_indices(event);
        let total = matching.len();
        if total == 0 {
            return Err(NotifyError::Misconfigured(format!(
                "no notifier matches team routing tag {:?}",
                event.routing_tag
            )));
        }
        let failures: Vec<String> = self
            .notify_partial(event, &matching)
            .await
            .into_iter()
            .filter_map(|(index, result)| {
                result
                    .err()
                    .map(|err| format!("{}: {err}", self.kind_at(index)))
            })
            .collect();
        if failures.is_empty() {
            Ok(())
        } else {
            Err(NotifyError::FanOut {
                failed: failures.len(),
                total,
                details: failures.join("; "),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(tag: Option<&str>) -> Event {
        Event {
            source_id: "s".into(),
            source_kind: "GitHub".into(),
            title: "t".into(),
            body: "b".into(),
            url: None,
            routing_tag: tag.map(ToOwned::to_owned),
        }
    }

    // --- SinkBreaker state machine: pure, no sleeping — time is an explicit
    // parameter, same pattern as `pipeline::backoff_secs`. ---

    #[test]
    fn breaker_should_allow_calls_when_closed() {
        let breaker = SinkBreaker::new();
        assert!(breaker.allows(Instant::now()));
    }

    #[tokio::test]
    async fn describe_should_list_sink_metadata() {
        use std::collections::HashMap;

        use crate::notify::webhook::{WebhookMethod, WebhookNotifier};

        let webhook = WebhookNotifier::new(
            reqwest::Client::new(),
            "hooks",
            "https://example.test/hook",
            WebhookMethod::Post,
            &HashMap::new(),
            None,
            None,
        )
        .expect("webhook");
        let notifier = CompositeNotifier::new(vec![RoutedSink::new(
            Sink::Webhook(webhook),
            vec!["platform".into()],
        )]);
        let views = notifier.describe();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].index, 0);
        assert_eq!(views[0].kind, "webhook");
        assert_eq!(views[0].name, "hooks");
        assert_eq!(views[0].tags, vec!["platform"]);
    }

    #[tokio::test]
    async fn test_at_should_reject_out_of_range_index() {
        use std::collections::HashMap;

        use crate::notify::webhook::{WebhookMethod, WebhookNotifier};

        let webhook = WebhookNotifier::new(
            reqwest::Client::new(),
            "hooks",
            "https://example.test/hook",
            WebhookMethod::Post,
            &HashMap::new(),
            None,
            None,
        )
        .expect("webhook");
        let notifier =
            CompositeNotifier::new(vec![RoutedSink::new(Sink::Webhook(webhook), Vec::new())]);
        let err = notifier
            .test_at(3, &event(None))
            .await
            .expect_err("index 3");
        assert!(err.to_string().contains("out of range"));
    }

    #[test]
    fn breaker_should_stay_closed_below_threshold() {
        let mut breaker = SinkBreaker::new();
        let config = BreakerConfig {
            failure_threshold: 3,
            cooldown: Duration::from_secs(60),
        };
        let now = Instant::now();
        breaker.record(now, false, &config);
        breaker.record(now, false, &config);
        assert!(breaker.allows(now));
    }

    #[test]
    fn breaker_should_open_after_threshold_consecutive_failures() {
        let mut breaker = SinkBreaker::new();
        let config = BreakerConfig {
            failure_threshold: 3,
            cooldown: Duration::from_secs(60),
        };
        let now = Instant::now();
        breaker.record(now, false, &config);
        breaker.record(now, false, &config);
        breaker.record(now, false, &config);
        assert!(!breaker.allows(now));
    }

    #[test]
    fn breaker_should_reject_while_cooldown_has_not_elapsed() {
        let mut breaker = SinkBreaker::new();
        let config = BreakerConfig {
            failure_threshold: 1,
            cooldown: Duration::from_secs(60),
        };
        let now = Instant::now();
        breaker.record(now, false, &config);
        assert!(!breaker.allows(now + Duration::from_secs(30)));
    }

    #[test]
    fn breaker_should_allow_trial_call_once_cooldown_elapses() {
        let mut breaker = SinkBreaker::new();
        let config = BreakerConfig {
            failure_threshold: 1,
            cooldown: Duration::from_secs(60),
        };
        let now = Instant::now();
        breaker.record(now, false, &config);
        assert!(breaker.allows(now + Duration::from_secs(60)));
    }

    #[test]
    fn breaker_should_reset_consecutive_failures_on_success() {
        let mut breaker = SinkBreaker::new();
        let config = BreakerConfig {
            failure_threshold: 3,
            cooldown: Duration::from_secs(60),
        };
        let now = Instant::now();
        breaker.record(now, false, &config);
        breaker.record(now, false, &config);
        breaker.record(now, true, &config);
        assert_eq!(breaker.consecutive_failures, 0);
        // Two more failures after the reset stay below the threshold again.
        breaker.record(now, false, &config);
        breaker.record(now, false, &config);
        assert!(breaker.allows(now));
    }

    #[test]
    fn breaker_should_close_again_after_a_successful_trial() {
        let mut breaker = SinkBreaker::new();
        let config = BreakerConfig {
            failure_threshold: 1,
            cooldown: Duration::from_secs(60),
        };
        let now = Instant::now();
        breaker.record(now, false, &config);
        let trial_at = now + Duration::from_secs(60);
        assert!(breaker.allows(trial_at));
        breaker.record(trial_at, true, &config);
        assert!(breaker.allows(trial_at));
    }

    #[test]
    fn breaker_should_reopen_after_a_failed_trial() {
        let mut breaker = SinkBreaker::new();
        let config = BreakerConfig {
            failure_threshold: 1,
            cooldown: Duration::from_secs(60),
        };
        let now = Instant::now();
        breaker.record(now, false, &config);
        let trial_at = now + Duration::from_secs(60);
        breaker.record(trial_at, false, &config);
        assert!(!breaker.allows(trial_at));
        assert!(breaker.allows(trial_at + Duration::from_secs(60)));
    }

    // --- CompositeNotifier integration: the breaker actually skips the
    // network call once tripped. Uses a loopback port with nothing listening
    // (connection refused near-instantly — no DNS lookup, no timeout wait). ---

    #[tokio::test]
    async fn notify_partial_should_skip_sink_after_repeated_failures() {
        use std::collections::HashMap;

        use crate::notify::webhook::{WebhookMethod, WebhookNotifier};

        let webhook = WebhookNotifier::new(
            reqwest::Client::new(),
            "w",
            "http://127.0.0.1:1/",
            WebhookMethod::Post,
            &HashMap::new(),
            None,
            None,
        )
        .expect("webhook");
        let notifier = CompositeNotifier::with_breaker_config(
            vec![RoutedSink::new(Sink::Webhook(webhook), Vec::new())],
            BreakerConfig {
                failure_threshold: 2,
                cooldown: Duration::from_secs(3_600),
            },
        );
        let event = event(None);

        let first = notifier.notify_partial(&event, &[0]).await;
        assert!(
            matches!(first[0].1, Err(NotifyError::Http(_))),
            "expected a real (failing) delivery attempt, got {:?}",
            first[0].1
        );
        let second = notifier.notify_partial(&event, &[0]).await;
        assert!(matches!(second[0].1, Err(NotifyError::Http(_))));

        let third = notifier.notify_partial(&event, &[0]).await;
        assert!(matches!(
            third[0].1,
            Err(NotifyError::BreakerOpen { backend: "webhook" })
        ));
    }

    #[tokio::test]
    async fn notify_partial_should_not_trip_breaker_on_unmatched_tag() {
        use std::collections::HashMap;

        use crate::notify::webhook::{WebhookMethod, WebhookNotifier};

        // A sink whose routing tag never matches must never even reach the
        // breaker check — confirms the early `sink.matches` short-circuit in
        // `notify_partial` still returns `Ok(())` unconditionally.
        let webhook = WebhookNotifier::new(
            reqwest::Client::new(),
            "w",
            "http://127.0.0.1:1/",
            WebhookMethod::Post,
            &HashMap::new(),
            None,
            None,
        )
        .expect("webhook");
        let notifier = CompositeNotifier::new(vec![RoutedSink::new(
            Sink::Webhook(webhook),
            vec!["other-team".into()],
        )]);

        for _ in 0..10 {
            let result = notifier
                .notify_partial(&event(Some("platform")), &[0])
                .await;
            assert!(matches!(result[0].1, Ok(())));
        }
    }
}
