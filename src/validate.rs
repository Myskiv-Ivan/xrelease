//! Configuration validation — static checks and optional online probes.
//!
//! Used by `xrelease validate` before deploy or in GitOps CI pipelines.

use std::collections::HashSet;
use std::net::SocketAddr;

use serde::Serialize;

use crate::config::Config;
use crate::rpm_limit::build_upstream_limiter;

/// Outcome of validating a loaded [`Config`].
#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    pub valid: bool,
    pub sources: usize,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub probes: Vec<OnlineProbe>,
}

/// Result of probing one source against its upstream.
#[derive(Debug, Clone, Serialize)]
pub struct OnlineProbe {
    pub source_id: String,
    pub kind: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub releases: Option<usize>,
    pub not_modified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Options for [`validate_full`].
#[derive(Debug, Clone, Default)]
pub struct ValidateOptions {
    /// Treat warnings as errors (GitOps CI strict mode).
    pub strict: bool,
    /// Probe each source with a live fetch after static checks.
    pub online: bool,
    /// Probe only this source id (requires `online`).
    pub source_filter: Option<String>,
}

impl ValidationReport {
    fn error(&mut self, message: impl Into<String>) {
        self.valid = false;
        self.errors.push(message.into());
    }

    fn warn(&mut self, message: impl Into<String>) {
        self.warnings.push(message.into());
    }

