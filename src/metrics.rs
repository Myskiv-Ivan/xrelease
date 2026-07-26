//! Prometheus-compatible counters and latency histograms.
//!
//! Exposition is hand-rolled text 0.0.4 (no `prometheus` crate) so the default
//! binary stays lean. Histograms use fixed second-buckets shared by poll,
//! notify, and outbox flush timings.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;

use crate::store::OutboxCounts;

/// Upper bounds (seconds) for latency histograms; `+Inf` is implicit.
const LATENCY_BOUNDS_SECS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
];

/// Fixed-bucket histogram with cumulative Prometheus semantics.
struct Histogram {
    bounds: &'static [f64],
    /// `bounds.len() + 1` counters (`le` buckets then `+Inf`).
    counts: Box<[AtomicU64]>,
    sum_micros: AtomicU64,
    count: AtomicU64,
}

impl Histogram {
    fn new(bounds: &'static [f64]) -> Self {
        let n = bounds.len() + 1;
        let mut counts = Vec::with_capacity(n);
        counts.resize_with(n, AtomicU64::default);
        Self {
            bounds,
            counts: counts.into_boxed_slice(),
            sum_micros: AtomicU64::new(0),
            count: AtomicU64::new(0),
        }
    }

    fn observe(&self, duration: Duration) {
        let secs = duration.as_secs_f64();
        let micros = u64::try_from(duration.as_micros()).unwrap_or(u64::MAX);
        self.sum_micros.fetch_add(micros, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);

        let mut idx = self.bounds.len();
        for (i, bound) in self.bounds.iter().enumerate() {
            if secs <= *bound {
                idx = i;
                break;
            }
        }
        self.counts[idx].fetch_add(1, Ordering::Relaxed);
    }

    fn render(&self, name: &str, help: &str, out: &mut String) {
        out.push_str(&format!("# HELP {name} {help}\n"));
        out.push_str(&format!("# TYPE {name} histogram\n"));

        let mut cumulative = 0u64;
        for (i, bound) in self.bounds.iter().enumerate() {
            cumulative += self.counts[i].load(Ordering::Relaxed);
            out.push_str(&format!("{name}_bucket{{le=\"{bound}\"}} {cumulative}\n"));
        }
        cumulative += self.counts[self.bounds.len()].load(Ordering::Relaxed);
        out.push_str(&format!("{name}_bucket{{le=\"+Inf\"}} {cumulative}\n"));

        let sum_secs = self.sum_micros.load(Ordering::Relaxed) as f64 / 1_000_000.0;
        let count = self.count.load(Ordering::Relaxed);
        out.push_str(&format!("{name}_sum {sum_secs}\n"));
        out.push_str(&format!("{name}_count {count}\n"));
    }
}

/// Per-source poll/notify counters (for labelled Prometheus series).
#[derive(Debug, Default)]
struct SourceMetrics {
    polls: AtomicU64,
    polls_not_modified: AtomicU64,
    polls_errors: AtomicU64,
    notifications: AtomicU64,
}

/// Per-sink delivery counters (keyed by kind + operator label).
#[derive(Debug, Default)]
struct SinkMetrics {
    ok: AtomicU64,
    error: AtomicU64,
    breaker: AtomicU64,
}

/// Outcome of one sink delivery attempt (for metrics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkDeliveryResult {
    /// Sink acknowledged the notification.
    Ok,
    /// Network / backend failure (not a breaker skip).
    Error,
    /// Circuit breaker open — call skipped without hitting the network.
    Breaker,
}

/// Live circuit-breaker view for a configured sink (scraped into gauges).
#[derive(Debug, Clone, Serialize)]
pub struct BreakerOpenView {
    /// Backend kind (`apprise`, `webhook`, …).
    pub kind: &'static str,
    /// Operator-facing sink label.
    pub label: String,
    /// `true` when the breaker is currently rejecting calls.
    pub open: bool,
}

