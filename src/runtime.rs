//! Runtime — how the `xrelease` backend binary is started.
//!
//! The only long-running mode is [`Runtime::serve`] (poller + HTTP API +
//! webhooks). Remote management lives in the separate `xrctl` binary (`cli/`).

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use serde::Serialize;
use tracing::{info, warn};

use crate::config::{self, Config, ConfigPaths, EffectiveRevision};
use crate::engine::Engine;
use crate::pipeline::Watch;
use crate::scheduler::WatchSupervisor;

/// Summary of a configured watch, for `xrelease sources` and JSON output.
#[derive(Debug, Clone, Serialize)]
pub struct SourceSummary {
    pub id: String,
    pub kind: String,
    pub kind_label: String,
    pub display_name: String,
    pub interval_secs: u64,
    pub jitter_secs: u64,
    pub routing_tag: Option<String>,
    /// Cron expression gating notification delivery (UTC), when configured.
    pub notify_schedule: Option<String>,
    /// Owning organization (`[[organizations]]` id). `None` = single-document mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization_id: Option<String>,
}

/// Per-source counters for observability UI / JSON API.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SourceMetricsView {
    pub polls: u64,
    pub polls_not_modified: u64,
    pub poll_errors: u64,
    pub notifications: u64,
}

/// One security advisory affecting a released version, for JSON output.
///
/// Mirrors [`crate::advisory::Advisory`] but renders [`crate::advisory::Severity`]
/// as its lowercase label rather than relying on serde's default enum
/// serialization — the derive would emit the Rust variant name (`"Critical"`),
/// which is not the stable API contract this type owns.
#[derive(Debug, Clone, Serialize)]
pub struct AdvisoryView {
    /// Primary database id (`GHSA-…`, `RUSTSEC-…`, `PYSEC-…`).
    pub id: String,
    /// Preferred human-facing id: a `CVE-…` alias when the database has one.
    pub display_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cvss_vector: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl From<crate::advisory::Advisory> for AdvisoryView {
    fn from(advisory: crate::advisory::Advisory) -> Self {
        Self {
            id: advisory.id,
            display_id: advisory.display_id,
            summary: advisory.summary,
            severity: advisory
                .severity
                .map(|severity| severity.as_str().to_owned()),
            cvss_vector: advisory.cvss_vector,
            url: advisory.url,
        }
    }
}

/// One release identity recorded in the seen-release catalogue.
#[derive(Debug, Clone, Serialize)]
pub struct SeenReleaseView {
    pub tag: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub first_seen_at: String,
    /// Known security advisories for this exact version. Empty for every
    /// source that is not a package registry, and for package sources until
    /// `[advisories]` is enabled and a delivery has looked this version up —
    /// see [`crate::advisory`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub advisories: Vec<AdvisoryView>,
}

/// Config plus live runtime state — consumed by the read-only observability UI.
#[derive(Debug, Clone, Serialize)]
pub struct SourceDetail {
    #[serde(flatten)]
    pub config: SourceSummary,
    /// Upstream catalog page (repo, package index, feed URL).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_page_url: Option<String>,
    pub initialized: bool,
    pub last_polled_at: Option<String>,
    pub seen_count: u64,
    pub latest_release_tag: Option<String>,
    pub seen_releases: Vec<SeenReleaseView>,
    /// Flattened per-source counters (`polls`, `poll_errors`, …).
    #[serde(flatten)]
    pub metrics: SourceMetricsView,
}

/// Process-wide counters for dashboard overview.
///
/// Mirrors the high-cardinality-free totals from [`crate::metrics::Metrics`]
/// (Prometheus labelled series stay scrape-only).
#[derive(Debug, Clone, Serialize)]
pub struct MetricsSnapshot {
    pub polls_total: u64,
    pub polls_not_modified: u64,
    pub poll_errors: u64,
    pub notifications_total: u64,
    pub webhooks_accepted: u64,
    pub webhooks_ignored: u64,
    pub webhooks_duplicates: u64,
    pub webhooks_errors: u64,
    pub config_apply_total: u64,
    pub config_apply_rejected_total: u64,
    pub notify_breaker_skips: u64,
    pub outbox_enqueued_total: u64,
    pub outbox_delivery_failures_total: u64,
    pub outbox_dead_lettered_total: u64,
    pub outbox_requeued_total: u64,
    pub http_rate_limited_total: u64,
    pub prune_deleted_total: u64,
}

/// Loaded configuration plus shared HTTP client.
pub struct Runtime {
    config_paths: ConfigPaths,
    bootstrap: Config,
    config: Config,
    effective_revision: Option<EffectiveRevision>,
    http: reqwest::Client,
}

impl Runtime {
    /// Resolve effective config (ledger → app file → error).
    ///
    /// Opens a short-lived [`crate::store::Store`] for ledger lookup; pool
    /// teardown is safe on the Tokio thread because store `Drop` offloads via
    /// [`crate::store::db_blocking`].
    pub fn new(config_paths: ConfigPaths, http: reqwest::Client) -> anyhow::Result<Self> {
        let bootstrap = config::load_infra_bootstrap(&config_paths)?;
        let store = crate::store::Store::open_from_config(&bootstrap.database)
            .context("opening state database for config resolution")?;
        // Only report a revision when the ledger is what actually booted us:
        // in `source = "local"` mode a leftover revision may still exist, but
        // the running config did not come from it, and claiming otherwise
        // would mislabel `desired_source` and wrongly block `/api/v1/reload`.
        // Multi-org instances have one revision per ORGANIZATION stream, not a
        // single global one — a leftover NULL-stream row from a
        // pre-[[organizations]] life must not masquerade as the boot identity.
        let effective_revision =
            if bootstrap.config_api.ledger_is_bootable() && bootstrap.organizations.is_empty() {
                config::effective_revision(&store)?
            } else {
                None
            };
        let config = config::resolve(&config_paths, Some(&store))?;
        Ok(Self {
            config_paths,
            bootstrap,
            config,
            effective_revision,
            http,
        })
    }

