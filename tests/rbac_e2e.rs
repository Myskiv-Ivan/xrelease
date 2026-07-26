//! End-to-end RBAC enforcement (native local-auth sessions). Requires PostgreSQL.
//!
//! Proves the authorization fix: a `viewer` session is authenticated but must
//! not reach operator/admin routes, while the static `api_key` (admin) can.

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use xrelease::api::{hash_password, router, AppState};
use xrelease::config::Config;
use xrelease::engine::Engine;
use xrelease::runtime::build_http_client;
use xrelease::store::AppUserInsert;

fn test_postgres_url() -> String {
    std::env::var("XRELEASE_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://xrelease:xrelease@127.0.0.1:5432/xrelease_test".to_owned())
}

/// Serialize against the shared test DB (see config_apply_e2e for the flake).
static DB_GUARD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const DESIRED: &str = r#"
notifiers:
  - type: apprise
    endpoint: http://127.0.0.1:9
    urls: ["mailto://a@b.c"]
sources:
  - type: github
    repo: org/rbac
"#;

fn config(postgres_url: &str) -> Config {
    toml::from_str(&format!(
        r#"
        [config_api]
        api_config = true
        source = "api"

        [database]
        postgres_url = "{postgres_url}"
        max_connections = 4

        [api]
        api_key = "admin-key"

        [api.local_auth]
        enabled = true
        session_secret = "rbac-e2e-session-secret-0123456789"
        admin_password = "adminpass"

        [[notifiers]]
        type = "apprise"
        endpoint = "http://127.0.0.1:9"
        urls = ["mailto://a@b.c"]

        [[sources]]
        type = "github"
        repo = "org/initial"
    "#
    ))
    .expect("parse config")
}

/// Build the router and return it plus a freshly-issued `viewer` session token.
async fn build_app_with_viewer(postgres_url: &str) -> Option<(axum::Router, String)> {
    let effective = config(postgres_url);
    let http = build_http_client().ok()?;
    let watches = effective.clone().into_watches().ok()?;
    let engine = Engine::open(&effective, http).ok()?;
    engine.store.truncate_all().ok()?;

    // Seed a viewer with a known password.
    let hash = hash_password("viewerpass").expect("hash");
    engine
        .store
        .insert_user(&AppUserInsert {
            username: Some("viewer"),
            password_hash: Some(&hash),
            oidc_sub: None,
            email: None,
            display_name: Some("Viewer"),
            role: "viewer",
            auth_source: "local",
        })
        .ok()?;

    let mut bootstrap = effective.clone();
    xrelease::config::strip_desired_sections(&mut bootstrap);
    let state = AppState::new_for_test(engine, bootstrap, effective, watches, None);
    let app = router(state);

    // Log in as the viewer to obtain a real session JWT.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    r#"{"username":"viewer","password":"viewerpass"}"#,
                ))
                .expect("login request"),
        )
        .await
        .expect("login response");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "viewer login should work"
    );
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("login json");
    assert_eq!(json["role"], "viewer");
    let token = json["access_token"].as_str().expect("token").to_owned();

    Some((app, token))
}

fn apply_request(bearer: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/v1/config/apply")
        .header("Authorization", format!("Bearer {bearer}"))
        .header("Content-Type", "application/yaml")
        .body(Body::from(DESIRED))
        .expect("apply request")
}

#[tokio::test]
async fn viewer_session_should_be_forbidden_from_config_apply() {
    let _db = DB_GUARD.lock().await;
    let Some((app, viewer_token)) = build_app_with_viewer(&test_postgres_url()).await else {
        eprintln!("skipping rbac e2e (postgres unavailable)");
        return;
    };

    // A valid viewer session — authenticated, but below admin.
    let response = app
        .clone()
        .oneshot(apply_request(&viewer_token))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // The static api_key is admin: same request succeeds.
    let response = app
        .clone()
        .oneshot(apply_request("admin-key"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn viewer_session_should_read_but_not_mutate() {
    let _db = DB_GUARD.lock().await;
    let Some((app, viewer_token)) = build_app_with_viewer(&test_postgres_url()).await else {
        eprintln!("skipping rbac e2e (postgres unavailable)");
        return;
    };

    // Reads are viewer-accessible.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/status")
                .header("Authorization", format!("Bearer {viewer_token}"))
                .body(Body::empty())
                .expect("status request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    // A non-apply mutation (operator gate) is also forbidden for a viewer.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/reload")
                .header("Authorization", format!("Bearer {viewer_token}"))
                .body(Body::empty())
                .expect("reload request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn logout_should_revoke_a_live_session_token() {
    let _db = DB_GUARD.lock().await;
    let Some((app, viewer_token)) = build_app_with_viewer(&test_postgres_url()).await else {
        eprintln!("skipping rbac e2e (postgres unavailable)");
        return;
    };
    let bearer = |uri: &str, method: &str| {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("Authorization", format!("Bearer {viewer_token}"))
            .body(Body::empty())
            .expect("request")
    };

    // The token works before logout.
    let response = app
        .clone()
        .oneshot(bearer("/api/v1/status", "GET"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    // Logout bumps the user's session version.
    let response = app
        .clone()
        .oneshot(bearer("/api/v1/auth/logout", "POST"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    // The same (still-unexpired) token is now rejected.
    let response = app
        .clone()
        .oneshot(bearer("/api/v1/status", "GET"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn unknown_user_login_should_fail_uniformly() {
    let _db = DB_GUARD.lock().await;
    let Some((app, _)) = build_app_with_viewer(&test_postgres_url()).await else {
        eprintln!("skipping rbac e2e (postgres unavailable)");
        return;
    };

    for body in [
        r#"{"username":"ghost","password":"whatever"}"#,
        r#"{"username":"viewer","password":"wrongpass"}"#,
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/login")
                    .header("Content-Type", "application/json")
                    .body(Body::from(body))
                    .expect("login request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn admin_should_list_and_create_local_users() {
    let _db = DB_GUARD.lock().await;
    let Some((app, viewer_token)) = build_app_with_viewer(&test_postgres_url()).await else {
        eprintln!("skipping rbac e2e (postgres unavailable)");
        return;
    };

    // Viewer cannot manage the directory.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/auth/users")
                .header("Authorization", format!("Bearer {viewer_token}"))
                .body(Body::empty())
                .expect("list users"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // Api-key (admin) lists the seeded viewer.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/auth/users")
                .header("Authorization", "Bearer admin-key")
                .body(Body::empty())
                .expect("list users"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(json["users"].as_array().expect("users").len(), 1);
    assert_eq!(json["users"][0]["auth_source"], "local");
    assert_eq!(json["users"][0]["username"], "viewer");

    // Create another local operator.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/users")
                .header("Authorization", "Bearer admin-key")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    r#"{"username":"ops","password":"ops-secret-1","role":"operator","display_name":"Ops"}"#,
                ))
                .expect("create user"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let created: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(created["username"], "ops");
    assert_eq!(created["role"], "operator");
    assert_eq!(created["auth_source"], "local");
    assert!(created["oidc_sub"].is_null() || created.get("oidc_sub").is_none());

    // Duplicate username → 409.
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/users")
                .header("Authorization", "Bearer admin-key")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    r#"{"username":"ops","password":"ops-secret-2","role":"viewer"}"#,
                ))
                .expect("duplicate create"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::CONFLICT);
}
