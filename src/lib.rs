//! `xrelease` — watch software release sources and fan notifications out through
//! [Apprise](https://github.com/caronc/apprise), webhooks, SMTP, brokers, …
//!
//! # Architecture in one paragraph
//!
//! Almost no release source pushes events, so the core is a **poll → diff →
//! notify** pipeline. A [`scheduler`] drives one independent loop per configured
//! [`pipeline::Watch`]. Each loop asks a [`sources::Provider`] for the current
//! list of [`model::Release`]s, the [`store::Store`] decides which of them are
//! new (it has never seen the identity before), and every newcomer is delivered
//! through a [`notify::Notifier`] (Apprise, Express, webhook, SMTP, brokers).
//!
//! # Binaries
//!
//! - **`xrelease`** (this crate) — instance process: `serve` (poller + outbox +
//!   sinks + HTTP API + webhooks) plus local ops (`validate`, `health`,
//!   `sources`, …).
//! - **`xrctl`** (`cli/` → `xrelease-cli`) — lean remote management client over
//!   HTTP; no Postgres, no local config authority.
//!
//! # Module map
//!
//! - [`config`] — bootstrap + desired YAML/TOML, env overlays, resolve.
//! - [`config_apply`] — push-apply / reload / rollback of desired state.
//! - [`crypto`] — AES-GCM for `app_secret` values (desired doc keeps `*_env` refs).
//! - [`model`] — the [`model::Release`] value type shared by every provider.
//! - [`sources`] — provider adapters behind one [`sources::Provider`] enum
//!   (static dispatch; see Best Practices, Chapter 6).
//! - [`store`] — PostgreSQL-backed "have I seen this release?" state.
//! - [`notify`] — the [`notify::Notifier`] trait, Apprise, webhooks, and
//!   optional message-broker sinks (Kafka / NATS / RabbitMQ) behind
//!   [`CompositeNotifier`] fan-out.
//! - [`pipeline`] — the single-source poll/diff/notify step, unit-testable.
//! - [`scheduler`]— spawns and supervises one polling loop per source.
//! - [`runtime`] — `serve` over the same pipeline.
//! - [`engine`] — shared store, HTTP client, notifier, metrics wiring.
//! - [`metrics`] — Prometheus counters + latency histograms (`GET /metrics`).
//! - [`validate`] — static config validation (`xrelease validate`).
//! - [`watch_lookup`] — O(1) source resolution for webhooks and API.
//! - [`api`] — HTTP API, webhooks, OpenAPI (`xrelease serve`).

#![forbid(unsafe_code)]

pub mod api;
pub mod config;
pub mod config_apply;
pub mod crypto;
pub mod engine;
pub mod error;
pub mod logging;
pub mod metrics;
pub mod model;
pub mod notify;
pub mod pipeline;
pub mod rpm_limit;
pub mod runtime;
pub mod scheduler;
pub mod sources;
pub mod store;
pub mod validate;
pub mod watch_lookup;