    /// Promote warnings to errors when `strict` is enabled (GitOps CI).
    pub fn apply_strict(&mut self, strict: bool) {
        if strict && !self.warnings.is_empty() {
            self.valid = false;
            for warn in self.warnings.clone() {
                self.errors.push(format!("strict: {warn}"));
            }
        }
    }
}

/// Static validation only (no network).
#[must_use]
pub fn validate(config: &Config) -> ValidationReport {
    validate_full(config, &ValidateOptions::default())
}

/// Static validation plus optional online probes.
#[must_use]
pub fn validate_full(config: &Config, _opts: &ValidateOptions) -> ValidationReport {
    let mut report = ValidationReport {
        valid: true,
        sources: config.sources.len(),
        errors: Vec::new(),
        warnings: Vec::new(),
        probes: Vec::new(),
    };

    validate_notifiers(config, &mut report);
    validate_database(config, &mut report);
    validate_config_api(config, &mut report);
    validate_team_routing(config, &mut report);
    validate_api(config, &mut report);
    validate_defaults(config, &mut report);
    validate_sources(config, &mut report);

    report
}

/// Run online probes and merge results into the report.
pub async fn probe_sources(
    config: &Config,
    http: &reqwest::Client,
    opts: &ValidateOptions,
) -> Vec<OnlineProbe> {
    let watches = match config.to_watches() {
        Ok(w) => w,
        Err(_) => return Vec::new(),
    };

    let limiter = build_upstream_limiter(config.defaults.upstream_requests_per_minute);
    let mut probes = Vec::new();

    for watch in &watches {
        let source_id = watch.provider.id().to_owned();
        if let Some(filter) = &opts.source_filter {
            if &source_id != filter {
                continue;
            }
        }

        if let Some(lim) = &limiter {
            lim.until_ready().await;
        }

        let kind = watch.provider.kind().to_owned();
        match watch.provider.fetch(http, None).await {
            Ok(outcome) => {
                let count = outcome.releases.len();
                probes.push(OnlineProbe {
                    source_id,
                    kind,
                    ok: true,
                    releases: Some(count),
                    not_modified: outcome.not_modified,
                    error: None,
                });
            }
            Err(err) => probes.push(OnlineProbe {
                source_id,
                kind,
                ok: false,
                releases: None,
                not_modified: false,
                error: Some(err.to_string()),
            }),
        }
    }

    probes
}

/// Merge online probe failures into a validation report.
pub fn merge_probes(report: &mut ValidationReport, probes: Vec<OnlineProbe>) {
    for probe in &probes {
        if !probe.ok {
            report.error(format!(
                "online probe `{}` failed: {}",
                probe.source_id,
                probe.error.as_deref().unwrap_or("unknown error")
            ));
        }
    }
    report.probes = probes;
}

fn validate_notifiers(config: &Config, report: &mut ValidationReport) {
    // No sinks: GitOps (`source = local`) must fail closed; API/UI authoring
    // (`source = api`) boots idle until the first apply — same as empty sources.
    if config.notifiers.is_empty() {
        let message =
            "no notifiers configured: add one or more `[[notifiers]]` / `notifiers:` entries";
        if config.config_api.ledger_is_bootable() {
            report.warn(format!(
                "{message} — API/UI mode: process will idle until delivery channels are applied"
            ));
        } else {
            report.error(message);
        }
    }

    if config.apprise != crate::config::AppriseConfig::default() {
        report.error(
            "top-level `[apprise]` / `apprise:` is removed; use `[[notifiers]]` with `type = \"apprise\"`",
        );
    }

    for (index, notifier) in config.notifiers.iter().enumerate() {
        validate_notifier_entry(index, notifier, report);
    }
}

fn validate_apprise_fields(apprise: &crate::config::AppriseConfig, report: &mut ValidationReport) {
    match apprise.format.as_str() {
        "markdown" | "text" | "html" => {}
        other => report.error(format!(
            "apprise.format must be markdown|text|html, got `{other}`"
        )),
    }
    if apprise.endpoint.trim().is_empty() {
        report.error("apprise.endpoint must not be empty");
    }
}

fn validate_notifier_entry(
    index: usize,
    notifier: &crate::config::NotifierConfig,
    report: &mut ValidationReport,
) {
    use crate::config::NotifierConfig;
    let prefix = format!("notifiers[{index}]");
    match notifier {
        NotifierConfig::Apprise(cfg) => {
            if !cfg.is_configured() {
                report.error(format!(
                    "{prefix} (apprise): set `urls`, `urls_env`, or `config_key`"
                ));
            }
            validate_apprise_fields(cfg, report);
        }
        NotifierConfig::Webhook(cfg) => {
            if !secret_configured(&cfg.url, cfg.url_env.as_deref(), "") {
                report.error(format!("{prefix} (webhook): set `url` or `url_env`"));
            }
        }
        NotifierConfig::Express(cfg) => {
            if cfg.base_url.trim().is_empty() {
                report.error(format!("{prefix} (express): `base_url` must not be empty"));
            }
            if cfg.group_chat_id.trim().is_empty() {
                report.error(format!(
                    "{prefix} (express): `group_chat_id` must not be empty"
                ));
            }
            if !secret_configured(
                &cfg.access_token,
                cfg.access_token_env.as_deref(),
                "XRELEASE_EXPRESS_ACCESS_TOKEN",
            ) {
                report.error(format!(
                    "{prefix} (express): set `access_token` or `access_token_env` \
                     (Bearer for POST /api/v4/botx/notifications/direct) — HMAC/GET /token is not used"
                ));
            }
        }
        NotifierConfig::Novu(cfg) => {
            if cfg.base_url.trim().is_empty() {
                report.error(format!("{prefix} (novu): `base_url` must not be empty"));
            }
            if cfg.workflow.trim().is_empty() {
                report.error(format!("{prefix} (novu): `workflow` must not be empty"));
            }
            let topic = cfg
                .topic_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let subscriber = cfg
                .subscriber_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());
            if topic.is_none() && subscriber.is_none() {
                report.error(format!(
                    "{prefix} (novu): set `topic_key` or `subscriber_id` \
                     (Novu Topic or Subscriber target)"
                ));
            }
            if !secret_configured(
                &cfg.api_key,
                cfg.api_key_env.as_deref(),
                "XRELEASE_NOVU_API_KEY",
            ) {
                report.error(format!(
                    "{prefix} (novu): set `api_key` or `api_key_env` (or `XRELEASE_NOVU_API_KEY`)"
                ));
            }
        }
        NotifierConfig::Slack(cfg) => {
            let has_webhook = secret_configured(
                &cfg.webhook_url,
                cfg.webhook_url_env.as_deref(),
                "XRELEASE_SLACK_WEBHOOK_URL",
            );
            let has_bot = secret_configured(
                &cfg.bot_token,
                cfg.bot_token_env.as_deref(),
                "XRELEASE_SLACK_BOT_TOKEN",
            );
            match (has_webhook, has_bot) {
                (true, true) => report.error(format!(
                    "{prefix} (slack): set either `webhook_url`/`webhook_url_env` or \
                     `bot_token`/`bot_token_env`+`channel`, not both"
                )),
                (false, false) => report.error(format!(
                    "{prefix} (slack): set `webhook_url` (or env) or `bot_token`+`channel` (or env)"
                )),
                (false, true) if cfg.channel.trim().is_empty() => report.error(format!(
                    "{prefix} (slack): `channel` must not be empty when using `bot_token`"
                )),
                _ => {}
            }
        }
        NotifierConfig::Telegram(cfg) => {
            if cfg.chat_id.trim().is_empty() {
                report.error(format!("{prefix} (telegram): `chat_id` must not be empty"));
            }
            if !secret_configured(
                &cfg.bot_token,
                cfg.bot_token_env.as_deref(),
                "XRELEASE_TELEGRAM_BOT_TOKEN",
            ) {
                report.error(format!(
                    "{prefix} (telegram): set `bot_token` or `bot_token_env` \
                     (or `XRELEASE_TELEGRAM_BOT_TOKEN`)"
                ));
            }
            if let Some(mode) = cfg
                .parse_mode
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                match mode {
                    "Markdown" | "MarkdownV2" | "HTML" => {}
                    other => report.error(format!(
                        "{prefix} (telegram): `parse_mode` must be Markdown|MarkdownV2|HTML, \
                         got `{other}`"
                    )),
                }
            }
        }
        NotifierConfig::Smtp(cfg) => {
            if cfg.host.trim().is_empty() {
                report.error(format!("{prefix} (smtp): `host` must not be empty"));
            }
            if cfg.from.trim().is_empty() {
                report.error(format!("{prefix} (smtp): `from` must not be empty"));
            }
            if cfg.to.is_empty() {
                report.error(format!(
                    "{prefix} (smtp): `to` must contain at least one address"
                ));
            }
            let has_password = secret_configured(
                &cfg.password,
                cfg.password_env.as_deref(),
                "XRELEASE_SMTP_PASSWORD",
            );
            if cfg.username.is_some() && !has_password {
                report.warn(format!(
                    "{prefix} (smtp): `username` is set but no password — set `password`, \
                     `password_env`, or `XRELEASE_SMTP_PASSWORD`"
                ));
            }
        }
        NotifierConfig::Kafka(cfg) => {
            if cfg.brokers.is_empty() {
                report.error(format!("{prefix} (kafka): `brokers` must not be empty"));
            }
            if cfg.topic.trim().is_empty() {
                report.error(format!("{prefix} (kafka): `topic` must not be empty"));
            }
        }
        NotifierConfig::Nats(cfg) => {
            if !secret_configured(&cfg.url, cfg.url_env.as_deref(), "XRELEASE_NATS_URL") {
                report.error(format!(
                    "{prefix} (nats): set `url` or `url_env` (or `XRELEASE_NATS_URL`)"
                ));
            }
            if cfg.subject.trim().is_empty() {
                report.error(format!("{prefix} (nats): `subject` must not be empty"));
            }
        }
        NotifierConfig::Rabbitmq(cfg) => {
            if !secret_configured(&cfg.url, cfg.url_env.as_deref(), "XRELEASE_RABBITMQ_URL") {
                report.error(format!(
                    "{prefix} (rabbitmq): set `url` or `url_env` (or `XRELEASE_RABBITMQ_URL`)"
                ));
            }
            if cfg.routing_key.trim().is_empty() {
                report.error(format!(
                    "{prefix} (rabbitmq): `routing_key` must not be empty"
                ));
            }
        }
    }
}

