//! Notification outbox flush, digest grouping, and per-sink delivery.

use std::collections::HashMap;
use std::time::Instant;

use chrono::{DateTime, Utc};
use futures_util::future::join_all;
use tracing::{debug, info, warn};

use crate::engine::Engine;
use crate::error::{NotifyError, PipelineError, StoreError};
use crate::metrics::SinkDeliveryResult;
use crate::notify::{CompositeNotifier, Event};
use crate::store::{OutboxRecord, SeenUpsert, OUTBOX_LEASE_SECS};

/// Record one sink attempt against both the labelled sink series and (for
/// breaker skips) the global counter.
fn record_sink_attempt(
    engine: &Engine,
    notifier: &CompositeNotifier,
    index: usize,
    result: &Result<(), NotifyError>,
) {
    let outcome = match result {
        Ok(()) => SinkDeliveryResult::Ok,
        Err(NotifyError::BreakerOpen { .. }) => SinkDeliveryResult::Breaker,
        Err(_) => SinkDeliveryResult::Error,
    };
    engine.metrics.record_sink_delivery(
        notifier.kind_at(index),
        &notifier.label_at(index),
        outcome,
    );
}

/// Fail a parent outbox row and bump lifecycle counters.
///
/// Returns `true` when the row transitioned to `dead`.
fn fail_outbox_row(engine: &Engine, outbox_id: i64, error: &str) -> Result<bool, StoreError> {
    let dead = engine.store.fail_notification(outbox_id, error)?;
    engine.metrics.record_outbox_delivery_failure();
    if dead {
        engine.metrics.record_outbox_dead_lettered(1);
    }
    Ok(dead)
}

/// Sync parent status from sink ledger; count a transition to `dead`.
///
/// Returns `true` when the row transitioned to `dead`.
fn sync_outbox_status(engine: &Engine, outbox_id: i64) -> Result<bool, StoreError> {
    let dead = engine.store.sync_outbox_sink_status(outbox_id)?;
    if dead {
        engine.metrics.record_outbox_dead_lettered(1);
    }
    Ok(dead)
}

async fn emit_dead_outbox_alert(engine: &Engine, outbox_id: i64, source_id: &str) {
    engine
        .emit_ops_alert(
            &format!("Outbox dead: {source_id}"),
            &format!(
                "Notification outbox row `{outbox_id}` for source `{source_id}` exhausted delivery retries and was marked dead."
            ),
        )
        .await;
}

async fn emit_breaker_alerts(engine: &Engine, notifier: &CompositeNotifier) {
    for label in notifier.drain_newly_opened_breakers() {
        engine
            .emit_ops_alert(
                &format!("Sink breaker open: {label}"),
                &format!(
                    "Notification sink `{label}` tripped its circuit breaker after consecutive failures. Delivery to this sink is paused for the configured cooldown."
                ),
            )
            .await;
    }
}

async fn fail_outbox_row_alert(
    engine: &Engine,
    outbox_id: i64,
    source_id: &str,
    error: &str,
) -> Result<(), StoreError> {
    if fail_outbox_row(engine, outbox_id, error)? {
        emit_dead_outbox_alert(engine, outbox_id, source_id).await;
    }
    Ok(())
}

async fn sync_outbox_status_alert(
    engine: &Engine,
    outbox_id: i64,
    source_id: &str,
) -> Result<(), StoreError> {
    if sync_outbox_status(engine, outbox_id)? {
        emit_dead_outbox_alert(engine, outbox_id, source_id).await;
    }
    Ok(())
}

/// Retry pending/failed outbox rows (global drain — used by background maintenance).
///
/// Rows are claimed and delivered **one wave at a time** (up to
/// [`Engine::outbox_flush_concurrency`] rows per wave): claiming the next wave
/// only after the current one finishes keeps a row's lease window scoped to
/// its own wave, regardless of how many waves the flush needs. Claiming the
/// whole `limit` upfront would start every row's `OUTBOX_LEASE_SECS` clock at
/// the same instant, so a row in a late wave could sit leased through several
/// waves of slow/retrying sinks ahead of it and risk lease expiry (and a
/// concurrent re-claim by another worker).
pub async fn flush_notification_outbox(
    engine: &Engine,
    limit: usize,
) -> Result<usize, PipelineError> {
    let started = Instant::now();
    let result = flush_notification_outbox_inner(engine, limit).await;
    engine
        .metrics
        .record_outbox_flush_duration(started.elapsed());
    result
}

