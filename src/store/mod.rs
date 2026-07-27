//! Persistent release state — PostgreSQL.
//!
//! [`Store`] is the public facade; the backend lives in [`postgres`].

mod postgres;

#[cfg(test)]
mod store_tests;

/// Shared access to the single test database used by every DB-touching unit
/// test in the lib test binary.
#[cfg(test)]
pub(crate) mod test_db {
    use super::Store;

    /// Serializes DB-using tests across **all** modules of this binary.
    ///
    /// `cargo test` runs one binary's tests on parallel threads, and
    /// [`Store::open_test`] **truncates every table** before returning. Without
    /// serialization one test wipes — or globally claims — rows another test is
    /// mid-way through asserting on: `claim_outbox_batch` is not scoped to a
    /// source, several tests reuse the same `(source_id, identity)` pair or the
    /// same `"s1"` source, and one test mutates a process-wide env var. That
    /// made `cargo test --workspace` fail non-deterministically while every
    /// test passed in isolation.
    ///
    /// This mirrors the `DB_GUARD` already used by each DB-touching e2e file in
    /// `tests/`. Guards do not serialize *across* binaries, so the connection
    /// pool stays capped at 4 in [`Store::open_test`].
    static DB_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A test store plus the guard serializing its access to the shared database.
    ///
    /// Derefs to [`Store`], so call sites read exactly as they did before the
    /// guard existed. The guard is released when this value drops.
    pub(crate) struct TestStore {
        store: Store,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl std::ops::Deref for TestStore {
        type Target = Store;

        fn deref(&self) -> &Self::Target {
            &self.store
        }
    }

    /// Open the truncated test database, or `None` when Postgres is unavailable
    /// (the caller then skips, matching the previous per-module behaviour).
    pub(crate) fn open() -> Option<TestStore> {
        // A panicking test poisons the lock; the DB is truncated on open anyway,
        // so recover it instead of cascading one failure into every later test.
        let guard = DB_GUARD.lock().unwrap_or_else(|err| err.into_inner());
        match Store::open_test() {
            Ok(store) => Some(TestStore {
                store,
                _guard: guard,
            }),
            Err(err) => {
                eprintln!("skipping store test (postgres unavailable): {err}");
                None
            }
        }
    }
}

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::config::DatabaseConfig;
pub use crate::error::StoreError;
use crate::model::Release;
use crate::notify::Event;

use postgres::PostgresStore;

/// Max delivery attempts before an outbox / sink row is marked `dead`.
///
/// Typed as [`i32`] to match PostgreSQL `INTEGER` columns: on PG 18+ the server
/// infers bind params from column types, and rust-postgres rejects `i64` (INT8)
/// for an INT4 slot (`error serializing parameter N`).
pub const OUTBOX_MAX_ATTEMPTS: i32 = 50;

/// Delivery lease window (seconds). A claimed outbox row stays locked this long
/// so a crashed worker's row becomes claimable again, while still bounding how
/// long a single in-flight delivery may run. Must exceed the HTTP delivery
/// timeout of the slowest sink.
pub const OUTBOX_LEASE_SECS: u32 = 300;

/// Default pooled connection ceiling when `[database].max_connections` is unset.
pub(crate) const DEFAULT_MAX_CONNECTIONS: u32 = 16;

/// Default connection-acquire timeout when `[database].connect_timeout_secs` is unset.
pub(crate) const DEFAULT_CONNECT_TIMEOUT_SECS: u32 = 5;

/// One release identity to upsert into `seen_release`.
#[derive(Debug, Clone)]
pub struct SeenUpsert<'a> {
    /// Stable upstream identity (tag name, digest, feed guid, …).
    pub identity: &'a str,
    /// Human-facing label when it differs from `identity`.
    pub display_tag: Option<&'a str>,
    /// Content fingerprint for change detection.
    pub content_digest: Option<&'a str>,
    /// Publication timestamp (UTC).
    pub published_at: Option<DateTime<Utc>>,
    /// Canonical release URL.
    pub url: Option<&'a str>,
}

impl<'a> SeenUpsert<'a> {
    /// Build from a [`Release`] plus optional digest (caller sets `published_at`).
    #[must_use]
    pub fn from_release(release: &'a Release, content_digest: Option<&'a str>) -> Self {
        let display_tag = if release.raw_tag == release.id {
            None
        } else {
            Some(release.raw_tag.as_str())
        };
        Self {
            identity: release.id.as_str(),
            display_tag,
            content_digest,
            published_at: None,
            url: release.url.as_deref(),
        }
    }

