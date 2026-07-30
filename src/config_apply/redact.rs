//! Secret redaction for `GET /api/v1/config`.

use std::path::Path;

use crate::config::{
    format_from_path, has_desired_state, strip_infra_sections, Config, DesiredFormat,
    EMPTY_ORGANIZATION_DOCUMENT,
};

/// Return a TOML representation of `config` with secrets replaced by placeholders.
#[must_use]
pub fn redact_config_toml(config: &Config) -> String {
    redact_desired_document(None, config)
}

/// Return a redacted desired-state document, preserving YAML vs TOML when `path` is set.
#[must_use]
pub fn redact_desired_document(path: Option<&Path>, config: &Config) -> String {
    let format = path
        .and_then(format_from_path)
        .unwrap_or(DesiredFormat::Toml);
    redact_desired_document_with_format(format, config)
}

/// Return a redacted **full** config document (infra + app) in the given wire format.
#[must_use]
pub fn redact_desired_document_with_format(format: DesiredFormat, config: &Config) -> String {
    let mut redacted = config.clone();
    redact_config(&mut redacted);
    serialize_redacted(format, &redacted)
}

/// Redact secrets and emit **desired-state only** (no `[database]` / `[api]` /
/// `[config_api]` / `[[organizations]]`).
///
/// Parsing `{}` through [`Config`] fills `Default` infra (`source = local`,
/// `ui_config = false`, …). Returning that blob as `desired_content` makes the
/// UI look like GitOps-local mode even when bootstrap is API/UI — so strip
/// infra before serializing. Empty desired documents stay `{}\n`.
#[must_use]
pub fn redact_desired_only_document(format: DesiredFormat, config: &Config) -> String {
    let mut redacted = config.clone();
    redact_config(&mut redacted);
    strip_infra_sections(&mut redacted);
    if !has_desired_state(&redacted) {
        return EMPTY_ORGANIZATION_DOCUMENT.to_owned();
    }
    serialize_desired_only(format, &redacted)
}

fn serialize_redacted(format: DesiredFormat, redacted: &Config) -> String {
    match format {
        DesiredFormat::Yaml => serde_yaml::to_string(redacted)
            .unwrap_or_else(|_| "# failed to serialize config\n".into()),
        DesiredFormat::Toml => toml::to_string_pretty(redacted)
            .unwrap_or_else(|_| "# failed to serialize config\n".into()),
    }
}

/// App-layer sections of a [`Config`], skipping empty/default ones. `Config`
/// always carries infra `Default`s after parse; emitting those makes empty/`{}`
/// docs look like `source = local` / full `[database]` / `[api]` — which is
/// bootstrap, not desired state.
#[derive(serde::Serialize)]
struct DesiredWire<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    defaults: Option<&'a crate::config::Defaults>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    notifiers: &'a Vec<crate::config::NotifierConfig>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    teams: &'a Vec<crate::config::TeamConfig>,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    presets: &'a std::collections::BTreeMap<String, crate::config::SourcePreset>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    sources: &'a Vec<crate::config::SourceConfig>,
}

impl<'a> DesiredWire<'a> {
    fn from_config(config: &'a Config) -> Self {
        Self {
            defaults: (config.defaults != crate::config::Defaults::default())
                .then_some(&config.defaults),
            notifiers: &config.notifiers,
            teams: &config.teams,
            presets: &config.presets,
            sources: &config.sources,
        }
    }
}

/// Serialize app-layer fields only (display path: errors degrade to a comment).
fn serialize_desired_only(format: DesiredFormat, config: &Config) -> String {
    serialize_desired_only_strict(format, config)
        .unwrap_or_else(|_| "# failed to serialize config\n".into())
}

/// Serialize app-layer fields only, propagating errors — the ledger wire
/// format. A serialization failure must abort the apply, not record a
/// `# failed to serialize` comment as an applied revision.
pub(crate) fn serialize_desired_only_strict(
    format: DesiredFormat,
    config: &Config,
) -> anyhow::Result<String> {
    use anyhow::Context;

    let wire = DesiredWire::from_config(config);
    match format {
        DesiredFormat::Yaml => {
            serde_yaml::to_string(&wire).context("serializing desired-state YAML document")
        }
        DesiredFormat::Toml => {
            toml::to_string_pretty(&wire).context("serializing desired-state TOML document")
        }
    }
}

fn redact_config(config: &mut Config) {
    // Connection URLs embed credentials (`postgres://user:pass@host/db`). Any
    // authenticated principal can `GET /api/v1/config` (incl. viewer), so mask
    // userinfo the same way as broker notifier URLs — host/db stay visible for
    // ops, passwords do not.
    if !config.database.postgres_url.trim().is_empty() {
        config.database.postgres_url = redact_url_userinfo(&config.database.postgres_url);
    }
    if config.api.api_key.as_ref().is_some_and(|v| !v.is_empty()) {
        config.api.api_key = Some("<redacted>".into());
    }
    if config
        .api
        .webhook_secret
        .as_ref()
        .is_some_and(|v| !v.is_empty())
    {
        config.api.webhook_secret = Some("<redacted>".into());
    }
    // Local-auth secrets live in the same serialized `[api]` table. Leaking the
    // session secret here is critical: `GET /api/v1/config` is readable by any
    // authenticated principal (incl. a `viewer`), and the HS256 session secret
    // is the whole key to forging an admin session JWT.
    if config
        .api
        .local_auth
        .session_secret
        .as_ref()
        .is_some_and(|v| !v.is_empty())
    {
        config.api.local_auth.session_secret = Some("<redacted>".into());
    }
    if config
        .api
        .local_auth
        .admin_password
        .as_ref()
        .is_some_and(|v| !v.is_empty())
    {
        config.api.local_auth.admin_password = Some("<redacted>".into());
    }
    redact_sources(config);
    redact_notifiers(config);
}

