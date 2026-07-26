//! A configured source watch (provider + schedule + filter).

use std::time::Duration;

use crate::sources::Provider;

use super::{Filter, NotifySchedule};

/// One thing to watch: a provider plus its schedule and filter.
#[derive(Debug, Clone)]
pub struct Watch {
    /// The source to poll.
    pub provider: Provider,
    /// Base interval between polls.
    pub interval: Duration,
    /// Maximum random jitter added to each interval (anti-thundering-herd).
    pub jitter: Duration,
    /// Poll once immediately when the process starts (before the first interval sleep).
    pub poll_on_startup: bool,
    /// Release filter.
    pub filter: Filter,
    /// Override the team routing tag for notifications from this source.
    ///
    /// When `Some`, routes notifications to sinks whose `tags` include this
    /// value (and to Apprise tag subsets). Useful for per-team routing without
    /// separate notifier configs.
    pub routing_tag: Option<String>,
    /// When set, notifications are held in the outbox until the next moment
    /// matching this cron expression (calendar-gated delivery).
    pub notify_schedule: Option<NotifySchedule>,
    /// Owning organization (`[[organizations]]` id). `None` = single-document mode.
    pub organization_id: Option<String>,
}
