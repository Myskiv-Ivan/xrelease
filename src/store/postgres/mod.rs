//! PostgreSQL-backed state store.

mod advisory;
mod app_secret;
mod config_revision;
mod migrate;
mod outbox;
mod users;

use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Utc};
use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};
use postgres::NoTls;
use postgres_openssl::MakeTlsConnector;
use r2d2::{Pool, PooledConnection};
use r2d2_postgres::PostgresConnectionManager;

use super::{PruneReport, SeenReleaseEntry, SeenUpsert, SourceStateRow, StoreError};
use crate::config::{DatabaseConfig, DatabaseSslMode};

type PlainPool = Pool<PostgresConnectionManager<NoTls>>;
type TlsPool = Pool<PostgresConnectionManager<MakeTlsConnector>>;
type PlainConn = PooledConnection<PostgresConnectionManager<NoTls>>;
type TlsConn = PooledConnection<PostgresConnectionManager<MakeTlsConnector>>;

enum PgPool {
    Plain(PlainPool),
    Tls(TlsPool),
}

enum PgConn {
    Plain(PlainConn),
    Tls(TlsConn),
}

impl Deref for PgConn {
    type Target = postgres::Client;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Plain(conn) => conn,
            Self::Tls(conn) => conn,
        }
    }
}

impl DerefMut for PgConn {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Plain(conn) => conn,
            Self::Tls(conn) => conn,
        }
    }
}

type SeenReleaseListRow = (
    String,
    Option<String>,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
    Option<String>,
);
type SeenReleaseMeta = (
    Option<String>,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
    Option<String>,
);

pub(super) const SCHEMA: &str = include_str!("../postgres_schema.sql");

/// Advisory-lock class for the single-poller lease (stable int4, not a PG oid).
pub(crate) const POLLER_LOCK_CLASS: i32 = 0x7852_656c; // "xRel"
/// Advisory-lock id within [`POLLER_LOCK_CLASS`].
pub(crate) const POLLER_LOCK_ID: i32 = 0x706f_6c6c; // "poll"

/// Session-level lease that serializes pollers on one PostgreSQL database.
///
/// Holds a pooled connection checked out for the process lifetime of
/// `serve`. Drop unlocks (and returns the connection) so a
/// subsequent poller can start; a hard crash releases the lock when the
/// backend closes the TCP session.
pub(crate) struct PollerLease {
    conn: Option<PgConn>,
}

impl Drop for PollerLease {
    fn drop(&mut self) {
        let Some(mut conn) = self.conn.take() else {
            return;
        };
        // Unlock before returning the connection to the pool — a still-locked
        // idle pooled session would block *other* databases' pollers only if
        // they shared the connection (they don't), but would leave *this*
        // database's lock held until the pool connection is closed, which is
        // too late for a clean hand-off after `check` exits.
        super::db_blocking(move || {
            let _ = conn.execute(
                "SELECT pg_advisory_unlock($1, $2)",
                &[&POLLER_LOCK_CLASS, &POLLER_LOCK_ID],
            );
            drop(conn);
        });
    }
}

/// Thread-safe PostgreSQL state backend.
pub(crate) struct PostgresStore {
    /// Wrapped in [`Option`] so [`Drop`] can move the pool onto a non-Tokio
    /// thread (sync `postgres` Client teardown calls `block_on`).
    pool: Option<PgPool>,
    /// When set (`XRELEASE_CONFIG_ENCRYPTION_KEY`), seals each `app_secret`
    /// value with AES-256-GCM (desired document keeps only `*_env` refs).
    ledger_cipher: Option<std::sync::Arc<crate::crypto::LedgerCipher>>,
}

impl Drop for PostgresStore {
    fn drop(&mut self) {
        let Some(pool) = self.pool.take() else {
            return;
        };
        if std::thread::panicking() {
            // Avoid a second panic from Client::drop on a Tokio worker during
            // unwind; leak the pool (process is already failing).
            std::mem::forget(pool);
            return;
        }
        // Runtime::new() opens a short-lived Store on the async thread; without
        // this offload, Drop panics with "Cannot start a runtime from within a
        // runtime" when closing idle pooled connections.
        super::db_blocking(|| drop(pool));
    }
}

