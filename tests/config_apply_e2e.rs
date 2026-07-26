//! E2E tests for push-applied config. Require PostgreSQL.

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use xrelease::api::{router, serve, AppState};
use xrelease::config::Config;
use xrelease::engine::Engine;
use xrelease::runtime::build_http_client;

fn test_postgres_url() -> Option<String> {
    Some(
        std::env::var("XRELEASE_TEST_POSTGRES_URL").unwrap_or_else(|_| {
            "postgres://xrelease:xrelease@127.0.0.1:5432/xrelease_test".to_owned()
        }),
    )
}

fn test_configs(postgres_url: &str) -> (Config, Config) {
    let bootstrap: Config = toml::from_str(&format!(
        r#"
        [config_api]
        api_config = true
        source = "api"

        [database]
        postgres_url = "{postgres_url}"

        [api]
        api_key = "test-key"
    "#
    ))
    .expect("parse bootstrap config");

    let effective: Config = toml::from_str(&format!(
        r#"
        [config_api]
        api_config = true
        source = "api"

        [database]
        postgres_url = "{postgres_url}"

        [api]
        api_key = "test-key"

        [[notifiers]]
        type = "apprise"
        endpoint = "http://127.0.0.1:9"
        urls = ["mailto://test@example.com"]

        [[sources]]
        type = "github"
        repo = "org/initial"
    "#
    ))
    .expect("parse effective config");

    (bootstrap, effective)
}

const DESIRED_V2: &str = r#"
[[notifiers]]
type = "apprise"
endpoint = "http://127.0.0.1:9"
urls = ["mailto://test@example.com"]
[[sources]]
type = "github"
repo = "org/updated"
"#;

async fn build_app(postgres_url: &str) -> Option<axum::Router> {
    let (bootstrap, effective) = test_configs(postgres_url);
    let http = build_http_client().ok()?;
    let watches = effective.to_watches().ok()?;
    let engine = Engine::open(&effective, http).ok()?;
    engine.store.truncate_all().ok()?;

    let state = AppState::new_for_test(engine, bootstrap, effective, watches, None);
    Some(router(state))
}

fn auth_request(method: &str, uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("Authorization", "Bearer test-key")
        .header("Content-Type", "application/toml")
        .body(Body::from(body.to_owned()))
        .expect("request")
}

/// Every test in this file truncates and rewrites the **same** PostgreSQL
/// tables, so they cannot safely overlap: cargo runs the tests of one binary
/// on parallel threads by default, and one test's `truncate_all()` was wiping
/// rows another had just written (observed as ~1 spurious failure in 4 runs).
/// Serializing here fixes it without depending on `--test-threads=1`, which
/// CI does not pass.
static DB_GUARD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn apply_should_hot_swap_sources_and_record_ledger() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    let Some(app) = build_app(&url).await else {
        eprintln!("skipping config apply e2e (postgres unavailable)");
        return;
    };

    let response = app
        .clone()
        .oneshot(auth_request("POST", "/api/v1/config/apply", DESIRED_V2))
        .await
        .expect("apply");
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["applied"], true);
    assert!(json["revision"].as_i64().is_some());

    let sources = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/sources")
                .header("Authorization", "Bearer test-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("sources");
    assert_eq!(sources.status(), StatusCode::OK);
    let sources_body = to_bytes(sources.into_body(), usize::MAX)
        .await
        .expect("sources body");
    let sources_json: Vec<serde_json::Value> =
        serde_json::from_slice(&sources_body).expect("sources json");
    assert_eq!(sources_json.len(), 1);
    assert_eq!(sources_json[0]["id"], "github:org/updated");

    let status = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/status")
                .header("Authorization", "Bearer test-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("status");
    let status_body = to_bytes(status.into_body(), usize::MAX)
        .await
        .expect("status body");
    let status_json: serde_json::Value = serde_json::from_slice(&status_body).expect("status json");
    assert_eq!(status_json["config_apply"]["desired_source"], "ledger");
    assert!(status_json["config_apply"]["revision"].is_number());
}

#[tokio::test]
async fn apply_same_sha_should_be_idempotent() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    let Some(app) = build_app(&url).await else {
        eprintln!("skipping config apply e2e (postgres unavailable)");
        return;
    };

    let first = app
        .clone()
        .oneshot(auth_request("POST", "/api/v1/config/apply", DESIRED_V2))
        .await
        .expect("first apply");
    assert_eq!(first.status(), StatusCode::OK);

    let second = app
        .oneshot(auth_request("POST", "/api/v1/config/apply", DESIRED_V2))
        .await
        .expect("second apply");
    assert_eq!(second.status(), StatusCode::OK);
    let body = to_bytes(second.into_body(), usize::MAX)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["applied"], false);
}

