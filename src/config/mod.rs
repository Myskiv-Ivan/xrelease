//! Declarative configuration — split infra (`bootstrap.toml`) and app (`app/releases.yaml`).
//!
//! The TOML maps one-to-one onto the [`crate::sources::Provider`] variants via a
//! serde-tagged enum ([`SourceConfig`]), so adding a `[[sources]]` block needs
//! no glue code beyond the provider itself. Secrets (Apprise URLs, DB URL) can
//! be supplied via `XRELEASE_*` environment variables so they stay out of the
//! committed config file (GitOps: structure in Git, secrets at deploy time).
//! The state database is PostgreSQL, configured via `[database].postgres_url`
//! or `XRELEASE_DATABASE_URL`.
//!
//! # Module layout
//!
//! Each submodule owns one config concern and is re-exported flat from here,
//! so `crate::config::ApiConfig` etc. resolve exactly as they did when this
//! was one file:
//!
//! - [`api`] — HTTP API / webhook ingress (`[api]`, OIDC).
//! - [`database`] — PostgreSQL settings (`[database]`).
//! - [`log`] — logging (`[log]`).
//! - [`notifiers`] — delivery sinks (`[[notifiers]]`).
//! - [`sources`] — watched sources (`[[sources]]`).

use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::notify::{
    BreakerConfig, CompositeNotifier, RoutedSink, DEFAULT_SINK_BREAKER_COOLDOWN_SECS,
    DEFAULT_SINK_BREAKER_FAILURE_THRESHOLD,
};
use crate::pipeline::Watch;

mod api;
mod config_api;
mod database;
mod desired_format;
mod log;
mod notifiers;
mod organizations;
mod paths;
mod resolve;
mod schema;
mod secrets;
mod sources;
mod vault;

pub use api::{ApiConfig, LocalAuthConfig, OidcConfig};
pub use config_api::{ConfigApiConfig, ConfigSource};
pub use database::{DatabaseConfig, DatabaseSslMode};
pub use desired_format::{
    detect_desired_format, format_from_path, load_desired_file, parse_desired_document,
    parse_desired_document_with_hint, DesiredFormat,
};
pub use log::LogConfig;
pub use notifiers::{
    AppriseConfig, ExpressCfg, KafkaCfg, NatsCfg, NotifierConfig, NovuCfg, RabbitmqCfg, SmtpCfg,
    SmtpTlsCfg, WebhookCfg, WebhookMethodCfg,
};
pub use organizations::{
    namespace_organization_desired, namespace_routing_tag, namespace_source_id,
    organization_id_from_source_id, OrganizationConfig, ORGANIZATION_SEP,
};
pub use paths::{ConfigPaths, DEFAULT_APP_PATH, DEFAULT_BOOTSTRAP_PATH};
pub use resolve::{
    compose_organizations, contains_app_sections, contains_infra_sections,
    desired_source_from_revision, effective_revision, ensure_desired_only, ensure_infra_only,
    has_desired_state, load_bootstrap, load_infra_bootstrap, merge_bootstrap_over_desired,
    merge_pushed_document, organization_app_path, organization_desired_raw, parse_bootstrap_file,
    resolve, strip_desired_sections, strip_infra_sections, ComposedOrganizations, DesiredSource,
    EffectiveRevision, EMPTY_ORGANIZATION_DOCUMENT,
};
pub use schema::{
    sink_kinds, SchemaOption, SinkOption, SMTP_TLS_MODES, SOURCE_KINDS, WEBHOOK_METHODS,
};
pub use secrets::{
    collect_secret_env_refs, is_ui_managed_secret_name, normalize_document_for_storage,
    normalize_secrets_to_refs, stale_ui_managed_secrets, SecretWrite,
};
pub use sources::{
    builtin_preset_schemas, builtin_source_presets, effective_source_presets, ArtifactHubCfg,
    BitbucketCfg, BitbucketEditionCfg, BuiltinPresetInfo, BuiltinPresetSchema, DockerCfg, FeedCfg,
    GiteaHostCfg, GiteaRepoCfg, GithubCfg, GitlabCfg, PackageCfg, RegistryImageCfg, SourceCommon,
    SourceConfig, SourcePreset, BUILTIN_PRESET_INFO,
};
pub(crate) use sources::{source_common, source_label, source_routing_tag};
pub use vault::{vault_get, vault_remove, vault_replace_all, vault_upsert};