pub(crate) async fn flush_notification_outbox_inner(
    engine: &Engine,
    limit: usize,
) -> Result<usize, PipelineError> {
    let concurrency = engine.outbox_flush_concurrency().await.max(1);
    let mut sent = 0usize;
    let mut first_err: Option<PipelineError> = None;
    let mut claimed_total = 0usize;

    while claimed_total < limit {
        let wave_limit = concurrency.min(limit - claimed_total);
        let wave = engine
            .store
            .claim_outbox_batch(wave_limit, OUTBOX_LEASE_SECS)?;
        if wave.is_empty() {
            break;
        }
        claimed_total += wave.len();
        let wave_len = wave.len();

        // Cron-deferred rows sharing one `(deliver_after, routing_tag)`
        // moment are grouped into a single digest message (see
        // `partition_for_digest`); everything else — including a "group" of
        // one deferred row — delivers exactly as before this existed.
        let units = partition_for_digest(wave);
        let deliveries = units.into_iter().map(|unit| async move {
            match unit {
                DeliveryUnit::Single(row) => {
                    let source_id = row.source_id.clone();
                    let delivered = deliver_claimed_outbox_row(engine, *row).await?;
                    Ok::<_, PipelineError>(vec![(source_id, delivered)])
                }
                DeliveryUnit::Digest(group) => deliver_claimed_outbox_digest(engine, group).await,
            }
        });

        for result in join_all(deliveries).await {
            match result {
                Ok(rows) => {
                    for (source_id, delivered) in rows {
                        if delivered {
                            sent += 1;
                            engine.metrics.record_notifications(&source_id, 1);
                        }
                    }
                }
                Err(err) => {
                    if first_err.is_none() {
                        first_err = Some(err);
                    } else {
                        warn!(error = %err, "outbox flush row failed after earlier error");
                    }
                }
            }
        }

        // A short wave (fewer rows than requested) means the claimable
        // backlog is drained for now (or the rest are leased by another
        // worker) — another claim right now would likely return empty too.
        if wave_len < wave_limit {
            break;
        }
    }

    if let Some(err) = first_err {
        return Err(err);
    }

    if sent > 0 {
        info!(sent, concurrency, "flushed notification outbox");
    }
    Ok(sent)
}

pub(crate) async fn deliver_claimed_outbox_row(
    engine: &Engine,
    row: OutboxRecord,
) -> Result<bool, PipelineError> {
    let event = row.to_event();
    let seen = seen_upsert_for(&row);
    let attempts_before = i32::try_from(row.attempts).unwrap_or(i32::MAX);
    attempt_notification_delivery(
        engine,
        row.id,
        attempts_before,
        &event,
        &row.source_id,
        &seen,
    )
    .await
}

/// Build the `seen_release` upsert payload for a claimed outbox row (shared by
/// the single-row and digest delivery paths).
pub(crate) fn seen_upsert_for(row: &OutboxRecord) -> SeenUpsert<'_> {
    SeenUpsert {
        identity: &row.identity,
        display_tag: row.display_tag.as_deref(),
        content_digest: row.content_digest.as_deref(),
        published_at: row.published_at,
        url: row.url.as_deref(),
    }
}

/// One flush-wave delivery unit: either a single row (immediate or a
/// cron-deferred row that is the only one due for its `(deliver_after,
/// routing_tag)` moment) or a digest of several cron-deferred rows sharing
/// one moment and team tag.
///
/// `Single` boxes its row so the enum stays small even though most units in
/// a typical wave are `Single` — without the box, every `Digest(Vec<..>)`
/// (24 bytes) would still pay for `OutboxRecord`'s full inline size (~256
/// bytes, dominated by its `String`/`Option<String>` fields) because Rust
/// sizes an enum to its largest variant.
pub(crate) enum DeliveryUnit {
    Single(Box<OutboxRecord>),
    Digest(Vec<OutboxRecord>),
}