fn secret_present(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty() && trimmed != "<redacted>" && !trimmed.contains("<redacted>")
}

/// True when a `*_env` ref name is declared (GitOps / UI). The value may arrive
/// later via process env or `app_secret` — structural validity does not require
/// it to be resolvable at validate time.
fn secret_env_named(named: Option<&str>) -> bool {
    named.map(str::trim).is_some_and(|name| !name.is_empty())
}

/// Inline value, named ref, or resolvable global (process env / vault).
fn secret_configured(inline: &str, named: Option<&str>, global: &str) -> bool {
    secret_present(inline)
        || secret_env_named(named)
        || (!global.is_empty() && crate::config::env_token(global).is_some())
}

fn validate_database(config: &Config, report: &mut ValidationReport) {
    let configured = !config.database.postgres_url.trim().is_empty();
    let from_env = std::env::var("XRELEASE_DATABASE_URL").is_ok_and(|url| !url.trim().is_empty());
    if !configured && !from_env {
        report.error("database.postgres_url is required (or set XRELEASE_DATABASE_URL)");
    }

    if config.database.prune_interval_hours > 0
        && config.database.prune_seen_after_days == 0
        && config.database.prune_webhooks_after_days == 0
    {
        report.warn(
            "database.prune_interval_hours is set but no prune_*_after_days — periodic prune will not run",
        );
    }
}

/// Enforce coherent `[config_api]` flags.
///
/// Supported operator profiles:
/// - **Local / GitOps:** `api_config = false`, `source = local` (`ui_config` off)
/// - **API / CI:** `api_config = true`, `source = api`, `ui_config = false`
/// - **UI editor:** `api_config = true`, `source = api`, `ui_config = true`
///
/// `api_config = true` + `source = local` remains allowed (routes mounted, apply
/// returns 409) for operators who want reload APIs without ledger authority.
fn validate_config_api(config: &Config, report: &mut ValidationReport) {
    use crate::config::ConfigSource;

    let ca = &config.config_api;

    if matches!(ca.source, ConfigSource::Api) && !ca.api_config {
        report.error(
            "config_api: source = \"api\" requires api_config = true \
             (ledger authority needs apply/rollback routes)",
        );
    }

    if ca.ui_config && !(ca.api_config && matches!(ca.source, ConfigSource::Api)) {
        report
            .error("config_api: ui_config = true requires api_config = true and source = \"api\"");
    }

    if matches!(ca.source, ConfigSource::Api) && !crate::crypto::LedgerCipher::key_env_present() {
        if crate::crypto::LedgerCipher::plaintext_ledger_allowed() {
            report.warn(
                "config_api: source = \"api\" with XRELEASE_ALLOW_PLAINTEXT_CONFIG_LEDGER — \
                 app_secret values are stored as plaintext; set \
                 XRELEASE_CONFIG_ENCRYPTION_KEY for production",
            );
        } else {
            report.error(
                "config_api: source = \"api\" requires XRELEASE_CONFIG_ENCRYPTION_KEY \
                 (AES-256-GCM seals app_secret values). Generate with \
                 `openssl rand -base64 32`. Lab-only escape: \
                 XRELEASE_ALLOW_PLAINTEXT_CONFIG_LEDGER=1",
            );
        }
    }
}