impl PostgresStore {
    /// Open a connection pool from [`DatabaseConfig`], applying the schema.
    ///
    /// `connect_timeout_secs` bounds the wait for a connection so startup probes
    /// and tests fail fast (instead of r2d2's 30s default) when the database is
    /// unreachable.
    pub fn open(config: &DatabaseConfig) -> Result<Self, StoreError> {
        let url = config.postgres_url.trim();
        if url.is_empty() {
            return Err(StoreError::Other(
                "database.postgres_url is required (or set XRELEASE_DATABASE_URL)".to_owned(),
            ));
        }
        let max_connections = config
            .max_connections
            .unwrap_or(super::DEFAULT_MAX_CONNECTIONS);
        let connect_timeout_secs = u64::from(
            config
                .connect_timeout_secs
                .unwrap_or(super::DEFAULT_CONNECT_TIMEOUT_SECS)
                .max(1),
        );
        let pg_config: postgres::Config = url.parse().map_err(|err: postgres::Error| {
            StoreError::Other(format!("invalid postgres url: {err}"))
        })?;

        let ssl_mode = config.effective_ssl_mode();
        let pool = if ssl_mode.uses_tls() {
            let connector = build_tls_connector(ssl_mode, config.ssl_root_cert.as_deref())?;
            let manager = PostgresConnectionManager::new(pg_config, connector);
            let pool = Pool::builder()
                .max_size(max_connections.max(1))
                .min_idle(Some(1))
                .connection_timeout(Duration::from_secs(connect_timeout_secs))
                .build(manager)?;
            PgPool::Tls(pool)
        } else {
            let manager = PostgresConnectionManager::new(pg_config, NoTls);
            let pool = Pool::builder()
                .max_size(max_connections.max(1))
                .min_idle(Some(1))
                .connection_timeout(Duration::from_secs(connect_timeout_secs))
                .build(manager)?;
            PgPool::Plain(pool)
        };

        let ledger_cipher = crate::crypto::LedgerCipher::from_env()?.map(std::sync::Arc::new);
        if ledger_cipher.is_some() {
            tracing::info!(
                "app_secret at-rest encryption enabled (XRELEASE_CONFIG_ENCRYPTION_KEY)"
            );
        } else {
            tracing::debug!(
                "app_secret at-rest encryption disabled (XRELEASE_CONFIG_ENCRYPTION_KEY unset)"
            );
        }
        let store = Self {
            pool: Some(pool),
            ledger_cipher,
        };
        {
            let mut client = store.conn()?;
            // The schema is all `IF NOT EXISTS` / idempotent DO blocks, so on
            // every boot after the first PostgreSQL emits a NOTICE per object
            // ("already exists, skipping"). rust-postgres forwards those to
            // `log`, which the tracing bridge surfaces at INFO — dozens of
            // lines of startup noise that hide real messages. Silence them at
            // the source for this session only; warnings and errors still pass.
            client.batch_execute("SET client_min_messages TO warning")?;
            migrate::apply_schema(&mut *client)?;
        }
        store.load_app_secrets_into_vault()?;
        tracing::debug!(ssl_mode = ssl_mode.as_str(), "postgresql pool opened");
        Ok(store)
    }

    /// Try to become the sole poller for this database.
    ///
    /// Uses `pg_try_advisory_lock` on a checked-out pooled connection that stays
    /// held until [`PollerLease`] drops. Does **not** coordinate outbox delivery
    /// (that remains `FOR UPDATE SKIP LOCKED` + lease columns).
    pub fn try_acquire_poller_lease(&self) -> Result<PollerLease, StoreError> {
        let mut conn = self.conn()?;
        let locked: bool = conn
            .query_one(
                "SELECT pg_try_advisory_lock($1, $2)",
                &[&POLLER_LOCK_CLASS, &POLLER_LOCK_ID],
            )?
            .get(0);
        if !locked {
            return Err(StoreError::PollerBusy);
        }
        tracing::info!("acquired single-poller lease on PostgreSQL database");
        Ok(PollerLease { conn: Some(conn) })
    }

