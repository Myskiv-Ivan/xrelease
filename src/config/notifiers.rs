//! Notification sink configuration (`[[notifiers]]`).
//!
//! Each `[[notifiers]]` entry is a tagged enum ([`NotifierConfig`]) mapping
//! onto one [`crate::notify::Sink`] variant. Feature-gated brokers (Kafka /
//! NATS / RabbitMQ) fail loudly at build time (not parse time) when their
//! cargo feature isn't compiled in, so a config referencing them is still
//! valid TOML — it just can't be *used* without the matching `--features`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::notify::{
    AppriseNotifier, ExpressNotifier, ExpressNotifierOptions, NovuNotifier, NovuNotifierOptions,
    Sink, SlackNotifier, SlackNotifierOptions, TelegramNotifier, TelegramNotifierOptions,
    WebhookMethod, WebhookNotifier,
};

fn default_format() -> String {
    "markdown".to_owned()
}

fn default_endpoint() -> String {
    "http://localhost:8000".to_owned()
}

/// Apprise delivery settings.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AppriseConfig {
    /// Base URL of the Apprise API server.
    #[serde(default = "default_endpoint")]
    pub endpoint: String,
    /// Stateless target URLs (mutually exclusive with `config_key`).
    #[serde(default)]
    pub urls: Vec<String>,
    /// Env var / vault name holding a JSON array of Apprise URLs (GitOps / UI).
    #[serde(default)]
    pub urls_env: Option<String>,
    /// Persistent Apprise config key (mutually exclusive with `urls`).
    #[serde(default)]
    pub config_key: Option<String>,
    /// Optional Apprise tag to route to a subset of configured targets.
    #[serde(default)]
    pub tag: Option<String>,
    /// When set, only events with a matching per-source `routing_tag` are
    /// delivered (team routing). Empty = receive all events.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Body format: `markdown` | `text` | `html`.
    #[serde(default = "default_format")]
    pub format: String,
}

impl Default for AppriseConfig {
    fn default() -> Self {
        Self {
            endpoint: default_endpoint(),
            urls: Vec::new(),
            urls_env: None,
            config_key: None,
            tag: None,
            tags: Vec::new(),
            format: default_format(),
        }
    }
}

impl AppriseConfig {
    /// Build the runtime notifier from this config.
    pub fn build(&self, client: reqwest::Client) -> anyhow::Result<AppriseNotifier> {
        let urls = resolve_apprise_urls(self);
        AppriseNotifier::new(
            client,
            &self.endpoint,
            &urls,
            self.config_key.as_deref(),
            self.tag.as_deref(),
            &self.format,
        )
        .map_err(Into::into)
    }

    /// Whether a usable Apprise target is set (URLs, `urls_env`, or a persistent
    /// config key in the desired document — YAML / UI / apply).
    #[must_use]
    pub fn is_configured(&self) -> bool {
        !self.urls.is_empty()
            || self
                .urls_env
                .as_deref()
                .map(str::trim)
                .is_some_and(|name| !name.is_empty())
            || self.config_key.is_some()
    }
}

fn resolve_apprise_urls(cfg: &AppriseConfig) -> Vec<String> {
    let live: Vec<String> = cfg
        .urls
        .iter()
        .map(|url| sanitize_secret(url))
        .filter(|url| !url.is_empty())
        .collect();
    if !live.is_empty() {
        return live;
    }
    let Some(raw) = cfg
        .urls_env
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .and_then(super::env_token)
    else {
        return Vec::new();
    };
    if let Ok(parsed) = serde_json::from_str::<Vec<String>>(&raw) {
        return parsed
            .into_iter()
            .map(|url| sanitize_secret(&url))
            .filter(|url| !url.is_empty())
            .collect();
    }
    // Single URL stored as plain text.
    let one = sanitize_secret(&raw);
    if one.is_empty() {
        Vec::new()
    } else {
        vec![one]
    }
}

/// HTTP method for a webhook notifier.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum WebhookMethodCfg {
    /// HTTP `POST` (default).
    #[default]
    Post,
    /// HTTP `PUT`.
    Put,
}

impl From<WebhookMethodCfg> for WebhookMethod {
    fn from(value: WebhookMethodCfg) -> Self {
        match value {
            WebhookMethodCfg::Post => Self::Post,
            WebhookMethodCfg::Put => Self::Put,
        }
    }
}

