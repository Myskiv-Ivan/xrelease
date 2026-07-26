//! Integration tests for the PostgreSQL store (require a running Postgres).

use crate::notify::Event;
use crate::store::{OutboxMeta, SeenUpsert, Store};

fn seen<'a>(identity: &'a str, content_digest: Option<&'a str>) -> SeenUpsert<'a> {
    SeenUpsert {
        identity,
        display_tag: None,
        content_digest,
        published_at: None,
        url: None,
    }
}

fn open_store() -> Option<Store> {
    match Store::open_test() {
        Ok(store) => Some(store),
        Err(err) => {
            eprintln!("skipping store test (postgres unavailable): {err}");
            None
        }
    }
}

#[test]
fn unseen_should_return_all_identities_before_baseline() {
    let Some(store) = open_store() else {
        return;
    };
    let fresh = store.unseen("s1", &["a", "b"]).expect("query");
    assert_eq!(fresh, vec!["a", "b"]);
}

#[test]
fn unseen_should_exclude_recorded_identities() {
    let Some(store) = open_store() else {
        return;
    };
    store.record_seen("s1", &seen("a", None)).expect("record");
    let fresh = store.unseen("s1", &["a", "b"]).expect("query");
    assert_eq!(fresh, vec!["b"]);
}

#[test]
fn outbox_should_enqueue_with_team_tag() {
    let Some(store) = open_store() else {
        return;
    };
    let event = Event {
        source_id: "github:org/app".into(),
        source_kind: "GitHub".into(),
        title: "app v1".into(),
        body: "notes".into(),
        url: None,
        routing_tag: Some("platform-team".into()),
    };
    let outcome = store
        .try_enqueue_notification(&event, "v1.0.0", OutboxMeta::default())
        .expect("enqueue")
        .expect("row");
    assert!(outcome.deliver_now);
    store
        .complete_notification(outcome.id, "github:org/app", &seen("v1.0.0", None))
        .expect("complete");
}

#[test]
fn claim_should_lease_row_and_block_concurrent_claim() {
    let Some(store) = open_store() else {
        return;
    };
    let event = Event {
        source_id: "github:org/app".into(),
        source_kind: "GitHub".into(),
        title: "app v1".into(),
        body: "notes".into(),
        url: None,
        routing_tag: None,
    };
    let outcome = store
        .try_enqueue_notification(&event, "v1.0.0", OutboxMeta::default())
        .expect("enqueue")
        .expect("row");

    // First claim leases the row.
    let first = store.claim_outbox_batch(10, 300).expect("claim");
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].id, outcome.id);

    // A concurrent worker sees nothing while the lease holds — no double delivery.
    assert!(store.claim_outbox_batch(10, 300).expect("claim").is_empty());
    assert!(!store.claim_outbox_row(outcome.id, 300).expect("claim row"));

    // Releasing the lease makes the row claimable again (prompt retry).
    store.release_outbox_lease(outcome.id).expect("release");
    let again = store.claim_outbox_batch(10, 300).expect("claim");
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].id, outcome.id);
}

/// The invariant `flush_notification_outbox`'s per-wave claiming depends on:
/// repeated small claims are equivalent to one big claim split across
/// round-trips — no row is ever returned twice, and every enqueued row is
/// eventually returned.
#[test]
fn repeated_small_claims_should_never_double_claim_across_calls() {
    let Some(store) = open_store() else {
        return;
    };
    let event = Event {
        source_id: "github:org/wave".into(),
        source_kind: "GitHub".into(),
        title: "app vN".into(),
        body: "notes".into(),
        url: None,
        routing_tag: None,
    };
    let enqueued: Vec<i64> = (0..5)
        .map(|n| {
            store
                .try_enqueue_notification(&event, &format!("v1.0.{n}"), OutboxMeta::default())
                .expect("enqueue")
                .expect("row")
                .id
        })
        .collect();

    // Claim in waves of 2 (mirrors flush_notification_outbox_inner with
    // concurrency=2): 2 + 2 + 1, then empty.
    let wave1 = store.claim_outbox_batch(2, 300).expect("claim");
    let wave2 = store.claim_outbox_batch(2, 300).expect("claim");
    let wave3 = store.claim_outbox_batch(2, 300).expect("claim");
    let wave4 = store.claim_outbox_batch(2, 300).expect("claim");

    assert_eq!(wave1.len(), 2);
    assert_eq!(wave2.len(), 2);
    assert_eq!(wave3.len(), 1);
    assert!(wave4.is_empty(), "backlog must be exhausted after 5 rows");

    let mut claimed_ids: Vec<i64> = [wave1, wave2, wave3]
        .into_iter()
        .flatten()
        .map(|row| row.id)
        .collect();
    claimed_ids.sort_unstable();
    let mut expected_ids = enqueued;
    expected_ids.sort_unstable();
    assert_eq!(claimed_ids, expected_ids, "every row claimed exactly once");
}