/// Read a secret by env-var name: process env first, then the `app_secret` vault.
///
/// Shared by [`sources`]' token resolution, [`notifiers`]' secret resolution,
/// and [`crate::validate`] structural checks.
pub(crate) fn env_token(var: &str) -> Option<String> {
    std::env::var(var)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| vault_get(var))
}

/// Top-level configuration.
#[derive(Debug, Clone, Deserialize, serde::Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Fallback schedule applied to sources that don't override it.
    #[serde(default)]
    pub defaults: Defaults,
    /// Apprise delivery settings — **not used as desired-state authority**.
    /// Desired documents must leave this at defaults and declare Apprise via
    /// `[[notifiers]]` (`type = "apprise"`). Kept on [`Config`] only so leftover
    /// top-level blocks fail loudly in [`crate::config::resolve::ensure_desired_only`].
    #[serde(default)]
    pub apprise: AppriseConfig,
    /// State database settings.
    #[serde(default)]
    pub database: DatabaseConfig,
    /// Logging settings.
    #[serde(default)]
    pub log: LogConfig,
    /// Optional HTTP API + webhook ingress.
    #[serde(default)]
    pub api: ApiConfig,
    /// Push-applied config API (`POST /api/v1/config/apply`).
    #[serde(default)]
    pub config_api: ConfigApiConfig,
    /// Organization catalogue (`[[organizations]]`) — bootstrap only.
    ///
    /// Empty = single desired-state document (`--app` / ledger).
    #[serde(default)]
    pub organizations: Vec<OrganizationConfig>,
    /// Additional delivery sinks (`[[notifiers]]`).
    ///
    /// Each entry is a tagged enum (`type = "webhook" | "slack" | "telegram" |
    /// "kafka" | "nats" | "rabbitmq" | "apprise" | "express" | "novu" | "smtp"`).
    /// Use `tags` on each sink to route notifications to teams (matches
    /// `routing_tag` on sources).
    #[serde(default)]
    pub notifiers: Vec<NotifierConfig>,
    /// Team catalogue — documents routing tags used by sources and notifiers.
    #[serde(default)]
    pub teams: Vec<TeamConfig>,
    /// Named source-field presets (`preset: name` on each source).
    #[serde(default)]
    pub presets: std::collections::BTreeMap<String, SourcePreset>,
    /// The sources to watch.
    #[serde(default)]
    pub sources: Vec<SourceConfig>,
}

/// Default schedule for sources that don't specify their own.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    /// Base poll interval, seconds.
    #[serde(default = "default_interval_secs")]
    pub interval_secs: u64,
    /// Maximum random jitter added per poll, seconds.
    #[serde(default = "default_jitter_secs")]
    pub jitter_secs: u64,
    /// Max outbound provider fetches per minute across all sources (`0` = unlimited).
    #[serde(default)]
    pub upstream_requests_per_minute: u32,
    /// When true (default), each source is polled once immediately on backend start.
    #[serde(default = "default_true")]
    pub poll_on_startup: bool,
    /// Default notification delivery schedule (crontab expression, UTC).
    /// Notifications are held in the outbox until the next matching moment.
    /// Unset = deliver immediately. Sources override with their own
    /// `notify_schedule`; an explicitly empty per-source value opts out.
    #[serde(default)]
    pub notify_schedule: Option<String>,
    /// Max concurrent outbox row deliveries per flush wave (clamped 1–64).
    #[serde(default = "default_outbox_flush_concurrency")]
    pub outbox_flush_concurrency: usize,
    /// Cap on exponential backoff between failed-delivery retries, seconds
    /// (clamped 60–86400). Doubles from the 60s flush cadence per attempt;
    /// unset = 3600 (1 hour).
    #[serde(default = "default_outbox_retry_backoff_max_secs")]
    pub outbox_retry_backoff_max_secs: u32,
    /// Consecutive failures before a notification sink's circuit breaker
    /// trips and it is skipped instead of retried every flush (clamped
    /// 1–1000). Unset = 5.
    #[serde(default = "default_sink_breaker_failure_threshold")]
    pub sink_breaker_failure_threshold: u32,
    /// How long a tripped sink is skipped before one trial delivery is let
    /// through, seconds (clamped 10–86400). Unset = 300 (5 minutes).
    #[serde(default = "default_sink_breaker_cooldown_secs")]
    pub sink_breaker_cooldown_secs: u32,
    /// Routing tag for ops meta-alerts (dead outbox / tripped breaker).
    ///
    /// When set, matches notifier `tags` the same way as per-source
    /// `routing_tag`. Unset = meta-alerts are metrics/UI-only (no notify).
    #[serde(default)]
    pub ops_routing_tag: Option<String>,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            interval_secs: default_interval_secs(),
            jitter_secs: default_jitter_secs(),
            upstream_requests_per_minute: 0,
            poll_on_startup: true,
            notify_schedule: None,
            outbox_flush_concurrency: default_outbox_flush_concurrency(),
            outbox_retry_backoff_max_secs: default_outbox_retry_backoff_max_secs(),
            sink_breaker_failure_threshold: default_sink_breaker_failure_threshold(),
            sink_breaker_cooldown_secs: default_sink_breaker_cooldown_secs(),
            ops_routing_tag: None,
        }
    }
}

