//! Shared RPM-based Governor limiters (ingress HTTP + outbound upstream).
//!
//! Both surfaces use the same direct (not-keyed) quota builder; type aliases
//! keep call sites readable.

use std::num::NonZeroU32;
use std::sync::Arc;

use governor::clock::DefaultClock;
use governor::state::direct::NotKeyed;
use governor::state::InMemoryState;
use governor::{Quota, RateLimiter};

/// Direct (global) requests-per-minute limiter.
pub type DirectRpmLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

/// Shared direct limiter for outbound provider fetches
/// (`[defaults].upstream_requests_per_minute`).
pub type UpstreamLimiter = DirectRpmLimiter;

/// Build a limiter from requests-per-minute. Returns `None` when disabled (`0`).
#[must_use]
pub fn build_direct_rpm_limiter(requests_per_minute: u32) -> Option<Arc<DirectRpmLimiter>> {
    if requests_per_minute == 0 {
        return None;
    }
    let rpm = NonZeroU32::new(requests_per_minute.max(1))?;
    let quota = Quota::per_minute(rpm);
    Some(Arc::new(RateLimiter::direct(quota)))
}

/// Build the global upstream (poll) limiter. Returns `None` when disabled (`0`).
#[must_use]
pub fn build_upstream_limiter(requests_per_minute: u32) -> Option<Arc<UpstreamLimiter>> {
    build_direct_rpm_limiter(requests_per_minute)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_direct_rpm_limiter_should_return_none_when_disabled() {
        assert!(build_direct_rpm_limiter(0).is_none());
    }

    #[test]
    fn build_direct_rpm_limiter_should_create_limiter() {
        assert!(build_direct_rpm_limiter(60).is_some());
    }

    #[test]
    fn build_upstream_limiter_should_return_none_when_disabled() {
        assert!(build_upstream_limiter(0).is_none());
    }

    #[test]
    fn build_upstream_limiter_should_create_limiter() {
        assert!(build_upstream_limiter(30).is_some());
    }
}
