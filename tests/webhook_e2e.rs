//! End-to-end webhook tests — Axum router + PostgreSQL + WireMock Apprise.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xrelease::api::{router, AppState};
use xrelease::config::Config;
use xrelease::engine::Engine;
use xrelease::runtime::build_http_client;

type HmacSha256 = Hmac<Sha256>;

fn test_postgres_url() -> String {
    std::env::var("XRELEASE_TEST_POSTGRES_URL")
        .unwrap_or_else(|_| "postgres://xrelease:xrelease@127.0.0.1:5432/xrelease_test".to_owned())
}

fn test_config(apprise_endpoint: &str, postgres_url: &str) -> Config {
    toml::from_str(&format!(
        r#"
        [[notifiers]]
        type = "apprise"
        endpoint = "{apprise_endpoint}"
        urls = ["mailto://test@example.com"]

        [database]
        postgres_url = "{postgres_url}"
        # Small pool: every e2e test opens its own Engine against the one shared
        # test database, so the default 16 × many parallel tests exhausts
        # Postgres's connection limit and a starved query 500s intermittently.
        max_connections = 4

        [api]
        webhook_secret = "wh-secret"

        [[sources]]
        type = "github"
        repo = "tokio-rs/tokio"
    "#
    ))
    .expect("parse config")
}

async fn build_app(apprise: &MockServer, postgres_url: &str) -> Option<axum::Router> {
    let config = test_config(&apprise.uri(), postgres_url);
    let http = build_http_client().ok()?;
    let watches = config.to_watches().ok()?;
    let engine = Engine::open(&config, http).ok()?;
    engine.store.truncate_all().ok()?;
    engine
        .store
        .mark_initialized("github:tokio-rs/tokio")
        .ok()?;
    let mut bootstrap = config.clone();
    xrelease::config::strip_desired_sections(&mut bootstrap);
    let state = AppState::new_for_test(engine, bootstrap, config, watches, None);
    Some(router(state))
}

fn github_signature(secret: &str, body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac");
    mac.update(body);
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

/// Serializes the tests in this binary: they share one Postgres database, the
/// same configured source (`github:tokio-rs/tokio`) and the same release
/// fixture, and `build_app`'s `truncate_all()` wipes rows the other test just
/// wrote — observed as either 0 or duplicate Apprise deliveries. Same pattern
/// as `config_apply_e2e.rs`; the small pool above handles cross-binary
/// contention, this guard handles intra-binary interference.
static DB_GUARD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn github_webhook_should_deliver_new_release() {
    let _db = DB_GUARD.lock().await;
    let postgres_url = test_postgres_url();
    let apprise = MockServer::start().await;
    let Some(app) = build_app(&apprise, &postgres_url).await else {
        eprintln!("skipping webhook e2e (postgres unavailable)");
        return;
    };

    Mock::given(method("POST"))
        .and(path("/notify"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .expect(1)
        .mount(&apprise)
        .await;

    let body = include_str!("fixtures/github_release_published.json");
    let sig = github_signature("wh-secret", body.as_bytes());

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/webhooks/github")
        .header("content-type", "application/json")
        .header("X-GitHub-Delivery", "test-delivery-1")
        .header("X-Hub-Signature-256", sig)
        .body(Body::from(body))
        .expect("request");

    let response = app.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn github_webhook_should_deduplicate_delivery_id() {
    let _db = DB_GUARD.lock().await;
    let postgres_url = test_postgres_url();
    let apprise = MockServer::start().await;
    let Some(app) = build_app(&apprise, &postgres_url).await else {
        eprintln!("skipping webhook e2e (postgres unavailable)");
        return;
    };

    Mock::given(method("POST"))
        .and(path("/notify"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .expect(1)
        .mount(&apprise)
        .await;

    let body = include_str!("fixtures/github_release_published.json");
    let sig = github_signature("wh-secret", body.as_bytes());

    for delivery in ["dup-delivery", "dup-delivery"] {
        let request = Request::builder()
            .method("POST")
            .uri("/api/v1/webhooks/github")
            .header("content-type", "application/json")
            .header("X-GitHub-Delivery", delivery)
            .header("X-Hub-Signature-256", sig.clone())
            .body(Body::from(body))
            .expect("request");

        let response = app.clone().oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);
    }
}
