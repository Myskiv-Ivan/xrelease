//! Integration tests for the PostgreSQL store (require a running Postgres).

use crate::advisory::{Advisory, Severity};
use crate::notify::Event;
use crate::store::{OutboxMeta, SeenUpsert, Store};

fn advisory(id: &str, severity: Option<Severity>) -> Advisory {
    Advisory {
        id: id.to_owned(),
        display_id: id.to_owned(),
        summary: Some(format!("{id} summary")),
        severity,
        cvss_vector: None,
        url: Some(format!("https://osv.dev/vulnerability/{id}")),
    }
}

fn seen<'a>(identity: &'a str, content_digest: Option<&'a str>) -> SeenUpsert<'a> {
    SeenUpsert {
        identity,
        display_tag: None,
        content_digest,
        published_at: None,
        url: None,
    }
}

use crate::store::test_db;

/// Open the shared test database, serialized against every other DB-using test
/// in this binary (see [`test_db`]).
fn open_store() -> Option<test_db::TestStore> {
    test_db::open()
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

/// Regression: a breaker-open skip makes no network call, so it must defer the
/// row without spending its retry budget.
///
/// One sink's breaker is shared by every row targeting it, so a backlog behind a
/// down sink used to burn each queued row's *parent* `attempts` while their sink
/// ledgers stayed untouched. At the cap `claim_outbox_batch` (`attempts < max`)
/// stopped returning the row while its status was still `failed` — never
/// delivered, never dead-lettered, and unreachable by `requeue_dead_outbox`.
#[test]
fn defer_delivery_should_postpone_claim_without_spending_attempts() {
    let Some(store) = open_store() else {
        return;
    };
    let event = Event {
        source_id: "github:org/deferral".into(),
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

    // Same deferral effect as `apply_delivery_backoff`…
    store
        .defer_delivery(outcome.id, 3_600.0)
        .expect("defer delivery");
    assert!(
        store.claim_outbox_batch(10, 10).expect("claim").is_empty(),
        "deferred row must not be claimable before the window passes"
    );

    // …but the retry budget is untouched, however many times it repeats.
    for _ in 0..(crate::store::OUTBOX_MAX_ATTEMPTS + 5) {
        store
            .defer_delivery(outcome.id, 3_600.0)
            .expect("defer delivery");
    }
    let entries = store.list_outbox_entries(10).expect("list");
    let row = entries
        .iter()
        .find(|e| e.id == outcome.id)
        .expect("row present");
    assert_eq!(
        row.attempts, 0,
        "a breaker deferral is not an attempt and must not consume the budget"
    );

    // Proof it is still deliverable once the deferral window is cleared.
    store
        .release_outbox_lease(outcome.id)
        .expect("release lease");
    let claimed = store.claim_outbox_batch(10, 300).expect("claim");
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].id, outcome.id);
}

