//! Per-sink routing by team tag (`Event::routing_tag`).

use crate::notify::Event;

/// Whether an event should be delivered to a sink configured with `sink_tags`.
///
/// - Empty `sink_tags` → deliver all events (broadcast sinks).
/// - Non-empty `sink_tags` → deliver only when `event.routing_tag` matches one entry.
pub fn matches_routing_tags(sink_tags: &[String], event_tag: Option<&str>) -> bool {
    if sink_tags.is_empty() {
        return true;
    }
    let Some(event_tag) = event_tag.map(str::trim).filter(|tag| !tag.is_empty()) else {
        return false;
    };
    sink_tags.iter().any(|tag| tag.trim() == event_tag)
}

/// Whether `event` should be routed to a sink with the given tag filter.
pub fn event_matches_sink_tags(event: &Event, sink_tags: &[String]) -> bool {
    matches_routing_tags(sink_tags, event.routing_tag.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::Event;

    fn event(tag: Option<&str>) -> Event {
        Event {
            source_id: "github:org/app".into(),
            source_kind: "GitHub".into(),
            title: "t".into(),
            body: "b".into(),
            url: None,
            routing_tag: tag.map(str::to_owned),
        }
    }

    #[test]
    fn empty_sink_tags_should_match_everything() {
        assert!(event_matches_sink_tags(&event(Some("platform")), &[]));
        assert!(event_matches_sink_tags(&event(None), &[]));
    }

    #[test]
    fn tagged_sink_should_match_only_listed_tags() {
        let tags = vec!["platform-team".into(), "security-team".into()];
        assert!(event_matches_sink_tags(
            &event(Some("platform-team")),
            &tags
        ));
        assert!(!event_matches_sink_tags(&event(Some("infra-team")), &tags));
        assert!(!event_matches_sink_tags(&event(None), &tags));
    }

    #[test]
    fn matching_indices_should_be_empty_when_no_sink_matches_tag() {
        use std::collections::HashMap;

        use crate::notify::webhook::{WebhookMethod, WebhookNotifier};
        use crate::notify::{CompositeNotifier, RoutedSink, Sink};

        let webhook = WebhookNotifier::new(
            reqwest::Client::new(),
            "w",
            "https://example.test/hook",
            WebhookMethod::Post,
            &HashMap::new(),
            None,
            None,
        )
        .expect("webhook");
        let notifier = CompositeNotifier::new(vec![RoutedSink::new(
            Sink::Webhook(webhook),
            vec!["platform".into()],
        )]);

        assert!(notifier.matching_indices(&event(Some("infra"))).is_empty());
        assert_eq!(notifier.matching_indices(&event(Some("platform"))), vec![0]);
    }
}