/// `type = "webhook"` notifier config.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebhookCfg {
    /// Optional label for logs (defaults to the URL host).
    pub name: Option<String>,
    /// Deliver only when the event routing tag matches one of these values.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Destination URL.
    /// Resolution: this field → [`Self::url_env`] → (empty).
    #[serde(default)]
    pub url: String,
    /// Env var / vault name holding the destination URL (GitOps / UI).
    #[serde(default)]
    pub url_env: Option<String>,
    /// HTTP method (`POST` default, or `PUT`).
    #[serde(default)]
    pub method: WebhookMethodCfg,
    /// Static headers added to every request (e.g. auth tokens).
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Env var / vault name holding a JSON object of header name→value pairs.
    #[serde(default)]
    pub headers_env: Option<String>,
    /// `Content-Type` header (default `application/json`).
    pub content_type: Option<String>,
    /// Optional body template; omitted = canonical JSON payload.
    pub template: Option<String>,
    /// Optional HMAC-SHA256 signing secret. When set, each request carries a
    /// `sha256=<hex>` signature so the receiver can verify authenticity. May be
    /// left empty and supplied via [`Self::secret_env`] or `XRELEASE_WEBHOOK_SECRET`.
    #[serde(default)]
    pub secret: String,
    /// Name of an env var holding the signing secret (keeps the value out of Git).
    #[serde(default)]
    pub secret_env: Option<String>,
    /// Header carrying the signature (default `X-Signature-256`).
    pub signature_header: Option<String>,
}

/// `type = "express"` notifier config (eXpress BotX chat).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpressCfg {
    /// Optional label for logs (defaults to `express:{group_chat_id}`).
    pub name: Option<String>,
    /// Team routing tags — only matching `routing_tag` events reach this chat.
    #[serde(default)]
    pub tags: Vec<String>,
    /// CTS server base URL (e.g. `https://cts.example.com`).
    pub base_url: String,
    /// BotX Bearer for `POST /api/v4/botx/notifications/direct`.
    /// Resolution: this field → [`Self::access_token_env`] → `XRELEASE_EXPRESS_ACCESS_TOKEN`.
    #[serde(default)]
    pub access_token: String,
    /// Env var name holding the Bearer token (GitOps).
    #[serde(default)]
    pub access_token_env: Option<String>,
    /// Target group chat id (UUID). The bot must be a member of the chat.
    pub group_chat_id: String,
    /// Optional recipient ids; empty = the whole chat.
    #[serde(default)]
    pub recipients: Vec<String>,
    /// Optional body template; omitted = title + Markdown body.
    pub template: Option<String>,
}

fn default_novu_base_url() -> String {
    "https://api.novu.co".to_owned()
}

fn default_slack_api_base() -> String {
    "https://slack.com/api".to_owned()
}

fn default_telegram_api_base() -> String {
    "https://api.telegram.org".to_owned()
}

/// `type = "novu"` notifier config (Novu workflow trigger).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NovuCfg {
    /// Optional label for logs (defaults to `novu:{workflow}`).
    pub name: Option<String>,
    /// Team routing tags — only matching `routing_tag` events reach this sink.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Novu API base (`https://api.novu.co`, `https://eu.api.novu.co`, or self-host).
    #[serde(default = "default_novu_base_url")]
    pub base_url: String,
    /// Secret API key for `Authorization: ApiKey …`.
    /// Resolution: this field → [`Self::api_key_env`] → `XRELEASE_NOVU_API_KEY`.
    #[serde(default)]
    pub api_key: String,
    /// Env var name holding the API key (GitOps).
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Workflow trigger identifier (`name` in `POST /v1/events/trigger`).
    pub workflow: String,
    /// Novu topic key template (e.g. `{{tag}}` or a fixed key). Wins over
    /// [`Self::subscriber_id`] when both are set.
    #[serde(default)]
    pub topic_key: Option<String>,
    /// Novu subscriber id template when not targeting a topic.
    #[serde(default)]
    pub subscriber_id: Option<String>,
}

/// `type = "slack"` notifier config (Incoming Webhook or Bot API).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SlackCfg {
    /// Optional label for logs.
    pub name: Option<String>,
    /// Team routing tags (empty = all events).
    #[serde(default)]
    pub tags: Vec<String>,
    /// Incoming webhook URL. Mutually exclusive with [`Self::bot_token`].
    /// Resolution: this field → [`Self::webhook_url_env`] → `XRELEASE_SLACK_WEBHOOK_URL`.
    #[serde(default)]
    pub webhook_url: String,
    /// Env var name holding the webhook URL (GitOps).
    #[serde(default)]
    pub webhook_url_env: Option<String>,
    /// Bot OAuth token `xoxb-…`. Mutually exclusive with [`Self::webhook_url`].
    /// Resolution: this field → [`Self::bot_token_env`] → `XRELEASE_SLACK_BOT_TOKEN`.
    #[serde(default)]
    pub bot_token: String,
    /// Env var name holding the bot token (GitOps).
    #[serde(default)]
    pub bot_token_env: Option<String>,
    /// Channel id/name for bot mode (templates allowed). Required with `bot_token`.
    #[serde(default)]
    pub channel: String,
    /// Slack Web API base for bot mode (default `https://slack.com/api`).
    #[serde(default = "default_slack_api_base")]
    pub api_base: String,
    /// Optional body template; omitted = title + Markdown body.
    pub template: Option<String>,
}