/// Regression: a row whose parent budget runs out while its sink ledger still
/// has retryable sinks stops being claimable but keeps status `failed`.
///
/// It is then invisible (not counted as dead-lettered) and unrecoverable, since
/// `requeue_dead_outbox` only revives `dead`. `finalize_exhausted_outbox` is the
/// reconciliation the flush loop now runs every cycle.
#[test]
fn finalize_exhausted_outbox_should_rescue_unclaimable_failed_rows() {
    let Some(store) = open_store() else {
        return;
    };
    let event = Event {
        source_id: "github:org/stranded".into(),
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

    // Spend the parent budget without ever dead-lettering a sink: exactly the
    // shape `apply_delivery_backoff` produces on the per-sink failure path.
    for _ in 0..crate::store::OUTBOX_MAX_ATTEMPTS {
        store
            .apply_delivery_backoff(outcome.id, 0.0)
            .expect("advance attempts");
    }
    assert!(
        store.claim_outbox_batch(10, 300).expect("claim").is_empty(),
        "a row at the attempt cap is no longer claimable"
    );

    // Reconciliation promotes it to `dead` so it is visible and recoverable.
    assert_eq!(
        store.finalize_exhausted_outbox().expect("finalize"),
        1,
        "exhausted row must be promoted to dead"
    );
    assert_eq!(store.outbox_counts().expect("counts").dead, 1);
    assert_eq!(store.requeue_dead_outbox().expect("requeue"), 1);
    let claimed = store.claim_outbox_batch(10, 300).expect("claim");
    assert_eq!(claimed.len(), 1, "requeue makes the row deliverable again");
    assert_eq!(claimed[0].id, outcome.id);
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
    assert_eq!(
        version, 2,
        "open must run every migration (v2 = sentinel publish-date cleanup)"
    );
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

// ── OIDC provisioning + SSO email linking ───────────────────────────────────
//
// These cover the auth-critical branches of `upsert_oidc_user` /
// `set_user_oidc_link_email`: who may be provisioned, whose email may adopt an
// existing account, and when a previously bound subject must be dropped.

use crate::store::{AppUserInsert, AppUserUpsertOidc};

/// Unique per call so parallel/repeat runs never collide on the shared test DB.
fn unique(prefix: &str) -> String {
    format!(
        "{prefix}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    )
}

fn local_user(store: &Store, email: &str) -> i64 {
    store
        .insert_user(&AppUserInsert {
            username: Some(&unique("user")),
            password_hash: Some("x"),
            oidc_sub: None,
            email: Some(email),
            display_name: None,
            role: "viewer",
            auth_source: "local",
        })
        .expect("insert local user")
        .id
}

fn oidc_upsert<'a>(
    sub: &'a str,
    email: Option<&'a str>,
    link_by_email: bool,
    allow_create: bool,
) -> AppUserUpsertOidc<'a> {
    AppUserUpsertOidc {
        oidc_sub: sub,
        email,
        display_name: None,
        role: "viewer",
        link_by_email,
        allow_create,
    }
}

#[test]
fn upsert_oidc_user_should_refuse_unknown_subject_when_create_disabled() {
    let Some(store) = open_store() else {
        return;
    };
    let sub = unique("sub-unknown");
    let email = format!("{}@example.com", unique("nobody"));

    let created = store
        .upsert_oidc_user(&oidc_upsert(&sub, Some(&email), true, false))
        .expect("upsert");

    assert!(
        created.is_none(),
        "unknown subject must not be provisioned while auto_create_users is off"
    );
    assert!(
        store.get_user_by_oidc_sub(&sub).expect("lookup").is_none(),
        "no row may be written for a refused sign-in"
    );
}

#[test]
fn upsert_oidc_user_should_create_unknown_subject_when_create_enabled() {
    let Some(store) = open_store() else {
        return;
    };
    let sub = unique("sub-new");

    let created = store
        .upsert_oidc_user(&oidc_upsert(&sub, None, false, true))
        .expect("upsert")
        .expect("row provisioned");

    assert_eq!(created.oidc_sub.as_deref(), Some(sub.as_str()));
    assert_eq!(created.auth_source, "oidc");
}

#[test]
fn upsert_oidc_user_should_adopt_local_account_on_verified_email() {
    let Some(store) = open_store() else {
        return;
    };
    let email = format!("{}@example.com", unique("verified"));
    let local_id = local_user(&store, &email);
    let sub = unique("sub-verified");

    // Creation is off: the sign-in may only succeed via the email link.
    let linked = store
        .upsert_oidc_user(&oidc_upsert(&sub, Some(&email), true, false))
        .expect("upsert")
        .expect("adopted the pre-created local account");

    assert_eq!(
        linked.id, local_id,
        "must reuse the local row, not fork one"
    );
    assert_eq!(linked.oidc_sub.as_deref(), Some(sub.as_str()));
    assert_eq!(
        linked.auth_source, "local",
        "adoption must keep password login working"
    );
}

#[test]
fn upsert_oidc_user_should_not_adopt_local_account_on_unverified_email() {
    let Some(store) = open_store() else {
        return;
    };
    let email = format!("{}@example.com", unique("unverified"));
    let local_id = local_user(&store, &email);
    let sub = unique("sub-unverified");

    // link_by_email=false models a token without `email_verified`. Account
    // takeover guard: an unproven address must not inherit the local account.
    let result = store
        .upsert_oidc_user(&oidc_upsert(&sub, Some(&email), false, false))
        .expect("upsert");

    assert!(
        result.is_none(),
        "unverified email must not adopt an existing account"
    );
    let untouched = store
        .get_user_by_id(local_id)
        .expect("lookup")
        .expect("local row still there");
    assert!(
        untouched.oidc_sub.is_none(),
        "local account must not have been bound to the unproven subject"
    );
}

#[test]
fn set_user_oidc_link_email_should_clear_subject_when_address_changes() {
    let Some(store) = open_store() else {
        return;
    };
    let email = format!("{}@example.com", unique("rotate"));
    let user_id = local_user(&store, &email);
    let sub = unique("sub-rotate");

    store
        .upsert_oidc_user(&oidc_upsert(&sub, Some(&email), true, false))
        .expect("upsert")
        .expect("linked");

    // Re-pointing the account at a different person must revoke the old
    // identity, otherwise they keep signing in to it.
    let moved = store
        .set_user_oidc_link_email(user_id, Some(&format!("{}@example.com", unique("other"))))
        .expect("relink");

    assert!(
        moved.oidc_sub.is_none(),
        "changing the SSO email must drop the previously bound subject"
    );
    assert!(
        store.get_user_by_oidc_sub(&sub).expect("lookup").is_none(),
        "the old subject must no longer resolve to any account"
    );
}

#[test]
fn set_user_oidc_link_email_should_reject_address_owned_by_another_user() {
    let Some(store) = open_store() else {
        return;
    };
    let taken = format!("{}@example.com", unique("taken"));
    local_user(&store, &taken);
    let other_id = local_user(&store, &format!("{}@example.com", unique("other")));

    let err = store
        .set_user_oidc_link_email(other_id, Some(&taken))
        .expect_err("must not let two accounts claim one SSO address");

    assert!(
        err.to_string().contains("already used by user"),
        "unexpected error: {err}"
    );
}

#[test]
fn record_advisories_should_round_trip_severity_and_optional_fields() {
    let Some(store) = open_store() else {
        return;
    };
    let ecosystem = unique("crates.io");
    store
        .record_advisories(
            &ecosystem,
            "serde",
            "1.0.0",
            &[
                advisory("RUSTSEC-2024-0001", Some(Severity::Critical)),
                advisory("RUSTSEC-2024-0002", None),
            ],
        )
        .expect("record");

    let found = store
        .advisories_for_versions(&ecosystem, "serde", &["1.0.0"])
        .expect("read");
    let mut versions = found.get("1.0.0").cloned().unwrap_or_default();
    versions.sort_by(|a, b| a.id.cmp(&b.id));

    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].id, "RUSTSEC-2024-0001");
    assert_eq!(versions[0].severity, Some(Severity::Critical));
    assert_eq!(
        versions[0].summary.as_deref(),
        Some("RUSTSEC-2024-0001 summary")
    );
    assert_eq!(
        versions[0].url.as_deref(),
        Some("https://osv.dev/vulnerability/RUSTSEC-2024-0001")
    );
    assert_eq!(
        versions[1].severity, None,
        "an advisory with no stated severity must not resurrect one on read"
    );
}