fn redact_sources(config: &mut Config) {
    use crate::config::SourceConfig;

    fn mask_token(token: &mut Option<String>, token_env: &Option<String>) {
        let has_inline = token.as_ref().is_some_and(|value| !value.trim().is_empty());
        let has_env = token_env
            .as_deref()
            .map(str::trim)
            .is_some_and(|name| !name.is_empty());
        if has_inline || has_env {
            *token = Some("<redacted>".into());
        }
    }

    for source in &mut config.sources {
        match source {
            SourceConfig::Github(c) => mask_token(&mut c.token, &c.token_env),
            SourceConfig::Codeberg(c) => mask_token(&mut c.token, &c.token_env),
            SourceConfig::Gitea(c) => mask_token(&mut c.token, &c.token_env),
            SourceConfig::Gitlab(c) => mask_token(&mut c.token, &c.token_env),
            SourceConfig::Bitbucket(c) => mask_token(&mut c.token, &c.token_env),
            SourceConfig::Docker(c) => mask_token(&mut c.token, &c.token_env),
            SourceConfig::Ghcr(c) => mask_token(&mut c.token, &c.token_env),
            SourceConfig::Quay(c) => mask_token(&mut c.token, &c.token_env),
            SourceConfig::Ecr(c) => mask_token(&mut c.token, &c.token_env),
            _ => {}
        }
    }
}

fn redact_notifiers(config: &mut Config) {
    use crate::config::NotifierConfig;

    for notifier in &mut config.notifiers {
        match notifier {
            NotifierConfig::Apprise(n) => {
                if !n.urls.is_empty()
                    || n.urls_env
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|name| !name.is_empty())
                {
                    n.urls = vec!["<redacted>".into()];
                }
            }
            NotifierConfig::Webhook(n) => {
                // Destination URLs often embed path tokens (Discord / Slack-style
                // hooks, signed callback URLs). Mask the whole URL on GET — same
                // posture as Slack `webhook_url` and Apprise target URLs.
                if !n.url.is_empty()
                    || n.url_env
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|name| !name.is_empty())
                {
                    n.url = "<redacted>".into();
                }
                if !n.secret.is_empty()
                    || n.secret_env
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|name| !name.is_empty())
                {
                    n.secret = "<redacted>".into();
                }
                // Header *values* are unconditionally redacted, not just
                // known-sensitive names: `headers` is the documented way to
                // carry a bearer token / API key to the destination
                // (`Authorization`, `X-Api-Key`, …), and there is no reliable
                // way to tell those apart from a benign custom header by name
                // alone. Keys stay visible so an operator can see which
                // headers are configured without exposing what they hold.
                for value in n.headers.values_mut() {
                    *value = "<redacted>".into();
                }
                if n.headers.is_empty()
                    && n.headers_env
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|name| !name.is_empty())
                {
                    n.headers
                        .insert("Authorization".into(), "<redacted>".into());
                }
            }
            NotifierConfig::Express(n) => {
                if !n.access_token.is_empty()
                    || n.access_token_env
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|name| !name.is_empty())
                {
                    n.access_token = "<redacted>".into();
                }
            }
            NotifierConfig::Novu(n) => {
                if !n.api_key.is_empty()
                    || n.api_key_env
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|name| !name.is_empty())
                {
                    n.api_key = "<redacted>".into();
                }
            }
            NotifierConfig::Slack(n) => {
                if !n.webhook_url.is_empty()
                    || n.webhook_url_env
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|name| !name.is_empty())
                {
                    n.webhook_url = "<redacted>".into();
                }
                if !n.bot_token.is_empty()
                    || n.bot_token_env
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|name| !name.is_empty())
                {
                    n.bot_token = "<redacted>".into();
                }
            }
            NotifierConfig::Telegram(n) => {
                if !n.bot_token.is_empty()
                    || n.bot_token_env
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|name| !name.is_empty())
                {
                    n.bot_token = "<redacted>".into();
                }
            }
            NotifierConfig::Smtp(n)
                if !n.password.is_empty()
                    || n.password_env
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|name| !name.is_empty()) =>
            {
                n.password = "<redacted>".into();
            }
            NotifierConfig::Smtp(_) => {}
            // AMQP/NATS URLs commonly embed credentials (`amqp://user:pass@host/vhost`,
            // `nats://user:pass@host:4222`). Redact only the userinfo segment —
            // unlike Apprise's stateless target URLs (redacted wholesale, since
            // the whole URL *is* the secret there), host/vhost/subject here are
            // useful, non-sensitive operational info. Apply restores the full
            // URL when the candidate still contains the `<redacted>` marker.
            NotifierConfig::Rabbitmq(n) => {
                if n.url_env
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|name| !name.is_empty())
                    && n.url.trim().is_empty()
                {
                    n.url = "<redacted>".into();
                } else {
                    n.url = redact_url_userinfo(&n.url);
                }
            }
            NotifierConfig::Nats(n) => {
                if n.url_env
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|name| !name.is_empty())
                    && n.url.trim().is_empty()
                {
                    n.url = "<redacted>".into();
                } else {
                    n.url = redact_url_userinfo(&n.url);
                }
            }
            NotifierConfig::Kafka(_) => {}
        }
    }
}