/// Group a claimed wave into delivery units for digest batching.
///
/// Only cron-deferred rows (`deliver_after.is_some()`) are grouped — immediate
/// rows always deliver individually, same as before this existed. Rows are
/// grouped by the exact `(deliver_after, routing_tag)` pair: [`NotifySchedule`]
/// resolves to a discrete cron moment (not the enqueue-time clock reading), so
/// independent watches sharing a schedule naturally compute the identical
/// `deliver_after` instant and cluster into one digest. A group of exactly one
/// row is delivered as a plain [`DeliveryUnit::Single`] — a "digest" of one
/// release reads worse than the plain per-release message it would replace.
pub(crate) fn partition_for_digest(wave: Vec<OutboxRecord>) -> Vec<DeliveryUnit> {
    let mut groups: HashMap<(DateTime<Utc>, Option<String>), Vec<OutboxRecord>> = HashMap::new();
    let mut units: Vec<DeliveryUnit> = Vec::new();

    for row in wave {
        match row.deliver_after {
            Some(at) => {
                groups
                    .entry((at, row.routing_tag.clone()))
                    .or_default()
                    .push(row);
            }
            None => units.push(DeliveryUnit::Single(Box::new(row))),
        }
    }

    for (_, mut rows) in groups {
        if rows.len() == 1 {
            units.push(DeliveryUnit::Single(Box::new(
                rows.pop().expect("just checked len == 1"),
            )));
        } else {
            units.push(DeliveryUnit::Digest(rows));
        }
    }

    units
}

/// Combine several cron-deferred outbox rows sharing one delivery moment and
/// team tag into a single digest [`Event`] instead of one message per row.
///
/// Reuses each row's already-rendered `title` (built once by [`build_event`]
/// at enqueue time) as the per-item line — no source is re-fetched and the
/// full per-release changelog body is deliberately left out, since a digest
/// is meant to summarize, not concatenate every release's full notes.
pub(crate) fn build_digest_event(group: &[OutboxRecord]) -> Event {
    let count = group.len();
    let routing_tag = group[0].routing_tag.clone();
    let title = format!("{count} new releases");

    let mut body = format!("**{count} releases** ready:\n\n");
    for row in group {
        body.push_str("- ");
        body.push_str(&row.title);
        if let Some(url) = &row.url {
            body.push_str(&format!(" — {url}"));
        }
        body.push('\n');
    }

    Event {
        // No single release owns a digest, so `source_id`/`source_kind` use a
        // synthetic placeholder rather than one arbitrary member's identity —
        // sink templates keying on these (e.g. a webhook JSON field or a
        // Kafka partition key) see every digest as one logical source.
        source_id: "digest".to_owned(),
        source_kind: "Digest".to_owned(),
        title,
        body,
        url: None,
        routing_tag,
    }
}

/// Backoff after a failed delivery attempt, in seconds — doubles from the
/// flush cadence (60s) per attempt, capped at `max_secs`
/// (`[defaults].outbox_retry_backoff_max_secs`).
///
/// `attempts` is the attempt count *after* the failure that triggered this
/// backoff (1 = first failure), so a fresh row's first retry lands ~60s
/// later — the same cadence as before backoff existed — and only later
/// attempts back off further.
pub(crate) fn backoff_secs(attempts: i32, max_secs: u32) -> f64 {
    const BASE_SECS: f64 = 60.0;
    let exponent = attempts.saturating_sub(1).clamp(0, 16);
    (BASE_SECS * 2f64.powi(exponent)).min(f64::from(max_secs))
}

