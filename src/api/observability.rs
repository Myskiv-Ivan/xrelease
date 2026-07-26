//! Read-only observability endpoints for dashboard UI (SvelteKit, etc.).

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::{Deserialize, Serialize};

use super::auth::{authorize_for_org, resolve_auth_principal};
use super::error::ApiError;
use super::role::AppRole;
use super::AppState;
use crate::config::{desired_source_from_revision, organization_id_from_source_id, DesiredSource};
use crate::metrics::BreakerOpenView;
use crate::runtime::{MetricsSnapshot, SourceDetail, SourceSummary};
use crate::store::OutboxEntry;

/// Push-applied config status for dashboard / probes.
#[derive(Debug, Serialize)]
pub struct ConfigApplyStatus {
    /// Whether POST apply/rollback routes are mounted (`[config_api].api_config`).
    pub api_config_enabled: bool,
    /// Whether the dashboard form editor is on (`ui_config` + api + source=api).
    pub ui_config_enabled: bool,
    /// Declared desired-state authority (`[config_api].source`).
    pub config_source: crate::config::ConfigSource,
    pub hmac_configured: bool,
    pub desired_source: DesiredSource,
    pub bootstrap_path: String,
    pub app_config_path: Option<String>,
    pub split_config: bool,
    pub revision: Option<i64>,
    pub content_sha256: Option<String>,
    pub revision_label: Option<String>,
    pub applied_at: Option<String>,
    pub last_rejected_revision: Option<i64>,
    pub last_rejected_error: Option<String>,
}

/// `GET /api/v1/status` — process overview for dashboard landing page.
#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub version: &'static str,
    pub uptime_secs: u64,
    pub database_ok: bool,
    pub sources_configured: usize,
    pub outbox_pending: usize,
    pub outbox_dead: usize,
    /// Pending/failed rows held until `deliver_after` (notify_schedule).
    pub outbox_deferred: usize,
    /// Live per-sink circuit-breaker open/closed view.
    pub breakers: Vec<BreakerOpenView>,
    pub metrics: MetricsSnapshot,
    pub config_apply: ConfigApplyStatus,
}

/// `GET /api/v1/outbox` — pending/failed notification queue.
#[derive(Debug, Serialize)]
pub struct OutboxListResponse {
    pub entries: Vec<OutboxEntry>,
}

#[derive(Debug, Deserialize)]
pub struct OutboxQuery {
    #[serde(default = "default_outbox_limit")]
    pub limit: usize,
    /// When set, return only rows whose `source_id` belongs to this organization.
    pub organization: Option<String>,
}

fn default_outbox_limit() -> usize {
    50
}

/// Shared `?organization=` filter for sources / teams list endpoints.
#[derive(Debug, Deserialize, Default)]
pub struct OrganizationQuery {
    pub organization: Option<String>,
}

/// Resolve optional org scope: when provided, require at least Viewer on that org.
fn require_org_viewer(
    state: &AppState,
    headers: &HeaderMap,
    organization: Option<&str>,
) -> Result<Option<String>, ApiError> {
    let org = organization
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    if let Some(ref org_id) = org {
        let principal = resolve_auth_principal(
            headers,
            &state.api,
            state.oidc.as_deref(),
            &state.engine.store,
        )?;
        authorize_for_org(&principal, AppRole::Viewer, Some(org_id))?;
    }
    Ok(org)
}

fn namespaced_belongs_to_org(namespaced: &str, organization: Option<&str>) -> bool {
    match organization {
        None => true,
        Some(org) => organization_id_from_source_id(namespaced) == Some(org),
    }
}

fn watch_matches_org(watch: &crate::pipeline::Watch, organization: Option<&str>) -> bool {
    match organization {
        None => true,
        Some(org) => watch.organization_id.as_deref() == Some(org),
    }
}

