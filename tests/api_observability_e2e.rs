//! E2E tests for read-only observability API (require PostgreSQL).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use xrelease::api::{router, AppState};
use xrelease::config::Config;
use xrelease::engine::Engine;
use xrelease::notify::Event;
use xrelease::runtime::build_http_client;
use xrelease::store::{OutboxMeta, SeenUpsert};

fn test_postgres_url() -> Option<String> {
    Some(
        std::env::var("XRELEASE_TEST_POSTGRES_URL").unwrap_or_else(|_| {
            "postgres://xrelease:xrelease@127.0.0.1:5432/xrelease_test".to_owned()
        }),
    )
}

fn observability_config(postgres_url: &str) -> Config {
    toml::from_str(&format!(
        r#"
        [[notifiers]]
        type = "apprise"
        endpoint = "http://127.0.0.1:9"
        urls = ["mailto://test@example.com"]

        [database]
        postgres_url = "{postgres_url}"
        # Small pool: every e2e test opens its own Engine against the one shared
        # test database, so the default 16 × many parallel tests exhausts
        # Postgres's connection limit and a starved query 500s intermittently.
        max_connections = 4

        [api]
        api_key = "test-key"

        [[sources]]
        type = "github"
        repo = "tokio-rs/tokio"
        routing_tag = "platform-team"

        [[sources]]
        type = "pypi"
        name = "requests"

        [[teams]]
        tag = "platform-team"
        name = "Platform Team"
    "#
    ))
    .expect("parse config")
}

async fn build_app(postgres_url: &str) -> Option<axum::Router> {
    let config = observability_config(postgres_url);
    let http = build_http_client().ok()?;
    let watches = config.to_watches().ok()?;
    let engine = Engine::open(&config, http).ok()?;
    engine.store.truncate_all().ok()?;

    engine.store.touch_polled("github:tokio-rs/tokio").ok()?;
    engine
        .store
        .record_seen_batch(
            "pypi:requests",
            &[SeenUpsert {
                identity: "2.0.0",
                display_tag: None,
                content_digest: Some("dig"),
                published_at: None,
                url: None,
            }],
        )
        .ok()?;

    let event = Event {
        source_id: "github:tokio-rs/tokio".into(),
        source_kind: "GitHub".into(),
        title: "New release".into(),
        body: "body".into(),
        url: Some("https://example.com".into()),
        routing_tag: Some("platform-team".into()),
    };
    let outbox_id = engine
        .store
        .try_enqueue_notification(&event, "v1.0.0", OutboxMeta::default())
        .ok()??
        .id;
    engine
        .store
        .fail_notification(outbox_id, "apprise down")
        .ok()?;

    let mut bootstrap = config.clone();
    xrelease::config::strip_desired_sections(&mut bootstrap);
    let state = AppState::new_for_test(engine, bootstrap, config, watches, None);
    Some(router(state))
}

fn auth_request(method: &str, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("Authorization", "Bearer test-key")
        .body(Body::empty())
        .expect("request")
}

/// Serializes the tests in this binary: they all run `build_app`'s
/// `truncate_all()` against the one shared Postgres database, so a parallel
/// test could wipe the seeded rows (seen releases, dead outbox row) another
/// test is asserting on. Same pattern as `config_apply_e2e.rs`; the small
/// pool in the config above handles cross-binary contention, this guard
/// handles intra-binary interference.
static DB_GUARD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn status_should_require_api_key() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    let Some(app) = build_app(&url).await else {
        eprintln!("skipping observability e2e (postgres unavailable)");
        return;
    };

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn status_should_return_json_with_valid_key() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    let Some(app) = build_app(&url).await else {
        return;
    };

    let response = app
        .oneshot(auth_request("GET", "/api/v1/status"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn sources_should_list_configured_watches() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    let Some(app) = build_app(&url).await else {
        return;
    };

    let response = app
        .oneshot(auth_request("GET", "/api/v1/sources"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let sources = json.as_array().expect("array");
    assert!(sources.len() >= 2);
}

#[tokio::test]
async fn source_detail_should_include_seen_releases() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    let Some(app) = build_app(&url).await else {
        return;
    };

    let response = app
        .oneshot(auth_request("GET", "/api/v1/sources/pypi%3Arequests"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["seen_count"], 1);
    let releases = json["seen_releases"].as_array().expect("releases");
    assert_eq!(releases.len(), 1);
    assert_eq!(releases[0]["tag"], "2.0.0");
}

#[tokio::test]
async fn outbox_should_list_failed_entries() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    let Some(app) = build_app(&url).await else {
        return;
    };

    let response = app
        .oneshot(auth_request("GET", "/api/v1/outbox?limit=10"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let entries = json["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["status"], "failed");
    assert_eq!(entries[0]["identity"], "v1.0.0");
    assert_eq!(entries[0]["last_error"], "apprise down");
}

#[tokio::test]
async fn outbox_requeue_should_revive_dead_rows() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    // Seed via build_app (truncate + one failed outbox row), then reopen store.
    let Some(_app) = build_app(&url).await else {
        eprintln!("skipping observability e2e (postgres unavailable)");
        return;
    };

    let config = observability_config(&url);
    let http = build_http_client().expect("http");
    let engine = Engine::open(&config, http).expect("engine");
    let entries = engine.store.list_outbox_entries(10).expect("list");
    assert_eq!(entries.len(), 1);
    let id = entries[0].id;
    for _ in 0..xrelease::store::OUTBOX_MAX_ATTEMPTS {
        engine
            .store
            .fail_notification(id, "exhausted")
            .expect("fail");
    }
    assert_eq!(engine.store.outbox_counts().expect("counts").dead, 1);

    let watches = config.to_watches().expect("watches");
    let mut bootstrap = config.clone();
    xrelease::config::strip_desired_sections(&mut bootstrap);
    let state = AppState::new_for_test(engine, bootstrap, config, watches, None);
    let app = router(state);

    let response = app
        .oneshot(auth_request("POST", "/api/v1/outbox/requeue"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["requeued"], 1);
}

#[tokio::test]
async fn teams_should_return_catalogue_with_source_counts() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    let Some(app) = build_app(&url).await else {
        return;
    };

    let response = app
        .oneshot(auth_request("GET", "/api/v1/teams"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let teams = json["teams"].as_array().expect("teams");
    assert_eq!(teams.len(), 1);
    assert_eq!(teams[0]["tag"], "platform-team");
    assert_eq!(teams[0]["name"], "Platform Team");
    assert_eq!(teams[0]["source_count"], 1);
}

#[tokio::test]
async fn teams_should_require_api_key() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    let Some(app) = build_app(&url).await else {
        return;
    };

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/teams")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn observability_reads_should_not_count_toward_rate_limit() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    let mut config = observability_config(&url);
    config.api.rate_limit_per_minute = 1;
    let http = build_http_client().expect("http");
    let watches = config.to_watches().expect("watches");
    let engine = match Engine::open(&config, http) {
        Ok(engine) => engine,
        Err(_) => return,
    };
    let _ = engine.store.truncate_all();
    let mut bootstrap = config.clone();
    xrelease::config::strip_desired_sections(&mut bootstrap);
    let state = AppState::new_for_test(engine, bootstrap, config, watches, None);
    let app = router(state);

    for _ in 0..5 {
        let response = app
            .clone()
            .oneshot(auth_request("GET", "/api/v1/outbox?limit=10"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
    }
}