/// Process-wide counters and histograms shared by scheduler, pipeline, and API.
pub struct Metrics {
    polls_total: AtomicU64,
    polls_not_modified: AtomicU64,
    polls_errors: AtomicU64,
    notifications_total: AtomicU64,
    webhooks_accepted: AtomicU64,
    webhooks_ignored: AtomicU64,
    webhooks_duplicates: AtomicU64,
    webhooks_errors: AtomicU64,
    config_apply_total: AtomicU64,
    config_apply_rejected_total: AtomicU64,
    notify_breaker_skips: AtomicU64,
    outbox_enqueued_total: AtomicU64,
    outbox_delivery_failures_total: AtomicU64,
    outbox_dead_lettered_total: AtomicU64,
    outbox_requeued_total: AtomicU64,
    http_rate_limited_total: AtomicU64,
    prune_deleted_total: AtomicU64,
    poll_duration: Histogram,
    notify_duration: Histogram,
    outbox_flush_duration: Histogram,
    by_source: Mutex<HashMap<String, SourceMetrics>>,
    by_sink: Mutex<HashMap<(String, String), SinkMetrics>>,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            polls_total: AtomicU64::new(0),
            polls_not_modified: AtomicU64::new(0),
            polls_errors: AtomicU64::new(0),
            notifications_total: AtomicU64::new(0),
            webhooks_accepted: AtomicU64::new(0),
            webhooks_ignored: AtomicU64::new(0),
            webhooks_duplicates: AtomicU64::new(0),
            webhooks_errors: AtomicU64::new(0),
            config_apply_total: AtomicU64::new(0),
            config_apply_rejected_total: AtomicU64::new(0),
            notify_breaker_skips: AtomicU64::new(0),
            outbox_enqueued_total: AtomicU64::new(0),
            outbox_delivery_failures_total: AtomicU64::new(0),
            outbox_dead_lettered_total: AtomicU64::new(0),
            outbox_requeued_total: AtomicU64::new(0),
            http_rate_limited_total: AtomicU64::new(0),
            prune_deleted_total: AtomicU64::new(0),
            poll_duration: Histogram::new(LATENCY_BOUNDS_SECS),
            notify_duration: Histogram::new(LATENCY_BOUNDS_SECS),
            outbox_flush_duration: Histogram::new(LATENCY_BOUNDS_SECS),
            by_source: Mutex::new(HashMap::new()),
            by_sink: Mutex::new(HashMap::new()),
        }
    }
}

impl Metrics {
    /// Create a zeroed metrics registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a completed poll attempt for a source.
    pub fn record_poll(&self, source_id: &str, outcome: PollOutcome) {
        self.polls_total.fetch_add(1, Ordering::Relaxed);
        match outcome {
            PollOutcome::Delivered | PollOutcome::NoOp => {}
            PollOutcome::NotModified => {
                self.polls_not_modified.fetch_add(1, Ordering::Relaxed);
            }
            PollOutcome::Error => {
                self.polls_errors.fetch_add(1, Ordering::Relaxed);
            }
        }

        {
            let mut map = self.by_source.lock().unwrap_or_else(|e| e.into_inner());
            let entry = map.entry(source_id.to_owned()).or_default();
            entry.polls.fetch_add(1, Ordering::Relaxed);
            match outcome {
                PollOutcome::NotModified => {
                    entry.polls_not_modified.fetch_add(1, Ordering::Relaxed);
                }
                PollOutcome::Error => {
                    entry.polls_errors.fetch_add(1, Ordering::Relaxed);
                }
                PollOutcome::Delivered | PollOutcome::NoOp => {}
            }
        }
    }

    /// Observe end-to-end `poll_once` wall time (fetch + diff + inline deliver).
    pub fn record_poll_duration(&self, duration: Duration) {
        self.poll_duration.observe(duration);
    }

    /// Observe one outbox-row delivery attempt (all pending sinks).
    pub fn record_notify_duration(&self, duration: Duration) {
        self.notify_duration.observe(duration);
    }

    /// Observe one `flush_notification_outbox` batch (claim + parallel deliver).
    pub fn record_outbox_flush_duration(&self, duration: Duration) {
        self.outbox_flush_duration.observe(duration);
    }

    /// Record successfully delivered notifications for a source.
    pub fn record_notifications(&self, source_id: &str, count: usize) {
        if count == 0 {
            return;
        }
        let n = count as u64;
        self.notifications_total.fetch_add(n, Ordering::Relaxed);
        self.by_source
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(source_id.to_owned())
            .or_default()
            .notifications
            .fetch_add(n, Ordering::Relaxed);
    }