/// `type = "telegram"` notifier config (Bot API `sendMessage`).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TelegramCfg {
    /// Optional label for logs.
    pub name: Option<String>,
    /// Team routing tags (empty = all events).
    #[serde(default)]
    pub tags: Vec<String>,
    /// Telegram Bot API base (default `https://api.telegram.org`).
    #[serde(default = "default_telegram_api_base")]
    pub api_base: String,
    /// Bot token from `@BotFather`.
    /// Resolution: this field → [`Self::bot_token_env`] → `XRELEASE_TELEGRAM_BOT_TOKEN`.
    #[serde(default)]
    pub bot_token: String,
    /// Env var name holding the bot token (GitOps).
    #[serde(default)]
    pub bot_token_env: Option<String>,
    /// Chat / channel / group id (templates allowed, e.g. `{{tag}}`).
    pub chat_id: String,
    /// Optional `parse_mode`: `Markdown`, `MarkdownV2`, or `HTML`.
    #[serde(default)]
    pub parse_mode: Option<String>,
    /// Optional body template; omitted = title + Markdown body.
    pub template: Option<String>,
}

/// `type = "kafka"` notifier config.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct KafkaCfg {
    /// Optional label for logs.
    pub name: Option<String>,
    /// Team routing tags (empty = all events).
    #[serde(default)]
    pub tags: Vec<String>,
    /// Bootstrap broker list (`host:port`).
    pub brokers: Vec<String>,
    /// Destination topic.
    pub topic: String,
    /// Optional partition-key template (e.g. `{{source_id}}`).
    pub key: Option<String>,
    /// Optional body template; omitted = canonical JSON payload.
    pub template: Option<String>,
}

/// `type = "nats"` notifier config.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NatsCfg {
    /// Optional label for logs.
    pub name: Option<String>,
    /// Team routing tags (empty = all events).
    #[serde(default)]
    pub tags: Vec<String>,
    /// Server URL (`nats://host:4222`).
    /// Resolution: this field → [`Self::url_env`] → `XRELEASE_NATS_URL`.
    #[serde(default)]
    pub url: String,
    /// Env var name holding the NATS URL (GitOps).
    #[serde(default)]
    pub url_env: Option<String>,
    /// Subject template (e.g. `releases.{{kind}}`).
    pub subject: String,
    /// Optional body template; omitted = canonical JSON payload.
    pub template: Option<String>,
}

/// `type = "rabbitmq"` notifier config.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RabbitmqCfg {
    /// Optional label for logs.
    pub name: Option<String>,
    /// Team routing tags (empty = all events).
    #[serde(default)]
    pub tags: Vec<String>,
    /// AMQP URL (`amqp://user:pass@host:5672/vhost`).
    /// Resolution: this field → [`Self::url_env`] → `XRELEASE_RABBITMQ_URL`.
    #[serde(default)]
    pub url: String,
    /// Env var name holding the AMQP URL (GitOps).
    #[serde(default)]
    pub url_env: Option<String>,
    /// Target exchange (empty string = default exchange).
    #[serde(default)]
    pub exchange: String,
    /// Routing-key template (e.g. `{{kind}}`).
    pub routing_key: String,
    /// Optional body template; omitted = canonical JSON payload.
    pub template: Option<String>,
}

/// SMTP transport encryption mode.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SmtpTlsCfg {
    /// STARTTLS (port 587, default).
    #[default]
    Starttls,
    /// Implicit TLS (port 465).
    Tls,
    /// Plain SMTP (no encryption).
    Plain,
}

impl From<SmtpTlsCfg> for crate::notify::SmtpTlsMode {
    fn from(value: SmtpTlsCfg) -> Self {
        match value {
            SmtpTlsCfg::Starttls => Self::StartTls,
            SmtpTlsCfg::Tls => Self::Tls,
            SmtpTlsCfg::Plain => Self::Plain,
        }
    }
}

fn default_smtp_port() -> u16 {
    587
}

fn default_smtp_body_format() -> String {
    "text".to_owned()
}

