//! Versioned schema upgrades on top of the greenfield DDL.
//!
//! Boot path:
//! 1. Apply [`super::SCHEMA`] (`CREATE … IF NOT EXISTS`) so empty databases
//!    reach the baseline shape.
//! 2. Under a session advisory lock, read/stamp [`schema_meta`] and run any
//!    numbered steps beyond the stored version up to [`CURRENT_SCHEMA_VERSION`].
//!
//! Existing pre-versioned databases (tables present, no `schema_meta` row) are
//! stamped at version 1 without rewriting rows — they already match the
//! greenfield baseline.

use postgres::GenericClient;

use crate::error::StoreError;

/// Schema version embedded in this binary. Bump when adding a step to
/// [`MIGRATIONS`].
pub const CURRENT_SCHEMA_VERSION: i32 = 2;

/// Advisory-lock class for schema migrate (distinct from the poller lease).
const MIGRATE_LOCK_CLASS: i32 = 0x7852_656c; // "xRel"
const MIGRATE_LOCK_ID: i32 = 0x7363_686d; // "schm"

/// Ordered upgrades from version `n` → `n+1`. Index `0` is unused; step `k`
/// lives at `MIGRATIONS[k]` and brings the DB to version `k`.
///
/// Version 1 is the greenfield baseline (`postgres_schema.sql`) — no SQL here.
/// Future ALTER/CREATE steps start at version 2.
const MIGRATIONS: &[&str] = &[
    "", // 0 — unused
    "", // 1 — greenfield stamp only
    // 2 — drop placeholder publish dates already persisted before
    // `sources::time::parse_rfc3339` learned to reject them (NuGet stamps
    // unlisted versions `1900-01-01`, .NET APIs leak `0001-01-01`, zeroed
    // structs serialise as the epoch). The `seen_release` upsert deliberately
    // never overwrites a non-null `published_at`, so without this step the
    // sentinels would survive the parser fix forever and keep rendering as
    // "Jan 1, 1900" release dates. NULL is correct: these versions have no
    // known publish date; a later poll may fill in a real one.
    "UPDATE seen_release SET published_at = NULL
      WHERE published_at < TIMESTAMPTZ '1980-01-01 00:00:00+00';",
];

/// Apply greenfield DDL, then run any pending versioned upgrades.
pub(super) fn apply_schema(client: &mut impl GenericClient) -> Result<(), StoreError> {
    client.batch_execute(super::SCHEMA)?;
    migrate(client)
}

fn migrate(client: &mut impl GenericClient) -> Result<(), StoreError> {
    // Serialize migrate across concurrent Store::open (health probes, CLI, serve).
    let locked: bool = client
        .query_one(
            "SELECT pg_try_advisory_lock($1, $2)",
            &[&MIGRATE_LOCK_CLASS, &MIGRATE_LOCK_ID],
        )?
        .get(0);
    if !locked {
        // Another opener is migrating; wait for the session lock.
        client.execute(
            "SELECT pg_advisory_lock($1, $2)",
            &[&MIGRATE_LOCK_CLASS, &MIGRATE_LOCK_ID],
        )?;
    }

    let result = migrate_locked(client);

    let _ = client.execute(
        "SELECT pg_advisory_unlock($1, $2)",
        &[&MIGRATE_LOCK_CLASS, &MIGRATE_LOCK_ID],
    );
    result
}

fn migrate_locked(client: &mut impl GenericClient) -> Result<(), StoreError> {
    let Some(mut current) = read_version(client)? else {
        // Fresh or pre-versioned DB: baseline DDL already applied.
        stamp_version(client, CURRENT_SCHEMA_VERSION)?;
        tracing::info!(
            version = CURRENT_SCHEMA_VERSION,
            "postgresql schema baseline stamped"
        );
        return Ok(());
    };

    if current > CURRENT_SCHEMA_VERSION {
        return Err(StoreError::Other(format!(
            "database schema version {current} is newer than this binary \
             (supports up to {CURRENT_SCHEMA_VERSION}) — upgrade xrelease"
        )));
    }

    while current < CURRENT_SCHEMA_VERSION {
        let next = current + 1;
        let sql = MIGRATIONS.get(next as usize).copied().ok_or_else(|| {
            StoreError::Other(format!("missing migration step for version {next}"))
        })?;
        if !sql.trim().is_empty() {
            client.batch_execute(sql).map_err(|err| {
                StoreError::Other(format!("schema migration to v{next} failed: {err}"))
            })?;
        }
        stamp_version(client, next)?;
        tracing::info!(
            from = current,
            to = next,
            "applied postgresql schema migration"
        );
        current = next;
    }
    Ok(())
}

fn read_version(client: &mut impl GenericClient) -> Result<Option<i32>, StoreError> {
    let row = client.query_opt(
        "SELECT version FROM schema_meta WHERE singleton = TRUE",
        &[],
    )?;
    Ok(row.map(|row| row.get(0)))
}

fn stamp_version(client: &mut impl GenericClient, version: i32) -> Result<(), StoreError> {
    client.execute(
        "INSERT INTO schema_meta (singleton, version, applied_at)
             VALUES (TRUE, $1, now())
         ON CONFLICT (singleton) DO UPDATE
             SET version = EXCLUDED.version, applied_at = EXCLUDED.applied_at",
        &[&version],
    )?;
    Ok(())
}

impl super::PostgresStore {
    /// Applied schema version from `schema_meta` (after [`apply_schema`]).
    pub(crate) fn schema_version(&self) -> Result<i32, StoreError> {
        let mut client = self.conn()?;
        read_version(&mut *client)?
            .ok_or_else(|| StoreError::Other("schema_meta row missing after migrate".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_schema_version_should_match_migrations_len_minus_one() {
        // MIGRATIONS[0] unused; highest defined index == CURRENT_SCHEMA_VERSION.
        assert_eq!(CURRENT_SCHEMA_VERSION as usize, MIGRATIONS.len() - 1);
    }

    #[test]
    fn baseline_migration_slot_should_be_empty() {
        assert!(MIGRATIONS[1].trim().is_empty());
    }

    #[test]
    fn upgrade_migration_slots_should_carry_sql() {
        // Steps past the baseline must do real work; an empty slot would stamp
        // a version bump that changed nothing.
        for (version, sql) in MIGRATIONS.iter().enumerate().skip(2) {
            assert!(!sql.trim().is_empty(), "migration step {version} is empty");
        }
    }
}
