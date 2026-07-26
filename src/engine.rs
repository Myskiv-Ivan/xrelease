//! Shared runtime context for scheduler, pipeline, and HTTP API.
//!
//! [`Engine`] is the single wiring point for PostgreSQL state, the upstream HTTP
//! client, Apprise delivery, and Prometheus counters. The long-running `serve`
//! process and the management API operate on the same instance so metrics and
//! state stay consistent.

use std::sync::Arc;

use anyhow::Context;
use tokio::sync::RwLock;

use crate::config::{Config, DatabaseConfig};
use crate::metrics::Metrics;
use crate::notify::CompositeNotifier;
use crate::pipeline::{DEFAULT_OUTBOX_FLUSH_CONCURRENCY, DEFAULT_OUTBOX_RETRY_BACKOFF_MAX_SECS};
use crate::rpm_limit::{build_upstream_limiter, UpstreamLimiter};
use crate::store::{PruneReport, Store};

/// Database retention policy for startup and periodic maintenance.
#[derive(Debug, Clone)]
pub struct PruneSettings {
    /// Delete `seen_release` rows older than N days (`0` = skip).
    pub seen_after_days: u32,
    /// Delete `webhook_delivery` rows older than N days (`0` = skip).
    pub webhooks_after_days: u32,
    /// Delete sent outbox rows older than N days (`0` = skip).
    pub outbox_sent_after_days: u32,
    /// Hours between periodic prune runs while `serve` is up (`0` = startup only).
    pub interval_hours: u32,
}

impl From<&DatabaseConfig> for PruneSettings {
    fn from(db: &DatabaseConfig) -> Self {
        Self {
            seen_after_days: db.prune_seen_after_days,
            webhooks_after_days: db.prune_webhooks_after_days,
            outbox_sent_after_days: db.prune_outbox_sent_after_days,
            interval_hours: db.prune_interval_hours,
        }
    }
}

impl PruneSettings {
    /// Whether periodic maintenance should run (interval set and at least one retention rule).
    #[must_use]
    pub fn periodic_enabled(&self) -> bool {
        self.interval_hours > 0
            && (self.seen_after_days > 0
                || self.webhooks_after_days > 0
                || self.outbox_sent_after_days > 0)
    }
}

/// Hot-swappable runtime state: notifier + poll/outbox tuning derived from
/// `[defaults]`, all behind one lock.
///
/// These were previously two independent locks (`notifier` in its own
/// `Arc<RwLock<_>>`, the rest in a sibling `RwLock<EngineRuntime>`).
/// [`Engine::apply_runtime_config`] wrote them one after another, so a reader
/// racing a config apply could observe a torn combination — e.g. the
/// freshly-swapped notifier paired with the stale `outbox_flush_concurrency`.
/// One lock makes a hot-swap atomic: every field changes together, or a
/// reader sees the fully-old set.
///
/// The notifier sits behind [`Arc`] so [`Engine::notifier_snapshot`] is a
/// pointer clone: delivery keeps a stable sink-index view for one attempt
/// without cloning the sink vector on every flush row.
#[derive(Clone)]
struct EngineRuntime {
    notifier: Arc<CompositeNotifier>,
    upstream_limiter: Option<Arc<UpstreamLimiter>>,
    outbox_flush_concurrency: usize,
    outbox_retry_backoff_max_secs: u32,
    /// Ops meta-alert routing tag (`[defaults].ops_routing_tag`).
    ops_routing_tag: Option<String>,
}

/// Process-wide services used by polling, webhooks, and CLI checks.
pub struct Engine {
    /// Persistent release state (PostgreSQL).
    pub store: Arc<Store>,
    /// Shared upstream HTTP client (User-Agent, timeout).
    pub http: reqwest::Client,
    /// Prometheus counters and latency histograms.
    pub metrics: Arc<Metrics>,
    /// Database retention for startup + periodic prune.
    prune: PruneSettings,
    /// Notifier + poll/outbox tuning; swapped as one unit on config apply.
    runtime: RwLock<EngineRuntime>,
}