#[test]
fn record_advisories_should_replace_the_prior_set_not_accumulate_it() {
    let Some(store) = open_store() else {
        return;
    };
    let ecosystem = unique("npm");
    store
        .record_advisories(
            &ecosystem,
            "left-pad",
            "1.3.0",
            &[advisory("GHSA-old", None)],
        )
        .expect("first write");

    // A later lookup can legitimately return a different set (a false positive
    // withdrawn, a correction) — the second write must fully replace the first,
    // not merge with it.
    store
        .record_advisories(
            &ecosystem,
            "left-pad",
            "1.3.0",
            &[advisory("GHSA-new", Some(Severity::Low))],
        )
        .expect("second write");

    let found = store
        .advisories_for_versions(&ecosystem, "left-pad", &["1.3.0"])
        .expect("read");
    let versions = found.get("1.3.0").expect("version present");
    assert_eq!(
        versions.len(),
        1,
        "stale entry must not survive the replace"
    );
    assert_eq!(versions[0].id, "GHSA-new");
}

#[test]
fn record_advisories_with_empty_slice_should_clear_existing_rows() {
    let Some(store) = open_store() else {
        return;
    };
    let ecosystem = unique("pypi");
    store
        .record_advisories(
            &ecosystem,
            "requests",
            "2.32.0",
            &[advisory("PYSEC-1", None)],
        )
        .expect("seed");
    store
        .record_advisories(&ecosystem, "requests", "2.32.0", &[])
        .expect("clear");

    let found = store
        .advisories_for_versions(&ecosystem, "requests", &["2.32.0"])
        .expect("read");
    assert!(
        !found.contains_key("2.32.0"),
        "a cleared version must be absent, not present with an empty Vec"
    );
}

