//! Direct SMTP delivery without an Apprise sidecar.

use lettre::message::{header::ContentType, Mailbox, Message, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};

use std::time::Duration;

use crate::error::NotifyError;
use crate::notify::payload::{default_markdown_body, default_plain_message, render_template_or};
use crate::notify::{Event, Notifier};

/// Bound on a single SMTP send so a stalled server can't hang delivery
/// indefinitely (mirrors the Kafka producer send timeout).
const SMTP_TIMEOUT: Duration = Duration::from_secs(30);

/// TLS mode for the SMTP transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtpTlsMode {
    /// STARTTLS on port 587 (default).
    StartTls,
    /// Implicit TLS on port 465.
    Tls,
    /// Plain SMTP (no encryption).
    Plain,
}

/// Runtime options for [`SmtpNotifier`].
#[derive(Clone)]
pub struct SmtpNotifierOptions {
    /// Diagnostic label for logs.
    pub name: String,
    /// SMTP server hostname.
    pub host: String,
    /// SMTP port.
    pub port: u16,
    /// Optional SMTP AUTH username.
    pub username: Option<String>,
    /// SMTP AUTH password.
    pub password: String,
    /// Sender mailbox (`From` header).
    pub from: String,
    /// Recipient mailboxes (`To` header).
    pub to: Vec<String>,
    /// Transport encryption mode.
    pub tls: SmtpTlsMode,
    /// Optional subject template (`{{title}}`, …); default = event title.
    pub subject_template: Option<String>,
    /// Optional body template; when set, wins over [`Self::body_format`] defaults.
    pub template: Option<String>,
    /// Body format when `template` is unset: `text` (default) or `markdown`.
    pub body_format: String,
}

/// Delivers notifications via SMTP.
#[derive(Clone)]
pub struct SmtpNotifier {
    mailer: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
    to: Vec<Mailbox>,
    name: String,
    subject_template: Option<String>,
    template: Option<String>,
    body_format: String,
}

fn apply_credentials(
    mut builder: AsyncSmtpTransportBuilder,
    username: &Option<String>,
    password: &str,
) -> AsyncSmtpTransportBuilder {
    if let Some(username) = username {
        if !password.is_empty() {
            builder = builder.credentials(Credentials::new(username.clone(), password.to_owned()));
        }
    }
    builder
}

type AsyncSmtpTransportBuilder = lettre::transport::smtp::AsyncSmtpTransportBuilder;

impl SmtpNotifier {
    /// Build an async SMTP transport from options.
    ///
    /// # Errors
    /// Returns [`NotifyError::Misconfigured`] when addresses or transport settings are invalid.
    pub fn new(options: &SmtpNotifierOptions) -> Result<Self, NotifyError> {
        if options.host.trim().is_empty() {
            return Err(NotifyError::Misconfigured(
                "smtp `host` must not be empty".to_owned(),
            ));
        }
        if options.to.is_empty() {
            return Err(NotifyError::Misconfigured(
                "smtp `to` must contain at least one address".to_owned(),
            ));
        }

        let from: Mailbox = options.from.parse().map_err(|_| {
            NotifyError::Misconfigured(format!("invalid smtp `from` address `{}`", options.from))
        })?;
        let to: Result<Vec<Mailbox>, NotifyError> = options
            .to
            .iter()
            .map(|addr| {
                addr.parse().map_err(|_| {
                    NotifyError::Misconfigured(format!("invalid smtp `to` address `{addr}`"))
                })
            })
            .collect();
        let to = to?;

        let mailer = match options.tls {
            SmtpTlsMode::StartTls => {
                let builder =
                    AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(options.host.as_str())
                        .map_err(|err| NotifyError::Misconfigured(format!("smtp relay: {err}")))?
                        .port(options.port);
                apply_credentials(builder, &options.username, &options.password)
            }
            SmtpTlsMode::Tls => {
                let tls = TlsParameters::new(options.host.clone()).map_err(|err| {
                    NotifyError::Misconfigured(format!("smtp tls parameters: {err}"))
                })?;
                let builder =
                    AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(options.host.as_str())
                        .port(options.port)
                        .tls(Tls::Wrapper(tls));
                apply_credentials(builder, &options.username, &options.password)
            }
            SmtpTlsMode::Plain => {
                let builder =
                    AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(options.host.as_str())
                        .port(options.port);
                apply_credentials(builder, &options.username, &options.password)
            }
        };

        Ok(Self {
            mailer: mailer.timeout(Some(SMTP_TIMEOUT)).build(),
            from,
            to,
            name: options.name.clone(),
            subject_template: options.subject_template.clone(),
            template: options.template.clone(),
            body_format: options.body_format.clone(),
        })
    }