fn default_interval_secs() -> u64 {
    86_400
}

fn default_jitter_secs() -> u64 {
    3_600
}

fn default_true() -> bool {
    true
}

fn default_outbox_flush_concurrency() -> usize {
    crate::pipeline::DEFAULT_OUTBOX_FLUSH_CONCURRENCY
}

fn default_outbox_retry_backoff_max_secs() -> u32 {
    crate::pipeline::DEFAULT_OUTBOX_RETRY_BACKOFF_MAX_SECS
}

fn default_sink_breaker_failure_threshold() -> u32 {
    DEFAULT_SINK_BREAKER_FAILURE_THRESHOLD
}

fn default_sink_breaker_cooldown_secs() -> u32 {
    DEFAULT_SINK_BREAKER_COOLDOWN_SECS
}

/// Documented team / routing group (GitOps catalogue).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TeamConfig {
    /// Routing tag shared by sources (`routing_tag`) and notifiers (`tags`).
    pub tag: String,
    /// Human-readable label for operators (optional).
    pub name: Option<String>,
}

impl Config {
    /// Build the fan-out notifier from `[[notifiers]]`.
    ///
    /// An empty sink list is allowed so `[config_api].source = "api"` /
    /// `ui_config` can boot idle until the first UI/CI apply.
    /// Delivery no-ops until channels are configured; [`crate::validate`] warns.
    ///
    /// When `XRELEASE_APPRISE_ENDPOINT` is set, it overlays the `endpoint` of
    /// every `type = "apprise"` notifier (Compose/Helm sidecar URL).
    ///
    /// # Errors
    /// Returns an error if a configured sink fails to build (invalid webhook
    /// URL/headers, misconfigured broker/SMTP credentials, …).
    pub fn build_notifiers(&self, http: reqwest::Client) -> anyhow::Result<CompositeNotifier> {
        let mut sinks: Vec<RoutedSink> = Vec::new();
        let apprise_endpoint_override = std::env::var("XRELEASE_APPRISE_ENDPOINT")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());

        for (index, notifier) in self.notifiers.iter().enumerate() {
            let tags = notifier.routing_tags();
            let built = match (notifier, apprise_endpoint_override.as_deref()) {
                (NotifierConfig::Apprise(cfg), Some(endpoint)) => {
                    let mut cfg = cfg.clone();
                    cfg.endpoint = endpoint.to_owned();
                    NotifierConfig::Apprise(cfg).build(http.clone())
                }
                _ => notifier.build(http.clone()),
            }
            .with_context(|| format!("notifier #{index} ([[notifiers]])"))?;
            sinks.push(RoutedSink::new(built, tags));
        }

