//! Notification outbox + per-sink delivery ledger.

use chrono::{DateTime, Utc};

use super::PostgresStore;
use crate::notify::Event;
use crate::store::{
    EnqueueOutcome, OutboxCounts, OutboxEntry, OutboxMeta, OutboxRecord, SeenUpsert, StoreError,
    OUTBOX_MAX_ATTEMPTS,
};

impl PostgresStore {
    /// Pending/failed outbox rows for observability UI (no message body).
    pub fn list_outbox_entries(&self, limit: usize) -> Result<Vec<OutboxEntry>, StoreError> {
        let mut client = self.conn()?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = client.query(
 "SELECT id, source_id, identity, status, attempts, last_error, created_at, title, url, routing_tag, deliver_after
 FROM notification_outbox
 WHERE status IN ('pending', 'failed', 'dead')
 ORDER BY created_at DESC
 LIMIT $1",
 &[&limit])?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let source_id: String = row.get(1);
                let organization_id =
                    crate::config::organization_id_from_source_id(&source_id).map(str::to_owned);
                OutboxEntry {
                    id: row.get(0),
                    source_id,
                    identity: row.get(2),
                    status: row.get(3),
                    attempts: row.get::<_, i32>(4) as u32,
                    last_error: row.get(5),
                    created_at: row.get::<_, DateTime<Utc>>(6).to_rfc3339(),
                    title: row.get(7),
                    url: row.get(8),
                    routing_tag: row.get(9),
                    deliver_after: row
                        .get::<_, Option<DateTime<Utc>>>(10)
                        .map(|at| at.to_rfc3339()),
                    organization_id,
                }
            })
            .collect())
    }
    /// Enqueue a notification for durable delivery.
    pub fn try_enqueue_notification(
        &self,
        event: &Event,
        identity: &str,
        meta: OutboxMeta<'_>,
    ) -> Result<Option<EnqueueOutcome>, StoreError> {
        let mut client = self.conn()?;
        // A row deferred by a notify schedule is enqueued but not delivered
        // inline — the background flush picks it up once `deliver_after`
        // passes (the claim queries gate on it).
        let deliver_now = meta.deliver_after.is_none_or(|at| at <= Utc::now());
        if let Some(row) = client.query_opt(
            "INSERT INTO notification_outbox (
 source_id, identity, content_digest, display_tag, published_at,
 title, body, url, routing_tag, source_kind, deliver_after, status, created_at
 ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'pending', now())
 ON CONFLICT (source_id, identity) DO NOTHING
 RETURNING id",
            &[
                &event.source_id,
                &identity,
                &meta.content_digest,
                &meta.display_tag,
                &meta.published_at,
                &event.title,
                &event.body,
                &event.url,
                &event.routing_tag,
                &event.source_kind,
                &meta.deliver_after,
            ],
        )? {
            let id: i64 = row.get(0);
            return Ok(Some(EnqueueOutcome {
                id,
                deliver_now,
                created: true,
            }));
        }

        let existing = client.query_opt(
            "SELECT id, status, content_digest FROM notification_outbox
 WHERE source_id = $1 AND identity = $2",
            &[&event.source_id, &identity],
        )?;

        let Some(row) = existing else {
            return Ok(None);
        };

        let id: i64 = row.get(0);
        let status: String = row.get(1);
        let stored_digest: Option<String> = row.get(2);

        match status.as_str() {
            "pending" | "failed" => Ok(Some(EnqueueOutcome {
                id,
                deliver_now: false,
                created: false,
            })),
            "sent" if stored_digest.as_deref() != meta.content_digest => {
                // One transaction: the parent reopen and the sink-row reset
                // must be atomic. Split, a crash — or a concurrent flush
                // claiming the reopened parent — between the two statements
                // sees zero pending sink rows, re-marks the parent `sent`
                // without delivering anything, and the late reset then strands
                // `pending` sink rows under a `sent` parent (unclaimable);
                // the content-update notification is silently lost.
                let mut tx = client.transaction()?;
                let updated = tx.execute(
                    "UPDATE notification_outbox
 SET status = 'pending',
 content_digest = $1,
 display_tag = $2,
 published_at = $3,
 title = $4,
 body = $5,
 url = $6,
 routing_tag = $7,
 source_kind = $8,
 deliver_after = $9,
 attempts = 0,
 last_error = NULL
 WHERE id = $10 AND status = 'sent'",
                    &[
                        &meta.content_digest,
                        &meta.display_tag,
                        &meta.published_at,
                        &event.title,
                        &event.body,
                        &event.url,
                        &event.routing_tag,
                        &event.source_kind,
                        &meta.deliver_after,
                        &id,
                    ],
                )?;
                if updated == 0 {
                    // Raced by another writer; dropping `tx` rolls back the no-op.
                    return Ok(None);
                }
                tx.execute(
                    "UPDATE notification_sink_delivery
 SET status = 'pending', attempts = 0, last_error = NULL, sent_at = NULL
 WHERE outbox_id = $1",
                    &[&id],
                )?;
                tx.commit()?;
                Ok(Some(EnqueueOutcome {
                    id,
                    deliver_now,
                    created: true,
                }))
            }
            _ => Ok(None),
        }
    }

    /// Atomically lease a batch of retryable outbox rows, oldest first.
    ///
    /// `FOR UPDATE SKIP LOCKED` plus the `locked_until` lease guarantee that two
    /// concurrent workers (background flush, inline poll, or separate replicas)
    /// never claim — and therefore never re-deliver — the same notification. The
    /// lease auto-expires after `lease_secs` so a worker that crashes mid-delivery
    /// does not strand the row.
    pub fn claim_outbox_batch(
        &self,
        limit: usize,
        lease_secs: u32,
    ) -> Result<Vec<OutboxRecord>, StoreError> {
        let mut client = self.conn()?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        // Lease arithmetic runs entirely on the database clock so every worker
        // shares one time authority — app-side clock skew cannot shorten or extend a lease.
        let lease_secs = f64::from(lease_secs);
        let rows = client.query(
            "UPDATE notification_outbox o
 SET locked_until = now() + make_interval(secs => $1::double precision)
 FROM (
 SELECT id FROM notification_outbox
 WHERE status IN ('pending', 'failed') AND attempts < $2
 AND (locked_until IS NULL OR locked_until < now())
 AND (deliver_after IS NULL OR deliver_after <= now())
 ORDER BY created_at ASC
 LIMIT $3
 FOR UPDATE SKIP LOCKED
 ) sub
 WHERE o.id = sub.id
 RETURNING o.id, o.source_id, o.identity, o.content_digest, o.display_tag,
 o.published_at, o.title, o.body, o.url, o.routing_tag,
 o.source_kind, o.attempts, o.deliver_after",
            &[&lease_secs, &OUTBOX_MAX_ATTEMPTS, &limit],
        )?;
        Ok(rows.into_iter().map(Self::outbox_record_from_row).collect())
    }

    /// Lease a single outbox row for inline delivery; returns whether the lease
    /// was acquired (false = another worker holds an unexpired lease).
    pub fn claim_outbox_row(&self, outbox_id: i64, lease_secs: u32) -> Result<bool, StoreError> {
        let mut client = self.conn()?;
        let lease_secs = f64::from(lease_secs);
        let row = client.query_opt(
            "UPDATE notification_outbox
 SET locked_until = now() + make_interval(secs => $1::double precision)
 WHERE id = $2
 AND status IN ('pending', 'failed')
 AND (locked_until IS NULL OR locked_until < now())
 AND (deliver_after IS NULL OR deliver_after <= now())
 RETURNING id",
            &[&lease_secs, &outbox_id],
        )?;
        Ok(row.is_some())
    }

    /// Release a delivery lease so a failed row can be retried promptly (instead
    /// of waiting for lease expiry). A no-op for rows already marked `sent`.
    pub fn release_outbox_lease(&self, outbox_id: i64) -> Result<(), StoreError> {
        let mut client = self.conn()?;
        client.execute(
            "UPDATE notification_outbox SET locked_until = NULL WHERE id = $1",
            &[&outbox_id],
        )?;
        Ok(())
    }

    /// Defer the next claim of this row by `backoff_secs` (exponential retry
    /// backoff after a failed delivery). Reuses the delivery lease column —
    /// `locked_until` already means "not claimable until this instant"; a
    /// failed delivery just sets it further out instead of clearing it, so
    /// claim queries need no changes.
    pub fn apply_delivery_backoff(
        &self,
        outbox_id: i64,
        backoff_secs: f64,
    ) -> Result<(), StoreError> {
        let mut client = self.conn()?;
        // Advance the parent `attempts` alongside the lease: it is the backoff
        // clock the next flush reads (`backoff_secs(attempts_before + 1)`).
        // A sink-delivery failure only touches the per-sink ledger, so without
        // this the parent stayed at 0 and every retry used the 60s floor — the
        // exponential backoff never escalated. On the sink path the
        // parent's dead-lettering is driven by `sync_outbox_sink_status` (not
        // by `attempts`), so this is purely the retry-spacing input. On the
        // rare routing-misconfig path `fail_notification` also bumps `attempts`
        // (and dead-letters at the cap); one extra increment there only makes a
        // permanently-misrouted row dead-letter a little sooner — harmless.
        client.execute(
            "UPDATE notification_outbox
 SET locked_until = now() + make_interval(secs => $1::double precision),
 attempts = attempts + 1
 WHERE id = $2",
            &[&backoff_secs, &outbox_id],
        )?;
        Ok(())
    }

    fn outbox_record_from_row(row: postgres::Row) -> OutboxRecord {
        OutboxRecord {
            id: row.get(0),
            source_id: row.get(1),
            identity: row.get(2),
            content_digest: row.get(3),
            display_tag: row.get(4),
            published_at: row.get(5),
            title: row.get(6),
            body: row.get(7),
            url: row.get(8),
            routing_tag: row.get(9),
            source_kind: row.get(10),
            attempts: row.get::<_, i32>(11) as u32,
            deliver_after: row.get(12),
        }
    }

    /// Mark delivery complete and record the release as seen in one transaction.
    pub fn complete_notification(
        &self,
        outbox_id: i64,
        source_id: &str,
        seen: &SeenUpsert<'_>,
    ) -> Result<bool, StoreError> {
        let mut client = self.conn()?;
        let mut tx = client.transaction()?;
        let updated = tx.execute(
            "UPDATE notification_outbox
 SET status = 'sent', sent_at = now(), last_error = NULL
 WHERE id = $1 AND status IN ('pending', 'failed')",
            &[&outbox_id],
        )?;
        if updated == 0 {
            tx.rollback()?;
            return Ok(false);
        }
        Self::write_seen_row(&mut tx, source_id, seen)?;
        tx.commit()?;
        Ok(true)
    }

    /// Record a failed delivery attempt; marks the row `dead` after max attempts.
    ///
    /// Returns `true` only on the call that first crosses the attempt cap
    /// (subsequent fails on an already-`dead` row return `false`).
    pub fn fail_notification(&self, outbox_id: i64, error: &str) -> Result<bool, StoreError> {
        let mut client = self.conn()?;
        let Some(row) = client.query_opt(
            "UPDATE notification_outbox
 SET attempts = attempts + 1,
 last_error = $1,
 status = CASE
 WHEN attempts + 1 >= $2 THEN 'dead'
 ELSE 'failed'
 END
 WHERE id = $3
 RETURNING status, attempts",
            &[&error, &OUTBOX_MAX_ATTEMPTS, &outbox_id],
        )?
        else {
            return Ok(false);
        };
        let status: String = row.get(0);
        let attempts: i32 = row.get(1);
        Ok(status == "dead" && attempts == OUTBOX_MAX_ATTEMPTS)
    }

    /// Create per-sink delivery rows for fan-out tracking (idempotent).
    ///
    /// `sinks` carries `(global_sink_index, kind)` pairs — the index is the
    /// position in the [`CompositeNotifier`] sink list, kept stable across
    /// retries so the ledger and `notify_partial` share one index space.
    pub fn ensure_sink_deliveries(
        &self,
        outbox_id: i64,
        sinks: &[(usize, &str)],
    ) -> Result<(), StoreError> {
        let mut client = self.conn()?;
        for (index, kind) in sinks {
            let sink_index = i32::try_from(*index).unwrap_or(i32::MAX);
            client.execute(
                "INSERT INTO notification_sink_delivery
 (outbox_id, sink_index, sink_kind, status)
 VALUES ($1, $2, $3, 'pending')
 ON CONFLICT (outbox_id, sink_index) DO NOTHING",
                &[&outbox_id, &sink_index, kind],
            )?;
        }
        Ok(())
    }

    /// Sink indices that still need delivery (`pending` / retryable `failed`).
    pub fn list_pending_sink_indices(&self, outbox_id: i64) -> Result<Vec<usize>, StoreError> {
        let mut client = self.conn()?;
        let rows = client.query(
            "SELECT sink_index FROM notification_sink_delivery
 WHERE outbox_id = $1 AND status IN ('pending', 'failed') AND attempts < $2
 ORDER BY sink_index ASC",
            &[&outbox_id, &OUTBOX_MAX_ATTEMPTS],
        )?;
        Ok(rows
            .into_iter()
            .map(|row| row.get::<_, i32>(0) as usize)
            .collect())
    }

    /// Mark one sink delivery as successful.
    pub fn complete_sink_delivery(
        &self,
        outbox_id: i64,
        sink_index: usize,
    ) -> Result<(), StoreError> {
        let mut client = self.conn()?;
        let sink_index = i32::try_from(sink_index).unwrap_or(i32::MAX);
        client.execute(
            "UPDATE notification_sink_delivery
 SET status = 'sent', sent_at = now(), last_error = NULL
 WHERE outbox_id = $1 AND sink_index = $2",
            &[&outbox_id, &sink_index],
        )?;
        Ok(())
    }

    /// Record a failed sink attempt; marks the sink `dead` after max attempts.
    pub fn fail_sink_delivery(
        &self,
        outbox_id: i64,
        sink_index: usize,
        error: &str,
    ) -> Result<(), StoreError> {
        let mut client = self.conn()?;
        let sink_index = i32::try_from(sink_index).unwrap_or(i32::MAX);
        client.execute(
            "UPDATE notification_sink_delivery
 SET attempts = attempts + 1,
 last_error = $1,
 status = CASE
 WHEN attempts + 1 >= $2 THEN 'dead'
 ELSE 'failed'
 END
 WHERE outbox_id = $3 AND sink_index = $4",
            &[&error, &OUTBOX_MAX_ATTEMPTS, &outbox_id, &sink_index],
        )?;
        Ok(())
    }

    /// Mark retryable sink rows `dead` (routing no longer matches).
    ///
    /// Empty `sink_indices` abandons every retryable row for the outbox id.
    pub fn abandon_retryable_sinks(
        &self,
        outbox_id: i64,
        sink_indices: &[usize],
        error: &str,
    ) -> Result<usize, StoreError> {
        let mut client = self.conn()?;
        let updated = if sink_indices.is_empty() {
            client.execute(
                "UPDATE notification_sink_delivery
 SET status = 'dead',
 last_error = $1,
 attempts = GREATEST(attempts, $2)
 WHERE outbox_id = $3 AND status IN ('pending', 'failed')",
                &[&error, &OUTBOX_MAX_ATTEMPTS, &outbox_id],
            )?
        } else {
            let indices: Vec<i32> = sink_indices
                .iter()
                .map(|&index| i32::try_from(index).unwrap_or(i32::MAX))
                .collect();
            client.execute(
                "UPDATE notification_sink_delivery
 SET status = 'dead',
 last_error = $1,
 attempts = GREATEST(attempts, $2)
 WHERE outbox_id = $3
 AND sink_index = ANY($4)
 AND status IN ('pending', 'failed')",
                &[&error, &OUTBOX_MAX_ATTEMPTS, &outbox_id, &indices],
            )?
        };
        Ok(usize::try_from(updated).unwrap_or(usize::MAX))
    }

    /// Whether every tracked sink row is `sent` (or no rows exist — legacy outbox).
    pub fn sinks_delivery_complete(&self, outbox_id: i64) -> Result<bool, StoreError> {
        let mut client = self.conn()?;
        let row = client.query_one(
            "SELECT COUNT(*)::BIGINT AS total,
 COUNT(*) FILTER (WHERE status != 'sent')::BIGINT AS unsent
 FROM notification_sink_delivery WHERE outbox_id = $1",
            &[&outbox_id],
        )?;
        let total: i64 = row.get("total");
        let unsent: i64 = row.get("unsent");
        Ok(total == 0 || unsent == 0)
    }

    /// Sync parent outbox status from per-sink rows after a delivery round.
    ///
    /// Parent becomes `dead` only when **no** retryable sinks remain and at least
    /// one sink is `dead`. Marking `dead` while other sinks are still `pending` /
    /// `failed` would drop the row from [`Self::claim_outbox_batch`] and strand
    /// undelivered sinks.
    ///
    /// Returns `true` when the parent row was marked `dead`.
    pub fn sync_outbox_sink_status(&self, outbox_id: i64) -> Result<bool, StoreError> {
        let mut client = self.conn()?;
        let row = client.query_one(
            "SELECT COUNT(*)::BIGINT AS total,
 COUNT(*) FILTER (WHERE status = 'dead')::BIGINT AS dead,
 COUNT(*) FILTER (WHERE status IN ('pending', 'failed'))::BIGINT AS retryable,
 COUNT(*) FILTER (WHERE status != 'sent')::BIGINT AS unsent
 FROM notification_sink_delivery WHERE outbox_id = $1",
            &[&outbox_id],
        )?;
        let total: i64 = row.get("total");
        let dead: i64 = row.get("dead");
        let retryable: i64 = row.get("retryable");
        let unsent: i64 = row.get("unsent");

        if total == 0 || unsent == 0 {
            return Ok(false);
        }

        if retryable == 0 && dead > 0 {
            let updated = client.execute(
                "UPDATE notification_outbox SET status = 'dead' WHERE id = $1 AND status <> 'dead'",
                &[&outbox_id],
            )?;
            return Ok(updated > 0);
        }

        client.execute(
            "UPDATE notification_outbox
 SET status = 'failed',
 last_error = 'partial sink delivery failure'
 WHERE id = $1 AND status IN ('pending', 'failed')",
            &[&outbox_id],
        )?;
        Ok(false)
    }

    /// Requeue every `dead` notification for another delivery round (operator
    /// dead-letter recovery). Resets attempts/lease on the outbox row and its
    /// not-yet-`sent` sink rows, so already-delivered sinks are not re-notified.
    pub fn requeue_dead_outbox(&self) -> Result<usize, StoreError> {
        let mut client = self.conn()?;
        let mut tx = client.transaction()?;
        let ids: Vec<i64> = tx
            .query(
                "SELECT id FROM notification_outbox WHERE status = 'dead'",
                &[],
            )?
            .into_iter()
            .map(|row| row.get(0))
            .collect();
        if ids.is_empty() {
            tx.commit()?;
            return Ok(0);
        }
        tx.execute(
            "UPDATE notification_outbox
 SET status = 'pending', attempts = 0, last_error = NULL, locked_until = NULL
 WHERE id = ANY($1)",
            &[&ids],
        )?;
        tx.execute(
            "UPDATE notification_sink_delivery
 SET status = 'pending', attempts = 0, last_error = NULL, sent_at = NULL
 WHERE outbox_id = ANY($1) AND status <> 'sent'",
            &[&ids],
        )?;
        tx.commit()?;
        Ok(ids.len())
    }

    /// Promote exhausted retry rows to `dead` (startup recovery after crash).
    pub fn finalize_exhausted_outbox(&self) -> Result<usize, StoreError> {
        let mut client = self.conn()?;
        let updated = client.execute(
            "UPDATE notification_outbox
 SET status = 'dead'
 WHERE status = 'failed' AND attempts >= $1",
            &[&OUTBOX_MAX_ATTEMPTS],
        )?;
        Ok(updated as usize)
    }

    /// Outbox depth by status for metrics and readiness probes.
    pub fn outbox_counts(&self) -> Result<OutboxCounts, StoreError> {
        let mut client = self.conn()?;
        let rows = client.query(
            "SELECT status, COUNT(*)::BIGINT FROM notification_outbox GROUP BY status",
            &[],
        )?;
        let mut counts = OutboxCounts::default();
        for row in rows {
            let status: String = row.get(0);
            let n = row.get::<_, i64>(1) as usize;
            match status.as_str() {
                "pending" => counts.pending = n,
                "failed" => counts.failed = n,
                "dead" => counts.dead = n,
                _ => {}
            }
        }
        let deferred: i64 = client
            .query_one(
                "SELECT COUNT(*)::BIGINT FROM notification_outbox
 WHERE status IN ('pending', 'failed')
 AND deliver_after IS NOT NULL
 AND deliver_after > now()",
                &[],
            )?
            .get(0);
        counts.deferred = deferred as usize;
        Ok(counts)
    }

    /// Count notifications waiting for delivery or retry (excludes dead).
    pub fn outbox_pending_count(&self) -> Result<usize, StoreError> {
        let counts = self.outbox_counts()?;
        Ok(counts.pending + counts.failed)
    }
}