    /// Attach a publication timestamp.
    #[must_use]
    pub fn with_published_at(mut self, published_at: Option<DateTime<Utc>>) -> Self {
        self.published_at = published_at;
        self
    }
}

/// Result of enqueueing a notification into the durable outbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnqueueOutcome {
    /// Outbox row id.
    pub id: i64,
    /// When true the caller should deliver immediately; otherwise the flush loop retries.
    pub deliver_now: bool,
    /// True when this call inserted a new row or reopened a sent row for redelivery.
    pub created: bool,
}

/// Extra columns stored alongside an outbox notification.
#[derive(Debug, Clone, Default)]
pub struct OutboxMeta<'a> {
    /// Content fingerprint for deduplication / reopen-on-change.
    pub content_digest: Option<&'a str>,
    /// Display tag for feed-style sources.
    pub display_tag: Option<&'a str>,
    /// Publication timestamp (UTC).
    pub published_at: Option<DateTime<Utc>>,
    /// Hold delivery until this moment (cron-gated notification schedule).
    /// `None` = deliver as soon as possible.
    pub deliver_after: Option<DateTime<Utc>>,
}

/// Applied-configuration ledger row status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigRevisionStatus {
    Applied,
    Rejected,
}

impl ConfigRevisionStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Rejected => "rejected",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "applied" => Some(Self::Applied),
            "rejected" => Some(Self::Rejected),
            _ => None,
        }
    }
}

/// One row from `config_revision`.
#[derive(Debug, Clone)]
pub struct ConfigRevisionRecord {
    pub id: i64,
    pub content: String,
    pub content_sha256: String,
    pub revision_label: Option<String>,
    pub applied_at: String,
    pub applied_by: Option<String>,
    pub source_addr: Option<String>,
    pub status: ConfigRevisionStatus,
    pub error: Option<String>,
    /// Ledger stream: `None` = single-document mode, else the org slug.
    pub organization_id: Option<String>,
}

/// One `config_revision` row **without its content**, for history listings.
///
/// `content` is deliberately absent: it is the raw desired-state document,
/// which `GET /api/v1/config` only ever returns through
/// [`crate::config_apply::redact_desired_document`]. A history endpoint
/// returning it verbatim would bypass that redaction entirely — and it would
/// also make a 50-row page carry 50 full config documents nobody displays.
/// Callers that genuinely need one revision's body go through the ledger
/// lookups that redact it.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigRevisionSummary {
    pub id: i64,
    pub content_sha256: String,
    pub revision_label: Option<String>,
    pub applied_at: String,
    pub applied_by: Option<String>,
    pub source_addr: Option<String>,
    pub status: ConfigRevisionStatus,
    pub error: Option<String>,
    /// Ledger stream: `None` = single-document mode, else the org slug.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization_id: Option<String>,
}

/// Input for appending to the config revision ledger.
#[derive(Debug, Clone)]
pub struct ConfigRevisionInsert<'a> {
    pub content: &'a str,
    pub content_sha256: &'a str,
    pub revision_label: Option<&'a str>,
    pub applied_by: Option<&'a str>,
    pub source_addr: Option<&'a str>,
    pub status: ConfigRevisionStatus,
    pub error: Option<&'a str>,
    /// Ledger stream: `None` = single-document mode, else the org slug.
    pub organization_id: Option<&'a str>,
}

/// Outbox depth by delivery status (excludes `sent`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutboxCounts {
    /// Rows waiting for first delivery.
    pub pending: usize,
    /// Rows that failed but are still retryable.
    pub failed: usize,
    /// Rows that exhausted retries.
    pub dead: usize,
    /// Pending/failed rows held until `deliver_after` (notify_schedule).
    pub deferred: usize,
}

/// Runtime fields from `source_state` for one source.
#[derive(Debug, Clone, Default)]
pub struct SourceStateRow {
    /// Whether the silent baseline has been recorded.
    pub initialized: bool,
    /// RFC 3339 timestamp of the most recent poll.
    pub last_polled_at: Option<String>,
    /// Newest filtered upstream tag for dashboard display.
    pub latest_release_tag: Option<String>,
}

/// One seen release row for observability UI.
#[derive(Debug, Clone)]
pub struct SeenReleaseEntry {
    /// Display tag or identity.
    pub tag: String,
    /// RFC 3339 publication timestamp.
    pub published_at: Option<String>,
    /// Canonical release URL.
    pub url: Option<String>,
    /// RFC 3339 first-seen timestamp.
    pub first_seen_at: String,
}