/// `GET /api/v1/status`
///
/// Bearer auth is enforced by the `require_management_auth` router layer
/// (see [`super::router`]), not here.
pub async fn get_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<StatusResponse>, ApiError> {
    let database_ok = state.engine.store.health().is_ok();
    let outbox_pending = state.engine.store.outbox_pending_count()?;
    let outbox = state.engine.store.outbox_counts()?;
    let breakers = state.engine.notifier_snapshot().await.breaker_states();
    // Across every stream: a multi-org admin's status banner must surface any
    // organization's failed push, not only the legacy single-document stream.
    let rejected = state.engine.store.latest_rejected_config_revision_any()?;
    let multi_org = !state.bootstrap.organizations.is_empty();
    let (revision, sources_configured) = {
        let desired = state.desired.read().await;
        (
            desired.effective_revision.clone(),
            desired.index.watches().len(),
        )
    };
    let routes = state.config_api.routes_enabled();
    let desired_source = if multi_org {
        // No single global document — per-org streams live under /organizations/…
        DesiredSource::Empty
    } else {
        desired_source_from_revision(revision.as_ref())
    };

    Ok(Json(StatusResponse {
        version: env!("CARGO_PKG_VERSION"),
        uptime_secs: state.started_at.elapsed().as_secs(),
        database_ok,
        sources_configured,
        outbox_pending,
        outbox_dead: outbox.dead,
        outbox_deferred: outbox.deferred,
        breakers,
        metrics: state.engine.metrics.snapshot(),
        config_apply: ConfigApplyStatus {
            api_config_enabled: routes,
            ui_config_enabled: state.config_api.ui_config_enabled(),
            config_source: state.config_api.source,
            hmac_configured: state.config_api.hmac_configured(),
            desired_source,
            bootstrap_path: state.config_paths.bootstrap.display().to_string(),
            app_config_path: state
                .config_paths
                .app
                .as_ref()
                .map(|path| path.display().to_string()),
            split_config: state.config_paths.has_app_file(),
            revision: revision.as_ref().map(|r| r.id),
            content_sha256: revision.as_ref().map(|r| r.content_sha256.clone()),
            revision_label: revision.as_ref().and_then(|r| r.revision_label.clone()),
            applied_at: revision.as_ref().map(|r| r.applied_at.clone()),
            last_rejected_revision: rejected.as_ref().map(|r| r.id),
            last_rejected_error: rejected.and_then(|r| r.error),
        },
    }))
}

/// `GET /api/v1/outbox`
pub async fn list_outbox(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<OutboxQuery>,
) -> Result<Json<OutboxListResponse>, ApiError> {
    let organization = require_org_viewer(&state, &headers, query.organization.as_deref())?;
    // Over-fetch when org-scoped so the post-filter still fills near `limit`.
    let fetch_limit = if organization.is_some() {
        200
    } else {
        query.limit.clamp(1, 200)
    };
    let limit = query.limit.clamp(1, 200);
    let entries = state
        .engine
        .store
        .list_outbox_entries(fetch_limit)?
        .into_iter()
        .filter(|entry| namespaced_belongs_to_org(&entry.source_id, organization.as_deref()))
        .take(limit)
        .collect();

    Ok(Json(OutboxListResponse { entries }))
}

/// `POST /api/v1/outbox/requeue` — revive dead-letter outbox rows.
#[derive(Debug, Serialize)]
pub struct OutboxRequeueResponse {
    pub requeued: usize,
}

/// `POST /api/v1/outbox/requeue`
///
/// Same effect as CLI `xrelease outbox requeue`: reset `dead` rows to
/// `pending` so the flush loop can retry after a sink outage or routing fix.
pub async fn requeue_outbox(
    State(state): State<Arc<AppState>>,
) -> Result<Json<OutboxRequeueResponse>, ApiError> {
    let requeued = state.engine.store.requeue_dead_outbox()?;
    state.engine.metrics.record_outbox_requeued(requeued);
    tracing::info!(requeued, "API outbox requeue");
    Ok(Json(OutboxRequeueResponse { requeued }))
}

/// One team-routing group enriched with the number of sources routing to it.
#[derive(Debug, Serialize)]
pub struct TeamView {
    pub tag: String,
    pub name: Option<String>,
    pub source_count: usize,
}

/// `GET /api/v1/teams` — routing catalogue for human-readable team labels.
#[derive(Debug, Serialize)]
pub struct TeamListResponse {
    pub teams: Vec<TeamView>,
}

/// `GET /api/v1/teams`
///
/// Returns the `[[teams]]` catalogue (tag + human-readable name) merged with
/// per-team source counts. Tags used by sources but absent from the catalogue
/// are appended (sorted) with `name = null` so the UI can still label them.
pub async fn list_teams(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<OrganizationQuery>,
) -> Result<Json<TeamListResponse>, ApiError> {
    let organization = require_org_viewer(&state, &headers, query.organization.as_deref())?;
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let desired = state.desired.read().await;
    for watch in desired.index.watches() {
        if !watch_matches_org(watch, organization.as_deref()) {
            continue;
        }
        if let Some(tag) = watch
            .routing_tag
            .as_deref()
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
        {
            *counts.entry(tag.to_owned()).or_default() += 1;
        }
    }

    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut teams: Vec<TeamView> = desired
        .teams
        .iter()
        .filter(|team| namespaced_belongs_to_org(&team.tag, organization.as_deref()))
        .map(|team| {
            seen.insert(team.tag.as_str());
            TeamView {
                source_count: counts.get(&team.tag).copied().unwrap_or(0),
                tag: team.tag.clone(),
                name: team.name.clone(),
            }
        })
        .collect();

    let mut untracked: Vec<TeamView> = counts
        .iter()
        .filter(|(tag, _)| !seen.contains(tag.as_str()))
        .map(|(tag, count)| TeamView {
            tag: tag.clone(),
            name: None,
            source_count: *count,
        })
        .collect();
    untracked.sort_by(|a, b| a.tag.cmp(&b.tag));
    teams.append(&mut untracked);

    Ok(Json(TeamListResponse { teams }))
}

