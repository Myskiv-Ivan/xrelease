//! eXpress (BotX) chat notification sink.
//!
//! Delivers each [`Event`] with a single call to the
//! [BotX Notifications API][api]:
//!
//! `POST /api/v4/botx/notifications/direct` with
//! `Authorization: Bearer <access_token>` and JSON body
//! `{group_chat_id, notification:{body}}`.
//!
//! There is **no** `GET /api/v2/botx/bots/…/token` step — the operator supplies
//! a BotX access token (from the CTS admin panel or a one-shot token fetch
//! outside xrelease). The bot must be a member of the target chat.
//!
//! [api]: https://docs.express.ms/chatbots/developer-guide/api/botx-api/notifications-api/

use crate::error::NotifyError;
use crate::notify::payload::{default_chat_message, render_template_or};
use crate::notify::{Event, Notifier};

/// Configuration for [`ExpressNotifier`].
#[derive(Clone)]
pub struct ExpressNotifierOptions {
    /// Diagnostic label for logs.
    pub name: String,
    /// CTS server base URL (e.g. `https://cts.example.com`).
    pub base_url: String,
    /// BotX Bearer token for `Authorization`.
    pub access_token: String,
    /// Target group chat id (UUID). The bot must be a member.
    pub group_chat_id: String,
    /// Optional recipient bot/user ids; empty = whole chat.
    pub recipients: Vec<String>,
    /// Optional body template; `None` = title + Markdown body.
    pub template: Option<String>,
}

/// Sink that delivers events to an eXpress chat through the BotX API.
#[derive(Clone)]
pub struct ExpressNotifier {
    client: reqwest::Client,
    /// CTS server base URL (e.g. `https://cts.example.com`), no trailing slash.
    base_url: String,
    /// BotX Bearer token.
    access_token: String,
    /// Target group chat id (UUID). The bot must be a member.
    group_chat_id: String,
    /// Optional recipient bot/user ids; empty = whole chat.
    recipients: Vec<String>,
    /// Optional body template; `None` = title + Markdown body.
    template: Option<String>,
    /// Diagnostic label for logs.
    name: String,
}

impl ExpressNotifier {
    /// Construct an eXpress BotX sink (Bearer + POST only).
    ///
    /// # Errors
    /// Returns [`NotifyError::Misconfigured`] when a required field is empty.
    pub fn new(
        client: reqwest::Client,
        options: &ExpressNotifierOptions,
    ) -> Result<Self, NotifyError> {
        let require = |field: &str, value: &str| -> Result<(), NotifyError> {
            if value.trim().is_empty() {
                Err(NotifyError::Misconfigured(format!(
                    "eXpress `{field}` must not be empty"
                )))
            } else {
                Ok(())
            }
        };
        require("base_url", &options.base_url)?;
        require("group_chat_id", &options.group_chat_id)?;
        require("access_token", &options.access_token)?;

        Ok(Self {
            client,
            base_url: options.base_url.trim_end_matches('/').to_owned(),
            access_token: options.access_token.trim().to_owned(),
            group_chat_id: options.group_chat_id.clone(),
            recipients: options.recipients.clone(),
            template: options.template.clone(),
            name: options.name.clone(),
        })
    }

    /// Diagnostic label for logs.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Render the chat message body (template or title + Markdown body).
    fn render_message(&self, event: &Event) -> String {
        render_template_or(self.template.as_deref(), event, default_chat_message)
    }

    fn direct_payload(&self, message: &str) -> serde_json::Value {
        let mut payload = serde_json::json!({
            "group_chat_id": self.group_chat_id,
            "notification": { "body": message },
        });
        if !self.recipients.is_empty() {
            payload["recipients"] = serde_json::json!(self.recipients);
        }
        payload
    }
}

impl Notifier for ExpressNotifier {
    fn notify(
        &self,
        event: &Event,
    ) -> impl std::future::Future<Output = Result<(), NotifyError>> + Send {
        let client = self.client.clone();
        let url = format!("{}/api/v4/botx/notifications/direct", self.base_url);
        let token = self.access_token.clone();
        let payload = self.direct_payload(&self.render_message(event));

        async move {
            let response = client
                .post(&url)
                .bearer_auth(&token)
                .json(&payload)
                .send()
                .await?;
            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(NotifyError::Rejected {
                    backend: "express",
                    status: status.as_u16(),
                    body: format!("POST /api/v4/botx/notifications/direct failed: {body}"),
                });
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> ExpressNotifierOptions {
        ExpressNotifierOptions {
            name: "express".into(),
            base_url: "https://cts.example.com/".into(),
            access_token: "permanent-bearer".into(),
            group_chat_id: "dec60c05-77b7-0d78-159e-b4fbee4d48f6".into(),
            recipients: Vec::new(),
            template: None,
        }
    }

    fn notifier() -> ExpressNotifier {
        ExpressNotifier::new(reqwest::Client::new(), &options()).expect("notifier")
    }

    fn event() -> Event {
        Event {
            source_id: "github:org/app".into(),
            source_kind: "GitHub".into(),
            title: "app: v2.0.0".into(),
            body: "release notes".into(),
            url: Some("https://example.test/r".into()),
            routing_tag: None,
        }
    }

    #[test]
    fn new_should_reject_empty_access_token() {
        let mut opts = options();
        opts.access_token.clear();
        assert!(ExpressNotifier::new(reqwest::Client::new(), &opts).is_err());
    }

    #[test]
    fn new_should_trim_trailing_slash_from_base_url() {
        assert_eq!(notifier().base_url, "https://cts.example.com");
    }

    #[test]
    fn render_message_should_default_to_title_body_and_url() {
        assert_eq!(
            notifier().render_message(&event()),
            "app: v2.0.0\n\nrelease notes\n\nhttps://example.test/r"
        );
    }

    #[test]
    fn render_message_should_use_template_when_set() {
        let mut bot = notifier();
        bot.template = Some("{{title}} — {{url}}".to_owned());
        assert_eq!(
            bot.render_message(&event()),
            "app: v2.0.0 — https://example.test/r"
        );
    }

    #[test]
    fn direct_payload_should_include_recipients_when_set() {
        let mut bot = notifier();
        bot.recipients = vec!["83fbf1c7-f14b-5176-bd32-ca15cf00d4b7".to_owned()];
        let payload = bot.direct_payload("hi");
        assert_eq!(payload["group_chat_id"], bot.group_chat_id);
        assert_eq!(payload["notification"]["body"], "hi");
        assert_eq!(
            payload["recipients"][0],
            "83fbf1c7-f14b-5176-bd32-ca15cf00d4b7"
        );
    }

    #[test]
    fn direct_payload_should_omit_recipients_when_empty() {
        let payload = notifier().direct_payload("hi");
        assert!(payload.get("recipients").is_none());
    }
}
