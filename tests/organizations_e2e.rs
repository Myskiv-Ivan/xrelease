//! Multi-organization end-to-end tests.
//!
//! Real bootstrap + per-org files in a temp dir, real PostgreSQL ledger
//! streams, the real router. Skips silently when the test database is
//! unreachable (same convention as the other e2e suites).

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use xrelease::api::{router, AppState};
use xrelease::config::{self, ConfigPaths};
use xrelease::engine::Engine;
use xrelease::runtime::build_http_client;

type HmacSha256 = Hmac<Sha256>;

fn test_postgres_url() -> String {
    std::env::var("XRELEASE_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://xrelease:xrelease@127.0.0.1:5432/xrelease_test".to_owned())
}

/// Every test rewrites the same PostgreSQL tables; serialize them (see
/// `config_apply_e2e.rs` for the flake this prevents).
static DB_GUARD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// One throwaway multi-org deployment on disk.
struct OrgFixture {
    base: PathBuf,
    paths: ConfigPaths,
}

impl OrgFixture {
    /// `source`: `[config_api].source` (`"api"` or `"local"`).
    ///
    /// Apprise points at a dead port: config-flow tests never deliver, and a
    /// hanging real endpoint would only slow them down. The webhook fan-out
    /// test overrides it with a live wiremock via [`Self::create_with_apprise`].
    fn create(tag: &str, postgres_url: &str, source: &str) -> Self {
        Self::create_with_apprise(tag, postgres_url, source, "http://127.0.0.1:9")
    }