#[test]
fn advisories_for_versions_should_batch_several_versions_in_one_call() {
    let Some(store) = open_store() else {
        return;
    };
    let ecosystem = unique("maven");
    store
        .record_advisories(
            &ecosystem,
            "com.example:lib",
            "1.0.0",
            &[advisory("GHSA-a", None)],
        )
        .expect("v1");
    store
        .record_advisories(
            &ecosystem,
            "com.example:lib",
            "2.0.0",
            &[advisory("GHSA-b", None), advisory("GHSA-c", None)],
        )
        .expect("v2");

    // "3.0.0" was never written — the batch call must still succeed and simply
    // omit it, matching a source whose latest seen release has no advisories.
    let found = store
        .advisories_for_versions(&ecosystem, "com.example:lib", &["1.0.0", "2.0.0", "3.0.0"])
        .expect("read");

    assert_eq!(
        found.len(),
        2,
        "undated version must be absent, not an error"
    );
    assert_eq!(found["1.0.0"].len(), 1);
    assert_eq!(found["2.0.0"].len(), 2);
}

#[test]
fn advisories_for_versions_should_scope_by_package_within_one_ecosystem() {
    let Some(store) = open_store() else {
        return;
    };
    let ecosystem = unique("nuget");
    store
        .record_advisories(&ecosystem, "PackageA", "1.0.0", &[advisory("GHSA-a", None)])
        .expect("package a");
    store
        .record_advisories(&ecosystem, "PackageB", "1.0.0", &[advisory("GHSA-b", None)])
        .expect("package b");

    let found = store
        .advisories_for_versions(&ecosystem, "PackageA", &["1.0.0"])
        .expect("read");
    assert_eq!(found["1.0.0"].len(), 1);
    assert_eq!(
        found["1.0.0"][0].id, "GHSA-a",
        "a same-version row from a different package must not leak in"
    );
}

#[test]
fn prune_with_zero_advisories_after_days_should_not_touch_release_advisory() {
    let Some(store) = open_store() else {
        return;
    };
    let ecosystem = unique("hex");
    store
        .record_advisories(
            &ecosystem,
            "phoenix",
            "1.7.0",
            &[advisory("GHSA-fresh", None)],
        )
        .expect("seed");

    let report = store.prune(0, 0, 0, 0).expect("prune");
    assert_eq!(
        report.advisories_deleted, 0,
        "advisories_after_days = 0 must disable that retention rule entirely"
    );

    let found = store
        .advisories_for_versions(&ecosystem, "phoenix", &["1.7.0"])
        .expect("read");
    assert_eq!(found["1.7.0"].len(), 1, "row must survive a disabled prune");
}

#[test]
fn prune_with_advisories_retention_should_not_delete_a_row_just_written() {
    let Some(store) = open_store() else {
        return;
    };
    let ecosystem = unique("rubygems");
    store
        .record_advisories(
            &ecosystem,
            "rails",
            "7.1.0",
            &[advisory("GHSA-fresh2", None)],
        )
        .expect("seed");

    // A large retention window must not delete a row `fetched_at = now()`.
    let report = store.prune(0, 0, 0, 365).expect("prune");
    assert_eq!(report.advisories_deleted, 0);

    let found = store
        .advisories_for_versions(&ecosystem, "rails", &["7.1.0"])
        .expect("read");
    assert_eq!(found["7.1.0"].len(), 1);
}

/// The whole point of `advisory_check`: a version OSV confirmed *clean* leaves
/// no `release_advisory` row, so without this ledger it is indistinguishable
/// from one that was never looked at — and the detail-page backfill would keep
/// re-querying it forever instead of moving on to versions it has not seen.
#[test]
fn checked_versions_should_remember_a_version_with_no_findings() {
    let Some(store) = open_store() else {
        return;
    };
    let ecosystem = unique("crates.io");
    store
        .record_advisories(&ecosystem, "serde", "1.0.0", &[])
        .expect("clean findings");
    store
        .record_advisory_check(&ecosystem, "serde", "1.0.0")
        .expect("mark checked");

    let found = store
        .advisories_for_versions(&ecosystem, "serde", &["1.0.0"])
        .expect("read findings");
    assert!(
        !found.contains_key("1.0.0"),
        "a clean version has no findings rows"
    );

    let checked = store
        .checked_versions(&ecosystem, "serde", &["1.0.0"])
        .expect("read checks");
    assert!(
        checked.contains("1.0.0"),
        "…but it is still recorded as checked"
    );
}