#[test]
fn delivery_backoff_should_defer_claim_and_advance_attempts() {
    let Some(store) = open_store() else {
        return;
    };
    let event = Event {
        source_id: "github:org/backoff".into(),
        source_kind: "GitHub".into(),
        title: "app v1".into(),
        body: "notes".into(),
        url: None,
        routing_tag: None,
    };
    let outcome = store
        .try_enqueue_notification(&event, "v1.0.0", OutboxMeta::default())
        .expect("enqueue")
        .expect("row");
    store.claim_outbox_batch(10, 300).expect("initial claim");

    // A short lease (10s) would normally make the row reclaimable almost
    // immediately; a 1-hour backoff must keep it unclaimable regardless.
    store
        .apply_delivery_backoff(outcome.id, 3_600.0)
        .expect("apply backoff");
    assert!(
        store.claim_outbox_batch(10, 10).expect("claim").is_empty(),
        "backed-off row must not be claimable before the backoff window passes"
    );

    // Backoff defers the lease AND advances `attempts` — that count is the
    // exponential-backoff clock the next flush reads, so it must escalate on
    // every deferral; status stays `pending` (dead-lettering is
    // driven by the per-sink ledger, not this counter).
    let entries = store.list_outbox_entries(10).expect("list");
    let row = entries
        .iter()
        .find(|e| e.id == outcome.id)
        .expect("row present");
    assert_eq!(row.status, "pending");
    assert_eq!(row.attempts, 1);

    // A second deferral escalates the clock again.
    store
        .apply_delivery_backoff(outcome.id, 3_600.0)
        .expect("apply backoff twice");
    let entries = store.list_outbox_entries(10).expect("list");
    let row = entries
        .iter()
        .find(|e| e.id == outcome.id)
        .expect("row present");
    assert_eq!(row.attempts, 2);
}

#[test]
fn schedule_deferred_outbox_row_should_wait_until_due() {
    let Some(store) = open_store() else {
        return;
    };
    let event = Event {
        source_id: "github:org/deferred".into(),
        source_kind: "GitHub".into(),
        title: "app v2".into(),
        body: "notes".into(),
        url: None,
        routing_tag: None,
    };

    // Future deliver_after: enqueued, but neither inline-deliverable nor claimable.
    let deferred = store
        .try_enqueue_notification(
            &event,
            "v2.0.0",
            OutboxMeta {
                deliver_after: Some(chrono::Utc::now() + chrono::TimeDelta::hours(1)),
                ..OutboxMeta::default()
            },
        )
        .expect("enqueue")
        .expect("row");
    assert!(
        !deferred.deliver_now,
        "deferred row must not deliver inline"
    );
    assert!(
        !store.claim_outbox_row(deferred.id, 300).expect("claim row"),
        "deferred row must not be claimable before deliver_after"
    );
    assert!(
        store.claim_outbox_batch(10, 300).expect("claim").is_empty(),
        "deferred row must not be claimable in batch"
    );

    // Past deliver_after: behaves like an immediate notification.
    let due = store
        .try_enqueue_notification(
            &event,
            "v2.0.1",
            OutboxMeta {
                deliver_after: Some(chrono::Utc::now() - chrono::TimeDelta::minutes(1)),
                ..OutboxMeta::default()
            },
        )
        .expect("enqueue")
        .expect("row");
    assert!(due.deliver_now, "past deliver_after delivers immediately");
    let claimed = store.claim_outbox_batch(10, 300).expect("claim");
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].id, due.id);
}

/// `deliver_after` must round-trip through `claim_outbox_batch` intact — the
/// pipeline's digest grouping keys claimed rows by `(deliver_after,
/// routing_tag)`, so a claimed row that silently lost its `deliver_after`
/// would never group with the sibling rows it was enqueued alongside.
#[test]
fn claim_outbox_batch_should_round_trip_deliver_after() {
    let Some(store) = open_store() else {
        return;
    };
    let event = Event {
        source_id: "github:org/digest".into(),
        source_kind: "GitHub".into(),
        title: "app vN".into(),
        body: "notes".into(),
        url: None,
        routing_tag: Some("platform".into()),
    };
    // Whole-second precision: TIMESTAMPTZ is microsecond-precision, but
    // `Utc::now()` can carry sub-microsecond residue that would make an exact
    // round-trip comparison flaky.
    let deliver_after = chrono::DateTime::from_timestamp(chrono::Utc::now().timestamp() - 3_600, 0)
        .expect("valid timestamp");
    store
        .try_enqueue_notification(
            &event,
            "v3.0.0",
            OutboxMeta {
                deliver_after: Some(deliver_after),
                ..OutboxMeta::default()
            },
        )
        .expect("enqueue")
        .expect("row");

    let claimed = store.claim_outbox_batch(10, 300).expect("claim");
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].deliver_after, Some(deliver_after));
    assert_eq!(claimed[0].routing_tag.as_deref(), Some("platform"));
}