impl Engine {
    /// Open the database and build the notifier from config.
    pub fn open(config: &Config, http: reqwest::Client) -> anyhow::Result<Arc<Self>> {
        let store =
            Arc::new(Store::open_from_config(&config.database).context("opening state database")?);
        let prune = PruneSettings::from(&config.database);
        let notifier = config.build_notifiers(http.clone())?;
        if notifier.is_empty() {
            tracing::warn!(
                "no notification sinks configured — delivery idle until UI/CI apply \
                 (set [[notifiers]])"
            );
        } else {
            tracing::info!(sinks = ?notifier.kinds(), "notification sinks configured");
        }
        let runtime = EngineRuntime {
            notifier: Arc::new(notifier),
            upstream_limiter: build_upstream_limiter(config.defaults.upstream_requests_per_minute),
            outbox_flush_concurrency: config.defaults.outbox_flush_concurrency.clamp(1, 64),
            outbox_retry_backoff_max_secs: config
                .defaults
                .outbox_retry_backoff_max_secs
                .clamp(60, 86_400),
            ops_routing_tag: config
                .defaults
                .ops_routing_tag
                .as_ref()
                .map(|tag| tag.trim().to_owned())
                .filter(|tag| !tag.is_empty()),
        };
        let outbox_flush_concurrency = runtime.outbox_flush_concurrency;
        let outbox_retry_backoff_max_secs = runtime.outbox_retry_backoff_max_secs;
        let has_upstream_limiter = runtime.upstream_limiter.is_some();
        let engine = Arc::new(Self {
            store,
            http,
            metrics: Arc::new(Metrics::new()),
            prune,
            runtime: RwLock::new(runtime),
        });

        if has_upstream_limiter {
            tracing::info!(
                rpm = config.defaults.upstream_requests_per_minute,
                "upstream fetch rate limiting enabled"
            );
        }
        if outbox_flush_concurrency != DEFAULT_OUTBOX_FLUSH_CONCURRENCY {
            tracing::info!(
                concurrency = outbox_flush_concurrency,
                "outbox flush concurrency overridden"
            );
        }
        if outbox_retry_backoff_max_secs != DEFAULT_OUTBOX_RETRY_BACKOFF_MAX_SECS {
            tracing::info!(
                max_secs = outbox_retry_backoff_max_secs,
                "outbox retry backoff cap overridden"
            );
        }

        engine.run_prune("startup")?;

        Ok(engine)
    }

    /// Retention settings for periodic maintenance.
    #[must_use]
    pub fn prune_settings(&self) -> &PruneSettings {
        &self.prune
    }

    /// Max concurrent deliveries inside one outbox flush batch.
    pub async fn outbox_flush_concurrency(&self) -> usize {
        self.runtime.read().await.outbox_flush_concurrency
    }

    /// Cap on exponential backoff between outbox delivery retries, in seconds.
    pub async fn outbox_retry_backoff_max_secs(&self) -> u32 {
        self.runtime.read().await.outbox_retry_backoff_max_secs
    }

    /// Replace notifier and runtime tuning after a successful config apply.
    ///
    /// The new notifier is built *before* the write lock is taken, so a
    /// build failure (should not happen — the caller already ran
    /// `validate_full` — but defend anyway) leaves the running config
    /// completely untouched rather than partially swapped.
    pub async fn apply_runtime_config(&self, config: &Config) -> anyhow::Result<()> {
        let notifier = config.build_notifiers(self.http.clone())?;
        if notifier.is_empty() {
            tracing::warn!("notification sinks hot-swapped to empty (delivery idle)");
        } else {
            tracing::info!(sinks = ?notifier.kinds(), "notification sinks hot-swapped");
        }
        let upstream_limiter = build_upstream_limiter(config.defaults.upstream_requests_per_minute);
        let outbox_flush_concurrency = config.defaults.outbox_flush_concurrency.clamp(1, 64);
        let outbox_retry_backoff_max_secs = config
            .defaults
            .outbox_retry_backoff_max_secs
            .clamp(60, 86_400);
        let ops_routing_tag = config
            .defaults
            .ops_routing_tag
            .as_ref()
            .map(|tag| tag.trim().to_owned())
            .filter(|tag| !tag.is_empty());

        let mut runtime = self.runtime.write().await;
        runtime.notifier = Arc::new(notifier);
        runtime.upstream_limiter = upstream_limiter;
        runtime.outbox_flush_concurrency = outbox_flush_concurrency;
        runtime.outbox_retry_backoff_max_secs = outbox_retry_backoff_max_secs;
        runtime.ops_routing_tag = ops_routing_tag;
        Ok(())
    }

