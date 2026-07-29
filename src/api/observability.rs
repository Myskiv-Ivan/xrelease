//! Read-only observability endpoints for dashboard UI (SvelteKit, etc.).

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use futures_util::future::join_all;
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

/// Advisory-enrichment status for the dashboard.
///
/// Without this the UI cannot tell an empty advisory column apart from a
/// disabled feature: "no known CVEs", "enrichment is off", and "this source is
/// not a package registry" all render identically. Exposing the toggle lets the
/// UI say which one it is.
#[derive(Debug, Serialize)]
pub struct AdvisoryStatus {
    /// Whether `[advisories]` is on **and** has a usable endpoint.
    pub enabled: bool,
    /// Advisory database being queried — surfaced so an operator can confirm a
    /// self-hosted OSV mirror is actually in use. Never carries credentials.
    pub endpoint: String,
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
    pub advisories: AdvisoryStatus,
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
    // One read for all four numbers: `outbox_pending_count` is itself
    // `outbox_counts().pending + .failed`, so asking for both ran the two
    // aggregate queries twice on an endpoint the dashboard polls.
    let outbox = state.engine.store.outbox_counts()?;
    let outbox_pending = outbox.pending + outbox.failed;
    let breakers = state.engine.notifier_snapshot().await.breaker_states();
    // Across every stream: a multi-org admin's status banner must surface any
    // organization's failed push, not only the single-document stream.
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
        // Read from `bootstrap`, not the merged desired config: `[advisories]`
        // is bootstrap-only (`contains_infra_sections` rejects it in a pushed
        // document), so bootstrap is the authority.
        advisories: AdvisoryStatus {
            enabled: state.bootstrap.advisories.active(),
            endpoint: state.bootstrap.advisories.endpoint_base().to_owned(),
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
///
/// Every read here is batched across the whole watch list — three queries
/// total, no matter how many sources are configured. `seen_releases` carries
/// exactly the one entry the table's "last release" column needs (see
/// [`crate::store::Store::latest_seen_by_source`]); listing a source's whole
/// catalogue on this page meant one unbounded scan per source and a response
/// holding `sources × 200` release objects that nothing rendered.
///
/// Advisories stay off this endpoint for the same reason: the table shows a
/// severity strip only on the detail page, so paying for them here would be
/// pure waste. [`source_detail`] opts in via `include_advisories`.
pub async fn source_details(
    state: &AppState,
) -> Result<Vec<SourceDetail>, crate::error::StoreError> {
    let states = state.engine.store.all_source_states()?;
    let seen_counts = state.engine.store.seen_counts_by_source()?;
    let mut latest_seen = state.engine.store.latest_seen_by_source()?;
    let desired = state.desired.read().await;

    desired
        .index
        .watches()
        .iter()
        .map(|watch| {
            let id = watch.provider.id();
            let runtime = states.get(id).cloned().unwrap_or_default();
            let seen_count = seen_counts.get(id).copied().unwrap_or(0);
            // Removed, not cloned: each source appears once in the watch list,
            // so nothing reads its entry again.
            let seen = latest_seen.remove(id).into_iter().collect();
            source_detail_from_watch(state, watch, runtime, seen_count, seen, false)
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
    // The full catalogue (newest first, capped) — this is the page that
    // actually lists releases one by one.
    let seen = state.engine.store.list_seen_releases(source_id, 200)?;
    let mut detail = source_detail_from_watch(state, &watch, runtime, seen_count, seen, true)?;
    // Delivery-time enrichment only fills `release_advisory` for versions that
    // actually notified. Baseline polls and older seen rows stay uncached, so the
    // detail page would otherwise show an empty Advisories column forever. Cap
    // the live backfill so a large catalogue cannot stall the request.
    if state.engine.advisories.active() {
        backfill_advisories_for_detail(state, &mut detail).await;
    }
    Ok(Some(detail))
}

/// Assemble one row from data the caller already fetched.
///
/// `seen` is supplied rather than read here so each caller can pay only for
/// what it renders: the list endpoint passes the single batched "latest" entry,
/// the detail endpoint the capped catalogue.
fn source_detail_from_watch(
    state: &AppState,
    watch: &crate::pipeline::Watch,
    runtime: crate::store::SourceStateRow,
    seen_count: u64,
    seen: Vec<crate::store::SeenReleaseEntry>,
    include_advisories: bool,
) -> Result<SourceDetail, crate::error::StoreError> {
    let config: SourceSummary = watch.summary();
    let id = config.id.clone();
    let latest_release_tag =
        effective_latest_release_tag(runtime.latest_release_tag.as_ref(), &seen);
    let mut advisories = if include_advisories {
        advisories_by_identity(
            &state.engine.store,
            &id,
            &config.kind,
            &config.display_name,
            &seen,
        )?
    } else {
        HashMap::new()
    };
    let seen_releases = seen
        .into_iter()
        .map(|entry| {
            let tag = entry.tag.clone();
            let advisories = advisories.remove(&entry.identity).unwrap_or_default();
            crate::runtime::SeenReleaseView {
                published_at: entry.published_at,
                url: entry.url.or_else(|| watch.provider.release_page_url(&tag)),
                first_seen_at: entry.first_seen_at,
                advisories,
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

/// Max OSV lookups on one source-detail request for versions never checked
/// before. Newest-first (`list_seen_releases` order).
///
/// Small on purpose: this runs inline on a page load. Because every verified
/// answer is remembered in `advisory_check`, successive requests keep starting
/// where the last one stopped, so a source with hundreds of synced releases is
/// covered in a handful of page loads rather than never.
const DETAIL_ADVISORY_BACKFILL: usize = 5;

/// Live-fill a few uncached package versions so the Advisories column is not
/// stuck empty after a silent baseline. Best-effort: lookup failures leave the
/// response unchanged (same contract as delivery enrichment).
async fn backfill_advisories_for_detail(state: &AppState, detail: &mut SourceDetail) {
    let Some((registry, name)) = crate::advisory::coordinate_from_source(
        &detail.config.id,
        &detail.config.kind,
        &detail.config.display_name,
    ) else {
        return;
    };
    let Some(ecosystem) = crate::advisory::Ecosystem::for_registry(registry) else {
        return;
    };

    // Positions, not versions: [`source_detail_from_watch`] has already read
    // every persisted advisory for this source out of `release_advisory` (see
    // `advisories_by_identity`), so a release still empty here either has no
    // findings or was never looked up. `advisory_check` is what separates
    // those two, and it is the reason this makes progress: without it, every
    // request would spend its whole budget on the same newest already-clean
    // releases and never reach the ones below them.
    //
    // Keying by position also sidesteps the identity/tag split: the earlier
    // read is keyed by `seen_release.identity` while a `SeenReleaseView` only
    // carries `tag`. They coincide for every package source (a package
    // `Release` uses the version as both), which is the only kind that gets
    // here, but relying on that to *match rows up* would be a trap for a
    // future source type.
    let empty: Vec<usize> = detail
        .seen_releases
        .iter()
        .enumerate()
        .filter(|(_, release)| release.advisories.is_empty())
        .map(|(index, _)| index)
        .collect();
    if empty.is_empty() {
        return;
    }

    // One query for the whole page, not one per candidate: the set of
    // already-checked versions is what turns the `take` below into "the next
    // few *unseen* versions" instead of "the same few every time".
    let candidates: Vec<&str> = empty
        .iter()
        .map(|&index| detail.seen_releases[index].tag.as_str())
        .collect();
    let checked = match state
        .engine
        .store
        .checked_versions(ecosystem.as_str(), name, &candidates)
    {
        Ok(checked) => checked,
        Err(err) => {
            tracing::debug!(
                source_id = %detail.config.id,
                error = %err,
                "advisory backfill skipped — check-state read failed"
            );
            return;
        }
    };

    let pending: Vec<usize> = empty
        .into_iter()
        .filter(|&index| !checked.contains(&detail.seen_releases[index].tag))
        .take(DETAIL_ADVISORY_BACKFILL)
        .collect();

    // Concurrently, not one after another. These are independent lookups of
    // distinct versions, and each already carries its own
    // `[advisories].timeout_secs` deadline inside reqwest — so running them
    // together bounds the whole batch at roughly *one* timeout instead of
    // stacking `DETAIL_ADVISORY_BACKFILL × timeout` (~25s at the defaults) onto
    // a page load. That stacking is why this used to need a separate wall-clock
    // budget; with the requests in flight together the per-request deadline is
    // the budget.
    //
    // Versions are cloned out first so nothing borrows `detail.seen_releases`
    // across the await — the results are applied by index afterwards.
    let batch: Vec<(usize, String)> = pending
        .into_iter()
        .map(|index| (index, detail.seen_releases[index].tag.clone()))
        .collect();
    let looked_up = join_all(batch.into_iter().map(|(index, version)| async move {
        let outcome = state
            .engine
            .advisories
            .lookup(registry, name, &version)
            .await;
        (index, version, outcome)
    }))
    .await;

    for (index, version, outcome) in looked_up {
        // Only a real OSV answer is worth remembering. Marking a failed or
        // breaker-skipped lookup as checked would silently retire that version
        // from every future backfill — it would read as "confirmed clean"
        // forever without OSV ever having been asked.
        if !outcome.verified {
            continue;
        }
        crate::pipeline::persist_advisory_result(
            &state.engine,
            ecosystem.as_str(),
            name,
            &version,
            &outcome.advisories,
        );
        detail.seen_releases[index].advisories =
            outcome.advisories.into_iter().map(Into::into).collect();
    }
}

/// Batch-fetch persisted advisories for every seen release of one source,
/// keyed by **identity** (the version string used at record / OSV lookup time).
///
/// Returns an empty map — never an error — for any non-package source or a
/// source with nothing seen yet: advisories are cosmetic here, never
/// load-bearing for this endpoint (same contract as [`crate::advisory`]).
fn advisories_by_identity(
    store: &crate::store::Store,
    source_id: &str,
    kind: &str,
    package_name: &str,
    seen: &[crate::store::SeenReleaseEntry],
) -> Result<HashMap<String, Vec<crate::runtime::AdvisoryView>>, crate::error::StoreError> {
    if seen.is_empty() {
        return Ok(HashMap::new());
    }
    let Some((registry, name)) =
        crate::advisory::coordinate_from_source(source_id, kind, package_name)
    else {
        return Ok(HashMap::new());
    };
    let Some(ecosystem) = crate::advisory::Ecosystem::for_registry(registry) else {
        return Ok(HashMap::new());
    };
    let versions: Vec<&str> = seen.iter().map(|entry| entry.identity.as_str()).collect();
    let by_version = store.advisories_for_versions(ecosystem.as_str(), name, &versions)?;
    Ok(by_version
        .into_iter()
        .map(|(version, advisories)| (version, advisories.into_iter().map(Into::into).collect()))
        .collect())
}

/// Tag for the "latest release" column: the stored pick, else the newest entry
/// already in hand.
///
/// The fallback costs no query. `seen` arrives newest-first under the same
/// comparison `set_latest_release_tag` used to choose the stored value, so its
/// first entry is what a `best_seen_identity` round trip would have returned —
/// and on the list endpoint that was one more unbounded scan per source. It is
/// also the better answer for feed sources, where the stored identity is an
/// opaque GUID and this is the human-readable title.
fn effective_latest_release_tag(
    stored: Option<&String>,
    seen: &[crate::store::SeenReleaseEntry],
) -> Option<String> {
    stored
        .cloned()
        .or_else(|| seen.first().map(|entry| entry.tag.clone()))
}