/// Copy secrets / `*_env` refs from `previous` when `next` has empty or
/// `<redacted>` placeholders.
///
/// After API normalize, live config usually holds **refs only** (empty inline +
/// `*_env`). Restore must therefore reinstate env names as well as prior
/// inline values, or a no-op UI edit would drop vault bindings.
#[must_use]
pub fn restore_redacted_secrets(previous: &Config, next: &mut Config) -> bool {
    use crate::config::{NotifierConfig, SourceConfig};

    fn is_placeholder(value: &str) -> bool {
        let trimmed = value.trim();
        trimmed.is_empty() || trimmed == "<redacted>"
    }

    fn env_blank(value: &Option<String>) -> bool {
        value
            .as_deref()
            .map(str::trim)
            .is_none_or(|name| name.is_empty())
    }

    fn restore_env_name(next: &mut Option<String>, prev: &Option<String>) -> bool {
        if !env_blank(next) || env_blank(prev) {
            return false;
        }
        *next = prev.clone();
        true
    }

    fn restore_inline(next: &mut String, prev: &str) -> bool {
        if !is_placeholder(next) || is_placeholder(prev) {
            return false;
        }
        *next = prev.to_owned();
        true
    }

    fn clear_redacted_optional(token: &mut Option<String>) -> bool {
        if token.as_deref() == Some("<redacted>") {
            *token = None;
            true
        } else {
            false
        }
    }

    /// True when a broker URL still carries a GET redaction marker in userinfo
    /// (`amqp://<redacted>@host`) or is wholly missing / `<redacted>`.
    fn url_needs_restore(value: &str) -> bool {
        is_placeholder(value) || value.contains("<redacted>")
    }

    fn source_key(source: &SourceConfig) -> Option<String> {
        match source {
            SourceConfig::Github(c) => Some(format!("github:{}", c.repo)),
            SourceConfig::Codeberg(c) => Some(format!("codeberg:{}", c.repo)),
            SourceConfig::Gitea(c) => Some(format!("gitea:{}:{}", c.host, c.repo)),
            SourceConfig::Gitlab(c) => Some(format!("gitlab:{}", c.project)),
            SourceConfig::Bitbucket(c) => Some(format!("bitbucket:{}", c.repo)),
            SourceConfig::Docker(c) => Some(format!("docker:{}", c.image)),
            SourceConfig::Ghcr(c) => Some(format!("ghcr:{}", c.image)),
            SourceConfig::Quay(c) => Some(format!("quay:{}", c.image)),
            SourceConfig::Ecr(c) => Some(format!("ecr:{}", c.image)),
            _ => None,
        }
    }

    fn restore_source_token(
        next_token: &mut Option<String>,
        next_env: &mut Option<String>,
        prev_token: &Option<String>,
        prev_env: &Option<String>,
    ) -> bool {
        let mut restored = false;
        let next_needs = next_token.as_deref().is_none_or(is_placeholder);
        if next_needs {
            if let Some(prev) = prev_token.as_deref().filter(|value| !is_placeholder(value)) {
                *next_token = Some(prev.to_owned());
                restored = true;
            }
        }
        if restore_env_name(next_env, prev_env) {
            restored = true;
        }
        // Refs-only previous: drop GET placeholder so normalize does not treat
        // `<redacted>` as a live secret write.
        if !env_blank(next_env) && clear_redacted_optional(next_token) {
            restored = true;
        }
        restored
    }

    let mut restored = false;

    for next_source in &mut next.sources {
        let Some(key) = source_key(next_source) else {
            continue;
        };
        let Some(prev_source) = previous
            .sources
            .iter()
            .find(|prev| source_key(prev).as_deref() == Some(key.as_str()))
        else {
            continue;
        };
        match (next_source, prev_source) {
            (SourceConfig::Github(n), SourceConfig::Github(p)) => {
                if restore_source_token(&mut n.token, &mut n.token_env, &p.token, &p.token_env) {
                    restored = true;
                }
            }
            (SourceConfig::Codeberg(n), SourceConfig::Codeberg(p)) => {
                if restore_source_token(&mut n.token, &mut n.token_env, &p.token, &p.token_env) {
                    restored = true;
                }
            }
            (SourceConfig::Gitea(n), SourceConfig::Gitea(p)) => {
                if restore_source_token(&mut n.token, &mut n.token_env, &p.token, &p.token_env) {
                    restored = true;
                }
            }
            (SourceConfig::Gitlab(n), SourceConfig::Gitlab(p)) => {
                if restore_source_token(&mut n.token, &mut n.token_env, &p.token, &p.token_env) {
                    restored = true;
                }
            }
            (SourceConfig::Bitbucket(n), SourceConfig::Bitbucket(p)) => {
                if restore_source_token(&mut n.token, &mut n.token_env, &p.token, &p.token_env) {
                    restored = true;
                }
            }
            (SourceConfig::Docker(n), SourceConfig::Docker(p)) => {
                if restore_source_token(&mut n.token, &mut n.token_env, &p.token, &p.token_env) {
                    restored = true;
                }
            }
            (SourceConfig::Ghcr(n), SourceConfig::Ghcr(p)) => {
                if restore_source_token(&mut n.token, &mut n.token_env, &p.token, &p.token_env) {
                    restored = true;
                }
            }
            (SourceConfig::Quay(n), SourceConfig::Quay(p)) => {
                if restore_source_token(&mut n.token, &mut n.token_env, &p.token, &p.token_env) {
                    restored = true;
                }
            }
            (SourceConfig::Ecr(n), SourceConfig::Ecr(p)) => {
                if restore_source_token(&mut n.token, &mut n.token_env, &p.token, &p.token_env) {
                    restored = true;
                }
            }
            _ => {}
        }
    }

    for next_notifier in &mut next.notifiers {
        match next_notifier {
            NotifierConfig::Apprise(next_cfg) => {
                let endpoint = next_cfg.endpoint.clone();
                let prev_cfg = previous.notifiers.iter().find_map(|prev| match prev {
                    NotifierConfig::Apprise(prev_cfg) if prev_cfg.endpoint == endpoint => {
                        Some(prev_cfg)
                    }
                    _ => None,
                });
                if let Some(prev_cfg) = prev_cfg {
                    if restore_env_name(&mut next_cfg.urls_env, &prev_cfg.urls_env) {
                        restored = true;
                    }
                    let next_urls_missing = next_cfg.urls.is_empty()
                        || next_cfg.urls.iter().all(|url| is_placeholder(url));
                    if next_urls_missing
                        && next_cfg.config_key.is_none()
                        && !prev_cfg.urls.is_empty()
                        && prev_cfg.urls.iter().all(|url| !is_placeholder(url))
                    {
                        next_cfg.urls = prev_cfg.urls.clone();
                        restored = true;
                    } else if next_urls_missing && !env_blank(&next_cfg.urls_env) {
                        next_cfg.urls.clear();
                        restored = true;
                    }
                }
            }
            NotifierConfig::Express(next_cfg) => {
                let chat = next_cfg.group_chat_id.clone();
                if let Some(prev_cfg) = previous.notifiers.iter().find_map(|prev| match prev {
                    NotifierConfig::Express(prev_cfg) if prev_cfg.group_chat_id == chat => {
                        Some(prev_cfg)
                    }
                    _ => None,
                }) {
                    if restore_inline(&mut next_cfg.access_token, &prev_cfg.access_token) {
                        restored = true;
                    }
                    if restore_env_name(&mut next_cfg.access_token_env, &prev_cfg.access_token_env)
                    {
                        restored = true;
                    }
                    if !env_blank(&next_cfg.access_token_env)
                        && is_placeholder(&next_cfg.access_token)
                    {
                        next_cfg.access_token.clear();
                        restored = true;
                    }
                }
            }
            NotifierConfig::Novu(next_cfg) => {
                let workflow = next_cfg.workflow.clone();
                let topic = next_cfg.topic_key.clone();
                let subscriber = next_cfg.subscriber_id.clone();
                if let Some(prev_cfg) = previous.notifiers.iter().find_map(|prev| match prev {
                    NotifierConfig::Novu(prev_cfg)
                        if prev_cfg.workflow == workflow
                            && prev_cfg.topic_key == topic
                            && prev_cfg.subscriber_id == subscriber =>
                    {
                        Some(prev_cfg)
                    }
                    _ => None,
                }) {
                    if restore_inline(&mut next_cfg.api_key, &prev_cfg.api_key) {
                        restored = true;
                    }
                    if restore_env_name(&mut next_cfg.api_key_env, &prev_cfg.api_key_env) {
                        restored = true;
                    }
                    if !env_blank(&next_cfg.api_key_env) && is_placeholder(&next_cfg.api_key) {
                        next_cfg.api_key.clear();
                        restored = true;
                    }
                }
            }
            NotifierConfig::Slack(next_cfg) => {
                let channel = next_cfg.channel.clone();
                let name = next_cfg.name.clone();
                if let Some(prev_cfg) = previous.notifiers.iter().find_map(|prev| match prev {
                    NotifierConfig::Slack(prev_cfg)
                        if (!channel.trim().is_empty() && prev_cfg.channel == channel)
                            || (channel.trim().is_empty() && prev_cfg.name == name) =>
                    {
                        Some(prev_cfg)
                    }
                    _ => None,
                }) {
                    if restore_inline(&mut next_cfg.webhook_url, &prev_cfg.webhook_url) {
                        restored = true;
                    }
                    if restore_inline(&mut next_cfg.bot_token, &prev_cfg.bot_token) {
                        restored = true;
                    }
                    if restore_env_name(&mut next_cfg.webhook_url_env, &prev_cfg.webhook_url_env) {
                        restored = true;
                    }
                    if restore_env_name(&mut next_cfg.bot_token_env, &prev_cfg.bot_token_env) {
                        restored = true;
                    }
                    if !env_blank(&next_cfg.webhook_url_env)
                        && is_placeholder(&next_cfg.webhook_url)
                    {
                        next_cfg.webhook_url.clear();
                        restored = true;
                    }
                    if !env_blank(&next_cfg.bot_token_env) && is_placeholder(&next_cfg.bot_token) {
                        next_cfg.bot_token.clear();
                        restored = true;
                    }
                }
            }
            NotifierConfig::Telegram(next_cfg) => {
                let chat_id = next_cfg.chat_id.clone();
                if let Some(prev_cfg) = previous.notifiers.iter().find_map(|prev| match prev {
                    NotifierConfig::Telegram(prev_cfg) if prev_cfg.chat_id == chat_id => {
                        Some(prev_cfg)
                    }
                    _ => None,
                }) {
                    if restore_inline(&mut next_cfg.bot_token, &prev_cfg.bot_token) {
                        restored = true;
                    }
                    if restore_env_name(&mut next_cfg.bot_token_env, &prev_cfg.bot_token_env) {
                        restored = true;
                    }
                    if !env_blank(&next_cfg.bot_token_env) && is_placeholder(&next_cfg.bot_token) {
                        next_cfg.bot_token.clear();
                        restored = true;
                    }
                }
            }
            NotifierConfig::Webhook(next_cfg) => {
                let name = next_cfg.name.clone();
                let next_url = next_cfg.url.clone();
                let url_placeholder = url_needs_restore(&next_url);

                let prev_cfg = previous
                    .notifiers
                    .iter()
                    .find_map(|prev| match prev {
                        NotifierConfig::Webhook(prev_cfg) => {
                            let by_name = name
                                .as_ref()
                                .is_some_and(|n| prev_cfg.name.as_ref() == Some(n));
                            let by_url = !url_placeholder && prev_cfg.url == next_url;
                            if by_name || by_url {
                                Some(prev_cfg)
                            } else {
                                None
                            }
                        }
                        _ => None,
                    })
                    .or_else(|| {
                        if !url_placeholder || name.is_some() {
                            return None;
                        }
                        let mut webhooks =
                            previous.notifiers.iter().filter_map(|prev| match prev {
                                NotifierConfig::Webhook(prev_cfg) => Some(prev_cfg),
                                _ => None,
                            });
                        let first = webhooks.next()?;
                        if webhooks.next().is_none() {
                            Some(first)
                        } else {
                            None
                        }
                    });

                if let Some(prev_cfg) = prev_cfg {
                    if url_placeholder && !url_needs_restore(&prev_cfg.url) {
                        next_cfg.url = prev_cfg.url.clone();
                        restored = true;
                    }
                    if restore_inline(&mut next_cfg.secret, &prev_cfg.secret) {
                        restored = true;
                    }
                    if restore_env_name(&mut next_cfg.url_env, &prev_cfg.url_env) {
                        restored = true;
                    }
                    if restore_env_name(&mut next_cfg.secret_env, &prev_cfg.secret_env) {
                        restored = true;
                    }
                    if restore_env_name(&mut next_cfg.headers_env, &prev_cfg.headers_env) {
                        restored = true;
                    }
                    if !env_blank(&next_cfg.url_env) && url_needs_restore(&next_cfg.url) {
                        next_cfg.url.clear();
                        restored = true;
                    }
                    if !env_blank(&next_cfg.secret_env) && is_placeholder(&next_cfg.secret) {
                        next_cfg.secret.clear();
                        restored = true;
                    }
                    if next_cfg.headers.is_empty() && !prev_cfg.headers.is_empty() {
                        next_cfg.headers = prev_cfg.headers.clone();
                        restored = true;
                    } else {
                        for (key, prev_val) in &prev_cfg.headers {
                            if next_cfg
                                .headers
                                .get(key)
                                .is_some_and(|value| is_placeholder(value))
                            {
                                next_cfg.headers.insert(key.clone(), prev_val.clone());
                                restored = true;
                            }
                        }
                    }
                    if !env_blank(&next_cfg.headers_env)
                        && next_cfg.headers.values().all(|value| is_placeholder(value))
                    {
                        next_cfg.headers.clear();
                        restored = true;
                    }
                }
            }
            NotifierConfig::Smtp(next_cfg) => {
                let host = next_cfg.host.clone();
                if let Some(prev_cfg) = previous.notifiers.iter().find_map(|prev| match prev {
                    NotifierConfig::Smtp(prev_cfg) if prev_cfg.host == host => Some(prev_cfg),
                    _ => None,
                }) {
                    if restore_inline(&mut next_cfg.password, &prev_cfg.password) {
                        restored = true;
                    }
                    if restore_env_name(&mut next_cfg.password_env, &prev_cfg.password_env) {
                        restored = true;
                    }
                    if !env_blank(&next_cfg.password_env) && is_placeholder(&next_cfg.password) {
                        next_cfg.password.clear();
                        restored = true;
                    }
                }
            }
            NotifierConfig::Rabbitmq(next_cfg) => {
                let routing_key = next_cfg.routing_key.clone();
                let name = next_cfg.name.clone();
                if let Some(prev_cfg) = previous.notifiers.iter().find_map(|prev| match prev {
                    NotifierConfig::Rabbitmq(prev_cfg)
                        if prev_cfg.routing_key == routing_key && prev_cfg.name == name =>
                    {
                        Some(prev_cfg)
                    }
                    _ => None,
                }) {
                    if url_needs_restore(&next_cfg.url) && !url_needs_restore(&prev_cfg.url) {
                        next_cfg.url = prev_cfg.url.clone();
                        restored = true;
                    }
                    if restore_env_name(&mut next_cfg.url_env, &prev_cfg.url_env) {
                        restored = true;
                    }
                    if !env_blank(&next_cfg.url_env) && url_needs_restore(&next_cfg.url) {
                        next_cfg.url.clear();
                        restored = true;
                    }
                }
            }
            NotifierConfig::Nats(next_cfg) => {
                let subject = next_cfg.subject.clone();
                let name = next_cfg.name.clone();
                if let Some(prev_cfg) = previous.notifiers.iter().find_map(|prev| match prev {
                    NotifierConfig::Nats(prev_cfg)
                        if prev_cfg.subject == subject && prev_cfg.name == name =>
                    {
                        Some(prev_cfg)
                    }
                    _ => None,
                }) {
                    if url_needs_restore(&next_cfg.url) && !url_needs_restore(&prev_cfg.url) {
                        next_cfg.url = prev_cfg.url.clone();
                        restored = true;
                    }
                    if restore_env_name(&mut next_cfg.url_env, &prev_cfg.url_env) {
                        restored = true;
                    }
                    if !env_blank(&next_cfg.url_env) && url_needs_restore(&next_cfg.url) {
                        next_cfg.url.clear();
                        restored = true;
                    }
                }
            }
            _ => {}
        }
    }
    restored
}

