//! E2E tests for read-only observability API (require PostgreSQL).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use xrelease::advisory::{Advisory, Severity};
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
    engine
        .store
        .record_advisories(
            "PyPI",
            "requests",
            "2.0.0",
            &[Advisory {
                id: "PYSEC-2024-0001".into(),
                display_id: "CVE-2024-00001".into(),
                summary: Some("Example advisory for e2e coverage".into()),
                severity: Some(Severity::High),
                cvss_vector: None,
                url: Some("https://osv.dev/vulnerability/PYSEC-2024-0001".into()),
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

/// The dashboard cannot otherwise tell "no known CVEs" apart from "enrichment
/// is off" — both render as an empty advisories column.
#[tokio::test]
async fn status_should_report_advisory_enrichment_as_disabled_by_default() {
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

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");

    // The test config sets no `[advisories]`, so this exercises the default.
    assert_eq!(
        json["advisories"]["enabled"], false,
        "advisory enrichment must stay opt-in — it discloses package names"
    );
    assert_eq!(
        json["advisories"]["endpoint"], "https://api.osv.dev",
        "the endpoint is reported even while disabled, so an operator can \
         confirm which database would be queried"
    );
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

/// End-to-end: a `record_advisories` write reaches the point-lookup JSON
/// response through the batch `advisories_for_versions` read path wired in
/// `source_detail_from_watch`.
#[tokio::test]
async fn source_detail_should_attach_persisted_advisories_to_the_matching_release() {
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
    let releases = json["seen_releases"].as_array().expect("releases");
    assert_eq!(releases[0]["tag"], "2.0.0");

    let advisories = releases[0]["advisories"].as_array().expect("advisories");
    assert_eq!(advisories.len(), 1);
    assert_eq!(advisories[0]["id"], "PYSEC-2024-0001");
    assert_eq!(
        advisories[0]["display_id"], "CVE-2024-00001",
        "the CVE alias must be preferred over the database-native id"
    );
    assert_eq!(
        advisories[0]["severity"], "high",
        "severity must serialize as its lowercase label, not the Rust variant name"
    );
    assert!(
        advisories[0].get("cvss_vector").is_none(),
        "None fields must be omitted, not null"
    );
}

/// The list endpoint deliberately does not pay the extra advisories query —
/// only the source-detail path opts in (see `source_details` in
/// `api/observability.rs`). Confirms that omission is real, not an oversight:
/// every source's `seen_releases` array must be present but advisory-free.
#[tokio::test]
async fn sources_list_should_omit_advisories_even_when_persisted() {
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
    let requests_source = json
        .as_array()
        .expect("array")
        .iter()
        .find(|source| source["id"] == "pypi:requests")
        .expect("pypi:requests present");
    let releases = requests_source["seen_releases"]
        .as_array()
        .expect("releases");
    assert_eq!(releases.len(), 1);
    assert!(
        releases[0].get("advisories").is_none(),
        "list endpoint must not attach advisories — they are omitted via \
         skip_serializing_if, not merely empty"
    );
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

/// Seen releases to seed for the backfill-progress test — comfortably more
/// than `DETAIL_ADVISORY_BACKFILL` (5) so a single request cannot cover them.
const SEEDED_RELEASES: usize = 12;

/// Build an app whose `[advisories]` points at `osv_endpoint`.
///
/// Separate from [`build_app`]: enabling enrichment changes what
/// `GET /api/v1/sources/{id}` *does* (it live-fills), so the other tests
/// deliberately run with it off.
async fn build_advisory_app(postgres_url: &str, osv_endpoint: &str) -> Option<axum::Router> {
    let config: Config = toml::from_str(&format!(
        r#"
        [[notifiers]]
        type = "apprise"
        endpoint = "http://127.0.0.1:9"
        urls = ["mailto://test@example.com"]

        [database]
        postgres_url = "{postgres_url}"
        max_connections = 4

        [api]
        api_key = "test-key"

        [advisories]
        enabled = true
        endpoint = "{osv_endpoint}"
        timeout_secs = 2
        # No in-process memoisation: this test counts OSV round trips, and the
        # progress it asserts must come from the persisted check ledger rather
        # than from a process-local cache that a restart would drop.
        cache_ttl_secs = 0

        [[sources]]
        type = "pypi"
        name = "requests"
    "#
    ))
    .expect("parse config");

    let http = build_http_client().ok()?;
    let watches = config.to_watches().ok()?;
    let engine = Engine::open(&config, http).ok()?;
    engine.store.truncate_all().ok()?;

    let seeded: Vec<String> = (0..SEEDED_RELEASES).map(|n| format!("1.{n}.0")).collect();
    let upserts: Vec<SeenUpsert<'_>> = seeded
        .iter()
        .map(|version| SeenUpsert {
            identity: version,
            display_tag: None,
            content_digest: None,
            published_at: None,
            url: None,
        })
        .collect();
    engine
        .store
        .record_seen_batch("pypi:requests", &upserts)
        .ok()?;

    let mut bootstrap = config.clone();
    xrelease::config::strip_desired_sections(&mut bootstrap);
    let state = AppState::new_for_test(engine, bootstrap, config, watches, None);
    Some(router(state))
}

/// The source-detail backfill is capped per request, so the only way a source
/// with more releases than that cap ever gets full coverage is if successive
/// requests resume where the last one stopped.
///
/// Before `advisory_check` existed, a version OSV confirmed *clean* left no
/// trace — so every request re-queried the same newest five forever and the
/// releases below them were never looked at once. This asserts the fix by the
/// only signal that matters: OSV is asked about strictly new versions.
#[tokio::test]
async fn advisory_backfill_should_advance_across_requests_instead_of_repeating() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };

    let osv = wiremock::MockServer::start().await;
    // Every version is clean — the case that used to be indistinguishable from
    // "never checked".
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/v1/query"))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({ "vulns": [] })),
        )
        .mount(&osv)
        .await;

    let Some(app) = build_advisory_app(&url, &osv.uri()).await else {
        eprintln!("skipping advisory backfill e2e (postgres unavailable)");
        return;
    };

    let queried = |requests: &[wiremock::Request]| -> Vec<String> {
        requests
            .iter()
            .filter_map(|request| {
                let body: serde_json::Value = serde_json::from_slice(&request.body).ok()?;
                Some(body.get("version")?.as_str()?.to_owned())
            })
            .collect()
    };

    for round in 1..=2 {
        let response = app
            .clone()
            .oneshot(auth_request("GET", "/api/v1/sources/pypi%3Arequests"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK, "round {round}");
    }

    let asked = queried(&osv.received_requests().await.expect("recorded requests"));
    // Two rounds × the per-request cap of 5. Exact, not a lower bound: a
    // regression that re-queried the same five would land on 10 calls too, so
    // only the uniqueness check below separates the two — and only an exact
    // count proves the cap itself still holds.
    assert_eq!(
        asked.len(),
        10,
        "two page loads must spend the full per-request budget: {asked:?}"
    );

    let unique: std::collections::HashSet<&String> = asked.iter().collect();
    assert_eq!(
        unique.len(),
        asked.len(),
        "no version may be re-queried once confirmed clean — asked {asked:?}"
    );
}

/// The background sweep is what covers releases nobody ever notified about —
/// everything a baseline (first) poll caught, which produces no notification
/// and therefore never reaches delivery-time enrichment.
///
/// Asserts the whole point of it: advisories land in the store with **no HTTP
/// request to the dashboard at all**.
#[tokio::test]
async fn advisory_sweep_should_fill_releases_in_the_background_without_a_page_load() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };

    let osv = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/v1/query"))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "vulns": [{
                    "id": "PYSEC-2099-0001",
                    "aliases": ["CVE-2099-0001"],
                    "summary": "Swept finding",
                    "database_specific": { "severity": "HIGH" }
                }]
            })),
        )
        .mount(&osv)
        .await;

    let config: Config = toml::from_str(&format!(
        r#"
        [[notifiers]]
        type = "apprise"
        endpoint = "http://127.0.0.1:9"
        urls = ["mailto://test@example.com"]

        [database]
        postgres_url = "{url}"
        max_connections = 4

        [api]
        api_key = "test-key"

        [advisories]
        enabled = true
        endpoint = "{}"
        timeout_secs = 2
        cache_ttl_secs = 0
        sweep_batch = 3

        [[sources]]
        type = "pypi"
        name = "requests"
    "#,
        osv.uri()
    ))
    .expect("parse config");

    let http = build_http_client().expect("http");
    let watches = config.to_watches().expect("watches");
    let Ok(engine) = Engine::open(&config, http) else {
        eprintln!("skipping advisory sweep e2e (postgres unavailable)");
        return;
    };
    engine.store.truncate_all().expect("truncate");

    // A silent baseline: releases recorded as seen, nothing ever notified.
    engine
        .store
        .record_seen_batch(
            "pypi:requests",
            &["2.30.0", "2.31.0", "2.32.0", "2.33.0"].map(|version| SeenUpsert {
                identity: version,
                display_tag: None,
                content_digest: None,
                published_at: None,
                url: None,
            }),
        )
        .expect("seed seen");

    let checked = xrelease::scheduler::sweep_advisories_now(&engine, &watches).await;
    assert_eq!(
        checked, 3,
        "one round must cover exactly `sweep_batch` versions"
    );

    // The batch limit leaves the rest for the next round rather than pulling a
    // source's whole history at a third party in one go.
    let remaining = engine
        .store
        .unchecked_seen_versions("pypi:requests", "PyPI", "requests", 10)
        .expect("work queue");
    assert_eq!(
        remaining.len(),
        1,
        "one of four must still be pending: {remaining:?}"
    );

    let second = xrelease::scheduler::sweep_advisories_now(&engine, &watches).await;
    assert_eq!(
        second, 1,
        "the second round drains the remainder instead of redoing the first three"
    );
    assert!(engine
        .store
        .unchecked_seen_versions("pypi:requests", "PyPI", "requests", 10)
        .expect("work queue")
        .is_empty());

    // Findings are queryable straight away — no dashboard request involved at
    // any point in this test.
    let versions = ["2.30.0", "2.31.0", "2.32.0", "2.33.0"];
    let stored = engine
        .store
        .advisories_for_versions("PyPI", "requests", &versions)
        .expect("read findings");
    for version in versions {
        assert_eq!(
            stored[version][0].display_id, "CVE-2099-0001",
            "{version} must carry the swept finding"
        );
    }
}

