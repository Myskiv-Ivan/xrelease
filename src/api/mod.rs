//! HTTP API and webhook ingress (OpenAPI 3 — see `/openapi.json`).

mod auth;
mod auth_routes;
mod bootstrap_admin;
mod config;
mod error;
mod handlers;
mod observability;
mod oidc;
mod organizations;
mod password;
mod rate_limit;
mod role;
mod session;
mod webhook;
mod webhook_id;
mod webhook_payload;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use axum::http::{HeaderName, HeaderValue, Method};
use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use tokio::sync::{Mutex, RwLock};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

use crate::config::{ApiConfig, Config, ConfigPaths, EffectiveRevision, TeamConfig};
use crate::engine::Engine;
use crate::pipeline::Watch;
use crate::scheduler::{self, WatchSupervisor};
use crate::watch_lookup::WatchIndex;

pub use handlers::WebhookResponse;
pub use oidc::OidcValidator;
pub use password::hash_password;
pub use role::AppRole;

pub use bootstrap_admin::ensure_bootstrap_admin;

/// Hot-swappable desired-state view under **one** lock.
///
/// Readers take a single read guard and see a consistent
/// `(index, effective, teams, revision, sha)` snapshot. Writers replace fields
/// under the same lock (after [`AppState::apply_lock`] serializes apply/reload),
/// so handlers never observe a torn mid-apply view.
#[derive(Debug)]
pub struct DesiredRuntime {
    pub index: WatchIndex,
    pub effective: Config,
    pub teams: Vec<TeamConfig>,
    /// Latest applied revision metadata, when booted/applied from the ledger.
    pub effective_revision: Option<EffectiveRevision>,
    /// SHA-256 of the desired-state document currently running, whatever its
    /// source. Ledger mode also carries it on [`EffectiveRevision`]; this field
    /// gives file mode the same identity so a reload can skip an unchanged file.
    pub effective_desired_sha: Option<String>,
}

/// Shared state for HTTP handlers and the background poller.
pub struct AppState {
    pub engine: Arc<Engine>,
    pub api: ApiConfig,
    /// Bootstrap config (local file + env) — never replaced by apply. A plain
    /// `Arc` on purpose: nothing writes it after boot, and keeping it behind a
    /// lock invited a bootstrap↔desired lock-order inversion (handlers took
    /// the two in different orders — harmless only while no writer existed).
    pub bootstrap: Arc<Config>,
    /// Effective desired-state runtime (watches + merged config + identity).
    pub desired: Arc<RwLock<DesiredRuntime>>,
    /// Bootstrap + optional app config file paths.
    pub config_paths: ConfigPaths,
    /// Push-applied config API settings (bootstrap-local).
    pub config_api: crate::config::ConfigApiConfig,
    /// Hot-swappable watch loops.
    pub supervisor: Arc<WatchSupervisor>,
    /// Serializes config apply/rollback (hot-swap + ledger must not interleave).
    pub apply_lock: Mutex<()>,
    /// Process start time for uptime in observability API.
    pub started_at: Instant,
    /// OIDC JWT validator (when `[api.oidc]` issuer is configured).
    pub oidc: Option<Arc<OidcValidator>>,
}

impl AppState {
    /// Build application state from runtime components.
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit boot wiring keeps call sites readable; avoid a builder until more fields appear"
    )]
    pub fn new(
        engine: Arc<Engine>,
        config_paths: ConfigPaths,
        bootstrap: Config,
        effective: Config,
        effective_revision: Option<EffectiveRevision>,
        watches: Vec<Watch>,
        api: ApiConfig,
        oidc: Option<Arc<OidcValidator>>,
        supervisor: Arc<WatchSupervisor>,
    ) -> Self {
        let teams = effective.teams.clone();
        let config_api = bootstrap.config_api.clone();
        // Booting from the ledger carries its sha on `effective_revision`.
        // Booting from the app file (or the [[organizations]] composition)
        // does not — without this, `reload`'s idempotency check
        // (`current_sha == Some(new_sha)`) would compare against `None` on the
        // very first reload after startup, treat an untouched desired state as
        // "changed", and hot-swap (rebuild the notifier, restart every poll
        // loop) for nothing. Best-effort: a read failure here just costs one
        // redundant reload later, not a functional break.
        let effective_desired_sha = if !bootstrap.organizations.is_empty() {
            crate::config::compose_organizations(
                &bootstrap,
                &config_paths,
                Some(&engine.store),
                None,
            )
            .ok()
            .map(|composed| composed.identity_sha256)
        } else {
            effective_revision.as_ref().map_or_else(
                || {
                    config_paths
                        .app
                        .as_ref()
                        .and_then(|path| std::fs::read_to_string(path).ok())
                        .map(|raw| crate::config_apply::content_sha256(&raw))
                },
                |revision| Some(revision.content_sha256.clone()),
            )
        };
        Self {
            engine,
            api,
            bootstrap: Arc::new(bootstrap),
            desired: Arc::new(RwLock::new(DesiredRuntime {
                index: WatchIndex::new(watches),
                effective,
                teams,
                effective_revision,
                effective_desired_sha,
            })),
            config_paths,
            config_api,
            supervisor,
            apply_lock: Mutex::new(()),
            started_at: Instant::now(),
            oidc,
        }
    }
}