#[test]
fn requeue_dead_outbox_should_revive_exhausted_rows() {
    let Some(store) = open_store() else {
        return;
    };
    let event = Event {
        source_id: "github:org/app".into(),
        source_kind: "GitHub".into(),
        title: "app v9".into(),
        body: "notes".into(),
        url: None,
        routing_tag: None,
    };
    let outcome = store
        .try_enqueue_notification(&event, "v9.9.9", OutboxMeta::default())
        .expect("enqueue")
        .expect("row");

    // Exhaust delivery retries so the row is marked `dead`.
    for _ in 0..crate::store::OUTBOX_MAX_ATTEMPTS {
        store.fail_notification(outcome.id, "boom").expect("fail");
    }
    assert!(store.claim_outbox_batch(10, 300).expect("claim").is_empty());

    // Requeue revives it (attempts reset → claimable again).
    assert_eq!(store.requeue_dead_outbox().expect("requeue"), 1);
    let claimed = store.claim_outbox_batch(10, 300).expect("claim");
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].id, outcome.id);
}

#[test]
fn abandon_retryable_sinks_should_dead_letter_orphans_only() {
    let Some(store) = open_store() else {
        return;
    };
    let event = Event {
        source_id: "github:org/app".into(),
        source_kind: "GitHub".into(),
        title: "app v10".into(),
        body: "notes".into(),
        url: None,
        routing_tag: Some("platform".into()),
    };
    let outcome = store
        .try_enqueue_notification(&event, "v10.0.0", OutboxMeta::default())
        .expect("enqueue")
        .expect("row");

    store
        .ensure_sink_deliveries(outcome.id, &[(0, "webhook"), (1, "smtp")])
        .expect("ensure");
    assert_eq!(
        store
            .list_pending_sink_indices(outcome.id)
            .expect("pending"),
        vec![0, 1]
    );

    let abandoned = store
        .abandon_retryable_sinks(outcome.id, &[1], "routing drift")
        .expect("abandon");
    assert_eq!(abandoned, 1);
    assert_eq!(
        store
            .list_pending_sink_indices(outcome.id)
            .expect("pending"),
        vec![0]
    );

    let abandoned_all = store
        .abandon_retryable_sinks(outcome.id, &[], "no match")
        .expect("abandon all");
    assert_eq!(abandoned_all, 1);
    assert!(store
        .list_pending_sink_indices(outcome.id)
        .expect("pending")
        .is_empty());
}

#[test]
fn poller_lease_should_be_exclusive_per_database() {
    let Some(store_a) = open_store() else {
        return;
    };
    // Second pool against the same DB (open_test truncates — open a sibling pool).
    let url = std::env::var("XRELEASE_TEST_POSTGRES_URL")
        .unwrap_or_else(|_| "postgres://xrelease:xrelease@127.0.0.1:5432/xrelease_test".to_owned());
    let config = crate::config::DatabaseConfig {
        postgres_url: url,
        max_connections: Some(4),
        connect_timeout_secs: Some(crate::store::DEFAULT_CONNECT_TIMEOUT_SECS),
        ..Default::default()
    };
    let store_b = match Store::open_from_config(&config) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("skipping poller lease test (postgres unavailable): {err}");
            return;
        }
    };

    let lease_a = store_a
        .try_acquire_poller_lease()
        .expect("first poller should acquire");
    let busy = store_b.try_acquire_poller_lease();
    assert!(
        matches!(busy, Err(crate::error::StoreError::PollerBusy)),
        "second poller must fail while first holds lease"
    );
    drop(lease_a);
    let _lease_b = store_b
        .try_acquire_poller_lease()
        .expect("lease should be free after first poller drops");
}

#[test]
fn open_should_stamp_schema_meta_baseline() {
    let Some(store) = open_store() else {
        return;
    };
    let version = store.schema_version().expect("schema_meta");
    assert_eq!(version, 1, "greenfield baseline is schema version 1");
}

#[test]
fn app_secret_should_upsert_resolve_and_delete() {
    let Some(store) = open_store() else {
        return;
    };
    let allow = crate::crypto::ledger::ALLOW_PLAINTEXT_ENV;
    let prev_allow = std::env::var(allow).ok();
    std::env::set_var(allow, "1");

    let name = "XRELEASE_UI_N_99_STORE_TEST";
    let write = crate::config::SecretWrite {
        name: name.to_owned(),
        value: "secret-value".into(),
    };
    store
        .upsert_app_secrets(&[write])
        .expect("upsert app_secret");
    assert_eq!(
        crate::config::vault_get(name).as_deref(),
        Some("secret-value")
    );
    assert_eq!(
        crate::config::env_token(name).as_deref(),
        Some("secret-value")
    );

    let deleted = store
        .delete_app_secrets(&[name.to_owned()])
        .expect("delete app_secret");
    assert_eq!(deleted, 1);
    assert!(crate::config::vault_get(name).is_none());

    match prev_allow {
        Some(value) => std::env::set_var(allow, value),
        None => std::env::remove_var(allow),
    }
}