    /// Snapshot the current notifier for one delivery attempt.
    ///
    /// Every call site that computes `matching_indices`/`kind_at` and later
    /// calls `notify_partial` for the *same* delivery attempt must use ONE
    /// snapshot throughout, never re-read the live notifier per call: a
    /// config apply can hot-swap it between those calls (removing or
    /// reordering sinks), and the per-sink delivery ledger
    /// (`notification_sink_delivery`) is keyed by *index* into the sink
    /// list — an index computed against one snapshot and used against a
    /// later, different one can target the wrong sink, or one that no
    /// longer exists, permanently stalling that ledger row.
    ///
    /// Returning [`Arc`] keeps that stable view as a pointer clone (breaker
    /// state is already shared inside [`CompositeNotifier`]).
    pub async fn notifier_snapshot(&self) -> Arc<CompositeNotifier> {
        Arc::clone(&self.runtime.read().await.notifier)
    }

    /// Deliver an ops meta-alert via configured sinks (not the outbox).
    ///
    /// Skipped when `[defaults].ops_routing_tag` is unset. Direct notify avoids
    /// enqueueing another outbox row if the alert itself fails to deliver.
    pub async fn emit_ops_alert(&self, title: &str, body: &str) {
        let (tag, notifier) = {
            let runtime = self.runtime.read().await;
            let Some(tag) = runtime.ops_routing_tag.clone() else {
                return;
            };
            (tag, Arc::clone(&runtime.notifier))
        };
        let event = crate::notify::Event {
            source_id: "xrelease:ops".into(),
            source_kind: "xrelease".into(),
            title: title.to_owned(),
            body: body.to_owned(),
            url: None,
            routing_tag: Some(tag),
        };
        if let Err(err) = crate::notify::Notifier::notify(notifier.as_ref(), &event).await {
            tracing::warn!(error = %err, title, "ops meta-alert delivery failed");
        } else {
            tracing::info!(title, "ops meta-alert delivered");
        }
    }

    /// Run database retention according to configured day thresholds.
    pub fn run_prune(&self, reason: &str) -> anyhow::Result<PruneReport> {
        let report = self.store.prune(
            self.prune.seen_after_days,
            self.prune.webhooks_after_days,
            self.prune.outbox_sent_after_days,
        )?;
        let deleted = report.seen_deleted + report.webhooks_deleted + report.outbox_deleted;
        self.metrics.record_prune_deleted(deleted);
        if report.any() {
            tracing::info!(
                reason,
                seen_deleted = report.seen_deleted,
                webhooks_deleted = report.webhooks_deleted,
                outbox_deleted = report.outbox_deleted,
                "database maintenance prune completed"
            );
        }
        Ok(report)
    }

    /// Drain pending notification outbox rows (startup + background retry).
    pub async fn flush_outbox(
        &self,
        limit: usize,
    ) -> Result<usize, crate::error::PipelineError> {
        crate::pipeline::flush_notification_outbox(self, limit).await
    }

    /// Wait for an upstream fetch quota slot (no-op when limiting is disabled).
    pub async fn acquire_upstream(&self) {
        let limiter = self.runtime.read().await.upstream_limiter.clone();
        if let Some(limiter) = limiter {
            limiter.until_ready().await;
        }
    }

    /// Readiness: PostgreSQL reachable and every configured notifier healthy.
    pub async fn ready_check(&self) -> EngineReady {
        let database_ok = self.store.health().is_ok();
        // Snapshot-then-check (not read-lock-held-across-await): health probes
        // are real network calls (e.g. Apprise) that must not block a
        // concurrent config apply's write-lock acquisition for their duration.
        let notifiers_ok = self.notifier_snapshot().await.health_check().await;
        let outbox = self.store.outbox_counts().unwrap_or_default();
        EngineReady {
            database_ok,
            notifiers_ok,
            outbox,
        }
    }
}

/// Component health for `/ready` and observability.
#[derive(Debug, Clone)]
pub struct EngineReady {
    pub database_ok: bool,
    /// All configured notification sinks passed health checks.
    pub notifiers_ok: bool,
    pub outbox: crate::store::OutboxCounts,
}

impl EngineReady {
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.database_ok && self.notifiers_ok
    }
}
