//! Shared, sink-agnostic event rendering.
//!
//! Webhook and message-broker sinks all serialise the same canonical
//! [`EventPayload`] (stable JSON contract) and reuse the same lightweight
//! [`render_template`] placeholder substitution for routing keys / subjects /
//! custom bodies. Keeping this in one module is the single source of truth for
//! "what an outbound event looks like", so adding a new sink never re-invents
//! the payload shape.

use serde::Serialize;

use crate::notify::Event;

/// Canonical, serialisable view of an [`Event`] for non-Apprise sinks.
///
/// This is the stable JSON contract published to webhooks and message brokers.
/// Field names are deliberately snake_case and flat for easy consumption.
/// `kind` mirrors `source_kind` (same alias as Mustache `{{kind}}` / Novu payload).
#[derive(Debug, Clone, Serialize)]
pub struct EventPayload<'a> {
    /// Stable source identifier (e.g. `github:tokio-rs/tokio`).
    pub source_id: &'a str,
    /// Human-readable source kind (e.g. `GitHub`).
    pub source_kind: &'a str,
    /// Alias of [`Self::source_kind`] for template / Novu parity (`{{kind}}`).
    pub kind: &'a str,
    /// Short notification title.
    pub title: &'a str,
    /// Notification body (Markdown).
    pub body: &'a str,
    /// Canonical release link, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<&'a str>,
    /// Routing tag (mirrors the per-source Apprise tag), when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<&'a str>,
}

impl<'a> EventPayload<'a> {
    /// Borrow an [`Event`] into its canonical payload view (no allocation).
    #[must_use]
    pub fn from_event(event: &'a Event) -> Self {
        Self {
            source_id: &event.source_id,
            source_kind: &event.source_kind,
            kind: &event.source_kind,
            title: &event.title,
            body: &event.body,
            url: event.url.as_deref(),
            tag: event.routing_tag.as_deref(),
        }
    }

    /// Serialise to a compact JSON string (the default body for webhooks / brokers).
    #[must_use]
    pub fn to_json(&self) -> String {
        // EventPayload is always serialisable (only strings); never fails in practice.
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_owned())
    }
}

/// Substitute `{{field}}` placeholders in `template` from an [`Event`].
///
/// Mustache-style double braces are used so the dialect never collides with
/// literal single braces in JSON bodies (`{"text":"{{title}}"}` is valid).
///
/// Supported placeholders: `{{source_id}}`, `{{source_kind}}` (alias `{{kind}}`),
/// `{{title}}`, `{{body}}`, `{{url}}`, `{{tag}}`. Surrounding whitespace inside
/// the braces is trimmed. Unknown placeholders and absent optionals render as
/// empty strings. Shared by custom webhook bodies, Kafka keys, NATS subjects,
/// and AMQP routing keys so every sink speaks one templating dialect.
#[must_use]
pub fn render_template(template: &str, event: &Event) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;

    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        if let Some(close) = after.find("}}") {
            let key = after[..close].trim();
            out.push_str(field_value(key, event));
            rest = &after[close + 2..];
        } else {
            // Unterminated placeholder: emit the remainder verbatim and stop.
            out.push_str(&rest[open..]);
            return out;
        }
    }
    out.push_str(rest);
    out
}

/// Render `template` when set and non-blank; otherwise run `fallback`.
///
/// Shared by sinks that accept an optional body/subject/message template.
/// Blank / whitespace-only templates are treated as unset so a cleared UI field
/// cannot silently deliver an empty message.
#[must_use]
pub fn render_template_or(
    template: Option<&str>,
    event: &Event,
    fallback: impl FnOnce(&Event) -> String,
) -> String {
    match template.map(str::trim).filter(|value| !value.is_empty()) {
        Some(template) => render_template(template, event),
        None => fallback(event),
    }
}

/// Body for webhook / broker sinks: optional template, else canonical JSON.
#[must_use]
pub fn render_body_or_json(template: Option<&str>, event: &Event) -> String {
    render_template_or(template, event, |event| {
        EventPayload::from_event(event).to_json()
    })
}

/// Default human-readable chat / email-style message: title, body, then URL.
///
/// Shared by Slack / Telegram / eXpress (and SMTP `text`) so every chat-like
/// sink includes the canonical release link when one exists.
#[must_use]
pub fn default_chat_message(event: &Event) -> String {
    let mut message = if event.body.is_empty() {
        event.title.clone()
    } else {
        let mut message = String::with_capacity(event.title.len() + event.body.len() + 2);
        message.push_str(&event.title);
        message.push_str("\n\n");
        message.push_str(&event.body);
        message
    };
    append_event_url(&mut message, event);
    message
}