    fn create_with_apprise(
        tag: &str,
        postgres_url: &str,
        source: &str,
        apprise_endpoint: &str,
    ) -> Self {
        let base =
            std::env::temp_dir().join(format!("xrelease-org-e2e-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("app/platform")).expect("platform dir");
        std::fs::create_dir_all(base.join("app/security")).expect("security dir");

        std::fs::write(
            base.join("bootstrap.toml"),
            format!(
                r#"
 [config_api]
 api_config = true
 source = "{source}"

 [database]
 postgres_url = "{postgres_url}"
 # Small pool: shared test DB, many parallel e2e Engines.
 max_connections = 4

 [api]
 api_key = "test-key"
 webhook_secret = "wh-secret"

 [[organizations]]
 id = "platform"
 name = "Platform Engineering"
 app = "app/platform/releases.yaml"

 [[organizations]]
 id = "security"
 app = "app/security/releases.yaml"
 "#
            ),
        )
        .expect("bootstrap");

        // Both orgs deliberately watch the SAME upstream repo: webhook events
        // must fan out to each org's own stream (isolation is between their
        // notifications, not their upstreams).
        std::fs::write(
            base.join("app/platform/releases.yaml"),
            format!(
                r#"
notifiers:
  - type: apprise
    endpoint: {apprise_endpoint}
    urls: ["mailto://platform@example.com"]
teams:
  - tag: core
sources:
  - type: github
    repo: shared/upstream
    routing_tag: core
"#
            ),
        )
        .expect("platform app");

        std::fs::write(
            base.join("app/security/releases.yaml"),
            format!(
                r#"
notifiers:
  - type: apprise
    endpoint: {apprise_endpoint}
    urls: ["mailto://security@example.com"]
sources:
  - type: github
    repo: shared/upstream
"#
            ),
        )
        .expect("security app");

        let paths = ConfigPaths::new(base.join("bootstrap.toml"), None);
        Self { base, paths }
    }
}

impl Drop for OrgFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

async fn build_app(fixture: &OrgFixture) -> Option<axum::Router> {
    let bootstrap = config::load_infra_bootstrap(&fixture.paths).ok()?;

    // Resolve through a short-lived store (same boot order as `Runtime::new`),
    // then open the Engine on the RESOLVED config so its notifier carries the
    // orgs' sinks — an Engine opened on the bootstrap would deliver nothing.
    let store = xrelease::store::Store::open_from_config(&bootstrap.database).ok()?;
    store.truncate_all().ok()?;
    let effective = config::resolve(&fixture.paths, Some(&store)).ok()?;
    drop(store);

    let http = build_http_client().ok()?;
    let engine = Engine::open(&effective, http).ok()?;
    let watches = effective.to_watches().ok()?;
    let state = AppState::new_for_test_with_paths(
        engine,
        bootstrap,
        effective,
        watches,
        None,
        fixture.paths.clone(),
    );
    Some(router(state))
}

fn auth_request(method: &str, uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("Authorization", "Bearer test-key")
        .header("Content-Type", "application/yaml")
        .body(Body::from(body.to_owned()))
        .expect("request")
}

async fn json_body(response: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json body")
}

const PLATFORM_V2: &str = r#"
notifiers:
  - type: apprise
    endpoint: http://127.0.0.1:9
    urls: ["mailto://platform@example.com"]
sources:
  - type: github
    repo: shared/upstream
  - type: github
    repo: org/platform-extra
"#;

const PLATFORM_V3: &str = r#"
notifiers:
  - type: apprise
    endpoint: http://127.0.0.1:9
    urls: ["mailto://platform@example.com"]
sources:
  - type: github
    repo: org/platform-only
"#;

#[tokio::test]
async fn org_apply_should_hot_swap_one_stream_and_guard_the_legacy_route() {
    let _db = DB_GUARD.lock().await;
    let fixture = OrgFixture::create("apply", &test_postgres_url(), "api");
    let Some(app) = build_app(&fixture).await else {
        eprintln!("skipping organizations e2e (postgres unavailable)");
        return;
    };

    // Catalogue lists both orgs with live source counts.
    let response = app
        .clone()
        .oneshot(auth_request("GET", "/api/v1/organizations", ""))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let catalogue = json_body(response).await;
    assert_eq!(catalogue["organizations"].as_array().map(Vec::len), Some(2));

    // Apply a new document to `platform` only.
    let response = app
        .clone()
        .oneshot(auth_request(
            "POST",
            "/api/v1/organizations/platform/config/apply",
            PLATFORM_V2,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let applied = json_body(response).await;
    assert_eq!(applied["applied"], serde_json::Value::Bool(true));
    let added: Vec<&str> = applied["sources_added"]
        .as_array()
        .map(|items| items.iter().filter_map(serde_json::Value::as_str).collect())
        .unwrap_or_default();
    assert!(
        added.contains(&"platform::github:org/platform-extra"),
        "unexpected sources_added: {added:?}"
    );

    // The org's authority is now its ledger stream…
    let response = app
        .clone()
        .oneshot(auth_request(
            "GET",
            "/api/v1/organizations/platform/config",
            "",
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().contains_key("etag"));
    let shown = json_body(response).await;
    assert_eq!(shown["desired_source"], "ledger");

    // …while `security` still boots from its file.
    let response = app
        .clone()
        .oneshot(auth_request(
            "GET",
            "/api/v1/organizations/security/config",
            "",
        ))
        .await
        .expect("response");
    let untouched = json_body(response).await;
    assert_eq!(untouched["desired_source"], "app_file");

    // Per-org history carries the stream tag.
    let response = app
        .clone()
        .oneshot(auth_request(
            "GET",
            "/api/v1/organizations/platform/config/revisions",
            "",
        ))
        .await
        .expect("response");
    let history = json_body(response).await;
    assert_eq!(history["total"], serde_json::Value::from(1));
    assert_eq!(history["revisions"][0]["organization_id"], "platform");

    // Whole-document mutation routes are refused on a multi-org instance.
    let response = app
        .clone()
        .oneshot(auth_request("POST", "/api/v1/config/apply", PLATFORM_V2))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::CONFLICT);

    // Unknown organizations are a 404, not a silent no-op stream.
    let response = app
        .clone()
        .oneshot(auth_request(
            "POST",
            "/api/v1/organizations/nope/config/apply",
            PLATFORM_V2,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn org_rollback_should_toggle_only_its_own_stream() {
    let _db = DB_GUARD.lock().await;
    let fixture = OrgFixture::create("rollback", &test_postgres_url(), "api");
    let Some(app) = build_app(&fixture).await else {
        eprintln!("skipping organizations e2e (postgres unavailable)");
        return;
    };

    // Only one applied revision in the stream → nothing earlier → 409.
    let response = app
        .clone()
        .oneshot(auth_request(
            "POST",
            "/api/v1/organizations/platform/config/apply",
            PLATFORM_V2,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let v2 = json_body(response).await;

    let response = app
        .clone()
        .oneshot(auth_request(
            "POST",
            "/api/v1/organizations/platform/config/rollback",
            "",
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::CONFLICT);

    // Second revision → rollback returns to the first.
    let response = app
        .clone()
        .oneshot(auth_request(
            "POST",
            "/api/v1/organizations/platform/config/apply",
            PLATFORM_V3,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(auth_request(
            "POST",
            "/api/v1/organizations/platform/config/rollback",
            "",
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let rolled = json_body(response).await;
    assert_eq!(rolled["content_sha256"], v2["content_sha256"]);
}

#[tokio::test]
async fn local_source_should_refuse_org_pushes_but_reload_from_files() {
    let _db = DB_GUARD.lock().await;
    let fixture = OrgFixture::create("local", &test_postgres_url(), "local");
    let Some(app) = build_app(&fixture).await else {
        eprintln!("skipping organizations e2e (postgres unavailable)");
        return;
    };

    // Git-pinned: pushes are 409.
    let response = app
        .clone()
        .oneshot(auth_request(
            "POST",
            "/api/v1/organizations/platform/config/apply",
            PLATFORM_V2,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::CONFLICT);

    // Unchanged files → reload is an idempotent no-op.
    let response = app
        .clone()
        .oneshot(auth_request("POST", "/api/v1/reload", ""))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let unchanged = json_body(response).await;
    assert_eq!(unchanged["applied"], serde_json::Value::Bool(false));

    // Edit one org's file on disk → reload converges the composition.
    std::fs::write(fixture.base.join("app/platform/releases.yaml"), PLATFORM_V3)
        .expect("rewrite platform app");
    let response = app
        .clone()
        .oneshot(auth_request("POST", "/api/v1/reload", ""))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let reloaded = json_body(response).await;
    assert_eq!(reloaded["applied"], serde_json::Value::Bool(true));
    let added: Vec<&str> = reloaded["sources_added"]
        .as_array()
        .map(|items| items.iter().filter_map(serde_json::Value::as_str).collect())
        .unwrap_or_default();
    assert!(
        added.contains(&"platform::github:org/platform-only"),
        "unexpected sources_added: {added:?}"
    );
}

/// Regression (cross-tenant ledger pollution): an org apply whose document
/// carries redaction placeholders triggers secret restore; the recorded ledger
/// row must contain ONLY this org's desired document — not the full multi-org
/// composition (other tenants' namespaced sources and their real Apprise
/// URLs), which the old post-compose restore serialized into the stream — and
/// a GET→apply round-trip of unchanged content must converge to an idempotent
/// no-op instead of appending a twin revision per save — from the first
/// round-trip on, since the seed apply already records the canonical body.
#[tokio::test]
async fn org_apply_with_redacted_secrets_should_stay_org_local_and_idempotent() {
    let _db = DB_GUARD.lock().await;
    let fixture = OrgFixture::create("redacted", &test_postgres_url(), "api");
    let Some(app) = build_app(&fixture).await else {
        eprintln!("skipping organizations e2e (postgres unavailable)");
        return;
    };

    // Seed the platform stream (real secrets, no placeholders → recorded as sent).
    let response = app
        .clone()
        .oneshot(auth_request(
            "POST",
            "/api/v1/organizations/platform/config/apply",
            PLATFORM_V2,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    // UI-style round-trip 1: GET returns the redacted authority document…
    let response = app
        .clone()
        .oneshot(auth_request(
            "GET",
            "/api/v1/organizations/platform/config",
            "",
        ))
        .await
        .expect("response");
    let shown = json_body(response).await;
    let redacted = shown["desired_content"]
        .as_str()
        .expect("desired_content")
        .to_owned();
    assert!(
        redacted.contains("<redacted>"),
        "GET must redact: {redacted}"
    );

    // …and applying it back restores the secret to the identical ledger body, so
    // it is a no-op already on the FIRST round-trip: the seed apply normalized
    // inline secrets to `*_env` refs before recording, so what GET redacts is
    // the canonical body. Restore therefore reproduces the stored sha, and no
    // twin revision is appended — which is the whole point of this regression.
    let response = app
        .clone()
        .oneshot(auth_request(
            "POST",
            "/api/v1/organizations/platform/config/apply",
            &redacted,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let canonicalized = json_body(response).await;
    assert_eq!(
        canonicalized["applied"],
        serde_json::Value::Bool(false),
        "restoring the redacted authority document must not re-apply: {canonicalized}"
    );

    // Round-trip 2 of the same canonical content stays a pure no-op.
    let response = app
        .clone()
        .oneshot(auth_request(
            "GET",
            "/api/v1/organizations/platform/config",
            "",
        ))
        .await
        .expect("response");
    let shown = json_body(response).await;
    let redacted_again = shown["desired_content"]
        .as_str()
        .expect("desired_content")
        .to_owned();
    let response = app
        .clone()
        .oneshot(auth_request(
            "POST",
            "/api/v1/organizations/platform/config/apply",
            &redacted_again,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let noop = json_body(response).await;
    assert_eq!(
        noop["applied"],
        serde_json::Value::Bool(false),
        "unchanged round-trip must not append a revision: {noop}"
    );
    assert_eq!(noop["revision"], canonicalized["revision"]);

    // The restored document stays ORG-LOCAL: no other tenant's namespaced ids
    // or secrets, no self-namespacing, and the Apprise sink survived restore.
    assert!(
        !redacted_again.contains("security::") && !redacted_again.contains("security@example.com"),
        "platform ledger leaked the security org: {redacted_again}"
    );
    assert!(
        !redacted_again.contains("platform::"),
        "org ledger row must stay unnamespaced: {redacted_again}"
    );
    assert!(
        redacted_again.contains("<redacted>"),
        "apprise sink must survive the restore round-trip: {redacted_again}"
    );

    // Exactly 1 revision: the seed. Neither round-trip added one.
    let response = app
        .clone()
        .oneshot(auth_request(
            "GET",
            "/api/v1/organizations/platform/config/revisions",
            "",
        ))
        .await
        .expect("response");
    let history = json_body(response).await;
    assert_eq!(history["total"], serde_json::Value::from(1));
}

#[tokio::test]
async fn webhook_should_fan_out_to_every_org_watching_the_repo() {
    let _db = DB_GUARD.lock().await;

    // Live Apprise mock: `notifications_sent` counts SUCCESSFUL synchronous
    // deliveries, so a dead endpoint would report 0 even though both orgs
    // enqueued. Expecting exactly 2 POSTs also proves the platform org's
    // catch-all sink hears its TAGGED source (`routing_tag: core`) — the
    // namespacing regression this suite guards against.
    //
    // Mount the `expect(2)` mock *after* confirming Postgres is available:
    // an early skip must not leave a MockServer that panics on Drop when
    // zero requests arrived.
    let apprise = MockServer::start().await;

    let fixture =
        OrgFixture::create_with_apprise("fanout", &test_postgres_url(), "api", &apprise.uri());
    let Some(app) = build_app(&fixture).await else {
        eprintln!("skipping organizations e2e (postgres unavailable)");
        return;
    };

    Mock::given(method("POST"))
        .and(path("/notify"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .expect(2)
        .mount(&apprise)
        .await;

    // A source's FIRST observation records a silent baseline (no history
    // flood) — that applies per org-namespaced id. Mark both initialized so
    // the pushed release is "new" for each org, which is the fan-out case
    // under test.
    let bootstrap = config::load_infra_bootstrap(&fixture.paths).expect("bootstrap");
    let store = xrelease::store::Store::open_from_config(&bootstrap.database).expect("store");
    store
        .mark_initialized("platform::github:shared/upstream")
        .expect("init platform source");
    store
        .mark_initialized("security::github:shared/upstream")
        .expect("init security source");
    drop(store);

    let body = serde_json::json!({
    "action": "published",
    "release": { "tag_name": "v9.9.9", "draft": false, "prerelease": false },
    "repository": { "full_name": "shared/upstream" },
    })
    .to_string();
    let mut mac = HmacSha256::new_from_slice(b"wh-secret").expect("hmac");
    mac.update(body.as_bytes());
    let signature = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/webhooks/github")
        .header("Content-Type", "application/json")
        .header("X-GitHub-Delivery", "org-fanout-1")
        .header("X-Hub-Signature-256", signature)
        .body(Body::from(body))
        .expect("request");

    let response = app.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let delivered = json_body(response).await;
    // One event, two organizations, one enqueued notification each.
    assert_eq!(delivered["accepted"], serde_json::Value::Bool(true));
    assert_eq!(delivered["notifications_sent"], serde_json::Value::from(2));
}