/// A source kind with no OSV coordinate must never enter the sweep — otherwise
/// a Docker or GitHub watch would be re-queried forever with a guessed
/// ecosystem that can only ever return nothing.
#[tokio::test]
async fn advisory_sweep_should_ignore_sources_without_an_osv_coordinate() {
    let _db = DB_GUARD.lock().await;
    let Some(url) = test_postgres_url() else {
        return;
    };

    let osv = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .respond_with(wiremock::ResponseTemplate::new(500))
        .mount(&osv)
        .await;

    let config: Config = toml::from_str(&format!(
        r#"
        [[notifiers]]
        type = "apprise"
        endpoint = "http://127.0.0.1:9"
        urls = ["mailto://test@example.com"]

        [database]
        postgres_url = "{url}"
        max_connections = 4

        [api]
        api_key = "test-key"

        [advisories]
        enabled = true
        endpoint = "{}"
        timeout_secs = 2

        [[sources]]
        type = "docker"
        image = "library/nginx"

        [[sources]]
        type = "github"
        repo = "tokio-rs/tokio"
    "#,
        osv.uri()
    ))
    .expect("parse config");

    let http = build_http_client().expect("http");
    let watches = config.to_watches().expect("watches");
    let Ok(engine) = Engine::open(&config, http) else {
        return;
    };
    engine.store.truncate_all().expect("truncate");
    engine
        .store
        .record_seen_batch(
            "docker:library/nginx",
            &[SeenUpsert {
                identity: "1.27.0",
                display_tag: None,
                content_digest: None,
                published_at: None,
                url: None,
            }],
        )
        .expect("seed seen");

    let checked = xrelease::scheduler::sweep_advisories_now(&engine, &watches).await;
    assert_eq!(checked, 0);
    assert!(
        osv.received_requests().await.expect("recorded").is_empty(),
        "a container tag is not a package coordinate — OSV must not be dialled"
    );
}