/// Redact the `user[:pass]@` userinfo segment of a URL-like string, leaving
/// the scheme/host/port/path intact. A no-op for strings without `://` or
/// without an `@` in the authority component.
fn redact_url_userinfo(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_owned();
    };
    let rest = &url[scheme_end + 3..];
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    // `rfind`, not `find`: userinfo/host are split on the *last* `@` per RFC
    // 3986 (a percent-encoded `@` could otherwise appear inside the userinfo).
    let Some(at) = authority.rfind('@') else {
        return url.to_owned();
    };
    format!("{}<redacted>@{}", &url[..scheme_end + 3], &rest[at + 1..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_should_mask_apprise_urls_and_api_key() {
        let config: Config = toml::from_str(
            r#"
            [[notifiers]]
            type = "apprise"
            urls = ["tgram://real/token"]

            [api]
            api_key = "super-secret"
        "#,
        )
        .expect("parse");
        let text = redact_config_toml(&config);
        assert!(!text.contains("super-secret"));
        assert!(!text.contains("tgram://real"));
        assert!(text.contains("<redacted>"));
    }

    #[test]
    fn redact_should_mask_postgres_url_credentials() {
        let config: Config = toml::from_str(
            r#"
            [database]
            postgres_url = "postgres://xrelease:hunter2@db.internal:5432/xrelease"
        "#,
        )
        .expect("parse");
        let text = redact_config_toml(&config);
        assert!(
            !text.contains("hunter2"),
            "password must not appear in GET /config: {text}"
        );
        assert!(text.contains("db.internal:5432/xrelease"));
        assert!(text.contains("<redacted>@"));
        assert!(!text.contains("organizations"), "empty catalogue must not serialize");
    }

    #[test]
    fn redact_url_userinfo_should_mask_credentials_only() {
        assert_eq!(
            redact_url_userinfo("amqp://guest:hunter2@rabbit.internal:5672/prod"),
            "amqp://<redacted>@rabbit.internal:5672/prod"
        );
        assert_eq!(
            redact_url_userinfo("nats://user:pass@nats.internal:4222"),
            "nats://<redacted>@nats.internal:4222"
        );
    }

    #[test]
    fn redact_url_userinfo_should_be_a_noop_without_credentials() {
        assert_eq!(
            redact_url_userinfo("nats://nats.internal:4222"),
            "nats://nats.internal:4222"
        );
        assert_eq!(redact_url_userinfo("not-a-url"), "not-a-url");
    }

    /// Regression: RabbitMQ/NATS notifier URLs commonly embed credentials
    /// (`amqp://user:pass@host/vhost`) — these were not redacted at all before
    /// this fix, leaking them verbatim through `GET /api/v1/config` to any
    /// bearer holder.
    #[test]
    fn redact_should_mask_rabbitmq_and_nats_url_credentials() {
        let config: Config = toml::from_str(
            r#"
            [[notifiers]]
            type = "rabbitmq"
            url = "amqp://guest:hunter2@rabbit.internal:5672/prod"
            routing_key = "releases"

            [[notifiers]]
            type = "nats"
            url = "nats://user:s3cr3t@nats.internal:4222"
            subject = "releases"
        "#,
        )
        .expect("parse");
        let text = redact_config_toml(&config);
        assert!(!text.contains("hunter2"));
        assert!(!text.contains("s3cr3t"));
        assert!(text.contains("rabbit.internal:5672/prod"));
        assert!(text.contains("nats.internal:4222"));
    }

    /// Regression: webhook `headers` is documented as the way to carry a
    /// bearer token / API key to the destination, but was never redacted.
    #[test]
    fn redact_should_mask_webhook_header_values_but_keep_keys() {
        let config: Config = toml::from_str(
            r#"
            [[notifiers]]
            type = "webhook"
            url = "https://hooks.example.com/xrelease"
            headers = { Authorization = "Bearer sekrit-token", "X-Team" = "platform" }
        "#,
        )
        .expect("parse");
        let text = redact_config_toml(&config);
        assert!(!text.contains("sekrit-token"));
        assert!(
            text.contains("Authorization"),
            "header name should stay visible: {text}"
        );
    }

    #[test]
    fn restore_should_bring_back_omitted_apprise_urls() {
        use crate::config::NotifierConfig;

        let previous: Config = toml::from_str(
            r#"
            [[notifiers]]
            type = "apprise"
            endpoint = "http://apprise:8000"
            urls = ["mailto://ops@example.com"]

            [[notifiers]]
            type = "express"
            base_url = "https://cts.example.com"
            group_chat_id = "g-1"
            access_token = "tok"
            tags = ["security-team"]
        "#,
        )
        .expect("parse");
        // UI dropped redacted urls — only endpoint/format remain.
        let mut next: Config = toml::from_str(
            r#"
            [[notifiers]]
            type = "apprise"
            endpoint = "http://apprise:8000"

            [[notifiers]]
            type = "express"
            base_url = "https://cts.example.com"
            group_chat_id = "g-1"
            access_token = "tok"
            tags = ["security-team"]

            [[sources]]
            type = "github"
            repo = "org/app"
            routing_tag = "platform-team"
        "#,
        )
        .expect("parse");
        let NotifierConfig::Apprise(before) = &next.notifiers[0] else {
            panic!("expected apprise");
        };
        assert!(
            !before.is_configured(),
            "urls omitted → not configured before restore"
        );
        assert!(restore_redacted_secrets(&previous, &mut next));
        let NotifierConfig::Apprise(after) = &next.notifiers[0] else {
            panic!("expected apprise");
        };
        assert!(after.is_configured());
        assert_eq!(after.urls, vec!["mailto://ops@example.com".to_owned()]);

        // After restore Apprise is a wildcard sink — platform-team sources must validate.
        let mut routed: Config = toml::from_str(
            r#"
            [database]
            postgres_url = "postgres://xrelease:xrelease@127.0.0.1:5432/xrelease"

            [api]
            require_auth = false

            [[notifiers]]
            type = "apprise"
            endpoint = "http://apprise:8000"

            [[notifiers]]
            type = "express"
            base_url = "https://cts.example.com"
            group_chat_id = "g-1"
            access_token = "tok"
            tags = ["security-team"]

            [[sources]]
            type = "github"
            repo = "org/app"
            routing_tag = "platform-team"
        "#,
        )
        .expect("parse");
        assert!(restore_redacted_secrets(&previous, &mut routed));
        let report =
            crate::validate::validate_full(&routed, &crate::validate::ValidateOptions::default());
        assert!(
            report.valid,
            "wildcard Apprise after restore should cover platform-team: {:?}",
            report.errors
        );
    }

    #[test]
    fn restore_should_bring_back_omitted_webhook_headers() {
        use crate::config::NotifierConfig;

        let previous: Config = toml::from_str(
            r#"
            [[notifiers]]
            type = "webhook"
            name = "n8n"
            url = "https://hooks.example.com/xrelease"
            secret = "sign-me"
            headers = { Authorization = "Bearer sekrit-token" }
        "#,
        )
        .expect("parse");
        let mut next: Config = toml::from_str(
            r#"
            [[notifiers]]
            type = "webhook"
            name = "n8n"
            url = "<redacted>"
        "#,
        )
        .expect("parse");
        assert!(restore_redacted_secrets(&previous, &mut next));
        let NotifierConfig::Webhook(cfg) = &next.notifiers[0] else {
            panic!("expected webhook");
        };
        assert_eq!(cfg.url, "https://hooks.example.com/xrelease");
        assert_eq!(cfg.secret, "sign-me");
        assert_eq!(
            cfg.headers.get("Authorization").map(String::as_str),
            Some("Bearer sekrit-token")
        );
    }

    #[test]
    fn redact_should_mask_webhook_url() {
        let config: Config = toml::from_str(
            r#"
            [[notifiers]]
            type = "webhook"
            url = "https://hooks.example.com/secret-path/abc"
            secret = "sign-me"
        "#,
        )
        .expect("parse");
        let text = redact_config_toml(&config);
        assert!(!text.contains("secret-path"));
        assert!(!text.contains("sign-me"));
        assert!(text.contains("<redacted>"));
    }

    #[test]
    fn restore_should_bring_back_broker_url_with_redacted_userinfo() {
        use crate::config::NotifierConfig;

        let previous: Config = toml::from_str(
            r#"
            [[notifiers]]
            type = "rabbitmq"
            name = "bus"
            url = "amqp://guest:hunter2@rabbit.internal:5672/prod"
            routing_key = "releases"

            [[notifiers]]
            type = "nats"
            name = "bus"
            url = "nats://user:s3cr3t@nats.internal:4222"
            subject = "releases"
        "#,
        )
        .expect("parse");
        let mut next: Config = toml::from_str(
            r#"
            [[notifiers]]
            type = "rabbitmq"
            name = "bus"
            url = "amqp://<redacted>@rabbit.internal:5672/prod"
            routing_key = "releases"

            [[notifiers]]
            type = "nats"
            name = "bus"
            url = "nats://<redacted>@nats.internal:4222"
            subject = "releases"
        "#,
        )
        .expect("parse");
        assert!(restore_redacted_secrets(&previous, &mut next));
        let NotifierConfig::Rabbitmq(rmq) = &next.notifiers[0] else {
            panic!("rabbitmq");
        };
        assert_eq!(rmq.url, "amqp://guest:hunter2@rabbit.internal:5672/prod");
        let NotifierConfig::Nats(nats) = &next.notifiers[1] else {
            panic!("nats");
        };
        assert_eq!(nats.url, "nats://user:s3cr3t@nats.internal:4222");
    }

    #[test]
    fn redact_desired_only_should_not_leak_config_default_infra() {
        // Parsing `{}` fills Config::default() — including config_api.source=local.
        let empty: Config = serde_yaml::from_str("{}").expect("parse empty");
        let wire = redact_desired_only_document(DesiredFormat::Yaml, &empty);
        assert_eq!(wire, EMPTY_ORGANIZATION_DOCUMENT);
        assert!(!wire.contains("config_api"));
        assert!(!wire.contains("source = \"local\""));
        assert!(!wire.contains("ui_config"));
    }

    #[test]
    fn redact_desired_only_should_keep_app_sections_without_infra() {
        let desired: Config = serde_yaml::from_str(
            r#"
notifiers:
  - type: apprise
    urls: ["mailto://secret:pass@example.com"]
sources:
  - type: github
    repo: org/app
"#,
        )
        .expect("parse");
        let wire = redact_desired_only_document(DesiredFormat::Yaml, &desired);
        assert!(wire.contains("org/app"));
        assert!(wire.contains("<redacted>"));
        assert!(!wire.contains("database"));
        assert!(!wire.contains("config_api"));
        assert!(!wire.contains("require_auth"));
        assert!(!wire.contains("organizations"));
        // Infra defaults must not pad the wire (parse-`{}` pollution).
        assert!(!wire.contains("ui_config"));
        assert!(!wire.contains("postgres_url"));
    }

    #[test]
    fn redact_should_mask_source_token_when_only_token_env_set() {
        let config: Config = toml::from_str(
            r#"
            [[sources]]
            type = "github"
            repo = "org/app"
            token_env = "XRELEASE_UI_SRC_0_TOKEN"
        "#,
        )
        .expect("parse");
        let text = redact_config_toml(&config);
        assert!(text.contains("<redacted>"));
        assert!(text.contains("XRELEASE_UI_SRC_0_TOKEN"));
    }

    #[test]
    fn restore_should_reinstate_env_refs_after_normalize() {
        use crate::config::{normalize_secrets_to_refs, NotifierConfig, SourceConfig};

        let previous: Config = toml::from_str(
            r#"
            [[notifiers]]
            type = "telegram"
            chat_id = "-100"
            bot_token_env = "XRELEASE_UI_N_0_TELEGRAM_BOT"

            [[sources]]
            type = "github"
            repo = "org/app"
            token_env = "XRELEASE_UI_SRC_0_TOKEN"
        "#,
        )
        .expect("parse");
        // GET redacted document as the UI might re-apply it (placeholders, env kept).
        let mut next: Config = toml::from_str(
            r#"
            [[notifiers]]
            type = "telegram"
            chat_id = "-100"
            bot_token = "<redacted>"

            [[sources]]
            type = "github"
            repo = "org/app"
            token = "<redacted>"
        "#,
        )
        .expect("parse");
        assert!(restore_redacted_secrets(&previous, &mut next));
        let writes = normalize_secrets_to_refs(&mut next, None);
        assert!(
            writes.is_empty(),
            "refs-only round-trip must not invent vault writes: {writes:?}"
        );
        match &next.notifiers[0] {
            NotifierConfig::Telegram(n) => {
                assert!(n.bot_token.is_empty());
                assert_eq!(
                    n.bot_token_env.as_deref(),
                    Some("XRELEASE_UI_N_0_TELEGRAM_BOT")
                );
            }
            other => panic!("unexpected {other:?}"),
        }
        match &next.sources[0] {
            SourceConfig::Github(c) => {
                assert!(c.token.is_none());
                assert_eq!(c.token_env.as_deref(), Some("XRELEASE_UI_SRC_0_TOKEN"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
