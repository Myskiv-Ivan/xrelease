//! Error types for the library.
//!
//! Following Best Practices Chapter 4, the library exposes specific
//! `thiserror` enums (one per subsystem) rather than a single catch-all, so
//! callers can match on what actually went wrong. The binary (`main.rs`) maps
//! these into `anyhow::Error` at its boundary.

/// Failure while fetching releases from an upstream source.
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    /// The HTTP request itself failed (DNS, TLS, timeout, non-2xx via
    /// `error_for_status`, ...).
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// The response body could not be parsed as RSS/Atom/JSON Feed.
    #[error("failed to parse feed: {0}")]
    Feed(#[from] feed_rs::parser::ParseFeedError),

    /// A container registry returned a non-success status.
    #[error("registry returned status {status} for {url}")]
    Registry {
        /// HTTP status code returned by the registry.
        status: u16,
        /// The URL that was requested.
        url: String,
    },

    /// Anything source-specific that does not fit the variants above.
    #[error("{0}")]
    Other(String),
}

/// Failure while delivering a notification.
#[derive(Debug, thiserror::Error)]
pub enum NotifyError {
    /// The HTTP request to the notification backend failed.
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// The notifier was constructed with an invalid configuration.
    #[error("notifier misconfigured: {0}")]
    Misconfigured(String),

    /// An HTTP backend (Apprise, generic webhook) returned a non-success status.
    #[error("{backend} rejected the notification (status {status}): {body}")]
    Rejected {
        /// Which sink produced the rejection (e.g. `apprise`, `webhook`).
        backend: &'static str,
        /// HTTP status code returned by the backend.
        status: u16,
        /// Response body, truncated by the backend itself.
        body: String,
    },

    /// A message-broker backend (Kafka, NATS, RabbitMQ) failed to accept the message.
    #[error("{backend} delivery failed: {message}")]
    Backend {
        /// Which sink produced the failure (e.g. `kafka`, `nats`, `rabbitmq`).
        backend: &'static str,
        /// Human-readable cause from the underlying client.
        message: String,
    },

    /// Fan-out delivery: one or more sinks failed. Carries the joined per-sink errors.
    #[error("{failed}/{total} notifier(s) failed: {details}")]
    FanOut {
        /// Number of sinks that failed.
        failed: usize,
        /// Total sinks attempted.
        total: usize,
        /// Joined per-sink error messages.
        details: String,
    },

    /// The sink's circuit breaker is open (repeated recent failures) — the
    /// call was skipped without touching the network.
    #[error("{backend} circuit breaker open — skipped after repeated failures")]
    BreakerOpen {
        /// Which sink was skipped (e.g. `apprise`, `webhook`).
        backend: &'static str,
    },
}

/// Failure while reading or writing the seen-release state store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The underlying PostgreSQL operation failed.
    #[error("postgres error: {0}")]
    Postgres(#[from] postgres::Error),

    /// Connection pool checkout or configuration failed.
    #[error("connection pool error: {0}")]
    Pool(#[from] r2d2::Error),

    /// Another `xrelease` poller (`serve`) already holds this database.
    #[error(
        "another xrelease poller already holds this PostgreSQL database \
         (single poller per DB — stop the other process, or use a separate database)"
    )]
    PollerBusy,

    /// Store-level error not wrapped by a driver.
    #[error("{0}")]
    Other(String),
}

/// Failure in the poll → diff → outbox → notify pipeline.
///
/// Typed boundary for [`crate::pipeline`] so HTTP handlers can map
/// [`SourceError`] / [`StoreError`] without flattening everything through
/// `anyhow` first. The binary still lifts these into `anyhow::Error` at
/// `main` when needed.
#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    /// Upstream fetch / parse failure.
    #[error(transparent)]
    Source(#[from] SourceError),

    /// PostgreSQL / state-store failure.
    #[error(transparent)]
    Store(#[from] StoreError),

    /// Sink delivery failure that escaped the outbox retry path.
    #[error(transparent)]
    Notify(#[from] NotifyError),

    /// Pipeline-level failure that does not fit the variants above.
    #[error("{0}")]
    Other(String),
}