/// Pending/failed outbox row for observability UI (no body).
#[derive(Debug, Clone, Serialize)]
pub struct OutboxEntry {
    pub id: i64,
    pub source_id: String,
    pub identity: String,
    pub status: String,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub created_at: String,
    pub title: String,
    pub url: Option<String>,
    /// Team routing tag carried on the event (for team-scoped outbox views).
    pub routing_tag: Option<String>,
    /// RFC 3339 moment before which delivery is held (cron-gated schedule).
    pub deliver_after: Option<String>,
    /// Owning organization derived from a namespaced `source_id` (`org::…`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization_id: Option<String>,
}

/// Full outbox row for delivery retry.
#[derive(Debug, Clone)]
pub struct OutboxRecord {
    pub id: i64,
    pub source_id: String,
    pub identity: String,
    pub content_digest: Option<String>,
    pub display_tag: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
    pub title: String,
    pub body: String,
    pub url: Option<String>,
    pub routing_tag: Option<String>,
    pub source_kind: String,
    pub attempts: u32,
    /// Cron-gated delivery moment (see [`OutboxMeta::deliver_after`]). Rows
    /// sharing one moment and `routing_tag` are eligible for digest grouping
    /// at flush time (see `crate::pipeline`).
    pub deliver_after: Option<DateTime<Utc>>,
}

impl OutboxRecord {
    /// Reconstruct the notification [`Event`] for delivery.
    #[must_use]
    pub fn to_event(&self) -> Event {
        Event {
            source_id: self.source_id.clone(),
            source_kind: self.source_kind.clone(),
            title: self.title.clone(),
            body: self.body.clone(),
            url: self.url.clone(),
            routing_tag: self.routing_tag.clone(),
        }
    }
}

/// Row counts removed by [`Store::prune`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PruneReport {
    /// `seen_release` rows deleted.
    pub seen_deleted: usize,
    /// `webhook_delivery` rows deleted.
    pub webhooks_deleted: usize,
    /// Sent `notification_outbox` rows deleted.
    pub outbox_deleted: usize,
}

impl PruneReport {
    /// Whether any rows were removed.
    #[must_use]
    pub fn any(&self) -> bool {
        self.seen_deleted > 0 || self.webhooks_deleted > 0 || self.outbox_deleted > 0
    }
}

/// Backend-agnostic release state store (PostgreSQL).
pub struct Store(PostgresStore);

/// Process-lifetime lease that enforces **one poller per database**.
///
/// Held across `xrelease serve`. Drop (or process exit)
/// releases the PostgreSQL session advisory lock.
#[must_use = "hold until the poller exits so the advisory lock stays taken"]
pub struct PollerLease(
    #[expect(
        dead_code,
        reason = "RAII guard only — Drop of the inner connection unlocks"
    )]
    postgres::PollerLease,
);

/// Run a blocking store call without starving the async runtime.
///
/// The sync [`postgres`] crate owns a nested current-thread Tokio [`Runtime`] and
/// calls `block_on` on every connect/query. That panics with
/// *"Cannot start a runtime from within a runtime"* if a Tokio context is already
/// entered on the calling thread. [`tokio::task::block_in_place`] is **not**
/// enough — the Handle remains active.
///
/// When called from inside a Tokio runtime we therefore run the closure on a
/// fresh OS thread (no entered Handle). On the multi-thread runtime we wait with
/// [`block_in_place`] so the worker can keep scheduling sibling tasks.
///
/// Outside a runtime (unit tests, CLI before `#[tokio::main]`) the closure runs
/// inline.
pub(crate) fn db_blocking<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    use tokio::runtime::{Handle, RuntimeFlavor};

    if Handle::try_current().is_err() {
        return f();
    }

    std::thread::scope(|scope| {
        let task = scope.spawn(f);
        match Handle::try_current() {
            Ok(handle) if handle.runtime_flavor() == RuntimeFlavor::MultiThread => {
                tokio::task::block_in_place(|| task.join().expect("db worker thread panicked"))
            }
            _ => task.join().expect("db worker thread panicked"),
        }
    })
}

macro_rules! delegate {
 ($self:expr, $method:ident ( $( $arg:expr ),* $(,)? )) => {
 db_blocking(|| $self.0.$method( $( $arg ),* ))
 };
}

impl Store {
    /// Open a PostgreSQL pool from [`DatabaseConfig`].
    pub fn open_from_config(config: &DatabaseConfig) -> Result<Self, StoreError> {
        // Pool construction (and `min_idle`) opens connections — must leave the
        // async runtime context; see [`db_blocking`].
        db_blocking(|| Ok(Self(PostgresStore::open(config)?)))
    }