fn build_cors_layer(origins: &[String]) -> Option<CorsLayer> {
    if origins.is_empty() {
        return None;
    }
    let allowed: Vec<HeaderValue> = origins
        .iter()
        .filter_map(|origin| origin.parse().ok())
        .collect();
    if allowed.is_empty() {
        return None;
    }
    Some(
        CorsLayer::new()
            .allow_origin(allowed)
            .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
            .allow_headers([
                axum::http::header::AUTHORIZATION,
                axum::http::header::CONTENT_TYPE,
                // Optimistic concurrency: a browser editor reads the config's
                // ETag and sends it back as If-Match on apply.
                axum::http::header::IF_MATCH,
                HeaderName::from_static("x-config-revision"),
                HeaderName::from_static("x-config-signature"),
                HeaderName::from_static("x-config-applied-by"),
            ])
            // Response headers are hidden from cross-origin JS unless exposed,
            // so without this the ETag half of the mechanism is unreadable.
            .expose_headers([axum::http::header::ETAG]),
    )
}

/// Build the Axum router with all API routes.
pub fn router(state: Arc<AppState>) -> Router {
    let limiter = rate_limit::build_rate_limiter(state.api.rate_limit_per_minute);

    let observability_routes = Router::new()
        .route("/api/v1/status", get(observability::get_status))
        .route("/api/v1/sources", get(observability::list_sources))
        .route("/api/v1/sources/{id}", get(observability::get_source))
        .route("/api/v1/outbox", get(observability::list_outbox))
        .route("/api/v1/teams", get(observability::list_teams))
        .route("/api/v1/notifiers", get(handlers::list_notifiers))
        .route("/api/v1/config", get(config::get_config))
        // Read-only audit history — belongs with the observability group
        // (bearer, no rate limit) since a dashboard polls it like any other view.
        .route("/api/v1/config/revisions", get(config::get_revisions))
        .route("/api/v1/config/schema", get(config::get_schema))
        .route("/api/v1/auth/me", get(auth_routes::me))
        .route("/api/v1/auth/logout", post(auth_routes::logout))
        // Organization catalogue + per-org config reads. Mounted
        // unconditionally: on a single-document instance they answer 404 with
        // an explanatory message, which beats a bare route-less 404.
        .route(
            "/api/v1/organizations",
            get(organizations::list_organizations),
        )
        .route(
            "/api/v1/organizations/{id}/config",
            get(organizations::get_org_config),
        )
        .route(
            "/api/v1/organizations/{id}/config/revisions",
            get(organizations::get_org_revisions),
        );

    // Whole-document / instance-wide mutations: require global operator+ (config
    // apply/rollback further demand global admin inside `require_config_apply_auth`).
    let mut mutations = Router::new()
        .route("/api/v1/check", post(handlers::trigger_check))
        .route("/api/v1/check/{id}", post(handlers::trigger_check_source))
        .route(
            "/api/v1/outbox/requeue",
            post(observability::requeue_outbox),
        )
        .route("/api/v1/notifiers/test", post(handlers::test_notifiers))
        .route("/api/v1/config/validate", post(config::post_validate))
        // Not gated by `[config_api]`: reload takes no body, so it can only
        // re-read the operator's own file — the zero-API fallback path.
        .route("/api/v1/reload", post(config::post_reload));

    if state.config_api.routes_enabled() {
        mutations = mutations
            .route("/api/v1/config/apply", post(config::post_apply))
            .route("/api/v1/config/rollback", post(config::post_rollback));
    }

    // Per-organization mutations: authenticated (viewer+) at the layer, then
    // each handler enforces the caller's role FOR THE PATH ORG (operator to
    // validate, admin to apply/rollback). They must NOT sit under the global
    // operator layer — a pure org-admin (global viewer + `admin:platform`)
    // would otherwise be blocked before the org-scoped check runs.
    let mut org_mutations = Router::new().route(
        "/api/v1/organizations/{id}/config/validate",
        post(organizations::post_org_validate),
    );
    if state.config_api.routes_enabled() {
        org_mutations = org_mutations
            .route(
                "/api/v1/organizations/{id}/config/apply",
                post(organizations::post_org_apply),
            )
            .route(
                "/api/v1/organizations/{id}/config/rollback",
                post(organizations::post_org_rollback),
            );
    }

    let mut webhooks = Router::new()
        .route("/api/v1/webhooks/github", post(webhook::github))
        .route("/api/v1/webhooks/gitlab", post(webhook::gitlab))
        .route("/api/v1/webhooks/gitea", post(webhook::gitea))
        .route("/api/v1/webhooks/bitbucket", post(webhook::bitbucket))
        .route("/api/v1/webhooks/docker", post(webhook::docker))
        .route("/api/v1/webhooks/generic", post(webhook::generic));

    // Reads are viewer-accessible; whole-document mutations require operator+
    // (config apply/rollback further demand admin inside
    // `require_config_apply_auth`). Both layers resolve + authenticate the
    // bearer, so a mutation route is never reachable by an unauthenticated
    // caller or a viewer session. Org mutations only *authenticate* at the
    // layer (viewer+) and do their org-scoped authZ per-handler.
    let reads = observability_routes.layer(middleware::from_fn_with_state(
        Arc::clone(&state),
        auth::require_management_auth,
    ));
    let mut mutations = mutations.layer(middleware::from_fn_with_state(
        Arc::clone(&state),
        auth::require_operator_auth,
    ));
    let mut org_mutations = org_mutations.layer(middleware::from_fn_with_state(
        Arc::clone(&state),
        auth::require_management_auth,
    ));

    // Instance-admin: user directory (list + create local accounts). Separated
    // from operator mutations so a pure operator cannot manage identities.
    let mut admin_routes = Router::new()
        .route(
            "/api/v1/auth/users",
            get(auth_routes::list_users).post(auth_routes::create_user),
        )
        .route(
            "/api/v1/auth/users/{id}/oidc",
            post(auth_routes::link_user_oidc),
        )
        .layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            auth::require_admin_auth,
        ));

    // Public (pre-auth) endpoints. `login` is the brute-force target, so it is
    // rate limited; `methods` is trivial and `oidc/sync` is gated by a valid
    // OIDC token it must validate anyway.
    let public_auth = Router::new()
        .route("/api/v1/auth/methods", get(auth_routes::auth_methods))
        .route("/api/v1/auth/oidc/sync", post(auth_routes::oidc_sync));
    let mut login_route = Router::new().route("/api/v1/auth/login", post(auth_routes::login));

    if let Some(lim) = limiter {
        let rate_state = rate_limit::RateLimitState {
            limiter: Arc::clone(&lim),
            metrics: Arc::clone(&state.engine.metrics),
        };
        mutations = mutations.layer(middleware::from_fn_with_state(
            rate_state.clone(),
            rate_limit::rate_limit_middleware,
        ));
        org_mutations = org_mutations.layer(middleware::from_fn_with_state(
            rate_state.clone(),
            rate_limit::rate_limit_middleware,
        ));
        admin_routes = admin_routes.layer(middleware::from_fn_with_state(
            rate_state.clone(),
            rate_limit::rate_limit_middleware,
        ));
        webhooks = webhooks.layer(middleware::from_fn_with_state(
            rate_state.clone(),
            rate_limit::rate_limit_middleware,
        ));
        login_route = login_route.layer(middleware::from_fn_with_state(
            rate_state,
            rate_limit::rate_limit_middleware,
        ));
    }

    let management = reads
        .merge(mutations)
        .merge(org_mutations)
        .merge(admin_routes);

    let mut app = Router::new()
        .route("/health", get(handlers::health))
        .route("/ready", get(handlers::ready))
        .route("/metrics", get(handlers::metrics))
        .route("/openapi.json", get(handlers::openapi_spec))
        .merge(public_auth)
        .merge(login_route)
        .merge(management)
        .merge(webhooks)
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone());

    if let Some(cors) = build_cors_layer(&state.api.cors_origins) {
        app = app.layer(cors);
    }

    app
}