/// `type = "smtp"` notifier config (direct email without Apprise).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SmtpCfg {
    /// Optional label for logs.
    pub name: Option<String>,
    /// Team routing tags (empty = all events).
    #[serde(default)]
    pub tags: Vec<String>,
    /// SMTP server hostname.
    pub host: String,
    /// SMTP port (`587` default for STARTTLS).
    #[serde(default = "default_smtp_port")]
    pub port: u16,
    /// Optional SMTP AUTH username.
    pub username: Option<String>,
    /// SMTP AUTH password. May be empty in TOML and supplied via
    /// [`Self::password_env`] or `XRELEASE_SMTP_PASSWORD`.
    #[serde(default)]
    pub password: String,
    /// Env var name holding the SMTP password (GitOps).
    #[serde(default)]
    pub password_env: Option<String>,
    /// Sender address (`From` header).
    pub from: String,
    /// Recipient addresses (`To` header).
    pub to: Vec<String>,
    /// Transport encryption (`starttls` default).
    #[serde(default)]
    pub tls: SmtpTlsCfg,
    /// Optional subject template (`{{title}}`, …).
    pub subject_template: Option<String>,
    /// Optional body template (`{{title}}`, `{{body}}`, …). When set, wins over
    /// [`Self::body_format`] defaults.
    pub template: Option<String>,
    /// Body format when [`Self::template`] is unset: `text` (default) or `markdown`.
    #[serde(default = "default_smtp_body_format")]
    pub body_format: String,
}

/// A single additional delivery sink (`[[notifiers]]`).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum NotifierConfig {
    /// Apprise HTTP API server.
    Apprise(AppriseConfig),
    /// Generic outgoing HTTP webhook.
    Webhook(WebhookCfg),
    /// eXpress (BotX) chat notification.
    Express(ExpressCfg),
    /// Novu workflow trigger.
    Novu(NovuCfg),
    /// Slack Incoming Webhook or Bot API.
    Slack(SlackCfg),
    /// Telegram Bot API.
    Telegram(TelegramCfg),
    /// Direct SMTP email delivery.
    Smtp(SmtpCfg),
    /// Kafka topic producer (requires `--features kafka`).
    Kafka(KafkaCfg),
    /// NATS subject publisher (requires `--features nats`).
    Nats(NatsCfg),
    /// RabbitMQ exchange publisher (requires `--features rabbitmq`).
    Rabbitmq(RabbitmqCfg),
}

impl NotifierConfig {
    /// Team routing tags for this sink (`empty` = receive all events).
    #[must_use]
    pub fn routing_tags(&self) -> Vec<String> {
        match self {
            Self::Apprise(cfg) => cfg.tags.clone(),
            Self::Webhook(cfg) => cfg.tags.clone(),
            Self::Express(cfg) => cfg.tags.clone(),
            Self::Novu(cfg) => cfg.tags.clone(),
            Self::Slack(cfg) => cfg.tags.clone(),
            Self::Telegram(cfg) => cfg.tags.clone(),
            Self::Smtp(cfg) => cfg.tags.clone(),
            Self::Kafka(cfg) => cfg.tags.clone(),
            Self::Nats(cfg) => cfg.tags.clone(),
            Self::Rabbitmq(cfg) => cfg.tags.clone(),
        }
    }

    /// Mutable access to routing tags (organization namespacing).
    pub(crate) fn routing_tags_mut(&mut self) -> &mut Vec<String> {
        match self {
            Self::Apprise(cfg) => &mut cfg.tags,
            Self::Webhook(cfg) => &mut cfg.tags,
            Self::Express(cfg) => &mut cfg.tags,
            Self::Novu(cfg) => &mut cfg.tags,
            Self::Slack(cfg) => &mut cfg.tags,
            Self::Telegram(cfg) => &mut cfg.tags,
            Self::Smtp(cfg) => &mut cfg.tags,
            Self::Kafka(cfg) => &mut cfg.tags,
            Self::Nats(cfg) => &mut cfg.tags,
            Self::Rabbitmq(cfg) => &mut cfg.tags,
        }
    }