    /// Become the sole poller for this database, or fail if another holds it.
    ///
    /// Call from `serve` only — not from `health`, `validate`,
    /// config resolve, or read-only API paths.
    pub fn try_acquire_poller_lease(&self) -> Result<PollerLease, StoreError> {
        db_blocking(|| self.0.try_acquire_poller_lease().map(PollerLease))
    }

    /// Reset all tables (integration tests).
    pub fn truncate_all(&self) -> Result<(), StoreError> {
        delegate!(self, truncate_all())
    }

    /// Applied PostgreSQL schema version (`schema_meta`).
    pub fn schema_version(&self) -> Result<i32, StoreError> {
        delegate!(self, schema_version())
    }

    /// Open a test database (requires `XRELEASE_TEST_POSTGRES_URL` or local Postgres).
    ///
    /// Truncates every table, so tests must not run this concurrently — prefer
    /// [`test_db::open`], which serializes callers. Direct use is only correct
    /// when a test deliberately needs a second, unguarded pool.
    #[cfg(test)]
    pub fn open_test() -> Result<Self, StoreError> {
        let url = std::env::var("XRELEASE_TEST_POSTGRES_URL").unwrap_or_else(|_| {
            "postgres://xrelease:xrelease@127.0.0.1:5432/xrelease_test".to_owned()
        });
        let config = DatabaseConfig {
            postgres_url: url,
            max_connections: Some(4),
            connect_timeout_secs: Some(DEFAULT_CONNECT_TIMEOUT_SECS),
            ..Default::default()
        };
        db_blocking(|| {
            let store = PostgresStore::open(&config)?;
            store.truncate_all()?;
            Ok(Self(store))
        })
    }

    /// Whether the source has had its silent baseline recorded yet.
    pub fn is_initialized(&self, source_id: &str) -> Result<bool, StoreError> {
        delegate!(self, is_initialized(source_id))
    }

    /// Mark a source as baselined; subsequent unseen identities will notify.
    pub fn mark_initialized(&self, source_id: &str) -> Result<(), StoreError> {
        delegate!(self, mark_initialized(source_id))
    }

    /// Record the timestamp of the most recent poll attempt.
    pub fn touch_polled(&self, source_id: &str) -> Result<(), StoreError> {
        delegate!(self, touch_polled(source_id))
    }

    /// Persist the newest filtered upstream tag for dashboard display.
    pub fn set_latest_release_tag(&self, source_id: &str, tag: &str) -> Result<(), StoreError> {
        delegate!(self, set_latest_release_tag(source_id, tag))
    }

    /// Runtime fields from `source_state` for one source.
    pub fn source_state_row(&self, source_id: &str) -> Result<SourceStateRow, StoreError> {
        delegate!(self, source_state_row(source_id))
    }

    /// All `source_state` rows keyed by `source_id` (for UI source list).
    pub fn all_source_states(&self) -> Result<HashMap<String, SourceStateRow>, StoreError> {
        delegate!(self, all_source_states())
    }

    /// Count seen identities per source (for UI dashboards).
    pub fn seen_counts_by_source(&self) -> Result<HashMap<String, u64>, StoreError> {
        delegate!(self, seen_counts_by_source())
    }

    /// Count seen identities for one source (detail page — avoids GROUP BY over all sources).
    pub fn seen_count(&self, source_id: &str) -> Result<u64, StoreError> {
        delegate!(self, seen_count(source_id))
    }

    /// Best-effort newest tag from the seen-release catalogue (UI backfill).
    pub fn best_seen_identity(&self, source_id: &str) -> Result<Option<String>, StoreError> {
        delegate!(self, best_seen_identity(source_id))
    }

    /// List synced release identities for one source (newest semver first).
    pub fn list_seen_releases(
        &self,
        source_id: &str,
        limit: usize,
    ) -> Result<Vec<SeenReleaseEntry>, StoreError> {
        delegate!(self, list_seen_releases(source_id, limit))
    }

    /// Pending/failed outbox rows for observability UI (no message body).
    pub fn list_outbox_entries(&self, limit: usize) -> Result<Vec<OutboxEntry>, StoreError> {
        delegate!(self, list_outbox_entries(limit))
    }

    /// Return the stored ETag for conditional upstream requests.
    pub fn get_etag(&self, source_id: &str) -> Result<Option<String>, StoreError> {
        delegate!(self, get_etag(source_id))
    }