    /// Record one sink delivery attempt (also bumps the global breaker-skip
    /// counter when `result` is [`SinkDeliveryResult::Breaker`]).
    pub fn record_sink_delivery(&self, kind: &str, label: &str, result: SinkDeliveryResult) {
        {
            let mut map = self.by_sink.lock().unwrap_or_else(|e| e.into_inner());
            let entry = map.entry((kind.to_owned(), label.to_owned())).or_default();
            match result {
                SinkDeliveryResult::Ok => {
                    entry.ok.fetch_add(1, Ordering::Relaxed);
                }
                SinkDeliveryResult::Error => {
                    entry.error.fetch_add(1, Ordering::Relaxed);
                }
                SinkDeliveryResult::Breaker => {
                    entry.breaker.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        if result == SinkDeliveryResult::Breaker {
            self.notify_breaker_skips.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record a sink delivery skipped by an open circuit breaker.
    ///
    /// Prefer [`Self::record_sink_delivery`] with [`SinkDeliveryResult::Breaker`]
    /// when the sink identity is known; this keeps the unlabelled total only.
    pub fn record_breaker_skip(&self) {
        self.notify_breaker_skips.fetch_add(1, Ordering::Relaxed);
    }

    /// Record newly created outbox work (insert or reopen-on-change).
    pub fn record_outbox_enqueued(&self, count: usize) {
        if count == 0 {
            return;
        }
        self.outbox_enqueued_total
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    /// Record a parent-row delivery failure (routing miss, exhausted attempt, …).
    pub fn record_outbox_delivery_failure(&self) {
        self.outbox_delivery_failures_total
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Record outbox rows that transitioned to `dead`.
    pub fn record_outbox_dead_lettered(&self, count: usize) {
        if count == 0 {
            return;
        }
        self.outbox_dead_lettered_total
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    /// Record dead-letter rows revived by an operator requeue.
    pub fn record_outbox_requeued(&self, count: usize) {
        if count == 0 {
            return;
        }
        self.outbox_requeued_total
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    /// Record an HTTP 429 from the management/webhook rate limiter.
    pub fn record_http_rate_limited(&self) {
        self.http_rate_limited_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Record rows deleted by a prune pass.
    pub fn record_prune_deleted(&self, count: usize) {
        if count == 0 {
            return;
        }
        self.prune_deleted_total
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    /// Record a webhook invocation result.
    pub fn record_webhook(&self, outcome: WebhookOutcome) {
        match outcome {
            WebhookOutcome::Accepted => {
                self.webhooks_accepted.fetch_add(1, Ordering::Relaxed);
            }
            WebhookOutcome::Ignored => {
                self.webhooks_ignored.fetch_add(1, Ordering::Relaxed);
            }
            WebhookOutcome::Duplicate => {
                self.webhooks_duplicates.fetch_add(1, Ordering::Relaxed);
            }
            WebhookOutcome::Error => {
                self.webhooks_errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Record a successful or idempotent config apply.
    pub fn record_config_apply(&self) {
        self.config_apply_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a rejected config apply (parse or validation failure).
    pub fn record_config_apply_rejected(&self) {
        self.config_apply_rejected_total
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Render counters in Prometheus text format 0.0.4.
    #[must_use]
    pub fn render_prometheus(
        &self,
        sources_configured: usize,
        outbox: &OutboxCounts,
        uptime_secs: u64,
        breakers: &[BreakerOpenView],
    ) -> String {
        let polls = self.polls_total.load(Ordering::Relaxed);
        let not_modified = self.polls_not_modified.load(Ordering::Relaxed);
        let poll_errors = self.polls_errors.load(Ordering::Relaxed);
        let notifications = self.notifications_total.load(Ordering::Relaxed);
        let wh_accepted = self.webhooks_accepted.load(Ordering::Relaxed);
        let wh_ignored = self.webhooks_ignored.load(Ordering::Relaxed);
        let wh_duplicates = self.webhooks_duplicates.load(Ordering::Relaxed);
        let wh_errors = self.webhooks_errors.load(Ordering::Relaxed);
        let config_apply = self.config_apply_total.load(Ordering::Relaxed);
        let config_apply_rejected = self.config_apply_rejected_total.load(Ordering::Relaxed);
        let breaker_skips = self.notify_breaker_skips.load(Ordering::Relaxed);
        let outbox_enqueued = self.outbox_enqueued_total.load(Ordering::Relaxed);
        let outbox_failures = self.outbox_delivery_failures_total.load(Ordering::Relaxed);
        let outbox_dead_lettered = self.outbox_dead_lettered_total.load(Ordering::Relaxed);
        let outbox_requeued = self.outbox_requeued_total.load(Ordering::Relaxed);
        let http_rate_limited = self.http_rate_limited_total.load(Ordering::Relaxed);
        let prune_deleted = self.prune_deleted_total.load(Ordering::Relaxed);

        let mut body = format!(
            "\
# HELP xrelease_info Static build metadata (always 1).
# TYPE xrelease_info gauge
xrelease_info{{version=\"{version}\"}} 1
# HELP xrelease_uptime_seconds Process uptime in seconds.
# TYPE xrelease_uptime_seconds gauge
xrelease_uptime_seconds {uptime}
# HELP xrelease_sources_configured Number of configured watches.
# TYPE xrelease_sources_configured gauge
xrelease_sources_configured {sources}
# HELP xrelease_polls_total Poll attempts from scheduler or API check.
# TYPE xrelease_polls_total counter
xrelease_polls_total {polls}
# HELP xrelease_polls_not_modified_total Upstream 304 Not Modified responses.
# TYPE xrelease_polls_not_modified_total counter
xrelease_polls_not_modified_total {not_modified}
# HELP xrelease_polls_errors_total Poll attempts that returned an error.
# TYPE xrelease_polls_errors_total counter
xrelease_polls_errors_total {poll_errors}
# HELP xrelease_notifications_total Notifications successfully delivered.
# TYPE xrelease_notifications_total counter
xrelease_notifications_total {notifications}
# HELP xrelease_source_polls_total Poll attempts per configured source.
# TYPE xrelease_source_polls_total counter
# HELP xrelease_source_polls_not_modified_total Upstream 304 responses per source.
# TYPE xrelease_source_polls_not_modified_total counter
# HELP xrelease_source_polls_errors_total Poll errors per source.
# TYPE xrelease_source_polls_errors_total counter
# HELP xrelease_source_notifications_total Notifications delivered per source.
# TYPE xrelease_source_notifications_total counter
# HELP xrelease_sink_deliveries_total Sink delivery attempts by kind, label, and result.
# TYPE xrelease_sink_deliveries_total counter
# HELP xrelease_sink_breaker_open Whether a sink circuit breaker is currently open (1) or closed (0).
# TYPE xrelease_sink_breaker_open gauge
# HELP xrelease_webhooks_accepted_total Webhooks that matched a source and were processed.
# TYPE xrelease_webhooks_accepted_total counter
xrelease_webhooks_accepted_total {wh_accepted}
# HELP xrelease_webhooks_ignored_total Webhooks intentionally skipped (draft, wrong action, filtered).
# TYPE xrelease_webhooks_ignored_total counter
xrelease_webhooks_ignored_total {wh_ignored}
# HELP xrelease_webhooks_duplicates_total Webhook replays deduplicated by delivery id.
# TYPE xrelease_webhooks_duplicates_total counter
xrelease_webhooks_duplicates_total {wh_duplicates}
# HELP xrelease_webhooks_errors_total Webhook requests that failed validation or processing.
# TYPE xrelease_webhooks_errors_total counter
xrelease_webhooks_errors_total {wh_errors}
# HELP xrelease_config_apply_total Successful or idempotent config apply calls.
# TYPE xrelease_config_apply_total counter
xrelease_config_apply_total {config_apply}
# HELP xrelease_config_apply_rejected_total Config apply attempts rejected (parse/validation).
# TYPE xrelease_config_apply_rejected_total counter
xrelease_config_apply_rejected_total {config_apply_rejected}
# HELP xrelease_outbox_pending Notification outbox rows awaiting first delivery.
# TYPE xrelease_outbox_pending gauge
xrelease_outbox_pending {outbox_pending}
# HELP xrelease_outbox_failed Notification outbox rows in retry state.
# TYPE xrelease_outbox_failed gauge
xrelease_outbox_failed {outbox_failed}
# HELP xrelease_outbox_dead Notification outbox rows that exhausted delivery retries.
# TYPE xrelease_outbox_dead gauge
xrelease_outbox_dead {outbox_dead}
# HELP xrelease_outbox_deferred Notification outbox rows held until deliver_after (notify_schedule).
# TYPE xrelease_outbox_deferred gauge
xrelease_outbox_deferred {outbox_deferred}
# HELP xrelease_outbox_enqueued_total Outbox rows newly inserted or reopened for delivery.
# TYPE xrelease_outbox_enqueued_total counter
xrelease_outbox_enqueued_total {outbox_enqueued}
# HELP xrelease_outbox_delivery_failures_total Parent outbox delivery failures (retryable or dead).
# TYPE xrelease_outbox_delivery_failures_total counter
xrelease_outbox_delivery_failures_total {outbox_failures}
# HELP xrelease_outbox_dead_lettered_total Outbox rows that transitioned to dead.
# TYPE xrelease_outbox_dead_lettered_total counter
xrelease_outbox_dead_lettered_total {outbox_dead_lettered}
# HELP xrelease_outbox_requeued_total Dead outbox rows revived by operator requeue.
# TYPE xrelease_outbox_requeued_total counter
xrelease_outbox_requeued_total {outbox_requeued}
# HELP xrelease_http_rate_limited_total HTTP requests rejected with 429 by the ingress rate limiter.
# TYPE xrelease_http_rate_limited_total counter
xrelease_http_rate_limited_total {http_rate_limited}
# HELP xrelease_prune_deleted_total Database rows deleted by retention prune.
# TYPE xrelease_prune_deleted_total counter
xrelease_prune_deleted_total {prune_deleted}
# HELP xrelease_notify_breaker_skips_total Sink deliveries skipped by an open circuit breaker.
# TYPE xrelease_notify_breaker_skips_total counter
xrelease_notify_breaker_skips_total {breaker_skips}
",
            version = env!("CARGO_PKG_VERSION"),
            uptime = uptime_secs,
            sources = sources_configured,
            outbox_pending = outbox.pending,
            outbox_failed = outbox.failed,
            outbox_dead = outbox.dead,
            outbox_deferred = outbox.deferred,
        );

        self.poll_duration.render(
            "xrelease_poll_duration_seconds",
            "Wall time of one poll_once cycle (fetch, diff, inline delivery).",
            &mut body,
        );
        self.notify_duration.render(
            "xrelease_notify_duration_seconds",
            "Wall time to deliver one leased outbox row to its pending sinks.",
            &mut body,
        );
        self.outbox_flush_duration.render(
            "xrelease_outbox_flush_duration_seconds",
            "Wall time of one outbox flush batch (claim + concurrent delivery).",
            &mut body,
        );

        {
            let by_source = self.by_source.lock().unwrap_or_else(|e| e.into_inner());
            let mut ids: Vec<&String> = by_source.keys().collect();
            ids.sort();
            for id in ids {
                let stats = &by_source[id];
                let label = escape_label(id);
                let p = stats.polls.load(Ordering::Relaxed);
                let nm = stats.polls_not_modified.load(Ordering::Relaxed);
                let pe = stats.polls_errors.load(Ordering::Relaxed);
                let n = stats.notifications.load(Ordering::Relaxed);
                if p > 0 {
                    body.push_str(&format!(
                        "xrelease_source_polls_total{{source=\"{label}\"}} {p}\n"
                    ));
                }
                if nm > 0 {
                    body.push_str(&format!(
                        "xrelease_source_polls_not_modified_total{{source=\"{label}\"}} {nm}\n"
                    ));
                }
                if pe > 0 {
                    body.push_str(&format!(
                        "xrelease_source_polls_errors_total{{source=\"{label}\"}} {pe}\n"
                    ));
                }
                if n > 0 {
                    body.push_str(&format!(
                        "xrelease_source_notifications_total{{source=\"{label}\"}} {n}\n"
                    ));
                }
            }
        }

        {
            let by_sink = self.by_sink.lock().unwrap_or_else(|e| e.into_inner());
            let mut keys: Vec<&(String, String)> = by_sink.keys().collect();
            keys.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
            for key in keys {
                let stats = &by_sink[key];
                let kind = escape_label(&key.0);
                let label = escape_label(&key.1);
                let ok = stats.ok.load(Ordering::Relaxed);
                let err = stats.error.load(Ordering::Relaxed);
                let br = stats.breaker.load(Ordering::Relaxed);
                if ok > 0 {
                    body.push_str(&format!(
                        "xrelease_sink_deliveries_total{{kind=\"{kind}\",sink=\"{label}\",result=\"ok\"}} {ok}\n"
                    ));
                }
                if err > 0 {
                    body.push_str(&format!(
                        "xrelease_sink_deliveries_total{{kind=\"{kind}\",sink=\"{label}\",result=\"error\"}} {err}\n"
                    ));
                }
                if br > 0 {
                    body.push_str(&format!(
                        "xrelease_sink_deliveries_total{{kind=\"{kind}\",sink=\"{label}\",result=\"breaker\"}} {br}\n"
                    ));
                }
            }
        }

        for view in breakers {
            let kind = escape_label(view.kind);
            let label = escape_label(&view.label);
            let open = u8::from(view.open);
            body.push_str(&format!(
                "xrelease_sink_breaker_open{{kind=\"{kind}\",sink=\"{label}\"}} {open}\n"
            ));
        }

        body
    }

    /// Global counters for JSON observability API.
    #[must_use]
    pub fn snapshot(&self) -> crate::runtime::MetricsSnapshot {
        crate::runtime::MetricsSnapshot {
            polls_total: self.polls_total.load(Ordering::Relaxed),
            polls_not_modified: self.polls_not_modified.load(Ordering::Relaxed),
            poll_errors: self.polls_errors.load(Ordering::Relaxed),
            notifications_total: self.notifications_total.load(Ordering::Relaxed),
            webhooks_accepted: self.webhooks_accepted.load(Ordering::Relaxed),
            webhooks_ignored: self.webhooks_ignored.load(Ordering::Relaxed),
            webhooks_duplicates: self.webhooks_duplicates.load(Ordering::Relaxed),
            webhooks_errors: self.webhooks_errors.load(Ordering::Relaxed),
            config_apply_total: self.config_apply_total.load(Ordering::Relaxed),
            config_apply_rejected_total: self.config_apply_rejected_total.load(Ordering::Relaxed),
            notify_breaker_skips: self.notify_breaker_skips.load(Ordering::Relaxed),
            outbox_enqueued_total: self.outbox_enqueued_total.load(Ordering::Relaxed),
            outbox_delivery_failures_total: self
                .outbox_delivery_failures_total
                .load(Ordering::Relaxed),
            outbox_dead_lettered_total: self.outbox_dead_lettered_total.load(Ordering::Relaxed),
            outbox_requeued_total: self.outbox_requeued_total.load(Ordering::Relaxed),
            http_rate_limited_total: self.http_rate_limited_total.load(Ordering::Relaxed),
            prune_deleted_total: self.prune_deleted_total.load(Ordering::Relaxed),
        }
    }

    /// Per-source counters for JSON observability API.
    #[must_use]
    pub fn source_stats(&self, source_id: &str) -> crate::runtime::SourceMetricsView {
        let map = self.by_source.lock().unwrap_or_else(|e| e.into_inner());
        map.get(source_id)
            .map(|stats| crate::runtime::SourceMetricsView {
                polls: stats.polls.load(Ordering::Relaxed),
                polls_not_modified: stats.polls_not_modified.load(Ordering::Relaxed),
                poll_errors: stats.polls_errors.load(Ordering::Relaxed),
                notifications: stats.notifications.load(Ordering::Relaxed),
            })
            .unwrap_or_default()
    }
}

fn escape_label(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Outcome of a single poll cycle (for metrics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollOutcome {
    /// New notifications were sent.
    Delivered,
    /// Poll succeeded but nothing to deliver.
    NoOp,
    /// Upstream returned 304.
    NotModified,
    /// Fetch or notify failed.
    Error,
}

/// Outcome of a webhook request (for metrics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebhookOutcome {
    Accepted,
    Ignored,
    Duplicate,
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_should_include_per_source_labels() {
        let metrics = Metrics::new();
        metrics.record_poll("github:a/b", PollOutcome::NotModified);
        metrics.record_notifications("github:a/b", 2);
        let outbox = OutboxCounts::default();
        let body = metrics.render_prometheus(1, &outbox, 0, &[]);
        assert!(body.contains("xrelease_source_polls_total{source=\"github:a/b\"}"));
        assert!(body.contains("xrelease_source_notifications_total{source=\"github:a/b\"} 2"));
        assert!(body.contains("xrelease_outbox_dead 0"));
        assert!(body.contains("xrelease_outbox_deferred 0"));
        assert!(body.contains("xrelease_uptime_seconds 0"));
    }

    #[test]
    fn render_should_include_breaker_skip_counter() {
        let metrics = Metrics::new();
        metrics.record_sink_delivery("apprise", "default", SinkDeliveryResult::Breaker);
        metrics.record_sink_delivery("apprise", "default", SinkDeliveryResult::Breaker);
        let body = metrics.render_prometheus(0, &OutboxCounts::default(), 12, &[]);
        assert!(body.contains("xrelease_notify_breaker_skips_total 2"));
        assert!(body.contains(
            "xrelease_sink_deliveries_total{kind=\"apprise\",sink=\"default\",result=\"breaker\"} 2"
        ));
        assert!(body.contains("xrelease_uptime_seconds 12"));
    }

    #[test]
    fn render_should_include_breaker_open_gauges() {
        let metrics = Metrics::new();
        let breakers = [BreakerOpenView {
            kind: "webhook",
            label: "ops".into(),
            open: true,
        }];
        let body = metrics.render_prometheus(0, &OutboxCounts::default(), 1, &breakers);
        assert!(body.contains("xrelease_sink_breaker_open{kind=\"webhook\",sink=\"ops\"} 1"));
    }

    #[test]
    fn snapshot_should_include_extended_counters() {
        let metrics = Metrics::new();
        metrics.record_outbox_enqueued(3);
        metrics.record_outbox_delivery_failure();
        metrics.record_outbox_dead_lettered(2);
        metrics.record_outbox_requeued(1);
        metrics.record_http_rate_limited();
        metrics.record_prune_deleted(5);
        metrics.record_config_apply();
        metrics.record_config_apply_rejected();
        metrics.record_breaker_skip();
        let snap = metrics.snapshot();
        assert_eq!(snap.outbox_enqueued_total, 3);
        assert_eq!(snap.outbox_delivery_failures_total, 1);
        assert_eq!(snap.outbox_dead_lettered_total, 2);
        assert_eq!(snap.outbox_requeued_total, 1);
        assert_eq!(snap.http_rate_limited_total, 1);
        assert_eq!(snap.prune_deleted_total, 5);
        assert_eq!(snap.config_apply_total, 1);
        assert_eq!(snap.config_apply_rejected_total, 1);
        assert_eq!(snap.notify_breaker_skips, 1);
    }

    #[test]
    fn histogram_should_accumulate_buckets_and_sum() {
        let metrics = Metrics::new();
        metrics.record_poll_duration(Duration::from_millis(3));
        metrics.record_notify_duration(Duration::from_millis(80));
        metrics.record_outbox_flush_duration(Duration::from_secs(2));
        let body = metrics.render_prometheus(0, &OutboxCounts::default(), 0, &[]);

        assert!(body.contains("# TYPE xrelease_poll_duration_seconds histogram"));
        assert!(body.contains("xrelease_poll_duration_seconds_bucket{le=\"0.005\"} 1"));
        assert!(body.contains("xrelease_poll_duration_seconds_count 1"));
        assert!(body.contains("xrelease_notify_duration_seconds_bucket{le=\"0.1\"} 1"));
        assert!(body.contains("xrelease_outbox_flush_duration_seconds_bucket{le=\"2.5\"} 1"));
        assert!(body.contains("xrelease_outbox_flush_duration_seconds_count 1"));
    }
}
