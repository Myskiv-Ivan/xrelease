//! Per-organization config API (`/api/v1/organizations/…`).
//!
//! Every route addresses ONE organization's desired-state document and ledger
//! stream. The runtime itself is always the composition of every org, so an
//! apply here recomposes the whole runtime with this org's candidate
//! substituted — through the same [`compose_organizations`] path boot and
//! reload use — and hot-swaps it atomically under the global `apply_lock`.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use tracing::info;

use super::auth;
use super::config::{check_if_match, RevisionsQuery};
use super::error::ApiError;
use super::role::AppRole;
use super::AppState;
use crate::config::{
    detect_desired_format, organization_desired_raw, parse_desired_document, DesiredSource,
    OrganizationConfig,
};
use crate::config_apply::{
    apply_document, content_sha256, redact_desired_only_document, rollback, validate_document,
    ApplyResponse, ApplyScope, ConfigSwapper, ValidateResponse,
};

/// `GET /api/v1/organizations` — the catalogue, for the UI switcher and `xrctl`.
pub async fn list_organizations(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let bootstrap = state.bootstrap.as_ref();
    let desired = state.desired.read().await;

    let organizations: Vec<serde_json::Value> = bootstrap
        .organizations
        .iter()
        .map(|org| {
            let id = &org.id.clone();
            let source_count = desired
                .index
                .watches()
                .iter()
                .filter(|watch| watch.organization_id.as_deref() == Some(&id))
                .count();
            json!({
            "id": id,
            "name": org.display_name(),
            "source_count": source_count,
            })
        })
        .collect();

    Ok(Json(json!({
    "organizations": organizations,
    "config_source": bootstrap.config_api.source,
    "api_config_enabled": bootstrap.config_api.routes_enabled(),
    "ui_config_enabled": bootstrap.config_api.ui_config_enabled(),
    })))
}

/// `GET /api/v1/organizations/{id}/config` — this org's desired document from
/// its authority (ledger stream or file), secrets redacted, with an `ETag` of
/// the raw document for `If-Match` on the apply route.
///
/// No per-org authorization on purpose: the router's `require_management_auth`
/// layer authenticates the bearer, and every authenticated principal holds at
/// least `viewer` for every organization (`role_for_org` floors at the global
/// role; api-key is admin everywhere) — same policy as the whole-instance
/// `GET /api/v1/config`, and reads return redacted documents only. Org-scoped
/// checks guard the MUTATION routes (operator to validate, admin to apply).
pub async fn get_org_config(
    State(state): State<Arc<AppState>>,
    Path(org_id): Path<String>,
) -> Result<Response, ApiError> {
    let bootstrap = state.bootstrap.as_ref();
    let org = require_organization(bootstrap, &org_id)?;
    let (raw, source) = org_authority_raw(&state, bootstrap, &org)?;
    let sha = content_sha256(&raw);

    let desired_content = parse_desired_document(&raw)
        .ok()
        .map(|parsed| redact_desired_only_document(detect_desired_format(&raw), &parsed));

    let body = json!({
    "organization": org.id,
    "name": org.display_name(),
    "desired_source": source,
    "content_sha256": sha,
    "desired_content": desired_content,
    "app_path": org.app_path(),
    "config_source": bootstrap.config_api.source,
    "accepts_pushed_config": bootstrap.config_api.accepts_pushed_config(),
    "api_config_enabled": bootstrap.config_api.routes_enabled(),
    "ui_config_enabled": bootstrap.config_api.ui_config_enabled(),
    });

    let etag = format!("\"{sha}\"");
    match HeaderValue::from_str(&etag) {
        Ok(value) => Ok(([(axum::http::header::ETAG, value)], Json(body)).into_response()),
        // A sha is always header-safe hex; fall back rather than fail the read.
        Err(_) => Ok(Json(body).into_response()),
    }
}