#[test]
fn checked_versions_should_omit_versions_never_looked_up() {
    let Some(store) = open_store() else {
        return;
    };
    let ecosystem = unique("npm");
    store
        .record_advisory_check(&ecosystem, "axios", "1.7.0")
        .expect("mark checked");

    let checked = store
        .checked_versions(&ecosystem, "axios", &["1.7.0", "1.8.0"])
        .expect("read checks");
    assert!(checked.contains("1.7.0"));
    assert!(
        !checked.contains("1.8.0"),
        "an unchecked version must stay a backfill candidate"
    );
}

#[test]
fn checked_versions_should_scope_by_package_within_one_ecosystem() {
    let Some(store) = open_store() else {
        return;
    };
    let ecosystem = unique("pypi");
    store
        .record_advisory_check(&ecosystem, "requests", "2.32.0")
        .expect("mark checked");

    let other = store
        .checked_versions(&ecosystem, "urllib3", &["2.32.0"])
        .expect("read checks");
    assert!(
        other.is_empty(),
        "a same-version marker from a different package must not leak in"
    );
}

/// A registry listing one version twice must not fail the whole baseline.
///
/// `ON CONFLICT DO UPDATE` errors outright when a single statement touches the
/// same row twice, so the batched insert dedupes with `DISTINCT ON`. The
/// per-item loop it replaced tolerated duplicates for free.
#[test]
fn record_seen_batch_should_tolerate_a_duplicated_identity() {
    let Some(store) = open_store() else {
        return;
    };
    let source = unique("npm:dupe");
    store
        .record_seen_batch(
            &source,
            &[
                seen("1.0.0", None),
                seen("1.0.0", None),
                seen("1.1.0", None),
            ],
        )
        .expect("duplicate identity must not fail the baseline");

    let listed = store.list_seen_releases(&source, 10).expect("list");
    assert_eq!(listed.len(), 2, "the duplicate collapses to one row");
}

#[test]
fn enrich_seen_metadata_should_fill_missing_publication_details() {
    let Some(store) = open_store() else {
        return;
    };
    let source = unique("github:org/enrich");
    store
        .record_seen_batch(&source, &[seen("v1.0.0", None)])
        .expect("seed seen");

    let published = chrono::DateTime::parse_from_rfc3339("2026-01-02T03:04:05Z")
        .expect("timestamp")
        .with_timezone(&chrono::Utc);
    let release = crate::model::Release::new("v1.0.0")
        .with_url(Some("https://example.test/v1.0.0".to_owned()))
        .with_published(Some(published));
    store
        .enrich_seen_metadata(&source, &[&release])
        .expect("enrich");

    let listed = store.list_seen_releases(&source, 10).expect("list");
    assert_eq!(
        listed[0].url.as_deref(),
        Some("https://example.test/v1.0.0")
    );
    assert!(listed[0].published_at.is_some());
}

/// `COALESCE(s.col, t.col)` — upstream re-reporting a release must never
/// rewrite the timestamp or URL already recorded for it.
#[test]
fn enrich_seen_metadata_should_not_overwrite_recorded_values() {
    let Some(store) = open_store() else {
        return;
    };
    let source = unique("github:org/stable");
    let original = chrono::DateTime::parse_from_rfc3339("2020-01-01T00:00:00Z")
        .expect("timestamp")
        .with_timezone(&chrono::Utc);
    store
        .record_seen_batch(
            &source,
            &[SeenUpsert {
                identity: "v1.0.0",
                display_tag: None,
                content_digest: None,
                published_at: Some(original),
                url: Some("https://example.test/original"),
            }],
        )
        .expect("seed seen");

    let later = chrono::DateTime::parse_from_rfc3339("2026-06-06T00:00:00Z")
        .expect("timestamp")
        .with_timezone(&chrono::Utc);
    let release = crate::model::Release::new("v1.0.0")
        .with_url(Some("https://example.test/rewritten".to_owned()))
        .with_published(Some(later));
    store
        .enrich_seen_metadata(&source, &[&release])
        .expect("enrich");

    let listed = store.list_seen_releases(&source, 10).expect("list");
    assert_eq!(
        listed[0].url.as_deref(),
        Some("https://example.test/original")
    );
    assert!(
        listed[0]
            .published_at
            .as_deref()
            .is_some_and(|at| at.starts_with("2020-01-01")),
        "the first-recorded publication date wins: {:?}",
        listed[0].published_at
    );
}

