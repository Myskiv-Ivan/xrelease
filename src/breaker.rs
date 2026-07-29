//! Shared circuit-breaker state machine.
//!
//! Extracted so the notification sinks and the advisory-enrichment lookup share
//! one implementation. Both need the same thing — stop dialling a dependency
//! that keeps failing, then let exactly one probe through after a cooldown — and
//! two copies of a state machine drift.
//!
//! Thresholds are passed per call rather than stored, so each caller keeps
//! owning its own configuration type ([`crate::notify::BreakerConfig`],
//! `AdvisoryConfig`) without this module depending on either.

use std::time::{Duration, Instant};

/// Three conceptual states without a separate enum tag: `open_until` is `None`
/// in the closed state; once set, calls are rejected until [`Instant::now`]
/// reaches it (open), at which point exactly one trial call is meant to pass
/// through (half-open) — success clears `open_until`, failure re-arms it for
/// another cooldown.
///
/// Under concurrent calls against the same breaker, two callers can both observe
/// the cooldown as elapsed and both proceed as the "one" trial call; the
/// consequence is at most one extra probe against a possibly-still-broken
/// dependency, not a correctness issue, so this is accepted rather than solved
/// with a compare-and-swap permit.
#[derive(Debug)]
pub(crate) struct Breaker {
    consecutive_failures: u32,
    open_until: Option<Instant>,
}

impl Breaker {
    pub(crate) fn new() -> Self {
        Self {
            consecutive_failures: 0,
            open_until: None,
        }
    }

    /// Whether a call should be attempted right now.
    pub(crate) fn allows(&self, now: Instant) -> bool {
        match self.open_until {
            None => true,
            Some(until) => now >= until,
        }
    }

    /// Close the breaker and clear the failure streak.
    ///
    /// For callers whose success path is separate from their failure path, so
    /// they need not pass a threshold and cooldown that success ignores.
    pub(crate) fn reset(&mut self) {
        self.consecutive_failures = 0;
        self.open_until = None;
    }

    /// Record a call outcome, transitioning state.
    ///
    /// Returns `true` when this call newly opens (or re-opens after a failed
    /// half-open trial) the breaker — used for ops meta-alerts.
    pub(crate) fn record(
        &mut self,
        now: Instant,
        success: bool,
        failure_threshold: u32,
        cooldown: Duration,
    ) -> bool {
        if success {
            self.reset();
            return false;
        }
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures >= failure_threshold {
            let already_open = self.open_until.is_some_and(|until| now < until);
            self.open_until = Some(now + cooldown);
            return !already_open;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THRESHOLD: u32 = 2;
    const COOLDOWN: Duration = Duration::from_secs(60);

    fn record(breaker: &mut Breaker, now: Instant, success: bool) -> bool {
        breaker.record(now, success, THRESHOLD, COOLDOWN)
    }

    #[test]
    fn new_breaker_should_allow_calls() {
        assert!(Breaker::new().allows(Instant::now()));
    }

    #[test]
    fn should_stay_closed_below_the_threshold() {
        let now = Instant::now();
        let mut breaker = Breaker::new();
        assert!(!record(&mut breaker, now, false));
        assert!(breaker.allows(now), "one failure of two must not open it");
    }

    #[test]
    fn should_open_at_the_threshold() {
        let now = Instant::now();
        let mut breaker = Breaker::new();
        record(&mut breaker, now, false);
        assert!(
            record(&mut breaker, now, false),
            "reaching the threshold reports a newly-opened breaker"
        );
        assert!(!breaker.allows(now));
    }

    #[test]
    fn should_reject_while_the_cooldown_has_not_elapsed() {
        let now = Instant::now();
        let mut breaker = Breaker::new();
        record(&mut breaker, now, false);
        record(&mut breaker, now, false);
        assert!(!breaker.allows(now + COOLDOWN / 2));
    }

    #[test]
    fn should_allow_one_trial_after_the_cooldown() {
        let now = Instant::now();
        let mut breaker = Breaker::new();
        record(&mut breaker, now, false);
        record(&mut breaker, now, false);
        assert!(breaker.allows(now + COOLDOWN));
    }

    #[test]
    fn success_should_close_and_reset_the_streak() {
        let now = Instant::now();
        let mut breaker = Breaker::new();
        record(&mut breaker, now, false);
        record(&mut breaker, now, false);
        record(&mut breaker, now + COOLDOWN, true);
        assert!(breaker.allows(now + COOLDOWN));
        // Streak reset: a single later failure must not re-open immediately.
        assert!(!record(&mut breaker, now + COOLDOWN, false));
    }

    #[test]
    fn failed_trial_should_rearm_and_report_reopening() {
        let now = Instant::now();
        let mut breaker = Breaker::new();
        record(&mut breaker, now, false);
        record(&mut breaker, now, false);
        // The cooldown has elapsed, so this failure is the half-open trial. It
        // re-arms *and* reports, because a recovery attempt that failed is news.
        assert!(record(&mut breaker, now + COOLDOWN, false));
        assert!(!breaker.allows(now + COOLDOWN));
        assert!(breaker.allows(now + COOLDOWN + COOLDOWN));
    }

    /// The `already_open` guard exists for this case only: a failure arriving
    /// while the breaker is *still* open must not re-report, or ops would get an
    /// alert per delivery for a dependency already known to be down.
    #[test]
    fn failure_while_still_open_should_not_report_again() {
        let now = Instant::now();
        let mut breaker = Breaker::new();
        record(&mut breaker, now, false);
        assert!(record(&mut breaker, now, false), "opens once");
        assert!(
            !record(&mut breaker, now + COOLDOWN / 2, false),
            "still inside the cooldown — not a new opening"
        );
    }
}