/// Deliver one already-leased outbox row, backing off the lease unless the row
/// is now fully delivered (so a partial/failed attempt is retryable, at an
/// exponentially increasing delay, instead of hammering a broken endpoint
/// every flush cycle until it exhausts [`crate::store::OUTBOX_MAX_ATTEMPTS`]).
///
/// `attempts_before` is the row's attempt count *before* this try (`0` for a
/// freshly enqueued row delivered inline). The caller must already hold the
/// delivery lease (via [`Store::claim_outbox_batch`] or [`Store::claim_outbox_row`]).
pub(crate) async fn attempt_notification_delivery(
    engine: &Engine,
    outbox_id: i64,
    attempts_before: i32,
    event: &Event,
    source_id: &str,
    seen: &SeenUpsert<'_>,
) -> Result<bool, PipelineError> {
    let started = Instant::now();
    let outcome = deliver_to_pending_sinks(engine, outbox_id, event, source_id, seen).await;
    engine.metrics.record_notify_duration(started.elapsed());
    if !matches!(outcome, Ok(true)) {
        let backoff = backoff_secs(
            attempts_before + 1,
            engine.outbox_retry_backoff_max_secs().await,
        );
        // Best-effort; lease expiry is the safety net if this fails.
        if let Err(err) = engine.store.apply_delivery_backoff(outbox_id, backoff) {
            warn!(outbox_id, error = %err, "failed to apply outbox delivery backoff");
        }
    }
    outcome
}

/// Deliver one outbox row to pending sinks only; complete when all sinks ack.
pub(crate) async fn deliver_to_pending_sinks(
    engine: &Engine,
    outbox_id: i64,
    event: &Event,
    source_id: &str,
    seen: &SeenUpsert<'_>,
) -> Result<bool, PipelineError> {
    // One snapshot for the whole attempt — see `Engine::notifier_snapshot`.
    // `ensure_sink_deliveries`/`list_pending_sink_indices`/etc. below are real
    // DB round-trips; re-reading the live notifier after any of them could
    // observe a config apply's hot-swap mid-attempt and compute indices
    // against a sink list that no longer matches the one `notify_partial`
    // ends up calling.
    let notifier = engine.notifier_snapshot().await;
    let matching = notifier.matching_indices(event);
    if matching.is_empty() {
        // Misconfigured team tags must not silently "succeed" — leave the row
        // retryable (`failed` / eventually `dead`) so fixing routing + flush
        // (or `outbox requeue`) can still deliver.
        let error = format!(
            "no notifier matches team routing tag {:?}",
            event.routing_tag
        );
        warn!(source = %source_id, outbox_id, tag = ?event.routing_tag, "{error}");
        fail_outbox_row_alert(engine, outbox_id, source_id, &error).await?;
        return Ok(false);
    }

    let sinks: Vec<(usize, &str)> = matching
        .iter()
        .map(|&index| (index, notifier.kind_at(index)))
        .collect();
    engine.store.ensure_sink_deliveries(outbox_id, &sinks)?;

    let pending = engine.store.list_pending_sink_indices(outbox_id)?;
    let pending_indices: Vec<usize> = if pending.is_empty() {
        if engine.store.sinks_delivery_complete(outbox_id)? {
            return engine
                .store
                .complete_notification(outbox_id, source_id, seen)
                .map_err(Into::into);
        }
        // No retryable sinks remain (the rest are `dead`); re-delivering `matching`
        // here would duplicate notifications to sinks that already succeeded.
        // Sync parent → `dead` so the row leaves the claimable set.
        sync_outbox_status_alert(engine, outbox_id, source_id).await?;
        return Ok(false);
    } else {
        pending
            .iter()
            .copied()
            .filter(|index| matching.contains(index))
            .collect()
    };

    if pending_indices.is_empty() {
        // Config drift: ledger still has retryable rows for sinks that no longer
        // match routing. Abandon them and fail the parent so flush cannot loop
        // forever without incrementing attempts.
        let error = format!(
            "no matching sinks for current routing tag {:?}",
            event.routing_tag
        );
        warn!(source = %source_id, outbox_id, tag = ?event.routing_tag, "{error}");
        engine
            .store
            .abandon_retryable_sinks(outbox_id, &[], &error)?;
        fail_outbox_row_alert(engine, outbox_id, source_id, &error).await?;
        sync_outbox_status_alert(engine, outbox_id, source_id).await?;
        return Ok(false);
    }

    let orphans: Vec<usize> = pending
        .iter()
        .copied()
        .filter(|index| !matching.contains(index))
        .collect();
    if !orphans.is_empty() {
        let error = format!("sink no longer matches routing tag {:?}", event.routing_tag);
        let abandoned = engine
            .store
            .abandon_retryable_sinks(outbox_id, &orphans, &error)?;
        debug!(
            source = %source_id,
            outbox_id,
            abandoned,
            orphans = orphans.len(),
            "abandoned sink ledger rows that no longer match routing"
        );
    }

    let results = notifier.notify_partial(event, &pending_indices).await;
    emit_breaker_alerts(engine, &notifier).await;

    let mut any_failed = false;
    for (index, result) in results {
        record_sink_attempt(engine, &notifier, index, &result);
        match result {
            Ok(()) => engine.store.complete_sink_delivery(outbox_id, index)?,
            Err(err) => {
                any_failed = true;
                if matches!(err, NotifyError::BreakerOpen { .. }) {
                    // A breaker-open skip is a DEFERRAL, not an attempt: no
                    // network call was made, so it must not burn the sink's
                    // retry budget (`fail_sink_delivery` marches `attempts`
                    // toward the dead-letter cap). Leaving the sink row as-is
                    // keeps it retryable; the parent still backs off below.
                    // Expected/intentional — already surfaced when the breaker
                    // first tripped, so `debug!` (a `warn!` every flush cycle
                    // for a known-broken, deliberately-skipped sink is noise).
                    debug!(
                        source = %source_id,
                        outbox_id,
                        sink_index = index,
                        "sink delivery skipped: circuit breaker open"
                    );
                } else {
                    engine
                        .store
                        .fail_sink_delivery(outbox_id, index, &err.to_string())?;
                    warn!(
                        source = %source_id,
                        outbox_id,
                        sink_index = index,
                        error = %err,
                        "sink delivery failed"
                    );
                }
            }
        }
    }

    sync_outbox_status_alert(engine, outbox_id, source_id).await?;

    if any_failed {
        return Ok(false);
    }

    if engine.store.sinks_delivery_complete(outbox_id)? {
        return engine
            .store
            .complete_notification(outbox_id, source_id, seen)
            .map_err(Into::into);
    }

    Ok(false)
}

