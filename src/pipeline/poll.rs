//! Poll → filter → diff → enqueue / deliver for one watch.

use std::collections::HashMap;

use chrono::Utc;
use std::time::Instant;

use tracing::{debug, info};

use crate::engine::Engine;
use crate::error::{PipelineError, StoreError};
use crate::metrics::PollOutcome;
use crate::model::Release;
use crate::notify::Event;
use crate::sources::Provider;
use crate::store::{OutboxMeta, SeenUpsert, Store, OUTBOX_LEASE_SECS};

use super::{attempt_notification_delivery, Watch};

/// Decide which of `releases` should trigger a notification, updating baseline state.
///
/// On the very first observation of a source this records a silent baseline and
/// returns an empty slice. Later polls return unseen identities and, when
/// `exclude_updated` is false, identities whose body/URL fingerprint changed.
pub(crate) fn select_for_delivery<'a>(
    source_id: &str,
    releases: &'a [Release],
    exclude_updated: bool,
    store: &Store,
) -> Result<Vec<&'a Release>, StoreError> {
    if !store.is_initialized(source_id)? {
        let digests: Vec<String> = releases.iter().map(Release::content_fingerprint).collect();
        let items: Vec<SeenUpsert<'_>> = releases
            .iter()
            .zip(digests.iter())
            .map(|(release, digest)| {
                SeenUpsert::from_release(release, Some(digest.as_str()))
                    .with_published_at(release.published_at)
            })
            .collect();
        store.record_seen_batch(source_id, &items)?;
        if let Some(latest) = crate::model::pick_latest(releases.iter()) {
            store.set_latest_release_tag(source_id, &latest.raw_tag)?;
        }
        return Ok(Vec::new());
    }

    let seen = store.load_seen_index(source_id)?;
    let mut deliverable: Vec<&Release> = releases
        .iter()
        .filter(|release| should_deliver_cached(release, exclude_updated, &seen))
        .collect();

    deliverable.sort_by(|a, b| (a.published_at, &a.version).cmp(&(b.published_at, &b.version)));
    Ok(deliverable)
}

pub(crate) fn should_deliver_cached(
    release: &Release,
    exclude_updated: bool,
    seen: &HashMap<String, Option<String>>,
) -> bool {
    let fingerprint = release.content_fingerprint();
    match seen.get(&release.id) {
        None => true,
        Some(_) if exclude_updated => false,
        Some(stored) => stored.as_deref() != Some(fingerprint.as_str()),
    }
}

/// Poll one source once and deliver notifications for any new releases.
///
/// When `force_fetch` is true (manual API poll), conditional requests are skipped so
/// upstream metadata such as `published_at` can be refreshed even if the etag is unchanged.
pub async fn poll_once(
    engine: &Engine,
    watch: &Watch,
    force_fetch: bool,
) -> Result<usize, PipelineError> {
    let started = Instant::now();
    let result = poll_once_inner(engine, watch, force_fetch).await;
    engine.metrics.record_poll_duration(started.elapsed());
    result
}

pub(crate) async fn poll_once_inner(
    engine: &Engine,
    watch: &Watch,
    force_fetch: bool,
) -> Result<usize, PipelineError> {
    let source_id = watch.provider.id();
    let stored_etag = if force_fetch {
        None
    } else {
        engine.store.get_etag(source_id)?
    };

    engine.acquire_upstream().await;

    let outcome = match watch
        .provider
        .fetch(&engine.http, stored_etag.as_deref())
        .await
    {
        Ok(outcome) => outcome,
        Err(err) => {
            engine.metrics.record_poll(source_id, PollOutcome::Error);
            return Err(err.into());
        }
    };

    engine.store.touch_polled(source_id)?;

    if outcome.not_modified {
        debug!(source = source_id, "upstream not modified (304)");
        if !force_fetch && engine.store.has_seen_missing_published_at(source_id)? {
            refresh_seen_metadata(watch, engine).await?;
        }
        backfill_latest_release_tag_if_missing(&engine.store, source_id)?;
        engine
            .metrics
            .record_poll(source_id, PollOutcome::NotModified);
        return Ok(0);
    }

    if let Some(etag) = &outcome.etag {
        engine.store.set_etag(source_id, Some(etag))?;
    }

    let sent = deliver_new_releases(watch, outcome.releases, engine).await?;
    engine.metrics.record_poll(
        source_id,
        if sent > 0 {
            PollOutcome::Delivered
        } else {
            PollOutcome::NoOp
        },
    );
    Ok(sent)
}

