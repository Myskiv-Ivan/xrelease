//! The single-source poll → diff → notify step.
//!
//! This module is deliberately free of timing and concurrency concerns *for the
//! watch loop* (those live in [`crate::scheduler`]) so the newness logic can be
//! unit-tested against an in-memory store with hand-built releases. Outbox flush
//! concurrency is intentional here: claimed rows are independent and share only
//! the Engine (leases already serialize duplicate delivery of one row).

mod filter;
mod outbox;
mod poll;
mod schedule;
mod watch;

pub use filter::Filter;
pub use outbox::flush_notification_outbox;
pub use poll::{deliver_new_releases, poll_once};
pub use schedule::NotifySchedule;
pub use watch::Watch;

/// Default parallel deliveries per outbox flush when config omits the knob.
pub const DEFAULT_OUTBOX_FLUSH_CONCURRENCY: usize = 8;

/// Default cap on exponential retry backoff when config omits the knob.
pub const DEFAULT_OUTBOX_RETRY_BACKOFF_MAX_SECS: u32 = 3_600;

// Crate-visible helpers shared across submodules / tests.
pub(crate) use outbox::{attempt_notification_delivery, persist_advisory_result, DeliveryContext};
#[cfg(test)]
pub(crate) use outbox::{backoff_secs, build_digest_event, partition_for_digest, DeliveryUnit};
#[cfg(test)]
pub(crate) use poll::select_for_delivery;

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};

    use crate::model::Release;
    use crate::store::OutboxRecord;

    fn release(id: &str) -> Release {
        Release::new(id)
    }

    fn outbox_row(
        id: i64,
        source_id: &str,
        routing_tag: Option<&str>,
        deliver_after: Option<DateTime<Utc>>,
    ) -> OutboxRecord {
        OutboxRecord {
            id,
            source_id: source_id.to_owned(),
            identity: format!("{source_id}-{id}"),
            content_digest: None,
            display_tag: None,
            published_at: None,
            title: format!("{source_id}: new release v{id}.0.0"),
            body: "changelog".to_owned(),
            url: Some(format!("https://example.test/{source_id}/v{id}.0.0")),
            routing_tag: routing_tag.map(ToOwned::to_owned),
            source_kind: "GitHub".to_owned(),
            attempts: 0,
            deliver_after,
        }
    }

    #[test]
    fn partition_for_digest_should_group_rows_sharing_moment_and_tag() {
        let at = Utc::now();
        let wave = vec![
            outbox_row(1, "a", Some("platform"), Some(at)),
            outbox_row(2, "b", Some("platform"), Some(at)),
            outbox_row(3, "c", Some("platform"), Some(at)),
        ];
        let units = partition_for_digest(wave);
        assert_eq!(units.len(), 1);
        match &units[0] {
            DeliveryUnit::Digest(group) => assert_eq!(group.len(), 3),
            DeliveryUnit::Single(_) => panic!("expected a digest group"),
        }
    }

    #[test]
    fn partition_for_digest_should_keep_different_tags_separate() {
        let at = Utc::now();
        let wave = vec![
            outbox_row(1, "a", Some("platform"), Some(at)),
            outbox_row(2, "b", Some("security"), Some(at)),
        ];
        let units = partition_for_digest(wave);
        assert_eq!(units.len(), 2);
        assert!(units
            .iter()
            .all(|unit| matches!(unit, DeliveryUnit::Single(_))));
    }

    #[test]
    fn partition_for_digest_should_keep_different_moments_separate() {
        let now = Utc::now();
        let wave = vec![
            outbox_row(1, "a", Some("platform"), Some(now)),
            outbox_row(
                2,
                "b",
                Some("platform"),
                Some(now + chrono::Duration::hours(1)),
            ),
        ];
        let units = partition_for_digest(wave);
        assert_eq!(units.len(), 2);
        assert!(units
            .iter()
            .all(|unit| matches!(unit, DeliveryUnit::Single(_))));
    }

    #[test]
    fn partition_for_digest_should_never_group_immediate_rows() {
        let wave = vec![
            outbox_row(1, "a", Some("platform"), None),
            outbox_row(2, "b", Some("platform"), None),
        ];
        let units = partition_for_digest(wave);
        // Immediate rows (no `deliver_after`) always deliver individually,
        // even when they would otherwise share a grouping key.
        assert_eq!(units.len(), 2);
        assert!(units
            .iter()
            .all(|unit| matches!(unit, DeliveryUnit::Single(_))));
    }

    #[test]
    fn partition_for_digest_should_not_wrap_a_lone_deferred_row_as_a_digest() {
        let wave = vec![outbox_row(1, "a", Some("platform"), Some(Utc::now()))];
        let units = partition_for_digest(wave);
        assert_eq!(units.len(), 1);
        assert!(matches!(units[0], DeliveryUnit::Single(_)));
    }

    #[test]
    fn build_digest_event_should_summarize_every_row() {
        let group = vec![
            outbox_row(1, "a", Some("platform"), Some(Utc::now())),
            outbox_row(2, "b", Some("platform"), Some(Utc::now())),
        ];
        let event = build_digest_event(&group);
        assert_eq!(event.title, "2 new releases");
        assert_eq!(event.routing_tag.as_deref(), Some("platform"));
        assert!(event.body.contains("a: new release v1.0.0"));
        assert!(event.body.contains("b: new release v2.0.0"));
        assert!(event.body.contains("https://example.test/a/v1.0.0"));
    }

    #[test]
    fn backoff_secs_first_failure_should_match_flush_cadence() {
        // Unaffected by backoff: same ~60s cadence as before it existed.
        assert_eq!(backoff_secs(1, 3_600), 60.0);
    }

    #[test]
    fn backoff_secs_should_double_per_attempt() {
        assert_eq!(backoff_secs(2, 3_600), 120.0);
        assert_eq!(backoff_secs(3, 3_600), 240.0);
        assert_eq!(backoff_secs(4, 3_600), 480.0);
    }

    #[test]
    fn backoff_secs_should_cap_at_max() {
        assert_eq!(backoff_secs(10, 3_600), 3_600.0);
        assert_eq!(backoff_secs(50, 3_600), 3_600.0);
    }

    #[test]
    fn backoff_secs_should_not_panic_on_zero_or_negative_attempts() {
        // Defensive: attempts should always be >= 1 by construction, but a
        // pure function must not panic on out-of-range input.
        assert_eq!(backoff_secs(0, 3_600), 60.0);
        assert_eq!(backoff_secs(-5, 3_600), 60.0);
    }

    #[test]
    fn select_for_delivery_should_return_nothing_on_first_poll() {
        let Some(store) = crate::store::test_db::open() else {
            return;
        };
        let releases = vec![release("v1.0.0"), release("v1.1.0")];
        let new = select_for_delivery("s1", &releases, true, &store).expect("select");
        assert!(new.is_empty());
    }

    #[test]
    fn select_for_delivery_should_return_only_unseen_after_baseline() {
        let Some(store) = crate::store::test_db::open() else {
            return;
        };
        let baseline = vec![release("v1.0.0")];
        select_for_delivery("s1", &baseline, true, &store).expect("baseline");

        let next = vec![release("v1.0.0"), release("v1.1.0")];
        let new = select_for_delivery("s1", &next, true, &store).expect("select");
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].id, "v1.1.0");
    }

    #[test]
    fn select_for_delivery_should_notify_on_body_update_when_not_excluded() {
        let Some(store) = crate::store::test_db::open() else {
            return;
        };
        let first = Release::new("v1.0.0").with_body(Some("initial".into()));
        select_for_delivery("s1", &[first], false, &store).expect("baseline");

        let updated = Release::new("v1.0.0").with_body(Some("changelog edited".into()));
        let releases = [updated];
        let deliverable = select_for_delivery("s1", &releases, false, &store).expect("select");
        assert_eq!(deliverable.len(), 1);
    }

    #[test]
    fn select_for_delivery_should_skip_body_update_when_excluded() {
        let Some(store) = crate::store::test_db::open() else {
            return;
        };
        let first = Release::new("v1.0.0").with_body(Some("initial".into()));
        select_for_delivery("s1", &[first], true, &store).expect("baseline");

        let updated = Release::new("v1.0.0").with_body(Some("changelog edited".into()));
        let releases = [updated];
        let deliverable = select_for_delivery("s1", &releases, true, &store).expect("select");
        assert!(deliverable.is_empty());
    }

    #[test]
    fn filter_should_reject_prereleases_by_default() {
        let filter = Filter::new(false, None, None).expect("filter");
        assert!(!filter.accepts(&Release::new("v1.0.0-rc1")));
    }

    #[test]
    fn filter_should_honor_pattern() {
        let filter = Filter::new(true, Some(r"^\d+\.\d+\.\d+$"), None).expect("filter");
        assert!(filter.accepts(&Release::new("1.2.3")));
        assert!(!filter.accepts(&Release::new("nightly")));
    }

    #[test]
    fn filter_should_honor_exclude_pattern() {
        let filter = Filter::new(true, None, Some(r"^nightly$")).expect("filter");
        assert!(filter.accepts(&Release::new("v1.2.3")));
        assert!(!filter.accepts(&Release::new("nightly")));
    }

    #[test]
    fn filter_exclude_pattern_takes_priority_over_include() {
        let filter = Filter::new(true, Some(r"^\d"), Some(r"nightly")).expect("filter");
        assert!(!filter.accepts(&Release::new("1-nightly")));
    }

    #[test]
    fn filter_prerelease_tags_should_restrict_subchannels() {
        let filter =
            Filter::with_prerelease_tags(true, Some(vec!["rc".into(), "beta".into()]), None, None)
                .expect("filter");
        assert!(filter.accepts(&Release::new("v1.0.0-rc1")));
        assert!(!filter.accepts(&Release::new("v1.0.0-alpha1")));
    }

    #[test]
    fn notify_schedule_should_accept_five_field_crontab() {
        let schedule = NotifySchedule::parse("0 9 * * MON-FRI").expect("parse");
        assert_eq!(schedule.expr(), "0 9 * * MON-FRI");
    }

    #[test]
    fn notify_schedule_should_accept_six_field_expression() {
        assert!(NotifySchedule::parse("0 0 9 * * *").is_ok());
    }

    #[test]
    fn notify_schedule_should_reject_garbage() {
        let err = NotifySchedule::parse("not a cron").expect_err("must fail");
        assert!(err.contains("invalid cron expression"));
    }

    #[test]
    fn notify_schedule_next_after_should_be_strictly_in_the_future() {
        let schedule = NotifySchedule::parse("*/5 * * * *").expect("parse");
        let now = Utc::now();
        let next = schedule.next_after(now).expect("next occurrence");
        assert!(next > now);
    }
}