/// `GET /api/v1/organizations/{id}/config/revisions` — this org's ledger stream.
///
/// Viewer-readable by any authenticated principal, like [`get_org_config`]
/// (metadata only — [`ConfigRevisionSummary`](crate::store::ConfigRevisionSummary)
/// omits document content).
pub async fn get_org_revisions(
    State(state): State<Arc<AppState>>,
    Path(org_id): Path<String>,
    Query(query): Query<RevisionsQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let bootstrap = state.bootstrap.as_ref();
    let org = require_organization(bootstrap, &org_id)?;
    let (limit, offset) = query.window();

    let revisions = state
        .engine
        .store
        .list_config_revisions_for(&org.id, limit, offset)?;
    let total = state.engine.store.count_config_revisions_for(&org.id)?;

    Ok(Json(json!({
    "organization": org.id,
    "revisions": revisions,
    "total": total,
    "limit": limit,
    "offset": offset,
    })))
}

/// `POST /api/v1/organizations/{id}/config/validate` — dry-run one org's
/// candidate document against the FULL composed runtime (cross-org id
/// collisions and routing mistakes surface here, not at apply time).
pub async fn post_org_validate(
    State(state): State<Arc<AppState>>,
    Path(org_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<ValidateResponse>, ApiError> {
    let raw = std::str::from_utf8(&body)
        .map_err(|_| ApiError::BadRequest("config body must be valid UTF-8".into()))?;
    let bootstrap = state.bootstrap.as_ref();
    let org = require_organization(bootstrap, &org_id)?;
    // Org-scoped authZ: validating (dry-run) one org's document requires at
    // least `operator` FOR THAT ORG. Done in the handler (not a router layer)
    // because only here is the path org id known.
    let principal = auth::resolve_auth_principal(
        &headers,
        &state.api,
        state.oidc.as_deref(),
        &state.engine.store,
    )?;
    auth::authorize_for_org(&principal, AppRole::Operator, Some(&org.id))?;
    let format_hint = super::config::desired_format_from_headers(&headers);
    let scope = ApplyScope::Organization {
        organization: &org.id,
        paths: &state.config_paths,
    };
    // Match Apply: restore secrets the UI omitted after GET redaction.
    let previous = state.current_config().await;
    let response = validate_document(
        bootstrap,
        raw,
        false,
        format_hint,
        &state.engine.store,
        &scope,
        Some(&previous),
    )
    .map_err(super::config::validation_error)?;
    Ok(Json(response))
}

/// `POST /api/v1/organizations/{id}/config/apply` — replace one org's desired
/// document: validate the recomposed runtime, hot-swap it, append to the org's
/// ledger stream.
pub async fn post_org_apply(
    State(state): State<Arc<AppState>>,
    Path(org_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<ApplyResponse>, ApiError> {
    let raw = std::str::from_utf8(&body)
        .map_err(|_| ApiError::BadRequest("config body must be valid UTF-8".into()))?;
    let bootstrap = state.bootstrap.as_ref();
    let org = require_organization(bootstrap, &org_id)?;
    // Requires `admin` FOR THIS ORG (org-scoped), + HMAC + push-authority.
    let principal = auth::require_config_apply_auth(&state, &headers, &body, Some(&org.id))?;
    require_pushes_accepted(bootstrap)?;
    let origin = super::config::apply_origin_from_headers(&headers, &principal);
    let format_hint = super::config::desired_format_from_headers(&headers);

    // Serialize with every other hot-swap (whole-doc apply, reload, rollback).
    let _apply_guard = state.apply_lock.lock().await;

    // Optimistic concurrency against THIS org's authority document, inside the
    // lock — same guarantee as the whole-document route, per stream.
    let (current_raw, _) = org_authority_raw(&state, bootstrap, &org)?;
    let current_sha = content_sha256(&current_raw);
    check_if_match(
        &headers,
        Some(current_sha.as_str()),
        &format!("GET /api/v1/organizations/{}/config", org.id),
    )?;

    let scope = ApplyScope::Organization {
        organization: &org.id,
        paths: &state.config_paths,
    };
    let result = apply_document(
        bootstrap,
        raw,
        format_hint,
        &origin,
        &state.engine.store,
        state.as_ref(),
        &scope,
    )
    .await;
    finish_org_apply(&state, &org.id, result).await
}

/// `POST /api/v1/organizations/{id}/config/rollback` — re-apply the previous
/// applied revision of this org's ledger stream.
pub async fn post_org_rollback(
    State(state): State<Arc<AppState>>,
    Path(org_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ApplyResponse>, ApiError> {
    let bootstrap = state.bootstrap.as_ref();
    let org = require_organization(bootstrap, &org_id)?;
    let principal = auth::require_config_apply_auth(&state, &headers, b"", Some(&org.id))?;
    require_pushes_accepted(bootstrap)?;
    let origin = super::config::apply_origin_from_headers(&headers, &principal);

    let _apply_guard = state.apply_lock.lock().await;
    let scope = ApplyScope::Organization {
        organization: &org.id,
        paths: &state.config_paths,
    };
    let result = rollback(
        bootstrap,
        &origin,
        &state.engine.store,
        state.as_ref(),
        &scope,
    )
    .await;
    finish_org_apply(&state, &org.id, result).await
}

/// Resolve the addressed organization or answer 404.
///
/// The catalogue is bootstrap-only, so an unknown id is a client mistake, not
/// a race with a concurrent apply (applies cannot add organizations).
fn require_organization(
    bootstrap: &crate::config::Config,
    org_id: &str,
) -> Result<OrganizationConfig, ApiError> {
    if bootstrap.organizations.is_empty() {
        return Err(ApiError::NotFound(
            "no [[organizations]] declared in bootstrap.toml \
 (single-document instance; use /api/v1/config)"
                .into(),
        ));
    }
    let wanted = org_id.trim();
    bootstrap
        .organizations
        .iter()
        .find(|org| org.id == wanted)
        .cloned()
        .ok_or_else(|| ApiError::NotFound(format!("unknown organization `{wanted}`")))
}

/// Refuse pushes on a Git-pinned instance (`[config_api].source = "local"`).
fn require_pushes_accepted(bootstrap: &crate::config::Config) -> Result<(), ApiError> {
    if bootstrap.config_api.accepts_pushed_config() {
        return Ok(());
    }
    Err(ApiError::Conflict(
        "[config_api].source = \"local\" pins desired state to the app files; \
 edit the organization's file in Git and POST /api/v1/reload (or send SIGHUP)"
            .into(),
    ))
}

/// One org's current authority document (ledger stream or file).
fn org_authority_raw(
    state: &AppState,
    bootstrap: &crate::config::Config,
    org: &OrganizationConfig,
) -> Result<(String, DesiredSource), ApiError> {
    organization_desired_raw(
        bootstrap,
        &state.config_paths,
        Some(&state.engine.store),
        org,
    )
    .map_err(ApiError::internal)
}

/// Publish + count + log one org apply/rollback outcome.
///
/// The org counterpart of `AppState::finish_apply`, minus `commit_effective`:
/// `effective_desired_sha` holds the *composed* multi-org identity, not one
/// document's sha, so a successful org apply **invalidates** it instead
/// (the next reload recomputes it from every org's authority).
async fn finish_org_apply(
    state: &AppState,
    organization: &str,
    result: anyhow::Result<ApplyResponse>,
) -> Result<Json<ApplyResponse>, ApiError> {
    match result {
        Ok(response) => {
            if response.applied {
                state.desired.write().await.effective_desired_sha = None;
                state.engine.metrics.record_config_apply();
            }
            info!(
            organization,
            revision = response.revision,
            applied = response.applied,
            sha = %response.content_sha256,
            "organization config apply completed"
            );
            Ok(Json(response))
        }
        Err(err) => {
            if err
                .downcast_ref::<crate::config_apply::ValidationRejected>()
                .is_some()
            {
                state.engine.metrics.record_config_apply_rejected();
            }
            Err(super::config::validation_error(err))
        }
    }
}