    fn conn(&self) -> Result<PgConn, StoreError> {
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| StoreError::Other("postgresql pool already closed".into()))?;
        match pool {
            PgPool::Plain(pool) => Ok(PgConn::Plain(pool.get()?)),
            PgPool::Tls(pool) => Ok(PgConn::Tls(pool.get()?)),
        }
    }
}

fn build_tls_connector(
    mode: DatabaseSslMode,
    root_cert: Option<&str>,
) -> Result<MakeTlsConnector, StoreError> {
    let mut builder = SslConnector::builder(SslMethod::tls())
        .map_err(|err| StoreError::Other(format!("postgresql tls connector: {err}")))?;

    match mode {
        DatabaseSslMode::Disable => {
            return Err(StoreError::Other(
                "internal: build_tls_connector called for ssl_mode=disable".into(),
            ));
        }
        DatabaseSslMode::Prefer | DatabaseSslMode::Require => {
            builder.set_verify(SslVerifyMode::NONE);
        }
        DatabaseSslMode::VerifyCa | DatabaseSslMode::VerifyFull => {
            builder.set_verify(SslVerifyMode::PEER);
            if let Some(path) = root_cert {
                let path = Path::new(path);
                if !path.is_file() {
                    return Err(StoreError::Other(format!(
                        "database.ssl_root_cert not found: {}",
                        path.display()
                    )));
                }
                builder
                    .set_ca_file(path)
                    .map_err(|err| StoreError::Other(format!("database.ssl_root_cert: {err}")))?;
            }
        }
    }

    let mut connector = MakeTlsConnector::new(builder.build());
    if matches!(mode, DatabaseSslMode::Prefer | DatabaseSslMode::Require) {
        connector.set_callback(|config, _domain| {
            config.set_verify(SslVerifyMode::NONE);
            Ok(())
        });
    }
    Ok(connector)
}

impl PostgresStore {
    /// Whether the source has had its silent baseline recorded yet.
    pub fn is_initialized(&self, source_id: &str) -> Result<bool, StoreError> {
        let mut client = self.conn()?;
        let row = client.query_opt(
            "SELECT initialized FROM source_state WHERE source_id = $1",
            &[&source_id],
        )?;
        Ok(row.map(|row| row.get(0)).unwrap_or(false))
    }

    /// Mark a source as baselined; subsequent unseen identities will notify.
    pub fn mark_initialized(&self, source_id: &str) -> Result<(), StoreError> {
        let mut client = self.conn()?;
        client.execute(
            "INSERT INTO source_state (source_id, initialized, last_polled_at)
                 VALUES ($1, TRUE, now())
             ON CONFLICT (source_id) DO UPDATE SET initialized = TRUE",
            &[&source_id],
        )?;
        Ok(())
    }

    /// Record the timestamp of the most recent poll attempt.
    pub fn touch_polled(&self, source_id: &str) -> Result<(), StoreError> {
        let mut client = self.conn()?;
        client.execute(
            "INSERT INTO source_state (source_id, initialized, last_polled_at)
                 VALUES ($1, FALSE, now())
             ON CONFLICT (source_id) DO UPDATE SET last_polled_at = EXCLUDED.last_polled_at",
            &[&source_id],
        )?;
        Ok(())
    }

    /// Persist the newest filtered upstream tag for dashboard display.
    pub fn set_latest_release_tag(&self, source_id: &str, tag: &str) -> Result<(), StoreError> {
        let mut client = self.conn()?;
        client.execute(
            "INSERT INTO source_state (source_id, initialized, last_polled_at, latest_release_tag)
                 VALUES ($1, FALSE, now(), $2)
             ON CONFLICT (source_id) DO UPDATE SET latest_release_tag = EXCLUDED.latest_release_tag",
            &[&source_id, &tag],
        )?;
        Ok(())
    }