fn validate_team_routing(config: &Config, report: &mut ValidationReport) {
    use std::collections::HashSet;

    let catalog: HashSet<String> = config.teams.iter().map(|team| team.tag.clone()).collect();

    let mut notifier_tags: HashSet<String> = HashSet::new();
    let mut wildcard_notifier = false;

    for notifier in &config.notifiers {
        let tags = notifier.routing_tags();
        if tags.is_empty() {
            wildcard_notifier = true;
        } else {
            notifier_tags.extend(tags);
        }
    }

    for team in &config.teams {
        if team.tag.trim().is_empty() {
            report.error("teams[].tag must not be empty");
        }
    }

    let effective_presets = crate::config::effective_source_presets(&config.presets);
    let mut used_team_tags: HashSet<String> = HashSet::new();

    for source in &config.sources {
        let id = crate::config::source_label(source);
        // Effective tag after preset merge (unknown preset → into_watches error elsewhere).
        let effective_tag = crate::config::source_common(source)
            .clone()
            .with_preset_resolved(&effective_presets, &id)
            .ok()
            .and_then(|common| common.routing_tag)
            .or_else(|| crate::config::source_routing_tag(source).map(str::to_owned));

        let Some(tag) = effective_tag else {
            if !wildcard_notifier && !notifier_tags.is_empty() {
                report.warn(format!(
                    "source `{id}` has no routing_tag — only wildcard notifiers will receive its events"
                ));
            }
            continue;
        };
        if tag.trim().is_empty() {
            report.warn(format!("source `{id}` has an empty routing_tag"));
            continue;
        }
        used_team_tags.insert(tag.clone());
        if !catalog.is_empty() && !catalog.contains(&tag) {
            report.warn(format!(
                "source `{id}` uses routing_tag `{tag}` which is not listed in [[teams]]"
            ));
        }
        if !wildcard_notifier && !notifier_tags.contains(&tag) {
            let mut available = notifier_tags.iter().cloned().collect::<Vec<_>>();
            available.sort();
            let available = if available.is_empty() {
                "(none — add tags on a channel, or leave a channel’s tags empty for wildcard)"
                    .into()
            } else {
                available.join(", ")
            };
            let message = format!(
                "source `{id}` routing_tag `{tag}` has no matching notifier — \
                 channel tags: [{available}]; select `{tag}` on a channel, or clear a channel’s tags (wildcard)"
            );
            // Incremental authoring: teams + sources without any delivery channel
            // yet is a supported idle variant (API/UI). Once at least one channel
            // exists, a tagged source with no match is a hard misconfiguration.
            let has_any_channel = !config.notifiers.is_empty();
            if has_any_channel {
                report.error(message);
            } else {
                report.warn(format!(
                    "{message} — no delivery channels yet; notifications will start after a channel is applied"
                ));
            }
        }
    }

    if let Some(tag) = config
        .defaults
        .ops_routing_tag
        .as_deref()
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
    {
        used_team_tags.insert(tag.to_owned());
        if !catalog.is_empty() && !catalog.contains(tag) {
            report.warn(format!(
                "defaults.ops_routing_tag `{tag}` is not listed in [[teams]]"
            ));
        }
        if !wildcard_notifier && !notifier_tags.contains(tag) {
            let has_any_channel = !config.notifiers.is_empty();
            let message = format!(
                "defaults.ops_routing_tag `{tag}` has no matching notifier — \
                 meta-alerts on dead outbox / tripped breakers will fail to deliver"
            );
            if has_any_channel {
                report.error(message);
            } else {
                report.warn(format!(
                    "{message} — no delivery channels yet; set a matching channel before relying on ops alerts"
                ));
            }
        }
    }

    for team in &config.teams {
        let tag = team.tag.trim();
        if tag.is_empty() {
            continue;
        }
        if !used_team_tags.contains(tag) && !notifier_tags.contains(tag) {
            report.warn(format!(
                "teams tag `{tag}` is unused by sources and notifiers"
            ));
        }
    }
}

fn validate_api(config: &Config, report: &mut ValidationReport) {
    if config.api.listen.parse::<SocketAddr>().is_err() {
        report.error(format!(
            "api.listen is not a valid socket address: `{}`",
            config.api.listen
        ));
    }

    let has_webhook_secret =
        config.api.webhook_secret.is_some() || std::env::var("XRELEASE_WEBHOOK_SECRET").is_ok();
    if !has_webhook_secret {
        report.warn("api.webhook_secret is not set — webhook signature verification is disabled");
    }

    let has_api_key = config.api.api_key.is_some() || std::env::var("XRELEASE_API_KEY").is_ok();
    let has_oidc = config
        .api
        .oidc
        .issuer
        .as_ref()
        .is_some_and(|value| !value.is_empty())
        || std::env::var("XRELEASE_OIDC_ISSUER").is_ok_and(|value| !value.is_empty());
    let has_local = config.api.local_auth_configured()
        || std::env::var("XRELEASE_SESSION_SECRET").is_ok_and(|value| !value.is_empty());
    if !has_api_key && !has_oidc && !has_local {
        if config.api.require_auth {
            report.error(
                "api.require_auth is true but api.api_key, api.oidc, and local session \
                 secret are unset — set a bearer secret, OIDC issuer, or \
                 XRELEASE_SESSION_SECRET (or set require_auth = false for lab-only)",
            );
        } else {
            report.warn(
                "api.require_auth is false and no credentials are set — management \
                 routes are unauthenticated (default is require_auth = true)",
            );
        }
    }

    if config.api.rate_limit_per_minute == 0 {
        report.warn("api.rate_limit_per_minute = 0 disables HTTP rate limiting");
    }
}

fn validate_defaults(config: &Config, report: &mut ValidationReport) {
    if config.defaults.interval_secs == 0 {
        report.error("defaults.interval_secs must be > 0");
    }
    if config.defaults.upstream_requests_per_minute == 0 && config.sources.len() > 20 {
        report.warn(
            "defaults.upstream_requests_per_minute = 0 with many sources — consider setting a global upstream cap (e.g. 60)",
        );
    }
}