    /// Persist the latest upstream ETag (or clear it when `None`).
    pub fn set_etag(&self, source_id: &str, etag: Option<&str>) -> Result<(), StoreError> {
        delegate!(self, set_etag(source_id, etag))
    }

    /// Whether any seen row for this source lacks `published_at`.
    pub fn has_seen_missing_published_at(&self, source_id: &str) -> Result<bool, StoreError> {
        delegate!(self, has_seen_missing_published_at(source_id))
    }

    /// Backfill upstream metadata on already-seen identities.
    pub fn enrich_seen_metadata(
        &self,
        source_id: &str,
        releases: &[&Release],
    ) -> Result<(), StoreError> {
        delegate!(self, enrich_seen_metadata(source_id, releases))
    }

    /// Mark a single release identity as seen (idempotent).
    pub fn record_seen(&self, source_id: &str, item: &SeenUpsert<'_>) -> Result<(), StoreError> {
        delegate!(self, record_seen(source_id, item))
    }

    /// Stored content fingerprint for a seen identity, if any.
    pub fn content_digest(
        &self,
        source_id: &str,
        identity: &str,
    ) -> Result<Option<String>, StoreError> {
        delegate!(self, content_digest(source_id, identity))
    }

    /// Whether this identity has ever been recorded for the source.
    pub fn has_seen(&self, source_id: &str, identity: &str) -> Result<bool, StoreError> {
        delegate!(self, has_seen(source_id, identity))
    }

