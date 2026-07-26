//! Slack notification sink (Incoming Webhook or Bot `chat.postMessage`).
//!
//! Two mutually exclusive modes:
//!
//! 1. **Incoming webhook** — `POST {webhook_url}` with `{ text }`
//!    ([docs](https://api.slack.com/messaging/webhooks)).
//! 2. **Bot token** — `POST https://slack.com/api/chat.postMessage` with
//!    `Authorization: Bearer {token}` and `{ channel, text }`
//!    ([docs](https://api.slack.com/methods/chat.postMessage)).

use crate::error::NotifyError;
use crate::notify::payload::{default_chat_message, render_template, render_template_or};
use crate::notify::{Event, Notifier};

fn default_api_base() -> String {
    "https://slack.com/api".to_owned()
}

/// Configuration for [`SlackNotifier`].
#[derive(Clone)]
pub struct SlackNotifierOptions {
    /// Diagnostic label for logs.
    pub name: String,
    /// Incoming webhook URL (mode 1).
    pub webhook_url: String,
    /// Bot OAuth token `xoxb-…` (mode 2).
    pub bot_token: String,
    /// Channel id/name for bot mode (supports templates).
    pub channel: String,
    /// Slack API base for bot mode (default `https://slack.com/api`).
    pub api_base: String,
    /// Optional body template; `None` = title + Markdown body.
    pub template: Option<String>,
}

#[derive(Clone)]
enum SlackMode {
    Webhook {
        url: String,
    },
    Bot {
        token: String,
        channel: String,
        api_base: String,
    },
}

/// Sink that posts release events to Slack.
#[derive(Clone)]
pub struct SlackNotifier {
    client: reqwest::Client,
    mode: SlackMode,
    template: Option<String>,
    name: String,
}

impl SlackNotifier {
    /// Construct a Slack sink (webhook **or** bot token + channel).
    ///
    /// # Errors
    /// Returns [`NotifyError::Misconfigured`] when neither mode is complete or
    /// both webhook and bot credentials are set.
    pub fn new(
        client: reqwest::Client,
        options: &SlackNotifierOptions,
    ) -> Result<Self, NotifyError> {
        let webhook = options.webhook_url.trim();
        let token = options.bot_token.trim();
        let channel = options.channel.trim();

        let mode = match (!webhook.is_empty(), !token.is_empty()) {
            (true, true) => {
                return Err(NotifyError::Misconfigured(
                    "slack: set either `webhook_url` or `bot_token`+`channel`, not both".to_owned(),
                ));
            }
            (true, false) => SlackMode::Webhook {
                url: webhook.to_owned(),
            },
            (false, true) => {
                if channel.is_empty() {
                    return Err(NotifyError::Misconfigured(
                        "slack `channel` must not be empty when using `bot_token`".to_owned(),
                    ));
                }
                let api_base = if options.api_base.trim().is_empty() {
                    default_api_base()
                } else {
                    options.api_base.trim_end_matches('/').to_owned()
                };
                SlackMode::Bot {
                    token: token.to_owned(),
                    channel: options.channel.clone(),
                    api_base,
                }
            }
            (false, false) => {
                return Err(NotifyError::Misconfigured(
                    "slack: set `webhook_url` or `bot_token`+`channel`".to_owned(),
                ));
            }
        };

        Ok(Self {
            client,
            mode,
            template: options.template.clone(),
            name: options.name.clone(),
        })
    }

    /// Diagnostic label for logs.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    fn render_message(&self, event: &Event) -> String {
        render_template_or(self.template.as_deref(), event, default_chat_message)
    }
}

impl Notifier for SlackNotifier {
    fn notify(
        &self,
        event: &Event,
    ) -> impl std::future::Future<Output = Result<(), NotifyError>> + Send {
        let client = self.client.clone();
        let text = self.render_message(event);
        let mode = self.mode.clone();

        async move {
            match mode {
                SlackMode::Webhook { url } => {
                    let response = client
                        .post(&url)
                        .json(&serde_json::json!({ "text": text }))
                        .send()
                        .await?;
                    let status = response.status();
                    if !status.is_success() {
                        let body = response.text().await.unwrap_or_default();
                        return Err(NotifyError::Rejected {
                            backend: "slack",
                            status: status.as_u16(),
                            body: format!("POST incoming webhook failed: {body}"),
                        });
                    }
                    Ok(())
                }
                SlackMode::Bot {
                    token,
                    channel,
                    api_base,
                } => {
                    let channel = render_template(&channel, event);
                    if channel.trim().is_empty() {
                        return Err(NotifyError::Misconfigured(
                            "slack `channel` rendered empty".to_owned(),
                        ));
                    }
                    let url = format!("{api_base}/chat.postMessage");
                    let response = client
                        .post(&url)
                        .bearer_auth(&token)
                        .json(&serde_json::json!({
                            "channel": channel,
                            "text": text,
                        }))
                        .send()
                        .await?;
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();
                    if !status.is_success() {
                        return Err(NotifyError::Rejected {
                            backend: "slack",
                            status: status.as_u16(),
                            body: format!("POST chat.postMessage failed: {body}"),
                        });
                    }
                    // Slack returns HTTP 200 with `"ok": false` on API errors.
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                        if json.get("ok").and_then(serde_json::Value::as_bool) == Some(false) {
                            let err = json
                                .get("error")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("unknown");
                            return Err(NotifyError::Rejected {
                                backend: "slack",
                                status: status.as_u16(),
                                body: format!("chat.postMessage ok=false: {err}"),
                            });
                        }
                    }
                    Ok(())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_should_accept_webhook_only() {
        let n = SlackNotifier::new(
            reqwest::Client::new(),
            &SlackNotifierOptions {
                name: "s".into(),
                webhook_url: "https://hooks.slack.com/services/T/B/X".into(),
                bot_token: String::new(),
                channel: String::new(),
                api_base: default_api_base(),
                template: None,
            },
        );
        assert!(n.is_ok());
    }

    #[test]
    fn new_should_reject_bot_without_channel() {
        let err = SlackNotifier::new(
            reqwest::Client::new(),
            &SlackNotifierOptions {
                name: "s".into(),
                webhook_url: String::new(),
                bot_token: "xoxb-test".into(),
                channel: String::new(),
                api_base: default_api_base(),
                template: None,
            },
        );
        assert!(err.is_err());
    }

    #[test]
    fn new_should_reject_both_modes() {
        let err = SlackNotifier::new(
            reqwest::Client::new(),
            &SlackNotifierOptions {
                name: "s".into(),
                webhook_url: "https://hooks.slack.com/services/T/B/X".into(),
                bot_token: "xoxb-test".into(),
                channel: "#ops".into(),
                api_base: default_api_base(),
                template: None,
            },
        );
        assert!(err.is_err());
    }
}