/// The batched `UPDATE … FROM unnest(…)` joins on identity, so the `source_id`
/// predicate is the only thing keeping two sources that saw the same tag apart.
#[test]
fn enrich_seen_metadata_should_only_touch_its_own_source() {
    let Some(store) = open_store() else {
        return;
    };
    let mine = unique("github:org/mine");
    let theirs = unique("github:org/theirs");
    store
        .record_seen_batch(&mine, &[seen("v1.0.0", None)])
        .expect("seed mine");
    store
        .record_seen_batch(&theirs, &[seen("v1.0.0", None)])
        .expect("seed theirs");

    let release =
        crate::model::Release::new("v1.0.0").with_url(Some("https://example.test/mine".to_owned()));
    store
        .enrich_seen_metadata(&mine, &[&release])
        .expect("enrich");

    let other = store.list_seen_releases(&theirs, 10).expect("list");
    assert!(
        other[0].url.is_none(),
        "another source's identical tag must be untouched"
    );
}

/// The sources-list page anchors the tag and the date on the *same* release, so
/// the batched pick must follow `source_state.latest_release_tag` — not simply
/// the newest timestamp, which a backported patch release inverts.
#[test]
fn latest_seen_by_source_should_follow_the_stored_latest_tag() {
    let Some(store) = open_store() else {
        return;
    };
    let source = unique("github:org/anchor");
    store
        .record_seen_batch(&source, &[seen("v2.0.0", None), seen("v1.9.1", None)])
        .expect("seed seen");
    store
        .set_latest_release_tag(&source, "v2.0.0")
        .expect("set latest");

    let latest = store.latest_seen_by_source().expect("batch read");
    assert_eq!(latest[&source].tag, "v2.0.0");
}

#[test]
fn latest_seen_by_source_should_return_one_entry_per_source() {
    let Some(store) = open_store() else {
        return;
    };
    let first = unique("github:org/one");
    let second = unique("github:org/two");
    store
        .record_seen_batch(&first, &[seen("v1.0.0", None), seen("v1.1.0", None)])
        .expect("seed first");
    store
        .record_seen_batch(&second, &[seen("v3.0.0", None)])
        .expect("seed second");

    let latest = store.latest_seen_by_source().expect("batch read");
    // One row each — the whole point is not shipping a source's catalogue to a
    // page that renders a single release.
    assert!(latest.contains_key(&first));
    assert_eq!(latest[&second].tag, "v3.0.0");
}

#[test]
fn latest_seen_by_source_should_fall_back_to_the_newest_row_without_a_stored_tag() {
    let Some(store) = open_store() else {
        return;
    };
    let source = unique("github:org/untagged");
    store
        .record_seen_batch(&source, &[seen("v1.0.0", None)])
        .expect("seed seen");

    let latest = store.latest_seen_by_source().expect("batch read");
    assert_eq!(
        latest[&source].tag, "v1.0.0",
        "a source polled before `latest_release_tag` was stored must still show a release"
    );
}

#[test]
fn latest_seen_by_source_should_omit_sources_with_nothing_seen() {
    let Some(store) = open_store() else {
        return;
    };
    let source = unique("github:org/empty");
    store.touch_polled(&source).expect("touch");

    let latest = store.latest_seen_by_source().expect("batch read");
    assert!(
        !latest.contains_key(&source),
        "a polled-but-empty source has no release to anchor on"
    );
}

/// The background sweep's work queue: everything seen for a source that OSV
/// has never been asked about.
#[test]
fn unchecked_seen_versions_should_return_only_never_checked_releases() {
    let Some(store) = open_store() else {
        return;
    };
    let ecosystem = unique("cargo-sweep");
    store
        .record_seen_batch(
            "cargo:serde",
            &[
                seen("1.0.0", None),
                seen("1.1.0", None),
                seen("1.2.0", None),
            ],
        )
        .expect("seed seen");
    store
        .record_advisory_check(&ecosystem, "serde", "1.1.0")
        .expect("mark checked");

    let pending = store
        .unchecked_seen_versions("cargo:serde", &ecosystem, "serde", 10)
        .expect("work queue");
    assert_eq!(pending.len(), 2);
    assert!(!pending.contains(&"1.1.0".to_owned()));
}