    pub(crate) fn build(&self, http: reqwest::Client) -> anyhow::Result<Sink> {
        match self {
            NotifierConfig::Apprise(cfg) => Ok(Sink::Apprise(cfg.build(http)?)),
            NotifierConfig::Webhook(cfg) => {
                let url = resolve_secret(&cfg.url, cfg.url_env.as_deref(), "");
                let name = cfg.name.clone().unwrap_or_else(|| {
                    if url.is_empty() {
                        "webhook".to_owned()
                    } else {
                        url.clone()
                    }
                });
                let secret = resolve_secret(
                    &cfg.secret,
                    cfg.secret_env.as_deref(),
                    "XRELEASE_WEBHOOK_SECRET",
                );
                let headers = resolve_webhook_headers(cfg);
                let notifier = WebhookNotifier::new(
                    http,
                    name,
                    url,
                    cfg.method.into(),
                    &headers,
                    cfg.content_type.as_deref(),
                    cfg.template.clone(),
                )?
                .with_signing(&secret, cfg.signature_header.as_deref())?;
                Ok(Sink::Webhook(notifier))
            }
            NotifierConfig::Express(cfg) => {
                let name = cfg
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("express:{}", cfg.group_chat_id));
                let access_token = resolve_secret(
                    &cfg.access_token,
                    cfg.access_token_env.as_deref(),
                    "XRELEASE_EXPRESS_ACCESS_TOKEN",
                );
                let notifier = ExpressNotifier::new(
                    http,
                    &ExpressNotifierOptions {
                        name,
                        base_url: cfg.base_url.clone(),
                        access_token,
                        group_chat_id: cfg.group_chat_id.clone(),
                        recipients: cfg.recipients.clone(),
                        template: cfg.template.clone(),
                    },
                )?;
                Ok(Sink::Express(notifier))
            }
            NotifierConfig::Novu(cfg) => {
                let name = cfg
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("novu:{}", cfg.workflow));
                let api_key = resolve_secret(
                    &cfg.api_key,
                    cfg.api_key_env.as_deref(),
                    "XRELEASE_NOVU_API_KEY",
                );
                let notifier = NovuNotifier::new(
                    http,
                    &NovuNotifierOptions {
                        name,
                        base_url: cfg.base_url.clone(),
                        api_key,
                        workflow: cfg.workflow.clone(),
                        topic_key: cfg.topic_key.clone(),
                        subscriber_id: cfg.subscriber_id.clone(),
                    },
                )?;
                Ok(Sink::Novu(notifier))
            }
            NotifierConfig::Slack(cfg) => {
                let webhook_url = resolve_secret(
                    &cfg.webhook_url,
                    cfg.webhook_url_env.as_deref(),
                    "XRELEASE_SLACK_WEBHOOK_URL",
                );
                let bot_token = resolve_secret(
                    &cfg.bot_token,
                    cfg.bot_token_env.as_deref(),
                    "XRELEASE_SLACK_BOT_TOKEN",
                );
                let name = cfg.name.clone().unwrap_or_else(|| {
                    if !webhook_url.is_empty() {
                        "slack:webhook".to_owned()
                    } else if !cfg.channel.trim().is_empty() {
                        format!("slack:{}", cfg.channel.trim())
                    } else {
                        "slack".to_owned()
                    }
                });
                let notifier = SlackNotifier::new(
                    http,
                    &SlackNotifierOptions {
                        name,
                        webhook_url,
                        bot_token,
                        channel: cfg.channel.clone(),
                        api_base: cfg.api_base.clone(),
                        template: cfg.template.clone(),
                    },
                )?;
                Ok(Sink::Slack(notifier))
            }
            NotifierConfig::Telegram(cfg) => {
                let name = cfg
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("telegram:{}", cfg.chat_id));
                let bot_token = resolve_secret(
                    &cfg.bot_token,
                    cfg.bot_token_env.as_deref(),
                    "XRELEASE_TELEGRAM_BOT_TOKEN",
                );
                let notifier = TelegramNotifier::new(
                    http,
                    &TelegramNotifierOptions {
                        name,
                        api_base: cfg.api_base.clone(),
                        bot_token,
                        chat_id: cfg.chat_id.clone(),
                        parse_mode: cfg.parse_mode.clone(),
                        template: cfg.template.clone(),
                    },
                )?;
                Ok(Sink::Telegram(notifier))
            }
            NotifierConfig::Smtp(cfg) => {
                let name = cfg
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("smtp:{}", cfg.host));
                let password = resolve_secret(
                    &cfg.password,
                    cfg.password_env.as_deref(),
                    "XRELEASE_SMTP_PASSWORD",
                );
                let notifier =
                    crate::notify::SmtpNotifier::new(&crate::notify::SmtpNotifierOptions {
                        name,
                        host: cfg.host.clone(),
                        port: cfg.port,
                        username: cfg.username.clone(),
                        password,
                        from: cfg.from.clone(),
                        to: cfg.to.clone(),
                        tls: cfg.tls.into(),
                        subject_template: cfg.subject_template.clone(),
                        template: cfg.template.clone(),
                        body_format: cfg.body_format.clone(),
                    })?;
                Ok(Sink::Smtp(notifier))
            }
            NotifierConfig::Kafka(cfg) => build_kafka_sink(cfg),
            NotifierConfig::Nats(cfg) => build_nats_sink(cfg),
            NotifierConfig::Rabbitmq(cfg) => build_rabbitmq_sink(cfg),
        }
    }
}