/// Plain-text notification (alias of [`default_chat_message`]).
///
/// Kept for call-site clarity in SMTP.
#[must_use]
pub fn default_plain_message(event: &Event) -> String {
    default_chat_message(event)
}

/// SMTP `body_format: markdown` default: release notes body + optional URL.
///
/// Subject already carries the title, so the body is the Markdown notes only.
#[must_use]
pub fn default_markdown_body(event: &Event) -> String {
    let mut message = event.body.clone();
    append_event_url(&mut message, event);
    message
}

/// Append `event.url` to `message` when present (shared by chat / SMTP defaults).
pub fn append_event_url(message: &mut String, event: &Event) {
    if let Some(url) = event
        .url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        if !message.is_empty() {
            message.push_str("\n\n");
        }
        message.push_str(url);
    }
}

fn field_value<'a>(key: &str, event: &'a Event) -> &'a str {
    match key {
        "source_id" => &event.source_id,
        "source_kind" | "kind" => &event.source_kind,
        "title" => &event.title,
        "body" => &event.body,
        "url" => event.url.as_deref().unwrap_or(""),
        "tag" => event.routing_tag.as_deref().unwrap_or(""),
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Event {
        Event {
            source_id: "github:tokio-rs/tokio".into(),
            source_kind: "GitHub".into(),
            title: "tokio: v1.0.0".into(),
            body: "notes".into(),
            url: Some("https://example.test".into()),
            routing_tag: Some("platform".into()),
        }
    }

    #[test]
    fn render_template_should_substitute_known_fields() {
        let out = render_template("releases.{{kind}}.{{source_id}}", &sample());
        assert_eq!(out, "releases.GitHub.github:tokio-rs/tokio");
    }

    #[test]
    fn render_template_should_emit_empty_for_unknown_or_missing() {
        let mut event = sample();
        event.url = None;
        let out = render_template("{{url}}|{{nope}}|{{tag}}", &event);
        assert_eq!(out, "||platform");
    }

    #[test]
    fn render_template_should_preserve_literal_json_braces() {
        let out = render_template(r#"{"text":"{{title}}"}"#, &sample());
        assert_eq!(out, r#"{"text":"tokio: v1.0.0"}"#);
    }

    #[test]
    fn render_template_should_tolerate_unterminated_placeholder() {
        let out = render_template("prefix {{title", &sample());
        assert_eq!(out, "prefix {{title");
    }

    #[test]
    fn to_json_should_omit_absent_optionals() {
        let mut event = sample();
        event.url = None;
        event.routing_tag = None;
        let payload = EventPayload::from_event(&event);
        let value: serde_json::Value = serde_json::from_str(&payload.to_json()).expect("json");
        assert!(value.get("url").is_none());
        assert!(value.get("tag").is_none());
        assert_eq!(value["source_id"], "github:tokio-rs/tokio");
    }

    #[test]
    fn render_body_or_json_should_prefer_template() {
        let out = render_body_or_json(Some("{{title}}"), &sample());
        assert_eq!(out, "tokio: v1.0.0");
    }

    #[test]
    fn render_body_or_json_should_fall_back_to_canonical_json() {
        let out = render_body_or_json(None, &sample());
        let value: serde_json::Value = serde_json::from_str(&out).expect("json");
        assert_eq!(value["title"], "tokio: v1.0.0");
    }

    #[test]
    fn default_markdown_body_should_be_notes_plus_url() {
        assert_eq!(
            default_markdown_body(&sample()),
            "notes\n\nhttps://example.test"
        );
    }

    #[test]
    fn default_chat_message_should_join_title_body_and_url() {
        assert_eq!(
            default_chat_message(&sample()),
            "tokio: v1.0.0\n\nnotes\n\nhttps://example.test"
        );
    }

    #[test]
    fn default_plain_message_should_match_chat_default() {
        assert_eq!(
            default_plain_message(&sample()),
            default_chat_message(&sample())
        );
    }

    #[test]
    fn render_template_or_should_treat_blank_as_unset() {
        let out = render_template_or(Some("   "), &sample(), |event| event.title.clone());
        assert_eq!(out, "tokio: v1.0.0");
    }

    #[test]
    fn to_json_should_include_kind_alias() {
        let value: serde_json::Value =
            serde_json::from_str(&EventPayload::from_event(&sample()).to_json()).expect("json");
        assert_eq!(value["kind"], "GitHub");
        assert_eq!(value["source_kind"], "GitHub");
    }
}