#[test]
fn unchecked_seen_versions_should_be_empty_once_a_source_has_converged() {
    let Some(store) = open_store() else {
        return;
    };
    let ecosystem = unique("npm-sweep");
    store
        .record_seen_batch("npm:axios", &[seen("1.7.0", None)])
        .expect("seed seen");
    store
        .record_advisory_check(&ecosystem, "axios", "1.7.0")
        .expect("mark checked");

    assert!(
        store
            .unchecked_seen_versions("npm:axios", &ecosystem, "axios", 10)
            .expect("work queue")
            .is_empty(),
        "a converged source must cost the sweep nothing"
    );
}

#[test]
fn unchecked_seen_versions_should_respect_the_batch_limit() {
    let Some(store) = open_store() else {
        return;
    };
    let ecosystem = unique("pypi-sweep");
    let versions: Vec<String> = (0..8).map(|n| format!("2.{n}.0")).collect();
    let upserts: Vec<SeenUpsert<'_>> = versions.iter().map(|v| seen(v, None)).collect();
    store
        .record_seen_batch("pypi:requests", &upserts)
        .expect("seed seen");

    let pending = store
        .unchecked_seen_versions("pypi:requests", &ecosystem, "requests", 3)
        .expect("work queue");
    assert_eq!(
        pending.len(),
        3,
        "the sweep must not pull a source's whole history in one round"
    );
}

#[test]
fn unchecked_seen_versions_should_not_query_for_a_zero_batch() {
    let Some(store) = open_store() else {
        return;
    };
    let ecosystem = unique("hex-sweep");
    store
        .record_seen_batch("hex:phoenix", &[seen("1.7.0", None)])
        .expect("seed seen");

    assert!(store
        .unchecked_seen_versions("hex:phoenix", &ecosystem, "phoenix", 0)
        .expect("work queue")
        .is_empty());
}

#[test]
fn record_advisory_check_should_be_idempotent() {
    let Some(store) = open_store() else {
        return;
    };
    let ecosystem = unique("maven");
    // Two writers can race for one coordinate (a delivery and a page backfill);
    // the second must refresh the row, not fail on the primary key.
    for _ in 0..2 {
        store
            .record_advisory_check(&ecosystem, "com.example:lib", "1.0.0")
            .expect("mark checked");
    }

    let checked = store
        .checked_versions(&ecosystem, "com.example:lib", &["1.0.0"])
        .expect("read checks");
    assert_eq!(checked.len(), 1);
}

#[test]
fn checked_versions_should_be_empty_without_a_query_for_no_versions() {
    let Some(store) = open_store() else {
        return;
    };
    let ecosystem = unique("packagist");
    let checked = store
        .checked_versions(&ecosystem, "monolog/monolog", &[])
        .expect("read checks");
    assert!(checked.is_empty());
}

#[test]
fn prune_should_expire_check_markers_on_the_advisory_retention_window() {
    let Some(store) = open_store() else {
        return;
    };
    let ecosystem = unique("nuget");
    store
        .record_advisory_check(&ecosystem, "Newtonsoft.Json", "13.0.3")
        .expect("mark checked");

    // Retention is shared with the findings: a fresh marker survives, so the
    // backfill does not immediately re-query what it just confirmed.
    let report = store.prune(0, 0, 0, 365).expect("prune");
    assert_eq!(report.advisory_checks_deleted, 0);
    assert!(store
        .checked_versions(&ecosystem, "Newtonsoft.Json", &["13.0.3"])
        .expect("read checks")
        .contains("13.0.3"));
}

#[test]
fn prune_with_zero_advisories_after_days_should_not_touch_check_markers() {
    let Some(store) = open_store() else {
        return;
    };
    let ecosystem = unique("hex");
    store
        .record_advisory_check(&ecosystem, "phoenix", "1.7.0")
        .expect("mark checked");

    let report = store.prune(0, 0, 0, 0).expect("prune");
    assert_eq!(report.advisory_checks_deleted, 0);
    assert!(store
        .checked_versions(&ecosystem, "phoenix", &["1.7.0"])
        .expect("read checks")
        .contains("1.7.0"));
}