    fn render_subject(&self, event: &Event) -> String {
        render_template_or(self.subject_template.as_deref(), event, |event| {
            event.title.clone()
        })
    }

    fn render_body(&self, event: &Event) -> String {
        render_template_or(self.template.as_deref(), event, |event| {
            match self.body_format.as_str() {
                "markdown" => default_markdown_body(event),
                _ => default_plain_message(event),
            }
        })
    }

    /// Diagnostic label for logs.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Build a plain-text MIME message for SMTP delivery.
pub(crate) fn build_message(
    from: &Mailbox,
    to: &[Mailbox],
    subject: &str,
    body: &str,
) -> Result<Message, NotifyError> {
    let text_part = SinglePart::builder()
        .header(ContentType::TEXT_PLAIN)
        .body(body.to_owned());

    let mut builder = Message::builder()
        .from(from.clone())
        .subject(subject.to_owned());
    for recipient in to {
        builder = builder.to(recipient.clone());
    }

    builder
        .singlepart(text_part)
        .map_err(|err| NotifyError::Backend {
            backend: "smtp",
            message: err.to_string(),
        })
}

impl Notifier for SmtpNotifier {
    fn notify(
        &self,
        event: &Event,
    ) -> impl std::future::Future<Output = Result<(), NotifyError>> + Send {
        let subject = self.render_subject(event);
        let body = self.render_body(event);
        let from = self.from.clone();
        let to = self.to.clone();
        let mailer = self.mailer.clone();

        async move {
            let message = build_message(&from, &to, &subject, &body)?;
            mailer
                .send(message)
                .await
                .map_err(|err| NotifyError::Backend {
                    backend: "smtp",
                    message: err.to_string(),
                })?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> Event {
        Event {
            source_id: "github:org/app".into(),
            source_kind: "GitHub".into(),
            title: "app: v2.0.0".into(),
            body: "Release notes".into(),
            url: Some("https://example.test/r".into()),
            routing_tag: None,
        }
    }

    fn options() -> SmtpNotifierOptions {
        SmtpNotifierOptions {
            name: "smtp".into(),
            host: "smtp.example.com".into(),
            port: 587,
            username: Some("user".into()),
            password: "secret".into(),
            from: "releases@example.com".into(),
            to: vec!["team@example.com".into()],
            tls: SmtpTlsMode::StartTls,
            subject_template: None,
            template: None,
            body_format: "text".into(),
        }
    }

    #[test]
    fn new_should_reject_empty_host() {
        let mut opts = options();
        opts.host.clear();
        assert!(SmtpNotifier::new(&opts).is_err());
    }

    #[test]
    fn new_should_reject_empty_recipients() {
        let mut opts = options();
        opts.to.clear();
        assert!(SmtpNotifier::new(&opts).is_err());
    }

    #[test]
    fn render_subject_should_use_template_when_set() {
        let mut opts = options();
        opts.subject_template = Some("[{{kind}}] {{title}}".into());
        let notifier = SmtpNotifier::new(&opts).expect("notifier");
        assert_eq!(notifier.render_subject(&event()), "[GitHub] app: v2.0.0");
    }

    #[test]
    fn render_body_should_prefer_template_over_body_format() {
        let mut opts = options();
        opts.template = Some("{{title}} → {{url}}".into());
        opts.body_format = "markdown".into();
        let notifier = SmtpNotifier::new(&opts).expect("notifier");
        assert_eq!(
            notifier.render_body(&event()),
            "app: v2.0.0 → https://example.test/r"
        );
    }

    #[test]
    fn build_message_should_include_title_and_body_in_text_part() {
        let from: Mailbox = "releases@example.com".parse().expect("from");
        let to: Mailbox = "team@example.com".parse().expect("to");
        let notifier = SmtpNotifier::new(&options()).expect("notifier");
        let subject = notifier.render_subject(&event());
        let body = notifier.render_body(&event());

        let message = build_message(&from, &[to], &subject, &body).expect("message");
        let raw = String::from_utf8(message.formatted()).expect("utf8");
        assert!(raw.contains("Subject: app: v2.0.0"));
        assert!(raw.contains("Release notes"));
        assert!(raw.contains("https://example.test/r"));
    }
}