/// A body that never parses into a config (unknown source kind) is a **client**
/// error — 400, not the 500 a bare `anyhow` used to produce.
#[tokio::test]
async fn validate_should_reject_an_unparseable_document_as_bad_request() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    let Some(app) = build_app(&url).await else {
        eprintln!("skipping config apply e2e (postgres unavailable)");
        return;
    };

    let unparseable = r#"
[[sources]]
type = "not-a-real-kind"
repo = "x/y"
"#;

    let response = app
        .oneshot(auth_request("POST", "/api/v1/config/validate", unparseable))
        .await
        .expect("validate");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// A document that *does* parse but is semantically wrong comes back 200 with
/// `valid: false` and a report — that is the dry-run's whole purpose.
#[tokio::test]
async fn validate_should_report_a_parseable_but_invalid_document() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    let Some(app) = build_app(&url).await else {
        eprintln!("skipping config apply e2e (postgres unavailable)");
        return;
    };

    // Parses fine; fails validation because a source routes to a team tag no
    // configured notifier matches.
    let invalid = r#"
[[notifiers]]
type = "apprise"
endpoint = "http://127.0.0.1:9"
urls = ["mailto://test@example.com"]
tags = ["platform-team"]
[[sources]]
type = "github"
repo = "org/app"
routing_tag = "nobody-listens-to-this"
"#;

    let response = app
        .oneshot(auth_request("POST", "/api/v1/config/validate", invalid))
        .await
        .expect("validate");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        json["valid"], false,
        "expected a validation failure: {json}"
    );
    assert!(
        json["report"]["errors"]
            .as_array()
            .is_some_and(|errors| !errors.is_empty()),
        "expected a non-empty error report: {json}"
    );
}

/// UI drops redacted Apprise `urls` on edit. Validate must restore them from
/// the live runtime (same as Apply), otherwise a tagged Express channel makes
/// Apprise look unconfigured and platform-team sources falsely orphan.
#[tokio::test]
async fn validate_should_restore_omitted_apprise_urls_for_routing() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    let Some(app) = build_app(&url).await else {
        eprintln!("skipping config apply e2e (postgres unavailable)");
        return;
    };

    // Live effective config already has Apprise urls (wildcard). Candidate
    // omits urls the way the UI write path does after GET redaction.
    let candidate = r#"
[[notifiers]]
type = "apprise"
endpoint = "http://127.0.0.1:9"
[[notifiers]]
type = "express"
base_url = "https://cts.example.com"
group_chat_id = "g-1"
access_token = "tok"
tags = ["security-team"]

[[teams]]
tag = "platform-team"
name = "Platform"

[[teams]]
tag = "security-team"
name = "Security"

[[sources]]
type = "github"
repo = "dotnet/runtime"
id = "dotnet-runtime"
routing_tag = "platform-team"

[[sources]]
type = "github"
repo = "org/sec"
routing_tag = "security-team"
"#;

    let response = app
        .oneshot(auth_request("POST", "/api/v1/config/validate", candidate))
        .await
        .expect("validate");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        json["valid"], true,
        "expected validate to restore Apprise urls and accept routing: {json}"
    );
}

#[tokio::test]
async fn apply_yaml_body_should_hot_swap_sources() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    let Some(app) = build_app(&url).await else {
        eprintln!("skipping config apply e2e (postgres unavailable)");
        return;
    };

    const YAML: &str = r#"
sources:
  - type: github
    repo: org/yaml-applied