/// Run the backend until shutdown: poller + shared background tasks + HTTP API.
pub async fn serve(listen: &str, state: Arc<AppState>) -> anyhow::Result<()> {
    let addr: SocketAddr = listen
        .parse()
        .map_err(|err| anyhow::anyhow!("invalid api.listen `{listen}`: {err}"))?;

    // Config-apply is opt-in and mutation-only (unlike the general management
    // require_auth = false lab path) — refuse to start
    // rather than silently expose an unauthenticated way to hot-swap sources
    // and notifiers. Checked before any side effect (watch loops, background
    // tasks) so a misconfigured deployment fails immediately, not mid-startup.
    if state.config_api.api_config
        && !auth::config_apply_auth_configured(
            state.api.auth_configured(),
            state.config_api.hmac_configured(),
        )
    {
        anyhow::bail!(
            "config_api.api_config is set but no authentication guards it at all — \
 set api.api_key, api.oidc.issuer, XRELEASE_SESSION_SECRET, or \
 XRELEASE_CONFIG_APPLY_SECRET before enabling POST /api/v1/config/apply \
 and /config/rollback (unauthenticated, they let anyone hijack notification delivery)"
        );
    }

    let watches: Vec<Watch> = state.desired.read().await.index.watches().to_vec();
    let engine = Arc::clone(&state.engine);
    state.supervisor.replace(watches).await?;
    let tasks = scheduler::start_shared_background(Arc::clone(&engine)).await;

    if state.config_api.api_config && !state.config_api.hmac_configured() {
        warn!(
            "config apply API is enabled with bearer auth but no HMAC body signature \
 (XRELEASE_CONFIG_APPLY_SECRET is unset) — apply/rollback accept any request \
 from a valid bearer holder with no body-integrity check"
        );
    }

    let oidc_refresh = state.oidc.as_ref().map(|validator| {
        let refresh = Arc::clone(validator);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(oidc::JWKS_RETRY_INTERVAL);
            loop {
                interval.tick().await;
                refresh.refresh_if_stale().await;
            }
        })
    });

    let reload_signal = spawn_sighup_reload(Arc::clone(&state));

    let app = router(state.clone());
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!(%addr, "HTTP API listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(scheduler::shutdown_signal())
        .await?;

    info!("HTTP server stopped; draining background tasks");
    if let Some(handle) = oidc_refresh {
        handle.abort();
    }
    if let Some(handle) = reload_signal {
        handle.abort();
    }
    state.supervisor.stop_watchers().await;
    tasks.graceful_shutdown(&engine).await;
    Ok(())
}

