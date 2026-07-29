//! Persisted advisory cache for released package versions (`release_advisory`,
//! `advisory_check`).
//!
//! A read-side cache, not a delivery-critical record: [`crate::advisory`]'s
//! never-block-delivery guarantee covers the OSV lookup itself, and these
//! tables exist only so the API can show what was found — and what has ever
//! been asked — without re-querying OSV on every page load. A write failure
//! here must never propagate past the caller in [`crate::pipeline::outbox`],
//! which already treats it as best-effort.

use std::collections::{HashMap, HashSet};

use super::PostgresStore;
use crate::advisory::{Advisory, Severity};
use crate::store::StoreError;

impl PostgresStore {
    /// Replace the advisory set for one `(ecosystem, package, version)`.
    ///
    /// Delete-then-insert inside one transaction, not a bare upsert: OSV can
    /// legitimately return *fewer* advisories on a later lookup (a false
    /// positive withdrawn, a fixed-in-version correction), and upserting alone
    /// would leave those stale rows behind forever. Row counts per release are
    /// small (rarely more than a handful), so a loop of parameterized
    /// statements inside the transaction is simpler than an `unnest`-batched
    /// insert for no measurable cost.
    ///
    /// The insert still carries `ON CONFLICT DO UPDATE`. Two writers can race
    /// for one `(ecosystem, package, version)` — an outbox delivery and a
    /// source-detail backfill, or two workers — and under `READ COMMITTED` both
    /// delete, then both insert: the loser hits a primary-key violation and
    /// aborts the whole transaction. Nothing breaks (every caller treats this
    /// as best-effort), but the cache write is lost and the log gets an error
    /// that means nothing was wrong. Making the insert idempotent lets the
    /// later writer simply refresh the row.
    pub fn record_advisories(
        &self,
        ecosystem: &str,
        package: &str,
        version: &str,
        advisories: &[Advisory],
    ) -> Result<(), StoreError> {
        let mut client = self.conn()?;
        let mut tx = client.transaction()?;
        tx.execute(
            "DELETE FROM release_advisory WHERE ecosystem = $1 AND package = $2 AND version = $3",
            &[&ecosystem, &package, &version],
        )?;
        for advisory in advisories {
            tx.execute(
                "INSERT INTO release_advisory
                     (ecosystem, package, version, advisory_id, display_id, summary,
                      severity, cvss_vector, url, fetched_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, now())
                 ON CONFLICT (ecosystem, package, version, advisory_id) DO UPDATE SET
                     display_id  = EXCLUDED.display_id,
                     summary     = EXCLUDED.summary,
                     severity    = EXCLUDED.severity,
                     cvss_vector = EXCLUDED.cvss_vector,
                     url         = EXCLUDED.url,
                     fetched_at  = EXCLUDED.fetched_at",
                &[
                    &ecosystem,
                    &package,
                    &version,
                    &advisory.id,
                    &advisory.display_id,
                    &advisory.summary,
                    &advisory.severity.map(Severity::as_str),
                    &advisory.cvss_vector,
                    &advisory.url,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Persisted advisories for several versions of one package, in a single
    /// round trip — the source-detail page needs this for every seen release
    /// at once, not one query per row.
    ///
    /// Versions with no cached advisories are simply absent from the returned
    /// map (not present with an empty `Vec`), so callers should default a
    /// missing key to empty rather than treat it as an error.
    pub fn advisories_for_versions(
        &self,
        ecosystem: &str,
        package: &str,
        versions: &[&str],
    ) -> Result<HashMap<String, Vec<Advisory>>, StoreError> {
        let mut out: HashMap<String, Vec<Advisory>> = HashMap::new();
        if versions.is_empty() {
            return Ok(out);
        }
        let mut client = self.conn()?;
        let rows = client.query(
            "SELECT version, advisory_id, display_id, summary, severity, cvss_vector, url
             FROM release_advisory
             WHERE ecosystem = $1 AND package = $2 AND version = ANY($3)
             ORDER BY version, advisory_id",
            &[&ecosystem, &package, &versions],
        )?;
        for row in rows {
            let version: String = row.get(0);
            let severity: Option<String> = row.get(4);
            let advisory = Advisory {
                id: row.get(1),
                display_id: row.get(2),
                summary: row.get(3),
                severity: severity.as_deref().and_then(Severity::parse),
                cvss_vector: row.get(5),
                url: row.get(6),
            };
            out.entry(version).or_default().push(advisory);
        }
        Ok(out)
    }

    /// Record that OSV was asked about `(ecosystem, package, version)` — the
    /// answer may or may not have found anything.
    ///
    /// Deliberately separate from [`Self::record_advisories`]: that table's
    /// contract is "the current findings for this version" and legitimately
    /// holds zero rows for a clean version, so it cannot double as "has this
    /// version ever been checked" — a never-checked version and a
    /// checked-and-clean one would be indistinguishable, and the source-detail
    /// backfill (which only has a few lookups' budget per page load) would
    /// keep re-confirming the same clean releases instead of ever reaching
    /// ones it has not looked at yet. Callers must call this only for a
    /// *verified* answer (a real OSV response, fresh or cached) — see
    /// [`crate::advisory::LookupOutcome::verified`] — never for a skip or a
    /// failed request, which would wrongly mark an unknown version "clean".
    pub fn record_advisory_check(
        &self,
        ecosystem: &str,
        package: &str,
        version: &str,
    ) -> Result<(), StoreError> {
        let mut client = self.conn()?;
        client.execute(
            "INSERT INTO advisory_check (ecosystem, package, version, checked_at)
             VALUES ($1, $2, $3, now())
             ON CONFLICT (ecosystem, package, version) DO UPDATE SET
                 checked_at = EXCLUDED.checked_at",
            &[&ecosystem, &package, &version],
        )?;
        Ok(())
    }

    /// Seen versions of one source that have never had a verified advisory
    /// check, newest-discovered first.
    ///
    /// The anti-join runs in the database rather than fetching every seen
    /// release and filtering in Rust: the background sweep asks this for every
    /// watched package source on every round, and all it wants is the next few
    /// items of work. Once a source has converged this returns zero rows
    /// without transferring anything.
    ///
    /// Ordered by `first_seen_at DESC` — newest discovery first — because
    /// `identity` is a version string that SQL cannot order meaningfully
    /// (`1.10.0` sorts before `1.9.0` lexicographically), and recency is the
    /// property worth prioritising anyway.
    ///
    /// `identity` breaks ties, and that matters more than it looks: a baseline
    /// poll records a source's entire back catalogue in **one** batch, so every
    /// row shares a `first_seen_at` to the microsecond — and those rows are
    /// exactly what this sweep exists to work through. Without a tiebreaker the
    /// order within that block is whatever the plan happens to produce, which
    /// makes each round's slice unpredictable and the behaviour untestable.
    pub fn unchecked_seen_versions(
        &self,
        source_id: &str,
        ecosystem: &str,
        package: &str,
        limit: usize,
    ) -> Result<Vec<String>, StoreError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut client = self.conn()?;
        let rows = client.query(
            "SELECT s.identity
             FROM seen_release s
             WHERE s.source_id = $1
               AND NOT EXISTS (
                   SELECT 1 FROM advisory_check c
                   WHERE c.ecosystem = $2 AND c.package = $3 AND c.version = s.identity
               )
             ORDER BY s.first_seen_at DESC, s.identity DESC
             LIMIT $4",
            &[&source_id, &ecosystem, &package, &limit],
        )?;
        Ok(rows.into_iter().map(|row| row.get(0)).collect())
    }

    /// Which of `versions` already have a check-state row — regardless of
    /// whether that check found anything.
    ///
    /// Batched for the same reason as [`Self::advisories_for_versions`]: the
    /// source-detail backfill needs this for every seen release at once.
    pub fn checked_versions(
        &self,
        ecosystem: &str,
        package: &str,
        versions: &[&str],
    ) -> Result<HashSet<String>, StoreError> {
        if versions.is_empty() {
            return Ok(HashSet::new());
        }
        let mut client = self.conn()?;
        let rows = client.query(
            "SELECT version FROM advisory_check
             WHERE ecosystem = $1 AND package = $2 AND version = ANY($3)",
            &[&ecosystem, &package, &versions],
        )?;
        Ok(rows.into_iter().map(|row| row.get(0)).collect())
    }
}