    /// Runtime fields from `source_state` for one source.
    pub fn source_state_row(&self, source_id: &str) -> Result<SourceStateRow, StoreError> {
        let mut client = self.conn()?;
        let row = client.query_opt(
            "SELECT initialized, last_polled_at, latest_release_tag FROM source_state WHERE source_id = $1",
            &[&source_id],
        )?;
        Ok(row
            .map(|row| SourceStateRow {
                initialized: row.get(0),
                last_polled_at: row
                    .get::<_, Option<DateTime<Utc>>>(1)
                    .map(|at| at.to_rfc3339()),
                latest_release_tag: row.get(2),
            })
            .unwrap_or_default())
    }

    /// All `source_state` rows keyed by `source_id` (for UI source list).
    pub fn all_source_states(&self) -> Result<HashMap<String, SourceStateRow>, StoreError> {
        let mut client = self.conn()?;
        let rows = client.query(
            "SELECT source_id, initialized, last_polled_at, latest_release_tag FROM source_state",
            &[],
        )?;
        Ok(rows
            .into_iter()
            .map(|row| {
                (
                    row.get::<_, String>(0),
                    SourceStateRow {
                        initialized: row.get(1),
                        last_polled_at: row
                            .get::<_, Option<DateTime<Utc>>>(2)
                            .map(|at| at.to_rfc3339()),
                        latest_release_tag: row.get(3),
                    },
                )
            })
            .collect())
    }

    /// Count seen identities per source (for UI dashboards).
    pub fn seen_counts_by_source(&self) -> Result<HashMap<String, u64>, StoreError> {
        let mut client = self.conn()?;
        let rows = client.query(
            "SELECT source_id, COUNT(*)::BIGINT FROM seen_release GROUP BY source_id",
            &[],
        )?;
        Ok(rows
            .into_iter()
            .map(|row| (row.get(0), row.get::<_, i64>(1) as u64))
            .collect())
    }

    /// Count seen identities for one source.
    pub fn seen_count(&self, source_id: &str) -> Result<u64, StoreError> {
        let mut client = self.conn()?;
        let row = client.query_one(
            "SELECT COUNT(*)::BIGINT FROM seen_release WHERE source_id = $1",
            &[&source_id],
        )?;
        Ok(row.get::<_, i64>(0) as u64)
    }