/// Process already-fetched releases: filter → diff → notify (via outbox).
pub async fn deliver_new_releases(
    watch: &Watch,
    releases: Vec<Release>,
    engine: &Engine,
) -> Result<usize, PipelineError> {
    let source_id = watch.provider.id();
    let filtered: Vec<Release> = releases
        .into_iter()
        .filter(|r| watch.filter.accepts(r))
        .collect();

    if let Some(latest) = crate::model::pick_latest(filtered.iter()) {
        engine
            .store
            .set_latest_release_tag(source_id, &latest.raw_tag)?;
    }

    let release_refs: Vec<&Release> = filtered.iter().collect();
    engine
        .store
        .enrich_seen_metadata(source_id, &release_refs)?;

    let deliverable = select_for_delivery(
        source_id,
        &filtered,
        watch.filter.excludes_updated(),
        &engine.store,
    )?;
    if deliverable.is_empty() {
        debug!(source = source_id, "no releases to deliver");
        return Ok(0);
    }

    let mut sent = 0usize;
    for release in deliverable {
        let mut event = build_event(&watch.provider, release);
        event.routing_tag.clone_from(&watch.routing_tag);
        let digest = release.content_fingerprint();
        let display_tag = (release.raw_tag != release.id).then_some(release.raw_tag.as_str());
        // Recomputed per enqueue so a batch straddling a cron moment defers
        // each notification to its own next occurrence.
        let deliver_after = watch
            .notify_schedule
            .as_ref()
            .and_then(|schedule| schedule.next_after(Utc::now()));
        let meta = OutboxMeta {
            content_digest: Some(&digest),
            display_tag,
            published_at: release.published_at,
            deliver_after,
        };
        let seen = SeenUpsert::from_release(release, Some(&digest))
            .with_published_at(release.published_at);
        let Some(enqueued) = engine
            .store
            .try_enqueue_notification(&event, &release.id, meta)?
        else {
            debug!(source = source_id, tag = %release.raw_tag, "notification already delivered");
            continue;
        };
        if enqueued.created {
            engine.metrics.record_outbox_enqueued(1);
        }
        // Catalogue as soon as the release is detected (baseline does the same on
        // first poll). Outbox rows track notification delivery separately.
        engine.store.record_seen(source_id, &seen)?;
        if !enqueued.deliver_now {
            debug!(
                source = source_id,
                tag = %release.raw_tag,
                "outbox row in flight; background flush will deliver"
            );
            continue;
        }

        // Lease the row so a concurrent flush cannot deliver it in parallel.
        if !engine
            .store
            .claim_outbox_row(enqueued.id, OUTBOX_LEASE_SECS)?
        {
            debug!(
                source = source_id,
                tag = %release.raw_tag,
                "outbox row leased by another worker; flush will deliver"
            );
            continue;
        }

        if attempt_notification_delivery(engine, enqueued.id, 0, &event, source_id, &seen).await? {
            sent += 1;
        }
    }

    if sent > 0 {
        info!(source = source_id, sent, "delivered notifications");
        engine.metrics.record_notifications(source_id, sent);
    }
    Ok(sent)
}

/// Re-fetch upstream without etag and merge `published_at` / `url` into existing rows.
pub(crate) async fn refresh_seen_metadata(
    watch: &Watch,
    engine: &Engine,
) -> Result<(), PipelineError> {
    let source_id = watch.provider.id();
    engine.acquire_upstream().await;
    let outcome = watch.provider.fetch(&engine.http, None).await?;
    if outcome.not_modified {
        return Ok(());
    }

    let filtered: Vec<Release> = outcome
        .releases
        .into_iter()
        .filter(|release| watch.filter.accepts(release))
        .collect();
    let release_refs: Vec<&Release> = filtered.iter().collect();
    engine
        .store
        .enrich_seen_metadata(source_id, &release_refs)?;

    if let Some(etag) = &outcome.etag {
        engine.store.set_etag(source_id, Some(etag))?;
    }
    Ok(())
}

pub(crate) fn backfill_latest_release_tag_if_missing(
    store: &Store,
    source_id: &str,
) -> Result<(), StoreError> {
    if store
        .source_state_row(source_id)?
        .latest_release_tag
        .is_some()
    {
        return Ok(());
    }
    if let Some(tag) = store.best_seen_identity(source_id)? {
        store.set_latest_release_tag(source_id, &tag)?;
    }
    Ok(())
}

/// Render a [`Release`] into a notification [`Event`].
pub(crate) fn build_event(provider: &Provider, release: &Release) -> Event {
    let name = provider.display_name();
    let version = release
        .version
        .as_ref()
        .map_or_else(|| release.raw_tag.clone(), ToString::to_string);

    let title = format!("{name}: new release {}", release.raw_tag);

    let mut body = format!("**{name}** published **{}**.\n\n", release.raw_tag);
    body.push_str(&format!("- Source: {}\n", provider.kind_label()));
    body.push_str(&format!("- Version: {version}\n"));
    if let Some(published) = release.published_at {
        // "2025-01-15" is more readable in a notification than a full RFC 3339 timestamp.
        body.push_str(&format!("- Published: {}\n", published.format("%Y-%m-%d")));
    }
    if let Some(url) = &release.url {
        body.push_str(&format!("- Link: {url}\n"));
    }
    // Append the release notes/changelog body when the source provides one.
    // GitHub REST and GitLab API both return Markdown; Docker/Atom do not.
    if let Some(notes) = &release.body {
        body.push_str("\n---\n\n");
        // Truncate by Unicode scalar values (not bytes) so emoji/CJK do not
        // overshoot the intended notification size limit.
        const MAX_BODY_CHARS: usize = 3_000;
        match notes.char_indices().nth(MAX_BODY_CHARS) {
            Some((idx, _)) => {
                body.push_str(&notes[..idx]);
                body.push_str("\n\n*…changelog truncated…*");
            }
            None => body.push_str(notes),
        }
    }

    Event {
        source_id: provider.id().to_owned(),
        source_kind: provider.kind_label().to_owned(),
        title,
        body,
        url: release.url.clone(),
        routing_tag: None,
    }
}