/// `GET /api/v1/sources/{id}` — one source with runtime state (UI detail page).
pub async fn get_source(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(source_id): Path<String>,
) -> Result<Json<SourceDetail>, ApiError> {
    let detail = source_detail(&state, &source_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("source `{source_id}` not found")))?;
    // Multi-org: require at least Viewer on the owning organization (same gate
    // as `?organization=` on the list endpoint).
    if let Some(org) = detail.config.organization_id.as_deref() {
        require_org_viewer(&state, &headers, Some(org))?;
    }
    Ok(Json(detail))
}

/// `GET /api/v1/sources` — config + live runtime state (read-only UI).
///
/// Bearer auth is enforced by the `require_management_auth` router layer
/// (see [`super::router`]), not here.
pub async fn list_sources(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<OrganizationQuery>,
) -> Result<Json<Vec<SourceDetail>>, ApiError> {
    let organization = require_org_viewer(&state, &headers, query.organization.as_deref())?;
    let details = source_details(&state).await?;
    let details = match organization.as_deref() {
        None => details,
        Some(org) => details
            .into_iter()
            .filter(|detail| detail.config.organization_id.as_deref() == Some(org))
            .collect(),
    };
    Ok(Json(details))
}

/// Build enriched source rows for `GET /api/v1/sources`.
pub async fn source_details(
    state: &AppState,
) -> Result<Vec<SourceDetail>, crate::error::StoreError> {
    let states = state.engine.store.all_source_states()?;
    let seen_counts = state.engine.store.seen_counts_by_source()?;
    let desired = state.desired.read().await;

    desired
        .index
        .watches()
        .iter()
        .map(|watch| {
            let id = watch.provider.id();
            let runtime = states.get(id).cloned().unwrap_or_default();
            let seen_count = seen_counts.get(id).copied().unwrap_or(0);
            source_detail_from_watch(state, watch, runtime, seen_count)
        })
        .collect::<Result<Vec<_>, _>>()
}

/// Look up one configured source by stable id.
pub async fn source_detail(
    state: &AppState,
    source_id: &str,
) -> Result<Option<SourceDetail>, crate::error::StoreError> {
    let watch = {
        let desired = state.desired.read().await;
        desired
            .index
            .watches()
            .iter()
            .find(|watch| watch.provider.id() == source_id)
            .cloned()
    };
    let Some(watch) = watch else {
        return Ok(None);
    };
    // Point lookups — avoid scanning every source_state / seen_release group.
    let runtime = state.engine.store.source_state_row(source_id)?;
    let seen_count = state.engine.store.seen_count(source_id)?;
    source_detail_from_watch(state, &watch, runtime, seen_count).map(Some)
}

fn source_detail_from_watch(
    state: &AppState,
    watch: &crate::pipeline::Watch,
    runtime: crate::store::SourceStateRow,
    seen_count: u64,
) -> Result<SourceDetail, crate::error::StoreError> {
    let config: SourceSummary = watch.summary();
    let id = config.id.clone();
    let latest_release_tag = effective_latest_release_tag(
        &state.engine.store,
        &id,
        runtime.latest_release_tag.as_ref(),
    );
    let seen_releases = state
        .engine
        .store
        .list_seen_releases(&id, 200)?
        .into_iter()
        .map(|entry| {
            let tag = entry.tag.clone();
            crate::runtime::SeenReleaseView {
                published_at: entry.published_at,
                url: entry.url.or_else(|| watch.provider.release_page_url(&tag)),
                first_seen_at: entry.first_seen_at,
                tag,
            }
        })
        .collect();
    Ok(SourceDetail {
        source_page_url: watch.provider.source_page_url(),
        initialized: runtime.initialized,
        last_polled_at: runtime.last_polled_at,
        seen_count,
        latest_release_tag,
        seen_releases,
        metrics: state.engine.metrics.source_stats(&id),
        config,
    })
}

fn effective_latest_release_tag(
    store: &crate::store::Store,
    source_id: &str,
    stored: Option<&String>,
) -> Option<String> {
    if let Some(tag) = stored {
        return Some(tag.clone());
    }
    store.best_seen_identity(source_id).ok().flatten()
}