#[cfg(feature = "kafka")]
fn build_kafka_sink(cfg: &KafkaCfg) -> anyhow::Result<Sink> {
    let name = cfg.name.clone().unwrap_or_else(|| cfg.topic.clone());
    let notifier = crate::notify::kafka::KafkaNotifier::new(
        name,
        &cfg.brokers,
        cfg.topic.clone(),
        cfg.key.clone(),
        cfg.template.clone(),
    )?;
    Ok(Sink::Kafka(notifier))
}

#[cfg(not(feature = "kafka"))]
fn build_kafka_sink(_cfg: &KafkaCfg) -> anyhow::Result<Sink> {
    anyhow::bail!("notifier `type = \"kafka\"` requires building xrelease with `--features kafka`")
}

#[cfg(feature = "nats")]
fn build_nats_sink(cfg: &NatsCfg) -> anyhow::Result<Sink> {
    let url = resolve_secret(&cfg.url, cfg.url_env.as_deref(), "XRELEASE_NATS_URL");
    let name = cfg.name.clone().unwrap_or_else(|| cfg.subject.clone());
    let notifier = crate::notify::nats::NatsNotifier::new(
        name,
        url,
        cfg.subject.clone(),
        cfg.template.clone(),
    )?;
    Ok(Sink::Nats(notifier))
}

#[cfg(not(feature = "nats"))]
fn build_nats_sink(_cfg: &NatsCfg) -> anyhow::Result<Sink> {
    anyhow::bail!("notifier `type = \"nats\"` requires building xrelease with `--features nats`")
}

#[cfg(feature = "rabbitmq")]
fn build_rabbitmq_sink(cfg: &RabbitmqCfg) -> anyhow::Result<Sink> {
    let url = resolve_secret(&cfg.url, cfg.url_env.as_deref(), "XRELEASE_RABBITMQ_URL");
    let name = cfg.name.clone().unwrap_or_else(|| cfg.routing_key.clone());
    let notifier = crate::notify::rabbitmq::RabbitMqNotifier::new(
        name,
        url,
        cfg.exchange.clone(),
        cfg.routing_key.clone(),
        cfg.template.clone(),
    )?;
    Ok(Sink::RabbitMq(notifier))
}

#[cfg(not(feature = "rabbitmq"))]
fn build_rabbitmq_sink(_cfg: &RabbitmqCfg) -> anyhow::Result<Sink> {
    anyhow::bail!(
        "notifier `type = \"rabbitmq\"` requires building xrelease with `--features rabbitmq`"
    )
}

/// Resolve a notifier secret from (in order): an inline value, a per-notifier
/// named env var, then a global fallback env var. Returns an empty string when
/// none resolve (the notifier constructor surfaces the misconfiguration).
///
/// Resolve inline → named env/vault → optional global env (blank global skips).
///
/// This keeps multi-team setups GitOps-clean: each `[[notifiers]]` references a
/// distinct env var **name** (committable), while the secret **value** lives in
/// `.env` / a Kubernetes Secret / `app_secret` only.
fn resolve_secret(inline: &str, named_env: Option<&str>, global_env: &str) -> String {
    // Treat a persisted API placeholder as unset so we fall through to env.
    let inline = sanitize_secret(inline);
    if !inline.trim().is_empty() {
        return inline;
    }
    if let Some(name) = named_env.map(str::trim).filter(|name| !name.is_empty()) {
        if let Some(value) = super::env_token(name) {
            return sanitize_secret(&value);
        }
    }
    if global_env.trim().is_empty() {
        return String::new();
    }
    sanitize_secret(&super::env_token(global_env).unwrap_or_default())
}

fn resolve_webhook_headers(cfg: &WebhookCfg) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    for (key, value) in &cfg.headers {
        let value = sanitize_secret(value);
        if !value.is_empty() {
            headers.insert(key.clone(), value);
        }
    }
    if !headers.is_empty() {
        return headers;
    }
    let Some(raw) = cfg
        .headers_env
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .and_then(super::env_token)
    else {
        return headers;
    };
    if let Ok(parsed) = serde_json::from_str::<HashMap<String, String>>(&raw) {
        for (key, value) in parsed {
            let value = sanitize_secret(&value);
            if !value.is_empty() {
                headers.insert(key, value);
            }
        }
    }
    headers
}