        let failure_threshold = self.defaults.sink_breaker_failure_threshold.clamp(1, 1_000);
        let cooldown_secs = self.defaults.sink_breaker_cooldown_secs.clamp(10, 86_400);
        if failure_threshold != DEFAULT_SINK_BREAKER_FAILURE_THRESHOLD
            || cooldown_secs != DEFAULT_SINK_BREAKER_COOLDOWN_SECS
        {
            tracing::info!(
                failure_threshold,
                cooldown_secs,
                "sink circuit breaker thresholds overridden"
            );
        }
        let breaker_config = BreakerConfig {
            failure_threshold,
            cooldown: Duration::from_secs(u64::from(cooldown_secs)),
        };

        Ok(CompositeNotifier::with_breaker_config(
            sinks,
            breaker_config,
        ))
    }

    /// Build the runtime [`Watch`] list, resolving ids, schedules, presets, and filters.
    ///
    /// Resolves `preset` against built-in + user catalogues ([`effective_source_presets`]).
    /// Sets [`Watch::organization_id`] from a namespaced source id (`org::…`).
    ///
    /// Prefer [`Self::to_watches`] when the [`Config`] must be kept (apply / validate /
    /// runtime); this consuming form is for one-shot ownership hand-off.
    pub fn into_watches(self) -> anyhow::Result<Vec<Watch>> {
        self.to_watches()
    }

    /// Borrowing form of [`Self::into_watches`] — clones only `[[sources]]`, not the
    /// whole config (notifiers, teams, infra overlays).
    pub fn to_watches(&self) -> anyhow::Result<Vec<Watch>> {
        let presets = effective_source_presets(&self.presets);
        self.sources
            .iter()
            .cloned()
            .map(|source| {
                let mut watch = source.into_watch(&self.defaults, &presets)?;
                watch.organization_id =
                    organization_id_from_source_id(watch.provider.id()).map(str::to_owned);
                Ok(watch)
            })
            .collect()
    }
}

/// Load merged effective config for CLI commands.
///
/// Uses the ledger when PostgreSQL is reachable (same boot order as [`Runtime::new`]).
/// Falls back to the on-disk app file when the database is unavailable (typical CI).
pub fn load_for_cli(paths: &ConfigPaths) -> anyhow::Result<Config> {
    let bootstrap = load_infra_bootstrap(paths)?;
    match crate::store::Store::open_from_config(&bootstrap.database) {
        Ok(store) => resolve(paths, Some(&store)),
        Err(_) => resolve(paths, None),
    }
}