fn validate_sources(config: &Config, report: &mut ValidationReport) {
    if config.sources.is_empty() {
        report.warn("no [[sources]] configured — backend will idle");
    }

    let effective_presets = crate::config::effective_source_presets(&config.presets);
    let mut used_presets = HashSet::new();
    for source in &config.sources {
        if let Some(name) = crate::config::source_common(source)
            .preset
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            used_presets.insert(name.to_owned());
            if !effective_presets.contains_key(name) {
                report.error(format!(
                    "source `{}`: unknown preset `{name}` \
                     (not a built-in and not listed under `presets`)",
                    crate::config::source_label(source)
                ));
            }
        }
    }
    // Only warn about *user*-defined presets that nothing references —
    // built-ins are always present and need not be used.
    for name in config.presets.keys() {
        if !used_presets.contains(name) {
            report.warn(format!("preset `{name}` is unused by any source"));
        }
    }

    let watches = match config.to_watches() {
        Ok(w) => w,
        Err(err) => {
            report.error(format!("failed to build watches: {err}"));
            return;
        }
    };

    let mut ids = HashSet::new();
    for watch in &watches {
        if !ids.insert(watch.provider.id()) {
            report.error(format!(
                "duplicate source id `{}` — set explicit `id` on one of the blocks",
                watch.provider.id()
            ));
        }

        if watch.interval.as_secs() == 0 {
            report.error(format!(
                "source `{}`: interval_secs must be > 0",
                watch.provider.id()
            ));
        }
    }

    for source in &config.sources {
        if let Some(msg) = source.lint(&effective_presets) {
            report.warn(msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::crypto::ledger::{ALLOW_PLAINTEXT_ENV, ENCRYPTION_KEY_ENV};

    /// Shared fixtures: DB URL + open management API for unit tests that are
    /// not about auth. Production default is `require_auth = true`.
    const TEST_DATABASE: &str = r#"
        [database]
        postgres_url = "postgres://xrelease:xrelease@127.0.0.1:5432/xrelease_test"

        [api]
        require_auth = false
    "#;

    /// Base64 of `0123456789abcdef0123456789abcdef` — unit-test ledger key only.
    const TEST_LEDGER_KEY_B64: &str = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";

    fn with_test_ledger_encryption_key<R>(f: impl FnOnce() -> R) -> R {
        let prev_key = std::env::var(ENCRYPTION_KEY_ENV).ok();
        let prev_allow = std::env::var(ALLOW_PLAINTEXT_ENV).ok();
        std::env::remove_var(ALLOW_PLAINTEXT_ENV);
        std::env::set_var(ENCRYPTION_KEY_ENV, TEST_LEDGER_KEY_B64);
        let out = f();
        match prev_key {
            Some(value) => std::env::set_var(ENCRYPTION_KEY_ENV, value),
            None => std::env::remove_var(ENCRYPTION_KEY_ENV),
        }
        match prev_allow {
            Some(value) => std::env::set_var(ALLOW_PLAINTEXT_ENV, value),
            None => std::env::remove_var(ALLOW_PLAINTEXT_ENV),
        }
        out
    }

    /// Clear management-auth env vars so unit tests observe config-only credentials.
    fn without_management_auth_env<R>(f: impl FnOnce() -> R) -> R {
        const KEYS: &[&str] = &[
            "XRELEASE_API_KEY",
            "XRELEASE_OIDC_ISSUER",
            "XRELEASE_SESSION_SECRET",
            "XRELEASE_WEBHOOK_SECRET",
        ];
        let prev: Vec<(String, Option<String>)> = KEYS
            .iter()
            .map(|key| ((*key).to_owned(), std::env::var(key).ok()))
            .collect();
        for key in KEYS {
            std::env::remove_var(key);
        }
        let out = f();
        for (key, value) in prev {
            match value {
                Some(value) => std::env::set_var(&key, value),
                None => std::env::remove_var(&key),
            }
        }
        out
    }

    fn without_ledger_encryption_key<R>(f: impl FnOnce() -> R) -> R {
        let prev_key = std::env::var(ENCRYPTION_KEY_ENV).ok();
        let prev_allow = std::env::var(ALLOW_PLAINTEXT_ENV).ok();
        std::env::remove_var(ENCRYPTION_KEY_ENV);
        std::env::remove_var(ALLOW_PLAINTEXT_ENV);
        let out = f();
        match prev_key {
            Some(value) => std::env::set_var(ENCRYPTION_KEY_ENV, value),
            None => std::env::remove_var(ENCRYPTION_KEY_ENV),
        }
        match prev_allow {
            Some(value) => std::env::set_var(ALLOW_PLAINTEXT_ENV, value),
            None => std::env::remove_var(ALLOW_PLAINTEXT_ENV),
        }
        out
    }

    #[test]
    fn validate_should_fail_without_any_notifier() {
        let config: Config = toml::from_str(
            r#"
            [[sources]]
            type = "github"
            repo = "a/b"
        "#,
        )
        .expect("parse");
        let report = validate(&config);
        assert!(!report.valid);
        assert!(report.errors.iter().any(|e| e.contains("notifiers")));
    }

    #[test]
    fn validate_should_warn_without_notifiers_in_api_mode() {
        with_test_ledger_encryption_key(|| {
            let config: Config = toml::from_str(&format!(
                r#"
            {TEST_DATABASE}

            [config_api]
            api_config = true
            source = "api"
            ui_config = true

            [[sources]]
            type = "github"
            repo = "a/b"
        "#
            ))
            .expect("parse");
            let report = validate(&config);
            assert!(
                report.valid,
                "API/UI idle boot must not fail closed on empty sinks: {:?}",
                report.errors
            );
            assert!(report.warnings.iter().any(|w| w.contains("notifiers")));
        });
    }

    #[test]
    fn validate_should_error_api_source_without_encryption_key() {
        without_ledger_encryption_key(|| {
            let config: Config = toml::from_str(&format!(
                r#"
            {TEST_DATABASE}

            [config_api]
            api_config = true
            source = "api"

            [[notifiers]]
            type = "webhook"
            url = "https://hooks.example.com/x"

            [[sources]]
            type = "github"
            repo = "a/b"
        "#
            ))
            .expect("parse");
            let report = validate(&config);
            assert!(!report.valid);
            assert!(
                report
                    .errors
                    .iter()
                    .any(|e| e.contains("XRELEASE_CONFIG_ENCRYPTION_KEY")),
                "errors: {:?}",
                report.errors
            );
        });
    }

    #[test]
    fn validate_should_pass_with_webhook_only() {
        let config: Config = toml::from_str(&format!(
            r#"
            {TEST_DATABASE}

            [[notifiers]]
            type = "webhook"
            url = "https://hooks.example.com/xrelease"

            [[sources]]
            type = "github"
            repo = "a/b"
        "#
        ))
        .expect("parse");
        let report = validate(&config);
        assert!(report.valid, "errors: {:?}", report.errors);
    }

    #[test]
    fn validate_should_error_novu_without_target_or_key() {
        let config: Config = toml::from_str(
            r#"
            [[notifiers]]
            type = "novu"
            workflow = "xrelease-new-release"

            [[sources]]
            type = "github"
            repo = "a/b"
        "#,
        )
        .expect("parse");
        let report = validate(&config);
        assert!(!report.valid);
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("topic_key") || e.contains("subscriber_id")),
            "errors: {:?}",
            report.errors
        );
        assert!(
            report.errors.iter().any(|e| e.contains("api_key")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_should_pass_novu_minimal() {
        let config: Config = toml::from_str(&format!(
            r#"
            {TEST_DATABASE}

            [[notifiers]]
            type = "novu"
            workflow = "xrelease-new-release"
            topic_key = "platform-team"
            api_key = "nv_test"

            [[sources]]
            type = "github"
            repo = "a/b"
        "#
        ))
        .expect("parse");
        let report = validate(&config);
        assert!(report.valid, "errors: {:?}", report.errors);
    }

    #[test]
    fn validate_should_error_slack_without_mode() {
        let config: Config = toml::from_str(
            r#"
            [[notifiers]]
            type = "slack"

            [[sources]]
            type = "github"
            repo = "a/b"
        "#,
        )
        .expect("parse");
        let report = validate(&config);
        assert!(!report.valid);
        assert!(
            report.errors.iter().any(|e| e.contains("slack")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_should_pass_slack_webhook_and_telegram() {
        let config: Config = toml::from_str(&format!(
            r#"
            {TEST_DATABASE}

            [[notifiers]]
            type = "slack"
            webhook_url = "https://hooks.slack.com/services/T/B/X"

            [[notifiers]]
            type = "telegram"
            chat_id = "-1001"
            bot_token = "123:ABC"

            [[sources]]
            type = "github"
            repo = "a/b"
        "#
        ))
        .expect("parse");
        let report = validate(&config);
        assert!(report.valid, "errors: {:?}", report.errors);
    }

    #[test]
    fn validate_should_error_when_require_auth_without_credentials() {
        without_management_auth_env(|| {
            let config: Config = toml::from_str(
                r#"
            [database]
            postgres_url = "postgres://xrelease:xrelease@127.0.0.1:5432/xrelease_test"

            [api]
            require_auth = true

            [[notifiers]]
            type = "apprise"
            urls = ["mailto://u:p@example.com"]

            [[sources]]
            type = "github"
            repo = "a/b"
        "#,
            )
            .expect("parse");
            let report = validate(&config);
            assert!(!report.valid);
            assert!(
                report
                    .errors
                    .iter()
                    .any(|e| e.contains("require_auth") && e.contains("unset")),
                "errors: {:?}",
                report.errors
            );
        });
    }

    #[test]
    fn validate_should_warn_when_management_api_is_open() {
        without_management_auth_env(|| {
            let config: Config = toml::from_str(&format!(
                r#"
            {TEST_DATABASE}

            [[notifiers]]
            type = "apprise"
            urls = ["mailto://u:p@example.com"]

            [[sources]]
            type = "github"
            repo = "a/b"
        "#
            ))
            .expect("parse");
            let report = validate(&config);
            assert!(report.valid, "errors: {:?}", report.errors);
            assert!(
                report
                    .warnings
                    .iter()
                    .any(|w| w.contains("unauthenticated")),
                "warnings: {:?}",
                report.warnings
            );
        });
    }

    #[test]
    fn validate_should_pass_with_express_only() {
        let config: Config = toml::from_str(&format!(
            r#"
            {TEST_DATABASE}

            [[notifiers]]
            type = "express"
            base_url = "https://cts.example.com"
            access_token = "permanent-bearer"
            group_chat_id = "dec60c05-77b7-0d78-159e-b4fbee4d48f6"

            [[sources]]
            type = "github"
            repo = "a/b"
        "#
        ))
        .expect("parse");
        let report = validate(&config);
        assert!(report.valid, "errors: {:?}", report.errors);
    }

    #[test]
    fn validate_should_fail_without_apprise_targets() {
        let config: Config = toml::from_str(
            r#"
            [[sources]]
            type = "github"
            repo = "a/b"
        "#,
        )
        .expect("parse");
        let report = validate(&config);
        assert!(!report.valid);
        assert!(
            report.errors.iter().any(|e| e.contains("notifiers")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_should_pass_minimal_config() {
        let config: Config = toml::from_str(&format!(
            r#"
            {TEST_DATABASE}

            [[notifiers]]
            type = "apprise"
            urls = ["mailto://u:p@example.com"]

            [[sources]]
            type = "github"
            repo = "a/b"
        "#
        ))
        .expect("parse");
        let report = validate(&config);
        assert!(report.valid, "errors: {:?}", report.errors);
        assert_eq!(report.sources, 1);
    }

    #[test]
    fn strict_should_promote_warnings_to_errors() {
        let config: Config = toml::from_str(
            r#"
            [[notifiers]]
            type = "apprise"
            urls = ["mailto://u:p@example.com"]

            [[sources]]
            type = "docker"
            image = "library/nginx"
        "#,
        )
        .expect("parse");
        let mut report = validate(&config);
        assert!(!report.warnings.is_empty(), "expected docker lint warning");
        report.apply_strict(true);
        assert!(!report.valid);
    }

    #[test]
    fn validate_should_parse_bitbucket_source() {
        let config: Config = toml::from_str(&format!(
            r#"
            {TEST_DATABASE}

            [[notifiers]]
            type = "apprise"
            urls = ["mailto://u:p@example.com"]

            [[sources]]
            type = "bitbucket"
            repo = "atlassian/python-bitbucket"
        "#
        ))
        .expect("parse");
        let report = validate(&config);
        assert!(report.valid, "errors: {:?}", report.errors);
        let watches = config.into_watches().expect("watches");
        assert_eq!(watches[0].provider.kind(), "bitbucket");
    }

    #[test]
    fn validate_should_parse_bitbucket_server() {
        let config: Config = toml::from_str(
            r#"
            [[notifiers]]
            type = "apprise"
            urls = ["mailto://u:p@example.com"]

            [[sources]]
            type = "bitbucket"
            edition = "server"
            host = "https://bitbucket.example.com"
            repo = "PROJ/app"
        "#,
        )
        .expect("parse");
        let watches = config.into_watches().expect("watches");
        assert_eq!(watches[0].provider.kind(), "bitbucket");
    }

    #[test]
    fn validate_should_parse_yarn_cpan_and_ecr() {
        let config: Config = toml::from_str(&format!(
            r#"
            {TEST_DATABASE}

            [[notifiers]]
            type = "apprise"
            urls = ["mailto://u:p@example.com"]

            [[sources]]
            type = "yarn"
            name = "lodash"

            [[sources]]
            type = "cpan"
            name = "Moose"

            [[sources]]
            type = "ecr"
            image = "docker/library/nginx"
            pattern = '^\d+\.\d+'
        "#
        ))
        .expect("parse");
        let report = validate(&config);
        assert!(report.valid, "errors: {:?}", report.errors);
        let watches = config.into_watches().expect("watches");
        assert_eq!(watches[0].provider.kind(), "yarn");
        assert_eq!(watches[1].provider.kind(), "cpan");
        assert_eq!(watches[2].provider.kind(), "ecr");
    }

    #[test]
    fn validate_should_accept_builtin_presets() {
        let config: Config = toml::from_str(&format!(
            r#"
            {TEST_DATABASE}

            [[notifiers]]
            type = "apprise"
            urls = ["mailto://u:p@example.com"]

            [[teams]]
            tag = "platform"

            [[sources]]
            type = "github"
            repo = "org/a"
            preset = "semver"
            routing_tag = "platform"

            [[sources]]
            type = "docker"
            image = "library/nginx"
            preset = "numeric"
            routing_tag = "platform"
        "#
        ))
        .expect("parse");
        let report = validate(&config);
        assert!(report.valid, "errors: {:?}", report.errors);
        let watches = config.into_watches().expect("watches");
        assert!(watches[0]
            .filter
            .accepts(&crate::model::Release::new("v1.0.0")));
        assert!(watches[1]
            .filter
            .accepts(&crate::model::Release::new("1.27.0")));
        assert!(!watches[1]
            .filter
            .accepts(&crate::model::Release::new("latest")));
    }

    #[test]
    fn validate_should_accept_source_presets() {
        let config: Config = toml::from_str(&format!(
            r#"
            {TEST_DATABASE}

            [[notifiers]]
            type = "apprise"
            urls = ["mailto://u:p@example.com"]

            [presets.semver]
            pattern = '^v?\d+\.\d+\.\d+$'
            routing_tag = "platform"

            [[teams]]
            tag = "platform"

            [[sources]]
            type = "github"
            repo = "org/a"
            preset = "semver"
        "#
        ))
        .expect("parse");
        let report = validate(&config);
        assert!(report.valid, "errors: {:?}", report.errors);
        let watches = config.into_watches().expect("watches");
        assert_eq!(watches[0].routing_tag.as_deref(), Some("platform"));
    }

    #[test]
    fn validate_should_error_on_unknown_preset() {
        let config: Config = toml::from_str(
            r#"
            [[notifiers]]
            type = "apprise"
            urls = ["mailto://u:p@example.com"]

            [[sources]]
            type = "github"
            repo = "org/a"
            preset = "missing"
        "#,
        )
        .expect("parse");
        let report = validate(&config);
        assert!(!report.valid);
        assert!(
            report.errors.iter().any(|e| e.contains("unknown preset")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_should_error_on_top_level_apprise() {
        let config: Config = toml::from_str(
            r#"
            [apprise]
            urls = ["mailto://u:p@example.com"]

            [[notifiers]]
            type = "apprise"
            urls = ["mailto://other@example.com"]

            [[sources]]
            type = "github"
            repo = "org/a"
        "#,
        )
        .expect("parse");
        let report = validate(&config);
        assert!(!report.valid);
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("top-level `[apprise]`") && e.contains("notifiers")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_should_warn_not_error_when_sources_have_no_channels_yet() {
        with_test_ledger_encryption_key(|| {
            let config: Config = toml::from_str(&format!(
                r#"
            {TEST_DATABASE}

            [config_api]
            api_config = true
            source = "api"

            [[teams]]
            tag = "security::dv"

            [[sources]]
            type = "github"
            repo = "github/wazuh"
            routing_tag = "security::dv"
        "#
            ))
            .expect("parse");
            let report = validate(&config);
            assert!(report.valid, "errors: {:?}", report.errors);
            assert!(
                report
                    .warnings
                    .iter()
                    .any(|w| w.contains("no matching notifier")),
                "warnings: {:?}",
                report.warnings
            );
        });
    }

    #[test]
    fn validate_should_error_orphan_tag_when_channels_exist() {
        let config: Config = toml::from_str(&format!(
            r#"
            {TEST_DATABASE}

            [[notifiers]]
            type = "webhook"
            url = "https://hooks.example/x"
            tags = ["other-team"]

            [[sources]]
            type = "github"
            repo = "github/wazuh"
            routing_tag = "security::dv"
        "#
        ))
        .expect("parse");
        let report = validate(&config);
        assert!(!report.valid);
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("no matching notifier")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_should_error_ops_routing_tag_without_matching_notifier() {
        let config: Config = toml::from_str(&format!(
            r#"
            {TEST_DATABASE}

            [defaults]
            ops_routing_tag = "ops"

            [[notifiers]]
            type = "webhook"
            url = "https://hooks.example/x"
            tags = ["platform"]

            [[sources]]
            type = "github"
            repo = "org/app"
            routing_tag = "platform"
        "#
        ))
        .expect("parse");
        let report = validate(&config);
        assert!(!report.valid);
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("ops_routing_tag") && e.contains("ops")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_should_accept_ops_routing_tag_with_matching_notifier() {
        let config: Config = toml::from_str(&format!(
            r#"
            {TEST_DATABASE}

            [defaults]
            ops_routing_tag = "ops"

            [[teams]]
            tag = "ops"

            [[teams]]
            tag = "platform"

            [[notifiers]]
            type = "webhook"
            url = "https://hooks.example/x"
            tags = ["ops", "platform"]

            [[sources]]
            type = "github"
            repo = "org/app"
            routing_tag = "platform"
        "#
        ))
        .expect("parse");
        let report = validate(&config);
        assert!(report.valid, "errors: {:?}", report.errors);
    }

    #[test]
    fn validate_should_warn_ops_routing_tag_missing_from_teams() {
        let config: Config = toml::from_str(&format!(
            r#"
            {TEST_DATABASE}

            [defaults]
            ops_routing_tag = "ops"

            [[teams]]
            tag = "platform"

            [[notifiers]]
            type = "webhook"
            url = "https://hooks.example/x"
            tags = ["ops", "platform"]

            [[sources]]
            type = "github"
            repo = "org/app"
            routing_tag = "platform"
        "#
        ))
        .expect("parse");
        let report = validate(&config);
        assert!(report.valid, "errors: {:?}", report.errors);
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("ops_routing_tag") && w.contains("[[teams]]")),
            "warnings: {:?}",
            report.warnings
        );
    }

    #[test]
    fn validate_should_reject_api_source_without_api_config() {
        let config: Config = toml::from_str(&format!(
            r#"
            {TEST_DATABASE}

            [config_api]
            api_config = false
            source = "api"

            [[notifiers]]
            type = "apprise"
            urls = ["mailto://u:p@example.com"]

            [[sources]]
            type = "github"
            repo = "org/a"
        "#
        ))
        .expect("parse");
        let report = validate(&config);
        assert!(!report.valid);
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("source = \"api\" requires api_config = true")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_should_reject_ui_config_without_api_source() {
        let config: Config = toml::from_str(&format!(
            r#"
            {TEST_DATABASE}

            [config_api]
            api_config = false
            source = "local"
            ui_config = true

            [[notifiers]]
            type = "apprise"
            urls = ["mailto://u:p@example.com"]

            [[sources]]
            type = "github"
            repo = "org/a"
        "#
        ))
        .expect("parse");
        let report = validate(&config);
        assert!(!report.valid);
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("ui_config = true requires")),
            "errors: {:?}",
            report.errors
        );
    }
}