/// Drop API redaction placeholders that were accidentally persisted as secrets.
fn sanitize_secret(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "<redacted>" || trimmed.contains("<redacted>") {
        String::new()
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn config_should_parse_notifiers_array() {
        let toml = r#"
            [[notifiers]]
            type = "webhook"
            name = "n8n"
            url = "https://n8n.example.com/hook"
            headers = { Authorization = "Bearer x" }

            [[sources]]
            type = "github"
            repo = "a/b"
        "#;
        let config: Config = toml::from_str(toml).expect("parse");
        assert_eq!(config.notifiers.len(), 1);
    }

    #[test]
    fn build_notifiers_should_fan_out_apprise_and_webhook() {
        let toml = r#"
            [[notifiers]]
            type = "apprise"
            endpoint = "http://apprise:8000"
            urls = ["mailto://u:p@example.com"]

            [[notifiers]]
            type = "webhook"
            url = "https://hooks.example.com/xrelease"
        "#;
        let config: Config = toml::from_str(toml).expect("parse");
        let composite = config
            .build_notifiers(reqwest::Client::new())
            .expect("notifiers");
        assert_eq!(composite.len(), 2);
        assert_eq!(composite.kinds(), vec!["apprise", "webhook"]);
    }

    #[test]
    fn build_notifiers_should_accept_webhook_only() {
        let toml = r#"
            [[notifiers]]
            type = "webhook"
            url = "https://hooks.example.com/xrelease"
        "#;
        let config: Config = toml::from_str(toml).expect("parse");
        let composite = config
            .build_notifiers(reqwest::Client::new())
            .expect("notifiers");
        assert_eq!(composite.len(), 1);
        assert_eq!(composite.kinds(), vec!["webhook"]);
    }

    #[test]
    fn build_notifiers_should_allow_empty_for_idle_boot() {
        let config: Config = toml::from_str("").expect("parse");
        let composite = config
            .build_notifiers(reqwest::Client::new())
            .expect("empty sinks are valid for UI-first idle boot");
        assert!(composite.is_empty());
        assert!(composite.kinds().is_empty());
    }

    #[test]
    fn api_mode_empty_orgs_should_build_idle_notifiers() {
        let config: Config = toml::from_str(
            r#"
            [database]
            postgres_url = "postgres://xrelease:xrelease@127.0.0.1/xrelease"

            [config_api]
            api_config = true
            source = "api"
            ui_config = true

            [[organizations]]
            id = "platform"
        "#,
        )
        .expect("parse");
        assert!(config.config_api.ledger_is_bootable());
        let composite = config
            .build_notifiers(reqwest::Client::new())
            .expect("idle boot");
        assert!(composite.is_empty());
        let key_env = "XRELEASE_CONFIG_ENCRYPTION_KEY";
        let prev = std::env::var(key_env).ok();
        std::env::set_var(key_env, "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=");
        let report = crate::validate::validate(&config);
        match prev {
            Some(value) => std::env::set_var(key_env, value),
            None => std::env::remove_var(key_env),
        }
        assert!(report.valid, "errors: {:?}", report.errors);
        assert!(report.warnings.iter().any(|w| w.contains("notifiers")));
    }

    #[test]
    fn config_should_parse_express_notifier() {
        let toml = r#"
            [[notifiers]]
            type = "express"
            base_url = "https://cts.example.com"
            access_token = "permanent-bearer"
            group_chat_id = "dec60c05-77b7-0d78-159e-b4fbee4d48f6"
            template = "{{title}}"

            [[sources]]
            type = "github"
            repo = "a/b"
        "#;
        let config: Config = toml::from_str(toml).expect("parse");
        assert_eq!(config.notifiers.len(), 1);
        match &config.notifiers[0] {
            NotifierConfig::Express(cfg) => {
                assert_eq!(cfg.base_url, "https://cts.example.com");
                assert_eq!(cfg.access_token, "permanent-bearer");
            }
            other => panic!("expected express, got {other:?}"),
        }
    }

    #[test]
    fn config_should_parse_and_build_novu_notifier() {
        let toml = r#"
            [[notifiers]]
            type = "novu"
            workflow = "xrelease-new-release"
            topic_key = "{{tag}}"
            api_key = "nv_test"
            tags = ["platform-team"]

            [[sources]]
            type = "github"
            repo = "a/b"
        "#;
        let config: Config = toml::from_str(toml).expect("parse");
        assert_eq!(config.notifiers.len(), 1);
        match &config.notifiers[0] {
            NotifierConfig::Novu(cfg) => {
                assert_eq!(cfg.workflow, "xrelease-new-release");
                assert_eq!(cfg.base_url, "https://api.novu.co");
                assert_eq!(cfg.topic_key.as_deref(), Some("{{tag}}"));
            }
            other => panic!("expected novu, got {other:?}"),
        }
        let composite = config
            .build_notifiers(reqwest::Client::new())
            .expect("notifiers");
        assert_eq!(composite.kinds(), vec!["novu"]);
    }

    #[test]
    fn config_should_parse_and_build_slack_webhook_notifier() {
        let toml = r#"
            [[notifiers]]
            type = "slack"
            name = "ops"
            webhook_url = "https://hooks.slack.com/services/T/B/X"
            tags = ["platform-team"]

            [[sources]]
            type = "github"
            repo = "a/b"
        "#;
        let config: Config = toml::from_str(toml).expect("parse");
        match &config.notifiers[0] {
            NotifierConfig::Slack(cfg) => {
                assert!(cfg.webhook_url.contains("hooks.slack.com"));
                assert!(cfg.bot_token.is_empty());
            }
            other => panic!("expected slack, got {other:?}"),
        }
        let composite = config
            .build_notifiers(reqwest::Client::new())
            .expect("notifiers");
        assert_eq!(composite.kinds(), vec!["slack"]);
    }

    #[test]
    fn config_should_parse_and_build_telegram_notifier() {
        let toml = r#"
            [[notifiers]]
            type = "telegram"
            chat_id = "-100123"
            bot_token = "123:ABC"
            parse_mode = "HTML"

            [[sources]]
            type = "github"
            repo = "a/b"
        "#;
        let config: Config = toml::from_str(toml).expect("parse");
        match &config.notifiers[0] {
            NotifierConfig::Telegram(cfg) => {
                assert_eq!(cfg.chat_id, "-100123");
                assert_eq!(cfg.api_base, "https://api.telegram.org");
            }
            other => panic!("expected telegram, got {other:?}"),
        }
        let composite = config
            .build_notifiers(reqwest::Client::new())
            .expect("notifiers");
        assert_eq!(composite.kinds(), vec!["telegram"]);
    }

    #[test]
    fn express_legacy_bot_id_fields_should_be_rejected() {
        let toml = r#"
            [[notifiers]]
            type = "express"
            base_url = "https://cts.example.com"
            access_token = "permanent-bearer"
            group_chat_id = "dec60c05-77b7-0d78-159e-b4fbee4d48f6"
            bot_id = "1586cad1-d017-5546-ad90-2b57a7ac668a"
        "#;
        assert!(
            toml::from_str::<Config>(toml).is_err(),
            "express bot_id was removed — use access_token only"
        );
    }

    #[test]
    fn resolve_secret_should_prefer_inline_then_named_then_global() {
        let named = "XRELEASE_TEST_EXPRESS_NAMED";
        let global = "XRELEASE_TEST_EXPRESS_GLOBAL";
        std::env::set_var(named, "named-value");
        std::env::set_var(global, "global-value");

        // Inline wins over everything.
        assert_eq!(resolve_secret("inline", Some(named), global), "inline");
        // Named env wins over global when inline is blank.
        assert_eq!(resolve_secret("  ", Some(named), global), "named-value");
        // Falls back to global when no inline and no named.
        assert_eq!(resolve_secret("", None, global), "global-value");

        std::env::remove_var(named);
        std::env::remove_var(global);
    }

    #[test]
    fn build_notifiers_should_accept_express_only() {
        let toml = r#"
            [[notifiers]]
            type = "express"
            base_url = "https://cts.example.com"
            access_token = "permanent-bearer"
            group_chat_id = "dec60c05-77b7-0d78-159e-b4fbee4d48f6"
        "#;
        let config: Config = toml::from_str(toml).expect("parse");
        let composite = config
            .build_notifiers(reqwest::Client::new())
            .expect("notifiers");
        assert_eq!(composite.len(), 1);
        assert_eq!(composite.kinds(), vec!["express"]);
    }

    #[test]
    fn config_should_parse_smtp_notifier() {
        let toml = r#"
            [[notifiers]]
            type = "smtp"
            host = "smtp.example.com"
            from = "releases@example.com"
            to = ["team@example.com"]
            username = "user"
            password = "secret"
        "#;
        let config: Config = toml::from_str(toml).expect("parse");
        assert_eq!(config.notifiers.len(), 1);
        match &config.notifiers[0] {
            NotifierConfig::Smtp(cfg) => {
                assert_eq!(cfg.host, "smtp.example.com");
                assert_eq!(cfg.port, 587);
            }
            other => panic!("expected smtp, got {other:?}"),
        }
    }

    #[test]
    fn build_notifiers_should_accept_smtp_only() {
        let toml = r#"
            [[notifiers]]
            type = "smtp"
            host = "smtp.example.com"
            from = "releases@example.com"
            to = ["team@example.com"]
        "#;
        let config: Config = toml::from_str(toml).expect("parse");
        let composite = config
            .build_notifiers(reqwest::Client::new())
            .expect("notifiers");
        assert_eq!(composite.len(), 1);
        assert_eq!(composite.kinds(), vec!["smtp"]);
    }
}