    /// Of the given identities, return those not yet recorded for this source.
    pub fn unseen<'a>(
        &self,
        source_id: &str,
        identities: &[&'a str],
    ) -> Result<Vec<&'a str>, StoreError> {
        delegate!(self, unseen(source_id, identities))
    }

    /// All seen identities for one source — single query for poll-time diffing.
    pub fn load_seen_index(
        &self,
        source_id: &str,
    ) -> Result<HashMap<String, Option<String>>, StoreError> {
        delegate!(self, load_seen_index(source_id))
    }

    /// Record many identities in one transaction (silent baseline on first poll).
    pub fn record_seen_batch(
        &self,
        source_id: &str,
        items: &[SeenUpsert<'_>],
    ) -> Result<(), StoreError> {
        delegate!(self, record_seen_batch(source_id, items))
    }

    /// Cheap liveness check used by the `health` subcommand / k8s probe.
    pub fn health(&self) -> Result<(), StoreError> {
        delegate!(self, health())
    }

    /// Claim a webhook delivery id for idempotency.
    pub fn claim_webhook_delivery(&self, delivery_id: &str) -> Result<bool, StoreError> {
        delegate!(self, claim_webhook_delivery(delivery_id))
    }

    /// Delete aged rows to bound database growth on long-running deployments.
    pub fn prune(
        &self,
        seen_after_days: u32,
        webhooks_after_days: u32,
        outbox_sent_after_days: u32,
    ) -> Result<PruneReport, StoreError> {
        delegate!(
            self,
            prune(seen_after_days, webhooks_after_days, outbox_sent_after_days)
        )
    }

    /// Enqueue a notification for durable delivery.
    pub fn try_enqueue_notification(
        &self,
        event: &Event,
        identity: &str,
        meta: OutboxMeta<'_>,
    ) -> Result<Option<EnqueueOutcome>, StoreError> {
        delegate!(self, try_enqueue_notification(event, identity, meta))
    }

    /// Atomically lease a batch of retryable notifications, oldest first.
    ///
    /// Concurrency-safe via `FOR UPDATE SKIP LOCKED` + the `locked_until` lease:
    /// no two workers claim the same row, so notifications are never delivered
    /// twice. Leases auto-expire after `lease_secs` for crash recovery.
    pub fn claim_outbox_batch(
        &self,
        limit: usize,
        lease_secs: u32,
    ) -> Result<Vec<OutboxRecord>, StoreError> {
        delegate!(self, claim_outbox_batch(limit, lease_secs))
    }

    /// Lease a single outbox row for inline delivery; `false` means another
    /// worker holds an unexpired lease and will deliver it.
    pub fn claim_outbox_row(&self, outbox_id: i64, lease_secs: u32) -> Result<bool, StoreError> {
        delegate!(self, claim_outbox_row(outbox_id, lease_secs))
    }

    /// Release a delivery lease so a failed row can be retried at the next flush.
    pub fn release_outbox_lease(&self, outbox_id: i64) -> Result<(), StoreError> {
        delegate!(self, release_outbox_lease(outbox_id))
    }

    /// Defer the next claim of this row by `backoff_secs` (exponential retry
    /// backoff after a failed delivery attempt).
    pub fn apply_delivery_backoff(
        &self,
        outbox_id: i64,
        backoff_secs: f64,
    ) -> Result<(), StoreError> {
        delegate!(self, apply_delivery_backoff(outbox_id, backoff_secs))
    }

    /// Defer the next claim by `backoff_secs` **without** counting an attempt —
    /// for an attempt that made no network call because every unsuccessful sink
    /// was skipped by an open circuit breaker.
    pub fn defer_delivery(&self, outbox_id: i64, backoff_secs: f64) -> Result<(), StoreError> {
        delegate!(self, defer_delivery(outbox_id, backoff_secs))
    }

    /// Mark delivery complete and record the release as seen in one transaction.
    pub fn complete_notification(
        &self,
        outbox_id: i64,
        source_id: &str,
        seen: &SeenUpsert<'_>,
    ) -> Result<bool, StoreError> {
        delegate!(self, complete_notification(outbox_id, source_id, seen))
    }

    /// Record a failed delivery attempt; marks the row `dead` after max attempts.
    ///
    /// Returns `true` when the row transitioned to `dead`.
    pub fn fail_notification(&self, outbox_id: i64, error: &str) -> Result<bool, StoreError> {
        delegate!(self, fail_notification(outbox_id, error))
    }

    /// Create per-sink delivery rows for fan-out tracking (idempotent).
    ///
    /// Each entry is a `(global_sink_index, kind)` pair so the ledger keys on the
    /// same sink index space used by [`crate::notify::CompositeNotifier`].
    pub fn ensure_sink_deliveries(
        &self,
        outbox_id: i64,
        sinks: &[(usize, &str)],
    ) -> Result<(), StoreError> {
        delegate!(self, ensure_sink_deliveries(outbox_id, sinks))
    }

    /// Sink indices that still need delivery (`pending` / retryable `failed`).
    pub fn list_pending_sink_indices(&self, outbox_id: i64) -> Result<Vec<usize>, StoreError> {
        delegate!(self, list_pending_sink_indices(outbox_id))
    }

    /// Mark one sink delivery as successful.
    pub fn complete_sink_delivery(
        &self,
        outbox_id: i64,
        sink_index: usize,
    ) -> Result<(), StoreError> {
        delegate!(self, complete_sink_delivery(outbox_id, sink_index))
    }

    /// Record a failed sink attempt; marks the sink `dead` after max attempts.
    pub fn fail_sink_delivery(
        &self,
        outbox_id: i64,
        sink_index: usize,
        error: &str,
    ) -> Result<(), StoreError> {
        delegate!(self, fail_sink_delivery(outbox_id, sink_index, error))
    }

    /// Mark retryable sink ledger rows `dead` when they no longer match routing
    /// (config drift). Prevents orphaned `pending`/`failed` rows from stranding
    /// the parent outbox in an infinite flush loop.
    ///
    /// When `sink_indices` is empty, every retryable row for `outbox_id` is
    /// abandoned; otherwise only the listed indices are.
    pub fn abandon_retryable_sinks(
        &self,
        outbox_id: i64,
        sink_indices: &[usize],
        error: &str,
    ) -> Result<usize, StoreError> {
        delegate!(
            self,
            abandon_retryable_sinks(outbox_id, sink_indices, error)
        )
    }

    /// Whether every tracked sink row is `sent` (or no rows exist — pre-sink-ledger outbox).
    pub fn sinks_delivery_complete(&self, outbox_id: i64) -> Result<bool, StoreError> {
        delegate!(self, sinks_delivery_complete(outbox_id))
    }

    /// Sync parent outbox status from per-sink rows after a delivery round.
    ///
    /// Returns `true` when the parent row was marked `dead`.
    pub fn sync_outbox_sink_status(&self, outbox_id: i64) -> Result<bool, StoreError> {
        delegate!(self, sync_outbox_sink_status(outbox_id))
    }

    /// Requeue every `dead` notification for another delivery round (operator
    /// dead-letter recovery). Returns the number of rows revived.
    pub fn requeue_dead_outbox(&self) -> Result<usize, StoreError> {
        delegate!(self, requeue_dead_outbox())
    }

    /// Promote exhausted retry rows to `dead` (startup recovery after crash).
    pub fn finalize_exhausted_outbox(&self) -> Result<usize, StoreError> {
        delegate!(self, finalize_exhausted_outbox())
    }

    /// Outbox depth by status for metrics and readiness probes.
    pub fn outbox_counts(&self) -> Result<OutboxCounts, StoreError> {
        delegate!(self, outbox_counts())
    }

    /// Latest successfully applied config revision in one ledger stream
    /// (`None` = the single-document stream).
    pub fn latest_applied_config_revision(
        &self,
        organization: Option<&str>,
    ) -> Result<Option<ConfigRevisionRecord>, StoreError> {
        delegate!(self, latest_applied_config_revision(organization))
    }

    /// Latest rejected config apply attempt in one ledger stream.
    pub fn latest_rejected_config_revision(
        &self,
        organization: Option<&str>,
    ) -> Result<Option<ConfigRevisionRecord>, StoreError> {
        delegate!(self, latest_rejected_config_revision(organization))
    }

    /// Latest rejected apply attempt across every stream (status banner).
    pub fn latest_rejected_config_revision_any(
        &self,
    ) -> Result<Option<ConfigRevisionRecord>, StoreError> {
        delegate!(self, latest_rejected_config_revision_any())
    }

    /// Previous successfully applied revision in one stream (rollback target).
    pub fn previous_applied_config_revision(
        &self,
        organization: Option<&str>,
    ) -> Result<Option<ConfigRevisionRecord>, StoreError> {
        delegate!(self, previous_applied_config_revision(organization))
    }

    /// Append a config apply attempt to the audit ledger.
    pub fn insert_config_revision(
        &self,
        row: &ConfigRevisionInsert<'_>,
    ) -> Result<i64, StoreError> {
        delegate!(self, insert_config_revision(row))
    }

    /// Upsert sealed secrets into `app_secret` and refresh the process vault.
    pub fn upsert_app_secrets(
        &self,
        writes: &[crate::config::SecretWrite],
    ) -> Result<(), StoreError> {
        delegate!(self, upsert_app_secrets(writes))
    }

    /// Delete sealed secrets from `app_secret` and the process vault.
    pub fn delete_app_secrets(&self, names: &[String]) -> Result<usize, StoreError> {
        delegate!(self, delete_app_secrets(names))
    }

    /// Page through the whole ledger (every stream), newest first (metadata only).
    pub fn list_config_revisions(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ConfigRevisionSummary>, StoreError> {
        delegate!(self, list_config_revisions(limit, offset))
    }

    /// Total config revision ledger rows across every stream (history pagination).
    pub fn count_config_revisions(&self) -> Result<i64, StoreError> {
        delegate!(self, count_config_revisions())
    }

    /// Page through one organization's ledger stream, newest first (metadata only).
    pub fn list_config_revisions_for(
        &self,
        organization: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ConfigRevisionSummary>, StoreError> {
        delegate!(self, list_config_revisions_for(organization, limit, offset))
    }

    /// Ledger rows in one organization's stream (history pagination).
    pub fn count_config_revisions_for(&self, organization: &str) -> Result<i64, StoreError> {
        delegate!(self, count_config_revisions_for(organization))
    }

    /// Count notifications waiting for delivery or retry (excludes dead).
    pub fn outbox_pending_count(&self) -> Result<usize, StoreError> {
        delegate!(self, outbox_pending_count())
    }

    /// Total rows in `app_user` (first-boot seed gate).
    pub fn count_users(&self) -> Result<i64, StoreError> {
        delegate!(self, count_users())
    }

    /// All application users (local + OIDC), oldest first.
    pub fn list_users(&self) -> Result<Vec<AppUser>, StoreError> {
        delegate!(self, list_users())
    }

    /// Look up a local user by username.
    pub fn get_user_by_username(&self, username: &str) -> Result<Option<AppUser>, StoreError> {
        delegate!(self, get_user_by_username(username))
    }

    /// Look up a user by primary key.
    pub fn get_user_by_id(&self, id: i64) -> Result<Option<AppUser>, StoreError> {
        delegate!(self, get_user_by_id(id))
    }

    /// Look up an OIDC-linked user by IdP subject.
    pub fn get_user_by_oidc_sub(&self, oidc_sub: &str) -> Result<Option<AppUser>, StoreError> {
        delegate!(self, get_user_by_oidc_sub(oidc_sub))
    }

    /// Local user with matching email and no `oidc_sub` (SSO auto-link candidate).
    pub fn find_linkable_local_user_by_email(
        &self,
        email: &str,
    ) -> Result<Option<AppUser>, StoreError> {
        delegate!(self, find_linkable_local_user_by_email(email))
    }

    /// Insert a new local or OIDC user row.
    pub fn insert_user(&self, user: &AppUserInsert<'_>) -> Result<AppUser, StoreError> {
        delegate!(self, insert_user(user))
    }

    /// Attach or clear an IdP subject on a local user (`auth_source` stays `local`).
    pub fn link_user_oidc_sub(
        &self,
        user_id: i64,
        oidc_sub: Option<&str>,
    ) -> Result<AppUser, StoreError> {
        delegate!(self, link_user_oidc_sub(user_id, oidc_sub))
    }

    /// Set / clear the SSO link email on a local user (clears a stale subject).
    pub fn set_user_oidc_link_email(
        &self,
        user_id: i64,
        email: Option<&str>,
    ) -> Result<AppUser, StoreError> {
        delegate!(self, set_user_oidc_link_email(user_id, email))
    }

    /// Create or refresh an OIDC identity (link local by verified email when
    /// permitted). `Ok(None)` means the subject resolved to no row and
    /// `allow_create` forbade provisioning one — the caller must deny sign-in.
    pub fn upsert_oidc_user(
        &self,
        user: &AppUserUpsertOidc<'_>,
    ) -> Result<Option<AppUser>, StoreError> {
        delegate!(self, upsert_oidc_user(user))
    }

    /// Record a successful login timestamp.
    pub fn touch_user_last_login(&self, id: i64) -> Result<(), StoreError> {
        delegate!(self, touch_user_last_login(id))
    }

    /// Invalidate every live session for a user (logout / password / role
    /// change); returns the new session version.
    pub fn bump_session_version(&self, id: i64) -> Result<i64, StoreError> {
        delegate!(self, bump_session_version(id))
    }
}

/// Persisted UI / management identity (`app_user`).
#[derive(Debug, Clone)]
pub struct AppUser {
    pub id: i64,
    pub username: Option<String>,
    pub password_hash: Option<String>,
    pub oidc_sub: Option<String>,
    pub email: Option<String>,
    pub display_name: Option<String>,
    /// `admin` | `operator` | `viewer`
    pub role: String,
    /// `local` | `oidc`
    pub auth_source: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_login_at: Option<DateTime<Utc>>,
    /// Session-invalidation epoch: a session JWT is valid only while its `ver`
    /// claim equals this. Bumped on logout / password / role change.
    pub session_version: i64,
}

/// Insert payload for a new `app_user` row.
#[derive(Debug, Clone)]
pub struct AppUserInsert<'a> {
    pub username: Option<&'a str>,
    pub password_hash: Option<&'a str>,
    pub oidc_sub: Option<&'a str>,
    pub email: Option<&'a str>,
    pub display_name: Option<&'a str>,
    pub role: &'a str,
    pub auth_source: &'a str,
}

/// OIDC upsert payload (matched on `oidc_sub`).
#[derive(Debug, Clone)]
pub struct AppUserUpsertOidc<'a> {
    pub oidc_sub: &'a str,
    pub email: Option<&'a str>,
    pub display_name: Option<&'a str>,
    pub role: &'a str,
    /// Adopt an existing local account whose email matches `email`.
    ///
    /// Caller sets this ONLY when the IdP asserted `email_verified` — an
    /// unverified address would let anyone who can mint a token for it take
    /// over the matching local account.
    pub link_by_email: bool,
    /// Insert a row when the subject resolves to nothing (`[api.oidc]
    /// auto_create_users`). When false the upsert returns `Ok(None)` and the
    /// caller must reject the sign-in.
    pub allow_create: bool,
}

#[cfg(test)]
mod offload_tests {
    use super::db_blocking;

    #[test]
    fn db_blocking_should_run_inline_without_runtime() {
        assert_eq!(db_blocking(|| 41 + 1), 42);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn db_blocking_should_offload_on_multi_thread_runtime() {
        let value = db_blocking(|| 41 + 1);
        assert_eq!(value, 42);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn db_blocking_should_run_inline_on_current_thread_runtime() {
        // Offloads to a scoped OS thread (current-thread cannot use block_in_place).
        let value = db_blocking(|| 41 + 1);
        assert_eq!(value, 42);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn db_blocking_should_allow_nested_tokio_runtime_block_on() {
        // Mirrors sync `postgres`: nested current-thread Runtime + block_on.
        let value = db_blocking(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("nested runtime");
            rt.block_on(async { 41 + 1 })
        });
        assert_eq!(value, 42);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn store_drop_should_not_panic_inside_tokio_runtime() {
        // Reproduces `Runtime::new` under `serve`: open a pool (schema apply)
        // then drop it on the Tokio thread. Without offloaded Drop this panics
        // with "Cannot start a runtime from within a runtime".
        let Some(store) = super::test_db::open() else {
            return;
        };
        drop(store);
    }
}