"#;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/config/apply")
                .header("Authorization", "Bearer test-key")
                .header("Content-Type", "application/yaml")
                .body(Body::from(YAML.to_owned()))
                .unwrap(),
        )
        .await
        .expect("apply yaml");
    assert_eq!(response.status(), StatusCode::OK);

    let sources = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/sources")
                .header("Authorization", "Bearer test-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("sources");
    let sources_body = to_bytes(sources.into_body(), usize::MAX)
        .await
        .expect("body");
    let sources_json: Vec<serde_json::Value> = serde_json::from_slice(&sources_body).expect("json");
    assert_eq!(sources_json[0]["id"], "github:org/yaml-applied");
}

#[tokio::test]
async fn apply_disabled_when_config_api_off() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    let (mut bootstrap, mut effective) = test_configs(&url);
    bootstrap.config_api.api_config = false;
    effective.config_api.api_config = false;
    let Some(http) = build_http_client().ok() else {
        return;
    };
    let watches = match effective.to_watches() {
        Ok(w) => w,
        Err(_) => return,
    };
    let Some(engine) = Engine::open(&effective, http).ok() else {
        eprintln!("skipping config apply e2e (postgres unavailable)");
        return;
    };
    if engine.store.truncate_all().is_err() {
        eprintln!("skipping config apply e2e (postgres unavailable)");
        return;
    }
    let state = AppState::new_for_test(engine, bootstrap, effective, watches, None);
    let app = router(state);

    let response = app
        .oneshot(auth_request("POST", "/api/v1/config/apply", DESIRED_V2))
        .await
        .expect("apply");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// File-mode reload: no ledger revision exists, so the app file is authoritative
/// and `POST /api/v1/reload` re-reads it and hot-swaps the watch set.
#[tokio::test]
async fn reload_should_hot_swap_from_app_file() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    let (bootstrap, effective) = test_configs(&url);
    let Some(http) = build_http_client().ok() else {
        return;
    };
    let watches = match effective.to_watches() {
        Ok(w) => w,
        Err(_) => return,
    };
    let Some(engine) = Engine::open(&effective, http).ok() else {
        eprintln!("skipping reload e2e (postgres unavailable)");
        return;
    };
    if engine.store.truncate_all().is_err() {
        eprintln!("skipping reload e2e (postgres unavailable)");
        return;
    }

    let dir = std::env::temp_dir().join(format!("xrelease-reload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let app_path = dir.join("releases.yaml");
    std::fs::write(
        &app_path,
        r#"
sources:
  - type: github
    repo: org/from-file
"#,
    )
    .expect("write app file");

    let state = AppState::new_for_test_with_app(
        engine,
        bootstrap,
        effective,
        watches,
        None,
        Some(app_path.clone()),
    );
    let app = router(state);

    // Boot seeds the reload baseline from the file itself, so an untouched
    // file is correctly a no-op — the real scenario is "operator edits the
    // file, then asks for a reload".
    let response = app
        .clone()
        .oneshot(auth_request("POST", "/api/v1/reload", ""))
        .await
        .expect("reload");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        json["applied"],
        serde_json::Value::Bool(false),
        "unchanged file must not churn poll loops: {json}"
    );

    std::fs::write(
        &app_path,
        r#"
sources:
  - type: github
    repo: org/edited-on-disk
"#,
    )
    .expect("edit app file");

    let response = app
        .oneshot(auth_request("POST", "/api/v1/reload", ""))
        .await
        .expect("reload after edit");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["applied"], serde_json::Value::Bool(true));
    assert!(
        json["sources_added"]
            .as_array()
            .is_some_and(|added| added.iter().any(|v| v == "github:org/edited-on-disk")),
        "expected the edited file's source to be added: {json}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Once a revision is applied, the ledger — not the file — is what a restart
/// boots from, so reload must refuse instead of silently diverging the two.
#[tokio::test]
async fn reload_should_conflict_when_ledger_is_authoritative() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    let Some(app) = build_app(&url).await else {
        eprintln!("skipping reload e2e (postgres unavailable)");
        return;
    };

    let response = app
        .clone()
        .oneshot(auth_request("POST", "/api/v1/config/apply", DESIRED_V2))
        .await
        .expect("apply");
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .oneshot(auth_request("POST", "/api/v1/reload", ""))
        .await
        .expect("reload");
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

/// Regression: `config_api.api_config = true` with neither bearer auth
/// (api_key/OIDC) nor an HMAC secret configured used to leave
/// `/config/apply` and `/config/rollback` completely unauthenticated. `serve`
/// must now refuse to start instead of exposing an unauthenticated way to
/// hot-swap sources and notifiers.
#[tokio::test]
async fn serve_should_refuse_to_start_when_config_api_has_no_auth() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    std::env::remove_var("XRELEASE_CONFIG_APPLY_SECRET");

    let (mut bootstrap, mut effective) = test_configs(&url);
    bootstrap.config_api.api_config = true;
    effective.config_api.api_config = true;
    bootstrap.api.api_key = None;
    effective.api.api_key = None;

    let Some(http) = build_http_client().ok() else {
        return;
    };
    let watches = match effective.to_watches() {
        Ok(w) => w,
        Err(_) => return,
    };
    let Some(engine) = Engine::open(&effective, http).ok() else {
        eprintln!("skipping config apply e2e (postgres unavailable)");
        return;
    };
    if engine.store.truncate_all().is_err() {
        eprintln!("skipping config apply e2e (postgres unavailable)");
        return;
    }
    let state = AppState::new_for_test(engine, bootstrap, effective, watches, None);

    // Port 0: the fail-closed check runs before any bind/listen, so this
    // returns fast on the `Err` path and never actually needs a real socket.
    let result = serve("127.0.0.1:0", state).await;
    assert!(
        result.is_err(),
        "serve() must refuse to start with config_api enabled and zero auth"
    );
}