/// Reload the app config file on `SIGHUP` — the signal operators and init
/// systems already use for "re-read your config" (`systemctl reload`).
///
/// Returns `None` on non-Unix targets, or when the signal cannot be installed.
#[cfg(unix)]
fn spawn_sighup_reload(state: Arc<AppState>) -> Option<tokio::task::JoinHandle<()>> {
    let mut hangup = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup()) {
        Ok(signal) => signal,
        Err(err) => {
            warn!(error = %err, "cannot listen for SIGHUP; config reload via signal disabled");
            return None;
        }
    };
    Some(tokio::spawn(async move {
        while hangup.recv().await.is_some() {
            match state.reload_from_disk().await {
                Ok(response) if response.applied => info!(
                sha = %response.content_sha256,
                source = %response.source_path,
                "SIGHUP: config reloaded"
                ),
                Ok(_) => info!("SIGHUP: config unchanged"),
                Err(err) => warn!(error = %err, "SIGHUP: config reload failed"),
            }
        }
    }))
}

#[cfg(not(unix))]
fn spawn_sighup_reload(_state: Arc<AppState>) -> Option<tokio::task::JoinHandle<()>> {
    None
}

impl AppState {
    /// Build state for integration tests without a running supervisor loop.
    #[doc(hidden)]
    pub fn new_for_test(
        engine: Arc<Engine>,
        bootstrap: Config,
        effective: Config,
        watches: Vec<Watch>,
        oidc: Option<Arc<OidcValidator>>,
    ) -> Arc<Self> {
        Self::new_for_test_with_app(
            engine,
            bootstrap,
            effective,
            watches,
            oidc,
            Some(PathBuf::from("app/releases.yaml")),
        )
    }

    /// [`Self::new_for_test`] with an explicit app config path (reload tests).
    #[doc(hidden)]
    pub fn new_for_test_with_app(
        engine: Arc<Engine>,
        bootstrap: Config,
        effective: Config,
        watches: Vec<Watch>,
        oidc: Option<Arc<OidcValidator>>,
        app: Option<PathBuf>,
    ) -> Arc<Self> {
        Self::new_for_test_with_paths(
            engine,
            bootstrap,
            effective,
            watches,
            oidc,
            ConfigPaths::new(PathBuf::from("bootstrap.toml"), app),
        )
    }

    /// [`Self::new_for_test`] with full [`ConfigPaths`] — multi-org tests need
    /// a real bootstrap path because `[[organizations]].app` files resolve
    /// relative to the bootstrap file's directory.
    #[doc(hidden)]
    pub fn new_for_test_with_paths(
        engine: Arc<Engine>,
        bootstrap: Config,
        effective: Config,
        watches: Vec<Watch>,
        oidc: Option<Arc<OidcValidator>>,
        config_paths: ConfigPaths,
    ) -> Arc<Self> {
        let api = effective.api.clone();
        let supervisor = Arc::new(WatchSupervisor::new(Arc::clone(&engine)));
        Arc::new(Self::new(
            engine,
            config_paths,
            bootstrap,
            effective,
            None,
            watches,
            api,
            oidc,
            supervisor,
        ))
    }
}
