//! Integration tests for the webhook sink and fan-out composite.
//!
//! Uses WireMock as a stand-in HTTP endpoint to assert that the generic webhook
//! notifier delivers the canonical payload, honours templates, and that
//! [`CompositeNotifier`] fan-out fails when any sink rejects.

use std::collections::HashMap;

use wiremock::matchers::{body_json_string, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use xrelease::notify::payload::EventPayload;
use xrelease::notify::{
    CompositeNotifier, Event, Notifier, RoutedSink, Sink, WebhookMethod, WebhookNotifier,
};

fn sample_event() -> Event {
    Event {
        source_id: "github:org/app".into(),
        source_kind: "GitHub".into(),
        title: "app: v2.0.0".into(),
        body: "release notes".into(),
        url: Some("https://example.test/r".into()),
        routing_tag: Some("platform".into()),
    }
}

#[tokio::test]
async fn webhook_should_post_canonical_json_payload() {
    let server = MockServer::start().await;
    let event = sample_event();
    let expected = EventPayload::from_event(&event).to_json();

    Mock::given(method("POST"))
        .and(path("/hook"))
        .and(header("content-type", "application/json"))
        .and(body_json_string(expected))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let notifier = WebhookNotifier::new(
        reqwest::Client::new(),
        "hook",
        format!("{}/hook", server.uri()),
        WebhookMethod::Post,
        &HashMap::new(),
        None,
        None,
    )
    .expect("notifier");

    notifier.notify(&event).await.expect("delivered");
}

#[tokio::test]
async fn webhook_should_render_template_body_and_custom_header() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/slack"))
        .and(header("x-token", "secret"))
        .and(body_json_string(r#"{"text":"GitHub: app: v2.0.0"}"#))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let mut headers = HashMap::new();
    headers.insert("X-Token".to_owned(), "secret".to_owned());

    let notifier = WebhookNotifier::new(
        reqwest::Client::new(),
        "slack",
        format!("{}/slack", server.uri()),
        WebhookMethod::Post,
        &headers,
        Some("application/json"),
        Some(r#"{"text":"{{source_kind}}: {{title}}"}"#.to_owned()),
    )
    .expect("notifier");

    notifier.notify(&sample_event()).await.expect("delivered");
}

#[tokio::test]
async fn webhook_should_error_on_non_success_status() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let notifier = WebhookNotifier::new(
        reqwest::Client::new(),
        "hook",
        format!("{}/hook", server.uri()),
        WebhookMethod::Post,
        &HashMap::new(),
        None,
        None,
    )
    .expect("notifier");

    let result = notifier.notify(&sample_event()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn webhook_should_sign_body_with_hmac_when_secret_set() {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let server = MockServer::start().await;
    let event = sample_event();
    let body = EventPayload::from_event(&event).to_json();

    let mut mac = Hmac::<Sha256>::new_from_slice(b"shh").expect("key");
    mac.update(body.as_bytes());
    let digest = mac.finalize().into_bytes();
    let expected = format!(
        "sha256={}",
        digest
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );

    Mock::given(method("POST"))
        .and(path("/hook"))
        .and(header("X-Signature-256", expected.as_str()))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let notifier = WebhookNotifier::new(
        reqwest::Client::new(),
        "hook",
        format!("{}/hook", server.uri()),
        WebhookMethod::Post,
        &HashMap::new(),
        None,
        None,
    )
    .expect("notifier")
    .with_signing("shh", None)
    .expect("signing");

    notifier.notify(&event).await.expect("delivered");
}

#[tokio::test]
async fn composite_should_fan_out_to_all_sinks() {
    let first = MockServer::start().await;
    let second = MockServer::start().await;
    for server in [&first, &second] {
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(server)
            .await;
    }

    let make = |uri: String| {
        RoutedSink::new(
            Sink::Webhook(
                WebhookNotifier::new(
                    reqwest::Client::new(),
                    "hook",
                    uri,
                    WebhookMethod::Post,
                    &HashMap::new(),
                    None,
                    None,
                )
                .expect("notifier"),
            ),
            Vec::new(),
        )
    };

    let composite = CompositeNotifier::new(vec![make(first.uri()), make(second.uri())]);
    assert_eq!(composite.len(), 2);
    composite.notify(&sample_event()).await.expect("fan-out");
}

#[tokio::test]
async fn composite_should_route_by_team_tag() {
    let platform = MockServer::start().await;
    let security = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&platform)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&security)
        .await;

    let make = |uri: String, tags: Vec<&str>| {
        RoutedSink::new(
            Sink::Webhook(
                WebhookNotifier::new(
                    reqwest::Client::new(),
                    "hook",
                    uri,
                    WebhookMethod::Post,
                    &HashMap::new(),
                    None,
                    None,
                )
                .expect("notifier"),
            ),
            tags.into_iter().map(str::to_owned).collect(),
        )
    };

    let composite = CompositeNotifier::new(vec![
        make(platform.uri(), vec!["platform-team"]),
        make(security.uri(), vec!["security-team"]),
    ]);

    let mut event = sample_event();
    event.routing_tag = Some("platform-team".into());
    composite.notify(&event).await.expect("routed delivery");
}

#[tokio::test]
async fn composite_should_fail_when_one_sink_rejects() {
    let ok = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&ok)
        .await;
    let bad = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&bad)
        .await;

    let make = |uri: String| {
        RoutedSink::new(
            Sink::Webhook(
                WebhookNotifier::new(
                    reqwest::Client::new(),
                    "hook",
                    uri,
                    WebhookMethod::Post,
                    &HashMap::new(),
                    None,
                    None,
                )
                .expect("notifier"),
            ),
            Vec::new(),
        )
    };

    let composite = CompositeNotifier::new(vec![make(ok.uri()), make(bad.uri())]);
    let result = composite.notify(&sample_event()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn express_should_post_direct_with_access_token_only() {
    use xrelease::notify::{ExpressNotifier, ExpressNotifierOptions};

    let server = MockServer::start().await;
    let chat_id = "dec60c05-77b7-0d78-159e-b4fbee4d48f6";

    // HMAC token endpoint must never be called.
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    let expected_body = serde_json::json!({
        "group_chat_id": chat_id,
        "notification": {
            "body": "app: v2.0.0\n\nrelease notes\n\nhttps://example.test/r"
        }
    });

    Mock::given(method("POST"))
        .and(path("/api/v4/botx/notifications/direct"))
        .and(header("authorization", "Bearer permanent-token"))
        .and(body_json_string(expected_body.to_string()))
        .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({
            "status": "ok",
            "result": { "sync_id": "00000000-0000-0000-0000-000000000001" }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let notifier = ExpressNotifier::new(
        reqwest::Client::new(),
        &ExpressNotifierOptions {
            name: "express".into(),
            base_url: server.uri(),
            access_token: "permanent-token".into(),
            group_chat_id: chat_id.to_owned(),
            recipients: Vec::new(),
            template: None,
        },
    )
    .expect("notifier");

    notifier.notify(&sample_event()).await.expect("delivered");
}
