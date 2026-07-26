//! Telegram Bot API notification sink.
//!
//! Delivers each [`Event`] via
//! [`sendMessage`](https://core.telegram.org/bots/api#sendmessage):
//!
//! `POST {api_base}/bot{token}/sendMessage` with `{ chat_id, text, parse_mode? }`.

use crate::error::NotifyError;
use crate::notify::payload::{default_chat_message, render_template, render_template_or};
use crate::notify::{Event, Notifier};

fn default_api_base() -> String {
    "https://api.telegram.org".to_owned()
}

/// Configuration for [`TelegramNotifier`].
#[derive(Clone)]
pub struct TelegramNotifierOptions {
    /// Diagnostic label for logs.
    pub name: String,
    /// Telegram Bot API base (default `https://api.telegram.org`).
    pub api_base: String,
    /// Bot token from `@BotFather`.
    pub bot_token: String,
    /// Chat / channel / group id (supports `{{tag}}` templates).
    pub chat_id: String,
    /// Optional `parse_mode`: `Markdown`, `MarkdownV2`, or `HTML`.
    pub parse_mode: Option<String>,
    /// Optional body template; `None` = title + Markdown body.
    pub template: Option<String>,
}

/// Sink that posts release events to a Telegram chat.
#[derive(Clone)]
pub struct TelegramNotifier {
    client: reqwest::Client,
    api_base: String,
    bot_token: String,
    chat_id: String,
    parse_mode: Option<String>,
    template: Option<String>,
    name: String,
}

impl TelegramNotifier {
    /// Construct a Telegram Bot API sink.
    ///
    /// # Errors
    /// Returns [`NotifyError::Misconfigured`] when a required field is empty.
    pub fn new(
        client: reqwest::Client,
        options: &TelegramNotifierOptions,
    ) -> Result<Self, NotifyError> {
        let require = |field: &str, value: &str| -> Result<(), NotifyError> {
            if value.trim().is_empty() {
                Err(NotifyError::Misconfigured(format!(
                    "telegram `{field}` must not be empty"
                )))
            } else {
                Ok(())
            }
        };
        let api_base = if options.api_base.trim().is_empty() {
            default_api_base()
        } else {
            options.api_base.clone()
        };
        require("bot_token", &options.bot_token)?;
        require("chat_id", &options.chat_id)?;

        let parse_mode = options
            .parse_mode
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        if let Some(mode) = &parse_mode {
            match mode.as_str() {
                "Markdown" | "MarkdownV2" | "HTML" => {}
                other => {
                    return Err(NotifyError::Misconfigured(format!(
                        "telegram `parse_mode` must be Markdown|MarkdownV2|HTML, got `{other}`"
                    )));
                }
            }
        }

        Ok(Self {
            client,
            api_base: api_base.trim_end_matches('/').to_owned(),
            bot_token: options.bot_token.trim().to_owned(),
            chat_id: options.chat_id.clone(),
            parse_mode,
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

impl Notifier for TelegramNotifier {
    fn notify(
        &self,
        event: &Event,
    ) -> impl std::future::Future<Output = Result<(), NotifyError>> + Send {
        let client = self.client.clone();
        let url = format!("{}/bot{}/sendMessage", self.api_base, self.bot_token);
        let chat_id = render_template(&self.chat_id, event);
        let text = self.render_message(event);
        let parse_mode = self.parse_mode.clone();

        async move {
            if chat_id.trim().is_empty() {
                return Err(NotifyError::Misconfigured(
                    "telegram `chat_id` rendered empty".to_owned(),
                ));
            }
            let mut payload = serde_json::json!({
                "chat_id": chat_id,
                "text": text,
                "disable_web_page_preview": true,
            });
            if let Some(mode) = parse_mode {
                payload["parse_mode"] = serde_json::json!(mode);
            }
            let response = client.post(&url).json(&payload).send().await?;
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if !status.is_success() {
                return Err(NotifyError::Rejected {
                    backend: "telegram",
                    status: status.as_u16(),
                    body: format!("POST sendMessage failed: {body}"),
                });
            }
            // Bot API returns HTTP 200 with `"ok": false` on logical errors.
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                if json.get("ok").and_then(serde_json::Value::as_bool) == Some(false) {
                    let description = json
                        .get("description")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("unknown");
                    return Err(NotifyError::Rejected {
                        backend: "telegram",
                        status: status.as_u16(),
                        body: format!("sendMessage ok=false: {description}"),
                    });
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> TelegramNotifierOptions {
        TelegramNotifierOptions {
            name: "tg".into(),
            api_base: "https://api.telegram.org/".into(),
            bot_token: "123:ABC".into(),
            chat_id: "-1001".into(),
            parse_mode: Some("HTML".into()),
            template: None,
        }
    }

    #[test]
    fn new_should_reject_empty_token() {
        let mut opts = options();
        opts.bot_token.clear();
        assert!(TelegramNotifier::new(reqwest::Client::new(), &opts).is_err());
    }

    #[test]
    fn new_should_reject_bad_parse_mode() {
        let mut opts = options();
        opts.parse_mode = Some("rich".into());
        assert!(TelegramNotifier::new(reqwest::Client::new(), &opts).is_err());
    }

    #[test]
    fn new_should_trim_api_base() {
        let n = TelegramNotifier::new(reqwest::Client::new(), &options()).expect("ok");
        assert_eq!(n.api_base, "https://api.telegram.org");
    }
}