/// Regression: booting from the app file (no ledger revision) must seed
/// `effective_desired_sha` from the file's own content. Without this, the
/// first `POST /api/v1/reload` after startup always compared against `None`,
/// treated an untouched file as "changed", and hot-swapped for nothing
/// (rebuilt the notifier, restarted every poll loop).
#[tokio::test]
async fn reload_should_be_a_noop_on_first_call_after_booting_from_app_file() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };

    let dir = std::env::temp_dir().join(format!("xrelease-reload-noop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let bootstrap_path = dir.join("bootstrap.toml");
    std::fs::write(
        &bootstrap_path,
        format!(
            r#"
            [database]
            postgres_url = "{url}"
            [api]
            api_key = "test-key"
        "#
        ),
    )
    .expect("write bootstrap");
    let app_path = dir.join("releases.yaml");
    std::fs::write(
        &app_path,
        r#"
notifiers:
  - type: apprise
    endpoint: http://127.0.0.1:9
    urls: ["mailto://test@example.com"]
  - type: apprise
    endpoint: http://127.0.0.1:9
    urls: ["mailto://test@example.com"]
  - type: apprise
    endpoint: http://127.0.0.1:9
    urls: ["mailto://test@example.com"]
  - type: apprise
    endpoint: http://127.0.0.1:9
    urls: ["mailto://test@example.com"]
sources:
  - type: github
    repo: org/unchanged
"#,
    )
    .expect("write app file");

    // Resolve exactly the way a real boot does (`Runtime::new`), so
    // `effective` reflects the same bytes reload will re-read.
    let paths = xrelease::config::ConfigPaths::new(bootstrap_path.clone(), Some(app_path.clone()));
    let bootstrap = xrelease::config::load_infra_bootstrap(&paths).expect("load bootstrap");
    let effective = xrelease::config::resolve(&paths, None).expect("resolve");

    let http = build_http_client().expect("http client");
    let watches = effective.to_watches().expect("watches");
    let Some(engine) = Engine::open(&effective, http).ok() else {
        eprintln!("skipping reload-noop e2e (postgres unavailable)");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    };
    if engine.store.truncate_all().is_err() {
        eprintln!("skipping reload-noop e2e (postgres unavailable)");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }

    let state = AppState::new_for_test_with_app(
        engine,
        bootstrap,
        effective,
        watches,
        None,
        Some(app_path.clone()),
    );
    let app = router(state);

    let response = app
        .oneshot(auth_request("POST", "/api/v1/reload", ""))
        .await
        .expect("reload");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        json["applied"],
        serde_json::Value::Bool(false),
        "reload of an unchanged file right after boot must be a no-op: {json}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The ledger history endpoint must page newest-first, expose verified
/// provenance, and — critically — **never** return the raw revision bodies,
/// which would bypass the secret redaction `GET /api/v1/config` applies.
#[tokio::test]
async fn revisions_history_should_list_metadata_without_content() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    let Some(app) = build_app(&url).await else {
        eprintln!("skipping revisions e2e (postgres unavailable)");
        return;
    };

    // One accepted apply and one rejected attempt — both must be recorded.
    let ok = app
        .clone()
        .oneshot(auth_request("POST", "/api/v1/config/apply", DESIRED_V2))
        .await
        .expect("apply");
    assert_eq!(ok.status(), StatusCode::OK);

    let bad = app
        .clone()
        .oneshot(auth_request(
            "POST",
            "/api/v1/config/apply",
            "[[sources]]\ntype = \"not-a-real-kind\"\nrepo = \"x/y\"\n",
        ))
        .await
        .expect("rejected apply");
    assert!(bad.status().is_client_error());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/config/revisions?limit=10")
                .header("Authorization", "Bearer test-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("revisions");
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let revisions = json["revisions"].as_array().expect("revisions array");
    assert!(
        revisions.len() >= 2,
        "expected the applied and the rejected attempt: {json}"
    );

    // Newest first.
    assert_eq!(revisions[0]["status"], "rejected");
    assert!(revisions[0]["error"].is_string());

    for revision in revisions {
        assert!(
            revision.get("content").is_none(),
            "history must not carry raw config bodies (redaction bypass): {revision}"
        );
        assert!(revision["content_sha256"].is_string());
        // Provenance is server-derived from the credential, not the header.
        assert_eq!(
            revision["applied_by"], "api_key",
            "applied_by must be the verified principal: {revision}"
        );
    }
}

/// A client-supplied `X-Config-Applied-By` must annotate, never replace, the
/// verified identity — otherwise the audit trail is trivially forgeable.
#[tokio::test]
async fn applied_by_header_should_not_override_verified_principal() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    let Some(app) = build_app(&url).await else {
        eprintln!("skipping applied_by e2e (postgres unavailable)");
        return;
    };

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/config/apply")
                .header("Authorization", "Bearer test-key")
                .header("Content-Type", "application/toml")
                .header("X-Config-Applied-By", "totally-not-an-admin")
                .body(Body::from(DESIRED_V2.to_owned()))
                .unwrap(),
        )
        .await
        .expect("apply");
    assert_eq!(response.status(), StatusCode::OK);

    let history = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/config/revisions?limit=1")
                .header("Authorization", "Bearer test-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("revisions");
    let body = to_bytes(history.into_body(), usize::MAX)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let applied_by = json["revisions"][0]["applied_by"]
        .as_str()
        .expect("applied_by");
    assert!(
        applied_by.starts_with("api_key"),
        "verified principal must lead: {applied_by}"
    );
    assert!(
        applied_by.contains("totally-not-an-admin"),
        "client claim should be kept as an annotation: {applied_by}"
    );
}

/// Optimistic concurrency: read the config's ETag, apply with `If-Match`, and
/// a second author who edited from the same base must be rejected with 412
/// instead of silently clobbering the first.
#[tokio::test]
async fn apply_with_stale_if_match_should_fail_precondition() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    let Some(app) = build_app(&url).await else {
        eprintln!("skipping if-match e2e (postgres unavailable)");
        return;
    };

    // Both "authors" read the same starting state.
    let read = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/config")
                .header("Authorization", "Bearer test-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("read config");
    assert_eq!(read.status(), StatusCode::OK);
    let base_etag = read
        .headers()
        .get(axum::http::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    // First author applies against that base and wins.
    let mut first = Request::builder()
        .method("POST")
        .uri("/api/v1/config/apply")
        .header("Authorization", "Bearer test-key")
        .header("Content-Type", "application/toml");
    if let Some(etag) = base_etag.as_deref() {
        first = first.header(axum::http::header::IF_MATCH, etag);
    }
    let response = app
        .clone()
        .oneshot(first.body(Body::from(DESIRED_V2.to_owned())).unwrap())
        .await
        .expect("first apply");
    assert_eq!(response.status(), StatusCode::OK, "first author should win");

    // Second author still holds the *old* ETag — must be refused.
    let stale = base_etag.unwrap_or_else(|| {
        "\"0000000000000000000000000000000000000000000000000000000000000000\"".to_owned()
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/config/apply")
                .header("Authorization", "Bearer test-key")
                .header("Content-Type", "application/toml")
                .header(axum::http::header::IF_MATCH, stale)
                .body(Body::from(
                    "[[notifiers]]\ntype = \"apprise\"\nendpoint = \"http://127.0.0.1:9\"\nurls = [\"mailto://test@example.com\"]\n\n[[sources]]\ntype = \"github\"\nrepo = \"org/second-author\"\n",
                ))
                .unwrap(),
        )
        .await
        .expect("second apply");
    assert_eq!(
        response.status(),
        StatusCode::PRECONDITION_FAILED,
        "stale If-Match must not clobber the first author"
    );

    // Re-reading gives the new ETag, and re-applying then succeeds.
    let read = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/config")
                .header("Authorization", "Bearer test-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("re-read");
    let fresh = read
        .headers()
        .get(axum::http::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .expect("etag after apply")
        .to_owned();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/config/apply")
                .header("Authorization", "Bearer test-key")
                .header("Content-Type", "application/toml")
                .header(axum::http::header::IF_MATCH, fresh)
                .body(Body::from(
                    "[[notifiers]]\ntype = \"apprise\"\nendpoint = \"http://127.0.0.1:9\"\nurls = [\"mailto://test@example.com\"]\n\n[[sources]]\ntype = \"github\"\nrepo = \"org/second-author\"\n",
                ))
                .unwrap(),
        )
        .await
        .expect("retry apply");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "retry after re-read wins"
    );
}

/// No `If-Match` at all stays unconditional — a CI pipeline pushing the
/// committed state has not necessarily read the running config first.
#[tokio::test]
async fn apply_without_if_match_should_stay_unconditional() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    let Some(app) = build_app(&url).await else {
        eprintln!("skipping if-match e2e (postgres unavailable)");
        return;
    };

    let response = app
        .oneshot(auth_request("POST", "/api/v1/config/apply", DESIRED_V2))
        .await
        .expect("apply");
    assert_eq!(response.status(), StatusCode::OK);
}

/// `config_api.source = "local"` declares the app file authoritative, so a push
/// must be refused rather than hot-swapped into config the next boot discards.
#[tokio::test]
async fn apply_should_be_refused_when_file_is_declared_authoritative() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    let (mut bootstrap, mut effective) = test_configs(&url);
    bootstrap.config_api.source = xrelease::config::ConfigSource::Local;
    effective.config_api.source = xrelease::config::ConfigSource::Local;

    let Some(http) = build_http_client().ok() else {
        return;
    };
    let watches = match effective.to_watches() {
        Ok(w) => w,
        Err(_) => return,
    };
    let Some(engine) = Engine::open(&effective, http).ok() else {
        eprintln!("skipping file-authority e2e (postgres unavailable)");
        return;
    };
    if engine.store.truncate_all().is_err() {
        eprintln!("skipping file-authority e2e (postgres unavailable)");
        return;
    }
    let state = AppState::new_for_test(engine, bootstrap, effective, watches, None);
    let app = router(state);

    let response = app
        .oneshot(auth_request("POST", "/api/v1/config/apply", DESIRED_V2))
        .await
        .expect("apply");
    assert_eq!(
        response.status(),
        StatusCode::CONFLICT,
        "file-authoritative instances must refuse pushed config"
    );
}

/// Rolling back when the ledger holds no earlier revision is a statement about
/// current state, not a server malfunction: it must be 409, not the 500 a bare
/// `anyhow` produced (which told operators their instance was broken when it
/// was merely on its first revision).
#[tokio::test]
async fn rollback_without_a_previous_revision_should_conflict() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };
    let Some(app) = build_app(&url).await else {
        eprintln!("skipping rollback e2e (postgres unavailable)");
        return;
    };

    // Exactly one applied revision exists, so there is nothing behind it.
    let response = app
        .clone()
        .oneshot(auth_request("POST", "/api/v1/config/apply", DESIRED_V2))
        .await
        .expect("apply");
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .oneshot(auth_request("POST", "/api/v1/config/rollback", ""))
        .await
        .expect("rollback");
    assert_eq!(response.status(), StatusCode::CONFLICT);
}