/// Deliver a digest group: one combined message for several cron-deferred
/// rows sharing a `(deliver_after, routing_tag)` moment.
///
/// Each row keeps its own per-sink ledger, lease, and backoff — grouping only
/// changes *what* gets sent (one [`Event`] instead of one per row) and *how
/// many* network calls happen (one [`crate::notify::CompositeNotifier::notify_partial`]
/// for the whole group), never the row-level bookkeeping that already makes a
/// single delivery safe to retry.
pub(crate) async fn deliver_claimed_outbox_digest(
    engine: &Engine,
    group: Vec<OutboxRecord>,
) -> Result<Vec<(String, bool)>, PipelineError> {
    let event = build_digest_event(&group);
    let outcomes = deliver_digest_to_pending_sinks(engine, &event, &group).await?;

    let mut results = Vec::with_capacity(group.len());
    for (row, delivered) in group.iter().zip(outcomes.iter()) {
        if !*delivered {
            let attempts_before = i32::try_from(row.attempts).unwrap_or(i32::MAX);
            let backoff = backoff_secs(
                attempts_before + 1,
                engine.outbox_retry_backoff_max_secs().await,
            );
            // Best-effort; lease expiry is the safety net if this fails.
            if let Err(err) = engine.store.apply_delivery_backoff(row.id, backoff) {
                warn!(outbox_id = row.id, error = %err, "failed to apply outbox delivery backoff");
            }
        }
        results.push((row.source_id.clone(), *delivered));
    }
    Ok(results)
}