    /// Build a runtime from an already-loaded config (tests / validate-only paths).
    pub fn from_config(config: Config, http: reqwest::Client) -> Self {
        Self {
            config_paths: ConfigPaths::new(
                PathBuf::from("bootstrap.toml"),
                Some(PathBuf::from("app/releases.yaml")),
            ),
            bootstrap: config.clone(),
            effective_revision: None,
            config,
            http,
        }
    }

    /// The resolved effective config (bootstrap + ledger/app-file + env).
    ///
    /// Lets the CLI reuse one [`Runtime::new`] resolution for every
    /// DB-requiring command (`serve`/`health`/`sources`/`outbox-requeue`/
    /// online `validate`) instead of resolving config a second time per
    /// invocation — see the perf note on `main.rs::main`.
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn watches(&self) -> anyhow::Result<Vec<Watch>> {
        self.config.to_watches()
    }

    pub fn source_summaries(&self) -> anyhow::Result<Vec<SourceSummary>> {
        Ok(self
            .watches()?
            .into_iter()
            .map(source_summary_from_watch)
            .collect())
    }

    /// Run the backend: poller + HTTP API + webhooks (blocks until shutdown).
    pub async fn serve(&self) -> anyhow::Result<()> {
        let engine = Engine::open(&self.config, self.http.clone())?;
        let _poller_lease = engine
            .store
            .try_acquire_poller_lease()
            .context("acquiring single-poller lease")?;
        let watches = self.watches()?;
        let api = self.config.api.clone();

        // Fail-closed when require_auth is set; otherwise warn so an open API is never silent.
        if !api.auth_configured() {
            if api.require_auth {
                anyhow::bail!(
                    "api.require_auth is set but no management authentication is configured — \
                     set api.api_key, api.oidc.issuer, or XRELEASE_SESSION_SECRET \
                     (or set api.require_auth = false for trusted lab-only)"
                );
            }
            warn!(
                listen = %api.listen,
                "management API is UNAUTHENTICATED: no api.api_key, no api.oidc.issuer, \
                 and no local session secret. Anyone who can reach this address can read \
                 observability data and trigger checks. Set api.api_key, api.oidc.issuer, \
                 or XRELEASE_SESSION_SECRET — or restrict access to a trusted network."
            );
        }
        if api.local_auth.enabled {
            crate::api::ensure_bootstrap_admin(&engine.store, &api)
                .context("seeding local admin user")?;
            if !api.local_auth_configured() {
                warn!(
                    "local UI auth is enabled but no session signing secret is set — \
                     password login is disabled and no admin is seeded until \
                     XRELEASE_SESSION_SECRET (and XRELEASE_ADMIN_PASSWORD) are set"
                );
            }
        }
        if !api.webhook_auth_configured() {
            warn!(
                "webhook ingress is UNAUTHENTICATED: no api.webhook_secret set — \
                 /api/v1/webhooks/* will accept unsigned POSTs. Set api.webhook_secret to enforce it."
            );
        }

        let oidc = crate::api::OidcValidator::try_from_config(&api.oidc, self.http.clone())
            .await?
            .map(std::sync::Arc::new);

        let supervisor = std::sync::Arc::new(WatchSupervisor::new(std::sync::Arc::clone(&engine)));
        let state = std::sync::Arc::new(crate::api::AppState::new(
            engine,
            self.config_paths.clone(),
            self.bootstrap.clone(),
            self.config.clone(),
            self.effective_revision.clone(),
            watches.clone(),
            api.clone(),
            oidc,
            supervisor,
        ));

        info!(
            sources = watches.len(),
            listen = %api.listen,
            "starting xrelease backend (serve: API + poller)"
        );

        crate::api::serve(&api.listen, state).await
    }

    pub fn health(&self) -> anyhow::Result<()> {
        let store = crate::store::Store::open_from_config(&self.config.database)
            .context("opening state database")?;
        store.health().context("database health check failed")?;
        Ok(())
    }

    /// Requeue dead-letter notifications for delivery; returns the count revived.
    pub fn requeue_dead_outbox(&self) -> anyhow::Result<usize> {
        let store = crate::store::Store::open_from_config(&self.config.database)
            .context("opening state database")?;
        store
            .requeue_dead_outbox()
            .context("requeueing dead outbox rows")
    }
}

impl Watch {
    #[must_use]
    pub fn summary(&self) -> SourceSummary {
        source_summary_from_watch(self.clone())
    }
}

fn source_summary_from_watch(watch: Watch) -> SourceSummary {
    SourceSummary {
        id: watch.provider.id().to_owned(),
        kind: watch.provider.kind().to_owned(),
        kind_label: watch.provider.kind_label().to_owned(),
        display_name: watch.provider.display_name().to_owned(),
        interval_secs: watch.interval.as_secs(),
        jitter_secs: watch.jitter.as_secs(),
        routing_tag: watch.routing_tag.clone(),
        notify_schedule: watch
            .notify_schedule
            .as_ref()
            .map(|schedule| schedule.expr().to_owned()),
        organization_id: watch.organization_id.clone(),
    }
}

pub fn build_http_client() -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("xrelease/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30))
        .build()
        .context("building HTTP client")
}