/// Overlay secrets/overrides from the environment.
///
/// Apprise **targets** (`urls` / `config_key`) are document-only (YAML / UI /
/// apply). The Apprise HTTP **endpoint** may be injected via
/// `XRELEASE_APPRISE_ENDPOINT` when building notifiers (see [`Config::build_notifiers`]).
fn apply_env_overrides(config: &mut Config) {
    if let Ok(value) = std::env::var("XRELEASE_DATABASE_URL") {
        config.database.postgres_url = value;
    }
    if let Ok(value) = std::env::var("XRELEASE_DATABASE_SSL_MODE") {
        if let Some(mode) = database::DatabaseSslMode::parse_libpq(&value) {
            config.database.ssl_mode = Some(mode);
        }
    }
    if let Ok(value) = std::env::var("XRELEASE_DATABASE_SSL_ROOT_CERT") {
        config.database.ssl_root_cert = Some(value);
    }
    // Canonical: `XRELEASE_LOG` only (blank values ignored — see `log.rs`).
    log::apply_log_env_overlay(&mut config.log);
    if let Ok(value) = std::env::var("XRELEASE_API_LISTEN") {
        config.api.listen = value;
    }
    if let Ok(value) = std::env::var("XRELEASE_WEBHOOK_SECRET") {
        config.api.webhook_secret = Some(value);
    }
    if let Ok(value) = std::env::var("XRELEASE_API_KEY") {
        config.api.api_key = Some(value);
    }
    if let Ok(value) = std::env::var("XRELEASE_API_RATE_LIMIT") {
        if let Ok(rpm) = value.parse() {
            config.api.rate_limit_per_minute = rpm;
        }
    }
    if let Ok(value) = std::env::var("XRELEASE_UPSTREAM_RPM") {
        if let Ok(rpm) = value.parse() {
            config.defaults.upstream_requests_per_minute = rpm;
        }
    }
    if let Ok(value) = std::env::var("XRELEASE_OIDC_ISSUER") {
        config.api.oidc.issuer = Some(value);
    }
    if let Ok(value) = std::env::var("XRELEASE_OIDC_DISCOVERY_URL") {
        config.api.oidc.discovery_url = Some(value);
    }
    if let Ok(value) = std::env::var("XRELEASE_OIDC_JWKS_URI") {
        config.api.oidc.jwks_uri = Some(value);
    }
    if let Ok(value) = std::env::var("XRELEASE_OIDC_AUDIENCE") {
        config.api.oidc.audience = value
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect();
    }
    if let Ok(value) = std::env::var("XRELEASE_OIDC_REQUIRED_SCOPE") {
        config.api.oidc.required_scope = Some(value);
    }
    if let Ok(value) = std::env::var("XRELEASE_OIDC_ROLE_CLAIM") {
        config.api.oidc.role_claim = value;
    }
    if let Ok(value) = std::env::var("XRELEASE_OIDC_ROLE_ADMIN") {
        config.api.oidc.role_admin = split_csv(&value);
    }
    if let Ok(value) = std::env::var("XRELEASE_OIDC_ROLE_OPERATOR") {
        config.api.oidc.role_operator = split_csv(&value);
    }
    if let Ok(value) = std::env::var("XRELEASE_OIDC_ROLE_VIEWER") {
        config.api.oidc.role_viewer = split_csv(&value);
    }
    if let Ok(value) = std::env::var("XRELEASE_OIDC_AUTO_CREATE_USERS") {
        config.api.oidc.auto_create_users = matches!(value.trim(), "1" | "true" | "yes" | "on");
    }
    if let Ok(value) = std::env::var("XRELEASE_OIDC_DEFAULT_ROLE") {
        config.api.oidc.default_role = value;
    }
    if let Ok(value) = std::env::var("XRELEASE_LOCAL_AUTH") {
        config.api.local_auth.enabled = matches!(value.trim(), "1" | "true" | "yes" | "on");
    }
    if let Ok(value) = std::env::var("XRELEASE_ADMIN_USER") {
        config.api.local_auth.admin_username = value;
    }
    if let Ok(value) = std::env::var("XRELEASE_ADMIN_PASSWORD") {
        config.api.local_auth.admin_password = Some(value);
    }
    if let Ok(value) = std::env::var("XRELEASE_SESSION_SECRET") {
        config.api.local_auth.session_secret = Some(value);
    }
    if let Ok(value) = std::env::var("XRELEASE_SESSION_TTL_SECS") {
        if let Ok(secs) = value.parse() {
            config.api.local_auth.session_ttl_secs = secs;
        }
    }
    if let Ok(value) = std::env::var("XRELEASE_CONFIG_API_ENABLED") {
        config.config_api.api_config = matches!(value.trim(), "1" | "true" | "yes" | "on");
    }
    if let Ok(value) = std::env::var("XRELEASE_CONFIG_API_APPLY_SCOPE") {
        config.config_api.apply_scope = Some(value);
    }
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_should_parse_a_full_example() {
        let toml = r#"
 [[notifiers]]
 type = "apprise"
 endpoint = "http://apprise:8000"
 urls = ["tgram://token/chat"]

 [[sources]]
 type = "github"
 repo = "tokio-rs/tokio"

 [[sources]]
 type = "docker"
 image = "library/nginx"
 pattern = '^\d+\.\d+\.\d+$'
 "#;
        let config: Config = toml::from_str(toml).expect("parse");
        assert_eq!(config.sources.len(), 2);
    }

    #[test]
    fn empty_config_should_default() {
        let config: Config = toml::from_str("").expect("parse");
        assert_eq!(config.defaults.interval_secs, 86_400);
        assert!(config.sources.is_empty());
        assert_eq!(config.defaults.sink_breaker_failure_threshold, 5);
        assert_eq!(config.defaults.sink_breaker_cooldown_secs, 300);
    }

    #[test]
    fn env_token_should_ignore_blank_values() {
        let key = "XRELEASE_TEST_BLANK_TOKEN";
        std::env::set_var(key, " ");
        assert!(env_token(key).is_none());
        std::env::set_var(key, "ghp_test");
        assert_eq!(env_token(key).as_deref(), Some("ghp_test"));
        std::env::remove_var(key);
    }
}