    /// Best-effort newest tag from the seen-release catalogue (UI backfill).
    /// The single "latest release" row for **every** source, in one query.
    ///
    /// What the sources-list page needs per row is one entry — the release whose
    /// tag matches `source_state.latest_release_tag`, so the tag and its date
    /// describe the same release. Fetching that with
    /// [`Self::list_seen_releases`] cost one unbounded scan *per source* (that
    /// query has no `LIMIT`; the cap is applied in Rust after a semver sort), so
    /// a hundred watched packages meant a hundred round trips, tens of thousands
    /// of `Release` values parsed, and all of it discarded except one row each —
    /// on an endpoint the dashboard polls on a timer.
    ///
    /// `DISTINCT ON` picks the first row per source under the `ORDER BY`:
    /// the tag-matching row when there is one, otherwise the most recently
    /// published, otherwise the most recently discovered. The tag comparison
    /// uses `COALESCE(display_tag, identity)` because that is exactly what
    /// [`crate::store::SeenReleaseEntry::tag`] resolves to, and what
    /// `set_latest_release_tag` stored.
    pub fn latest_seen_by_source(&self) -> Result<HashMap<String, SeenReleaseEntry>, StoreError> {
        let mut client = self.conn()?;
        let rows = client.query(
            "SELECT DISTINCT ON (s.source_id)
                    s.source_id, s.identity, s.display_tag,
                    s.first_seen_at, s.published_at, s.url
             FROM seen_release s
             LEFT JOIN source_state st ON st.source_id = s.source_id
             ORDER BY s.source_id,
                      (st.latest_release_tag IS NOT NULL
                       AND COALESCE(s.display_tag, s.identity) = st.latest_release_tag) DESC,
                      s.published_at DESC NULLS LAST,
                      s.first_seen_at DESC",
            &[],
        )?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let source_id: String = row.get(0);
                let identity: String = row.get(1);
                let display_tag: Option<String> = row.get(2);
                let first_seen_at: DateTime<Utc> = row.get(3);
                let published_at: Option<DateTime<Utc>> = row.get(4);
                let entry = SeenReleaseEntry {
                    tag: display_tag.unwrap_or_else(|| identity.clone()),
                    identity,
                    published_at: published_at.map(|at| at.to_rfc3339()),
                    url: row.get(5),
                    first_seen_at: first_seen_at.to_rfc3339(),
                };
                (source_id, entry)
            })
            .collect())
    }

    pub fn best_seen_identity(&self, source_id: &str) -> Result<Option<String>, StoreError> {
        let mut client = self.conn()?;
        let rows = client.query(
            "SELECT identity FROM seen_release WHERE source_id = $1",
            &[&source_id],
        )?;
        if rows.is_empty() {
            return Ok(None);
        }
        let releases: Vec<crate::model::Release> = rows
            .into_iter()
            .map(|row| crate::model::Release::new(row.get::<_, String>(0)))
            .collect();
        Ok(crate::model::pick_latest(releases.iter()).map(|release| release.raw_tag.clone()))
    }

    /// List synced release identities for one source (newest semver first).
    pub fn list_seen_releases(
        &self,
        source_id: &str,
        limit: usize,
    ) -> Result<Vec<SeenReleaseEntry>, StoreError> {
        let mut client = self.conn()?;
        let rows: Vec<SeenReleaseListRow> = client
            .query(
                "SELECT identity, display_tag, first_seen_at, published_at, url FROM seen_release WHERE source_id = $1",
                &[&source_id],
            )?
            .into_iter()
            .map(|row| {
                (
                    row.get(0),
                    row.get(1),
                    row.get(2),
                    row.get(3),
                    row.get(4),
                )
            })
            .collect();

        let mut releases: Vec<crate::model::Release> = rows
            .iter()
            .map(|(id, display_tag, _, published_at, _)| {
                let label = display_tag.as_deref().unwrap_or(id.as_str());
                crate::model::Release::new(id)
                    .display(label)
                    .with_published(*published_at)
            })
            .collect();
        crate::model::sort_releases_newest_first(&mut releases);

        let meta: HashMap<String, SeenReleaseMeta> = rows
            .into_iter()
            .map(|(id, display_tag, first_seen_at, published_at, url)| {
                (id, (display_tag, first_seen_at, published_at, url))
            })
            .collect();

        Ok(releases
            .into_iter()
            .take(limit)
            .map(|release| {
                let (display_tag, first_seen_at, published_at, url) = meta
                    .get(&release.id)
                    .map(|(tag, seen, published, link)| {
                        (tag.clone(), Some(*seen), *published, link.clone())
                    })
                    .unwrap_or_default();
                SeenReleaseEntry {
                    identity: release.id.clone(),
                    tag: display_tag.unwrap_or_else(|| release.raw_tag.clone()),
                    published_at: published_at.map(|at| at.to_rfc3339()),
                    url,
                    first_seen_at: first_seen_at.map(|at| at.to_rfc3339()).unwrap_or_default(),
                }
            })
            .collect())
    }

    /// Return the stored ETag for conditional upstream requests.
    pub fn get_etag(&self, source_id: &str) -> Result<Option<String>, StoreError> {
        let mut client = self.conn()?;
        let row = client.query_opt(
            "SELECT etag FROM source_state WHERE source_id = $1",
            &[&source_id],
        )?;
        Ok(row.and_then(|row| row.get(0)))
    }

    /// Persist the latest upstream ETag (or clear it when `None`).
    pub fn set_etag(&self, source_id: &str, etag: Option<&str>) -> Result<(), StoreError> {
        let mut client = self.conn()?;
        client.execute(
            "INSERT INTO source_state (source_id, initialized, last_polled_at, etag)
                 VALUES ($1, FALSE, now(), $2)
             ON CONFLICT (source_id) DO UPDATE SET etag = EXCLUDED.etag",
            &[&source_id, &etag],
        )?;
        Ok(())
    }

    /// Whether any seen row for this source lacks `published_at`.
    pub fn has_seen_missing_published_at(&self, source_id: &str) -> Result<bool, StoreError> {
        let mut client = self.conn()?;
        let row = client.query_one(
            "SELECT EXISTS(
                SELECT 1 FROM seen_release
                 WHERE source_id = $1 AND published_at IS NULL
             )",
            &[&source_id],
        )?;
        Ok(row.get(0))
    }

    /// Backfill upstream metadata on already-seen identities.
    pub fn enrich_seen_metadata(
        &self,
        source_id: &str,
        releases: &[&crate::model::Release],
    ) -> Result<(), StoreError> {
        if releases.is_empty() {
            return Ok(());
        }
        // One statement for the whole set (`unnest` of parallel arrays), the
        // same shape `ensure_sink_deliveries` uses. This runs on *every*
        // non-304 poll and covers every release the source lists — not just new
        // ones — so the per-release loop it replaces cost one round trip per
        // known version, every poll: hundreds for a mature npm or PyPI package.
        let mut identities: Vec<&str> = Vec::with_capacity(releases.len());
        let mut published: Vec<Option<DateTime<Utc>>> = Vec::with_capacity(releases.len());
        let mut urls: Vec<Option<&str>> = Vec::with_capacity(releases.len());
        let mut display_tags: Vec<Option<&str>> = Vec::with_capacity(releases.len());
        for release in releases {
            let display_tag = (release.raw_tag != release.id).then_some(release.raw_tag.as_str());
            // Nothing to merge in — skipped before the round trip, as before.
            if release.published_at.is_none() && release.url.is_none() && display_tag.is_none() {
                continue;
            }
            identities.push(release.id.as_str());
            published.push(release.published_at);
            urls.push(release.url.as_deref());
            display_tags.push(display_tag);
        }
        if identities.is_empty() {
            return Ok(());
        }

        let mut client = self.conn()?;
        client.execute(
            "UPDATE seen_release s SET
                published_at = COALESCE(s.published_at, t.published_at),
                url = COALESCE(s.url, t.url),
                display_tag = COALESCE(s.display_tag, t.display_tag)
             FROM unnest($2::text[], $3::timestamptz[], $4::text[], $5::text[])
                  AS t(identity, published_at, url, display_tag)
             WHERE s.source_id = $1 AND s.identity = t.identity",
            &[&source_id, &identities, &published, &urls, &display_tags],
        )?;
        Ok(())
    }

    /// Mark a single release identity as seen (idempotent).
    pub fn record_seen(&self, source_id: &str, item: &SeenUpsert<'_>) -> Result<(), StoreError> {
        let mut client = self.conn()?;
        Self::write_seen_row(&mut *client, source_id, item)?;
        Ok(())
    }

    /// Upsert one `seen_release` row. Generic over [`postgres::GenericClient`] so
    /// the same statement serves both pooled connections and transactions.
    pub(super) fn write_seen_row<C: postgres::GenericClient>(
        conn: &mut C,
        source_id: &str,
        item: &SeenUpsert<'_>,
    ) -> Result<(), StoreError> {
        conn.execute(
            "INSERT INTO seen_release (source_id, identity, display_tag, first_seen_at, content_digest, published_at, url)
                 VALUES ($1, $2, $3, now(), $4, $5, $6)
             ON CONFLICT (source_id, identity) DO UPDATE SET
                 display_tag = COALESCE(EXCLUDED.display_tag, seen_release.display_tag),
                 content_digest = EXCLUDED.content_digest,
                 published_at = COALESCE(seen_release.published_at, EXCLUDED.published_at),
                 url = COALESCE(seen_release.url, EXCLUDED.url)",
            &[
                &source_id,
                &item.identity,
                &item.display_tag,
                &item.content_digest,
                &item.published_at,
                &item.url,
            ],
        )?;
        Ok(())
    }

    /// Stored content fingerprint for a seen identity, if any.
    pub fn content_digest(
        &self,
        source_id: &str,
        identity: &str,
    ) -> Result<Option<String>, StoreError> {
        let mut client = self.conn()?;
        let row = client.query_opt(
            "SELECT content_digest FROM seen_release WHERE source_id = $1 AND identity = $2",
            &[&source_id, &identity],
        )?;
        Ok(row.and_then(|row| row.get(0)))
    }

    /// Whether this identity has ever been recorded for the source.
    pub fn has_seen(&self, source_id: &str, identity: &str) -> Result<bool, StoreError> {
        let mut client = self.conn()?;
        let row = client.query_one(
            "SELECT EXISTS(SELECT 1 FROM seen_release WHERE source_id = $1 AND identity = $2)",
            &[&source_id, &identity],
        )?;
        Ok(row.get(0))
    }

    /// Of the given identities, return those not yet recorded for this source.
    ///
    /// One round-trip: fetch the recorded subset via `= ANY($2)` and diff in
    /// memory, preserving input order.
    pub fn unseen<'a>(
        &self,
        source_id: &str,
        identities: &[&'a str],
    ) -> Result<Vec<&'a str>, StoreError> {
        if identities.is_empty() {
            return Ok(Vec::new());
        }
        let mut client = self.conn()?;
        let lookup: Vec<&str> = identities.to_vec();
        let rows = client.query(
            "SELECT identity FROM seen_release WHERE source_id = $1 AND identity = ANY($2)",
            &[&source_id, &lookup],
        )?;
        let seen: std::collections::HashSet<String> = rows
            .into_iter()
            .map(|row| row.get::<_, String>(0))
            .collect();
        Ok(identities
            .iter()
            .copied()
            .filter(|identity| !seen.contains(*identity))
            .collect())
    }

    /// All seen identities for one source — single query for poll-time diffing.
    pub fn load_seen_index(
        &self,
        source_id: &str,
    ) -> Result<HashMap<String, Option<String>>, StoreError> {
        let mut client = self.conn()?;
        let rows = client.query(
            "SELECT identity, content_digest FROM seen_release WHERE source_id = $1",
            &[&source_id],
        )?;
        Ok(rows
            .into_iter()
            .map(|row| (row.get(0), row.get(1)))
            .collect())
    }

    /// Record many identities in one transaction (silent baseline on first poll).
    pub fn record_seen_batch(
        &self,
        source_id: &str,
        items: &[SeenUpsert<'_>],
    ) -> Result<(), StoreError> {
        if items.is_empty() {
            return self.mark_initialized(source_id);
        }
        // Batched for the same reason as `enrich_seen_metadata`: this is the
        // silent baseline on a source's first poll, so `items` is the package's
        // *entire* published history — one round trip per version turned adding
        // a mature dependency into hundreds of statements.
        let identities: Vec<&str> = items.iter().map(|item| item.identity).collect();
        let display_tags: Vec<Option<&str>> = items.iter().map(|item| item.display_tag).collect();
        let digests: Vec<Option<&str>> = items.iter().map(|item| item.content_digest).collect();
        let published: Vec<Option<DateTime<Utc>>> =
            items.iter().map(|item| item.published_at).collect();
        let urls: Vec<Option<&str>> = items.iter().map(|item| item.url).collect();

        let mut client = self.conn()?;
        let mut tx = client.transaction()?;
        // `DISTINCT ON` is load-bearing, not tidiness: `ON CONFLICT DO UPDATE`
        // errors outright if one statement touches the same row twice, and an
        // upstream listing a version more than once would otherwise fail the
        // whole baseline. The per-item loop this replaces tolerated that
        // silently.
        tx.execute(
            "INSERT INTO seen_release
                 (source_id, identity, display_tag, first_seen_at, content_digest, published_at, url)
             SELECT DISTINCT ON (t.identity)
                    $1, t.identity, t.display_tag, now(), t.content_digest, t.published_at, t.url
             FROM unnest($2::text[], $3::text[], $4::text[], $5::timestamptz[], $6::text[])
                  AS t(identity, display_tag, content_digest, published_at, url)
             ORDER BY t.identity
             ON CONFLICT (source_id, identity) DO UPDATE SET
                 display_tag = COALESCE(EXCLUDED.display_tag, seen_release.display_tag),
                 content_digest = EXCLUDED.content_digest,
                 published_at = COALESCE(seen_release.published_at, EXCLUDED.published_at),
                 url = COALESCE(seen_release.url, EXCLUDED.url)",
            &[
                &source_id,
                &identities,
                &display_tags,
                &digests,
                &published,
                &urls,
            ],
        )?;
        tx.execute(
            "INSERT INTO source_state (source_id, initialized, last_polled_at)
                 VALUES ($1, TRUE, now())
             ON CONFLICT (source_id) DO UPDATE SET initialized = TRUE",
            &[&source_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Cheap liveness check used by the `health` subcommand / k8s probe.
    pub fn health(&self) -> Result<(), StoreError> {
        let mut client = self.conn()?;
        client.query_one("SELECT 1", &[])?;
        Ok(())
    }

    /// Claim a webhook delivery id for idempotency.
    pub fn claim_webhook_delivery(&self, delivery_id: &str) -> Result<bool, StoreError> {
        let mut client = self.conn()?;
        let row = client.query_opt(
            "INSERT INTO webhook_delivery (delivery_id, received_at)
                 VALUES ($1, now())
             ON CONFLICT (delivery_id) DO NOTHING
             RETURNING delivery_id",
            &[&delivery_id],
        )?;
        Ok(row.is_some())
    }

    /// Delete aged rows to bound database growth on long-running deployments.
    pub fn prune(
        &self,
        seen_after_days: u32,
        webhooks_after_days: u32,
        outbox_sent_after_days: u32,
        advisories_after_days: u32,
    ) -> Result<PruneReport, StoreError> {
        let mut report = PruneReport::default();
        let mut client = self.conn()?;
        // Retention cutoffs use the database clock (`now()`), consistent with
        // delivery leases — one clock authority for every worker.
        if seen_after_days > 0 {
            let days = i32::try_from(seen_after_days).unwrap_or(i32::MAX);
            report.seen_deleted = client.execute(
                "DELETE FROM seen_release
                 WHERE first_seen_at < now() - make_interval(days => $1::int)",
                &[&days],
            )? as usize;
        }
        if webhooks_after_days > 0 {
            let days = i32::try_from(webhooks_after_days).unwrap_or(i32::MAX);
            report.webhooks_deleted = client.execute(
                "DELETE FROM webhook_delivery
                 WHERE received_at < now() - make_interval(days => $1::int)",
                &[&days],
            )? as usize;
        }
        if outbox_sent_after_days > 0 {
            let days = i32::try_from(outbox_sent_after_days).unwrap_or(i32::MAX);
            report.outbox_deleted = client.execute(
                "DELETE FROM notification_outbox
                 WHERE status = 'sent' AND sent_at < now() - make_interval(days => $1::int)",
                &[&days],
            )? as usize;
            // `NOT EXISTS` (anti-join), not `NOT IN`: the latter re-scans the
            // parent id set and, on a large outbox, plans far worse.
            let _ = client.execute(
                "DELETE FROM notification_sink_delivery d
                 WHERE NOT EXISTS (
                     SELECT 1 FROM notification_outbox o WHERE o.id = d.outbox_id
                 )",
                &[],
            )?;
        }
        if advisories_after_days > 0 {
            let days = i32::try_from(advisories_after_days).unwrap_or(i32::MAX);
            report.advisories_deleted = client.execute(
                "DELETE FROM release_advisory
                 WHERE fetched_at < now() - make_interval(days => $1::int)",
                &[&days],
            )? as usize;
            // Same cutoff as the findings above: letting a check-state row
            // expire simply means that version gets re-verified next time it
            // is in the backfill's path — a feature, not data loss, since OSV
            // can publish a new advisory against an old version later.
            report.advisory_checks_deleted = client.execute(
                "DELETE FROM advisory_check
                 WHERE checked_at < now() - make_interval(days => $1::int)",
                &[&days],
            )? as usize;
        }
        Ok(report)
    }

    /// Clear all tables — test helper.
    pub(crate) fn truncate_all(&self) -> Result<(), StoreError> {
        let mut client = self.conn()?;
        client.batch_execute(
            "TRUNCATE notification_sink_delivery, notification_outbox, seen_release, \
             source_state, webhook_delivery, config_revision, app_secret, app_user, \
             release_advisory, advisory_check \
             RESTART IDENTITY CASCADE",
        )?;
        // Keep the process vault aligned with the empty `app_secret` table.
        crate::config::vault_replace_all(std::collections::HashMap::new());
        Ok(())
    }
}
