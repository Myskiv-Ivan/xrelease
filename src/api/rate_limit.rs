//! HTTP rate limiting for management and webhook routes.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::metrics::Metrics;
use crate::rpm_limit::{build_direct_rpm_limiter, DirectRpmLimiter};

/// Shared direct rate limiter (global, not per-IP).
pub type HttpRateLimiter = DirectRpmLimiter;

/// Rate-limiter middleware state: quota + metrics for 429 counters.
#[derive(Clone)]
pub struct RateLimitState {
    pub limiter: Arc<HttpRateLimiter>,
    pub metrics: Arc<Metrics>,
}

/// Build a limiter from requests-per-minute. Returns `None` when disabled (`0`).
#[must_use]
pub fn build_rate_limiter(requests_per_minute: u32) -> Option<Arc<HttpRateLimiter>> {
    build_direct_rpm_limiter(requests_per_minute)
}

/// Axum middleware — returns 429 when the quota is exhausted.
pub async fn rate_limit_middleware(
    State(state): State<RateLimitState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if state.limiter.check().is_err() {
        state.metrics.record_http_rate_limited();
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "rate limit exceeded; retry later",
        )
            .into_response();
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_rate_limiter_should_return_none_when_disabled() {
        assert!(build_rate_limiter(0).is_none());
    }

    #[test]
    fn build_rate_limiter_should_create_limiter() {
        assert!(build_rate_limiter(60).is_some());
    }
}