/// Deliver one digest [`Event`] to the union of sinks still pending for *any*
/// row in `group`, then apply the shared result to each row's own ledger.
///
/// The digest is sent **once** — a chatty per-row re-send would defeat the
/// point of batching — so a row that already completed a given sink (from an
/// earlier partial-failure retry that regrouped different rows together)
/// simply does not receive that sink's result again: each row's own current
/// pending set (re-read fresh after the send) gates which `(index, result)`
/// pairs apply to it. A row whose remaining sinks are a strict subset of the
/// group's union is therefore never redundantly marked failed for a sink it
/// already has `sent`.
pub(crate) async fn deliver_digest_to_pending_sinks(
    engine: &Engine,
    event: &Event,
    group: &[OutboxRecord],
) -> Result<Vec<bool>, PipelineError> {
    // One snapshot for the whole digest attempt — see `Engine::notifier_snapshot`.
    let notifier = engine.notifier_snapshot().await;
    let matching = notifier.matching_indices(event);
    if matching.is_empty() {
        let error = format!(
            "no notifier matches team routing tag {:?}",
            event.routing_tag
        );
        warn!(digest_size = group.len(), tag = ?event.routing_tag, "{error}");
        for row in group {
            fail_outbox_row_alert(engine, row.id, &row.source_id, &error).await?;
        }
        return Ok(vec![false; group.len()]);
    }

    let sink_refs: Vec<(usize, &str)> = matching
        .iter()
        .map(|&index| (index, notifier.kind_at(index)))
        .collect();
    for row in group {
        engine.store.ensure_sink_deliveries(row.id, &sink_refs)?;
    }

    // Per-row pending sinks (matching current routing only); ledger rows for
    // sinks that no longer match routing are abandoned same as a
    // non-digested delivery so they cannot strand a row forever.
    let mut union_pending: Vec<usize> = Vec::new();
    let mut row_pending: Vec<Vec<usize>> = Vec::with_capacity(group.len());
    for row in group {
        let raw_pending = engine.store.list_pending_sink_indices(row.id)?;
        let orphans: Vec<usize> = raw_pending
            .iter()
            .copied()
            .filter(|index| !matching.contains(index))
            .collect();
        if !orphans.is_empty() {
            let error = format!("sink no longer matches routing tag {:?}", event.routing_tag);
            engine
                .store
                .abandon_retryable_sinks(row.id, &orphans, &error)?;
        }
        let pending: Vec<usize> = raw_pending
            .into_iter()
            .filter(|index| matching.contains(index))
            .collect();
        for &index in &pending {
            if !union_pending.contains(&index) {
                union_pending.push(index);
            }
        }
        row_pending.push(pending);
    }

    if union_pending.is_empty() {
        // Every row's remaining sinks are already `dead` (or none were ever
        // pending) — nothing to send; just sync each row's parent status.
        let mut outcomes = Vec::with_capacity(group.len());
        for row in group {
            let outcome = if engine.store.sinks_delivery_complete(row.id)? {
                let seen = seen_upsert_for(row);
                engine
                    .store
                    .complete_notification(row.id, &row.source_id, &seen)?
            } else {
                sync_outbox_status_alert(engine, row.id, &row.source_id).await?;
                false
            };
            outcomes.push(outcome);
        }
        return Ok(outcomes);
    }

    let results = notifier.notify_partial(event, &union_pending).await;
    emit_breaker_alerts(engine, &notifier).await;

    // Counted once per skipped *call* (`union_pending` — one send for the
    // whole group), not once per row the call happens to cover below.
    for (index, result) in &results {
        record_sink_attempt(engine, &notifier, *index, result);
    }

    let mut outcomes = Vec::with_capacity(group.len());
    for (row, pending) in group.iter().zip(row_pending.iter()) {
        for (index, result) in &results {
            if !pending.contains(index) {
                continue;
            }
            match result {
                Ok(()) => engine.store.complete_sink_delivery(row.id, *index)?,
                Err(err) => {
                    if matches!(err, NotifyError::BreakerOpen { .. }) {
                        // Deferral, not an attempt — do not burn the sink's
                        // retry budget (see the non-digest path for rationale).
                        debug!(
                            outbox_id = row.id,
                            sink_index = index,
                            "digest sink delivery skipped: circuit breaker open"
                        );
                    } else {
                        engine
                            .store
                            .fail_sink_delivery(row.id, *index, &err.to_string())?;
                        warn!(
                            outbox_id = row.id,
                            sink_index = index,
                            error = %err,
                            "digest sink delivery failed"
                        );
                    }
                }
            }
        }
        sync_outbox_status_alert(engine, row.id, &row.source_id).await?;

        let outcome = if engine.store.sinks_delivery_complete(row.id)? {
            let seen = seen_upsert_for(row);
            engine
                .store
                .complete_notification(row.id, &row.source_id, &seen)?
        } else {
            false
        };
        outcomes.push(outcome);
    }

    Ok(outcomes)
}
