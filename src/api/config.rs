//! Config apply / validate / rollback / read handlers.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use tracing::info;

use super::auth;
use super::error::ApiError;
use super::AppState;
use crate::config::{
    desired_source_from_revision, detect_desired_format, load_desired_file, parse_desired_document,
    DesiredFormat,
};
use crate::config_apply::ConfigSwapper;
use crate::config_apply::{
    apply_document, redact_config_toml, redact_desired_only_document, reload_document,
    reload_organizations, rollback, validate_document, ApplyOrigin, ApplyResponse, ApplyScope,
    DocumentRejected, NothingToRollBackTo, ReloadResponse, ValidateResponse, ValidationRejected,
};

/// `GET /api/v1/config` — effective running config (secrets redacted).
///
/// Carries an `ETag` of the running desired document so a caller can edit it
/// and apply back with `If-Match`, and be rejected (412) instead of silently
/// clobbering whoever wrote in between.
pub async fn get_config(State(state): State<Arc<AppState>>) -> Result<Response, ApiError> {
    // Snapshot under the desired lock, then drop it before ledger I/O so a
    // concurrent apply is not blocked on a redacted-document read.
    let (source, desired_sha, content, revision, ui_meta, multi_org) = {
        let bootstrap = state.bootstrap.as_ref();
        let desired = state.desired.read().await;
        let revision = desired.effective_revision.clone();
        let source = desired_source_from_revision(revision.as_ref());
        let desired_sha = desired.effective_desired_sha.clone();
        let content = redact_config_toml(&desired.effective);
        let ui_meta = (
            bootstrap.config_api.routes_enabled(),
            bootstrap.config_api.ui_config_enabled(),
            bootstrap.config_api.source,
        );
        let multi_org = !bootstrap.organizations.is_empty();
        (source, desired_sha, content, revision, ui_meta, multi_org)
    };
    let (api_config_enabled, ui_config_enabled, config_source) = ui_meta;

    // Multi-org: the ignored root `app/releases.yaml` must not appear as
    // `desired_content` (Overview graph / Technical would show phantom sources).
    // Per-org documents live under `/api/v1/organizations/{id}/config`.
    // Catalogue authority is also not a single global app file — surface `empty`
    // so the UI does not imply GitOps file mode.
    let (desired_content, source) = if multi_org {
        (None, crate::config::DesiredSource::Empty)
    } else {
        let content = match source {
            crate::config::DesiredSource::Ledger => state
                .engine
                .store
                .latest_applied_config_revision(None)?
                .and_then(|record| {
                    parse_desired_document(&record.content).ok().map(|parsed| {
                        redact_desired_only_document(
                            detect_desired_format(&record.content),
                            &parsed,
                        )
                    })
                }),
            crate::config::DesiredSource::AppFile => {
                state.config_paths.app.as_ref().and_then(|path| {
                    load_desired_file(path).ok().map(|parsed| {
                        let format = crate::config::format_from_path(path.as_path())
                            .unwrap_or(DesiredFormat::Yaml);
                        redact_desired_only_document(format, &parsed)
                    })
                })
            }
            crate::config::DesiredSource::Empty => {
                Some(crate::config::EMPTY_ORGANIZATION_DOCUMENT.to_owned())
            }
        };
        (content, source)
    };

    let body = json!({
     "content": content,
     "desired_content": desired_content,
     "desired_source": source,
     "content_sha256": desired_sha,
     "revision": revision.as_ref().map(|r| r.id),
     "revision_label": revision.as_ref().and_then(|r| r.revision_label.clone()),
     "applied_at": revision.as_ref().map(|r| r.applied_at.clone()),
     "bootstrap_path": state.config_paths.bootstrap.display().to_string(),
     "app_config_path": state
    .config_paths
    .app
    .as_ref()
    .map(|path| path.display().to_string()),
     "split_config": state.config_paths.has_app_file(),
     "api_config_enabled": api_config_enabled,
     // The dashboard reads this to decide whether to render an editor at
     // all, so one frontend build serves every mode.
     "ui_config_enabled": ui_config_enabled,
     "config_source": config_source,
     "bootstrap_sections": ["database", "api", "log", "config_api", "advisories", "organizations"],
     });

    match desired_sha {
        Some(sha) => {
            let etag = format!("\"{sha}\"");
            match HeaderValue::from_str(&etag) {
                Ok(value) => Ok(([(axum::http::header::ETAG, value)], Json(body)).into_response()),
                // A sha is always header-safe hex; fall back rather than fail
                // the whole read if that ever stops holding.
                Err(_) => Ok(Json(body).into_response()),
            }
        }
        None => Ok(Json(body).into_response()),
    }
}

/// `GET /api/v1/config/schema` — what the desired-state config accepts.
///
/// Lets a UI build **controls** (source-kind select, sink picker, team-tag
/// multi-select, schedule widget) instead of a raw config text box, without
/// hardcoding option lists that drift from the Rust enums. Two parts of this
/// only the server can answer: which broker sinks this binary actually
/// compiled in, and which team tags this deployment defines.
pub async fn get_schema(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let desired = state.desired.read().await;
    let bootstrap = state.bootstrap.as_ref();
    let team_tags: Vec<&str> = desired
        .effective
        .teams
        .iter()
        .map(|team| team.tag.as_str())
        .collect();

    let effective_presets = crate::config::effective_source_presets(&desired.effective.presets);

    Ok(Json(json!({
    "source_kinds": crate::config::SOURCE_KINDS,
    "sink_kinds": crate::config::sink_kinds(),
    "smtp_tls_modes": crate::config::SMTP_TLS_MODES,
    "webhook_methods": crate::config::WEBHOOK_METHODS,
    "team_tags": team_tags,
    "preset_names": effective_presets.keys().cloned().collect::<Vec<_>>(),
    "builtin_presets": crate::config::builtin_preset_schemas(),
    "source_common_fields": [
    "preset",
    "include_prerelease",
    "prerelease_tags",
    "exclude_updated",
    "pattern",
    "exclude_pattern",
    "routing_tag",
    "interval_secs",
    "jitter_secs",
    "poll_on_startup",
    "notify_schedule",
    ],
    "template_placeholders": [
    "source_id",
    "source_kind",
    "kind",
    "title",
    "body",
    "url",
    "tag",
    ],
    "notify_schedule": {
    // The schedule widget writes a crontab expression; 5-field form is
    // normalized to the 6-field one server-side, and both are accepted.
    "syntax": "crontab",
    "timezone": "UTC",
    "accepts_fields": [5, 6],
    "example": "0 9 * * MON-FRI",
    },
    "limits": {
    "outbox_flush_concurrency": { "min": 1, "max": 64 },
    "outbox_retry_backoff_max_secs": { "min": 60, "max": 86_400 },
    "sink_breaker_failure_threshold": { "min": 1, "max": 1_000 },
    "sink_breaker_cooldown_secs": { "min": 10, "max": 86_400 },
    },
    // Editing surfaces should mirror the server's own boundary rather than
    // re-deriving it: these sections are bootstrap-only and are rejected in
    // any pushed document.
    "readonly_sections": ["database", "api", "log", "config_api"],
    "config_authority": {
    "source": bootstrap.config_api.source,
    "routes_enabled": bootstrap.config_api.routes_enabled(),
    "accepts_pushed_config": bootstrap.config_api.accepts_pushed_config(),
    },
    })))
}

/// Page window for `GET /api/v1/config/revisions` (global and per-org).
#[derive(Debug, Deserialize)]
pub struct RevisionsQuery {
    limit: Option<i64>,
    offset: Option<i64>,
}

impl RevisionsQuery {
    /// Clamped `(limit, offset)` — clamp rather than reject: a history view is
    /// a convenience surface, and failing a dashboard poll over a too-large
    /// `limit` helps nobody.
    pub(super) fn window(&self) -> (i64, i64) {
        (
            self.limit
                .unwrap_or(REVISIONS_DEFAULT_LIMIT)
                .clamp(1, REVISIONS_MAX_LIMIT),
            self.offset.unwrap_or(0).max(0),
        )
    }
}

/// Largest history page a single request may fetch.
const REVISIONS_MAX_LIMIT: i64 = 200;
const REVISIONS_DEFAULT_LIMIT: i64 = 50;

/// `GET /api/v1/config/revisions` — paginated apply/reject audit history.
///
/// Metadata only: the revision *bodies* are omitted (see
/// [`crate::store::ConfigRevisionSummary`]) so this endpoint cannot leak an
/// unredacted config document.
pub async fn get_revisions(
    State(state): State<Arc<AppState>>,
    Query(query): Query<RevisionsQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (limit, offset) = query.window();

    let revisions = state.engine.store.list_config_revisions(limit, offset)?;
    let total = state.engine.store.count_config_revisions()?;

    Ok(Json(json!({
    "revisions": revisions,
    "total": total,
    "limit": limit,
    "offset": offset,
    })))
}

/// `POST /api/v1/config/validate` — dry-run candidate TOML/YAML (no persistence).
pub async fn post_validate(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<ValidateResponse>, ApiError> {
    let raw = std::str::from_utf8(&body)
        .map_err(|_| ApiError::BadRequest("config body must be valid UTF-8".into()))?;
    let bootstrap = state.bootstrap.as_ref();
    require_single_document_mode(bootstrap)?;
    let format_hint = desired_format_from_headers(&headers);
    // Match Apply: restore secrets the UI omitted after GET redaction.
    let previous = state.current_config().await;
    let response = validate_document(
        bootstrap,
        raw,
        false,
        format_hint,
        &state.engine.store,
        &ApplyScope::Whole,
        Some(&previous),
    )
    .map_err(validation_error)?;
    Ok(Json(response))
}

/// Refuse whole-document mutation routes on a multi-org instance.
///
/// With `[[organizations]]` the runtime is a *composition* — accepting one
/// whole document here would stomp every org at once, bypass the per-org
/// namespacing, and write a `NULL`-stream ledger row that a multi-org boot
/// never reads (the change would silently vanish on restart).
fn require_single_document_mode(bootstrap: &crate::config::Config) -> Result<(), ApiError> {
    if bootstrap.organizations.is_empty() {
        return Ok(());
    }
    Err(ApiError::Conflict(
        "this instance runs [[organizations]]; use \
 /api/v1/organizations/{id}/config/… for the organization you are changing"
            .into(),
    ))
}

/// `POST /api/v1/config/apply` — whole-document replace with hot-swap.
pub async fn post_apply(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<ApplyResponse>, ApiError> {
    let principal = auth::require_config_apply_auth(&state, &headers, &body, None)?;
    let raw = std::str::from_utf8(&body)
        .map_err(|_| ApiError::BadRequest("config body must be valid UTF-8".into()))?;
    let origin = apply_origin_from_headers(&headers, &principal);
    let format_hint = desired_format_from_headers(&headers);

    // Serialize apply/rollback: concurrent pushes must not interleave swap + ledger.
    let _apply_guard = state.apply_lock.lock().await;

    // Optimistic concurrency, checked *inside* the apply lock so the window
    // between "read current sha" and "apply" cannot be raced. Without this a
    // UI/CLI with several concurrent authors resolves silently as
    // last-writer-wins — fine for one CI pipeline, not for humans.
    enforce_if_match(&state, &headers).await?;
    let bootstrap = state.bootstrap.as_ref();
    require_single_document_mode(bootstrap)?;
    let result = apply_document(
        bootstrap,
        raw,
        format_hint,
        &origin,
        &state.engine.store,
        state.as_ref(),
        &ApplyScope::Whole,
    )
    .await;
    state.finish_apply(result).await
}

/// `POST /api/v1/reload` — re-read the app config file and hot-swap it.
///
/// The file-mode counterpart of `/config/apply`: the body is ignored, so a
/// caller can only ask the process to re-read config it does not control.
pub async fn post_reload(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ReloadResponse>, ApiError> {
    let response = state.reload_from_disk().await?;
    info!(
    applied = response.applied,
    sha = %response.content_sha256,
    source = %response.source_path,
    "config reload completed"
    );
    Ok(Json(response))
}

/// `POST /api/v1/config/rollback` — re-apply the previous ledger revision.
pub async fn post_rollback(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<ApplyResponse>, ApiError> {
    let principal = auth::require_config_apply_auth(&state, &headers, b"", None)?;
    let origin = apply_origin_from_headers(&headers, &principal);

    let _apply_guard = state.apply_lock.lock().await;
    let bootstrap = state.bootstrap.as_ref();
    require_single_document_mode(bootstrap)?;
    let result = rollback(
        bootstrap,
        &origin,
        &state.engine.store,
        state.as_ref(),
        &ApplyScope::Whole,
    )
    .await;
    state.finish_apply(result).await
}

/// Enforce an `If-Match` precondition against the running desired document.
///
/// Absent header = unconditional apply (a CI pipeline pushing the committed
/// state does not need to have read it first). `If-Match: *` requires that
/// *some* config is applied. Otherwise every supplied ETag is compared —
/// RFC 9110 allows a list — and any match satisfies the precondition.
async fn enforce_if_match(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let current = state.desired.read().await.effective_desired_sha.clone();
    check_if_match(headers, current.as_deref(), "GET /api/v1/config")
}

/// RFC 9110 `If-Match` comparison against `current` — the one matcher shared
/// by the whole-document route and the per-organization routes (which compare
/// against that org's authority sha instead of the global identity).
pub(super) fn check_if_match(
    headers: &HeaderMap,
    current: Option<&str>,
    reread_hint: &str,
) -> Result<(), ApiError> {
    let Some(header) = headers.get(axum::http::header::IF_MATCH) else {
        return Ok(());
    };
    let header = header
        .to_str()
        .map_err(|_| ApiError::BadRequest("invalid If-Match header".into()))?;

    if header.trim() == "*" {
        return if current.is_some() {
            Ok(())
        } else {
            Err(ApiError::PreconditionFailed(
                "If-Match: * requires an already-applied config".into(),
            ))
        };
    }

    let matches = header.split(',').any(|candidate| {
        let candidate = candidate.trim().trim_start_matches("W/").trim_matches('"');
        current == Some(candidate)
    });

    if matches {
        Ok(())
    } else {
        Err(ApiError::PreconditionFailed(format!(
            "config changed since it was read (current sha256 {}); re-read \
 {reread_hint} and re-apply",
            current.unwrap_or("<none>")
        )))
    }
}

pub(super) fn desired_format_from_headers(headers: &HeaderMap) -> Option<DesiredFormat> {
    headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(DesiredFormat::from_content_type)
}

pub(super) fn apply_origin_from_headers(
    headers: &HeaderMap,
    principal: &auth::AuthPrincipal,
) -> ApplyOrigin {
    // `applied_by` is the *verified* identity, not the `X-Config-Applied-By`
    // header: the header is fine as a CI label but worthless as an audit
    // record once humans author changes, since any caller can set it freely.
    // When a client sends one anyway it is kept as an annotation appended to
    // the verified principal, never as a replacement for it.
    let verified = principal.audit_label();
    let claimed = headers
        .get("X-Config-Applied-By")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let applied_by = match (verified, claimed) {
        (Some(verified), Some(claimed)) => Some(format!("{verified} ({claimed})")),
        (Some(verified), None) => Some(verified),
        (None, Some(claimed)) => Some(format!("unauthenticated ({claimed})")),
        (None, None) => None,
    };

    ApplyOrigin {
        revision_label: headers
            .get("X-Config-Revision")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
        applied_by,
        source_addr: headers
            .get("X-Forwarded-For")
            .or_else(|| headers.get("X-Real-IP"))
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
    }
}

impl ConfigSwapper for AppState {
    async fn current_config(&self) -> crate::config::Config {
        self.desired.read().await.effective.clone()
    }

    async fn apply_runtime(&self, config: &crate::config::Config) -> anyhow::Result<()> {
        let watches = config.to_watches()?;
        // Order: notifier/tuning first, then poll loops, then the API-visible
        // desired index. `WatchSupervisor::replace` is infallible (drain +
        // spawn); if `apply_runtime_config` fails the running set is untouched.
        self.engine.apply_runtime_config(config).await?;
        self.supervisor.replace(watches.clone()).await;
        let mut desired = self.desired.write().await;
        desired.index = crate::watch_lookup::WatchIndex::new(watches);
        desired.teams.clone_from(&config.teams);
        desired.effective.clone_from(config);
        Ok(())
    }
}

impl AppState {
    /// Publish the just-applied revision as the running effective state.
    ///
    /// Infallible on purpose: the only fallible step (the revision-metadata
    /// re-read) is display-only and swallowed, so callers never have to thread
    /// a `?` through the hot-swap tail. Config *authority* decisions must not
    /// key off the best-effort `effective_revision` written here — see
    /// [`Self::reload_from_disk`], which queries the ledger directly.
    pub async fn commit_effective(&self, response: &ApplyResponse) {
        if !response.applied {
            return;
        }
        // Set the optimistic-concurrency identity FIRST, straight from the
        // response (no DB query) — it is the `If-Match` comparison key, so it
        // must never lag the just-applied config. Previously this ran *after*
        // a `?`-propagating ledger re-read, so a DB blip in that one-statement
        // window left `effective_desired_sha` pointing at the old revision
        // while runtime + ledger were on the new one, and a subsequent stale
        // apply could then be wrongly accepted.
        let revision = match self.engine.store.latest_applied_config_revision(None) {
            Ok(Some(record)) => Some(record.into()),
            Ok(None) => None,
            Err(err) => {
                tracing::warn!(error = %err, "config applied but revision metadata refresh failed");
                None
            }
        };
        let mut desired = self.desired.write().await;
        desired.effective_desired_sha = Some(response.content_sha256.clone());
        // Revision metadata (id/label/applied_at) is display-only; refresh it
        // best-effort rather than failing the whole apply if the read blips.
        if let Some(record) = revision {
            desired.effective_revision = Some(record);
        }
    }

    /// Shared tail for `apply` and `rollback` (both re-apply a whole document):
    /// publish the new effective state, count the outcome, and map failures to
    /// their HTTP shape. Unifying it here means rollback rejections are counted
    /// on the same metric as apply rejections — previously rollback dropped
    /// them silently — and neither handler can drift from the other.
    async fn finish_apply(
        &self,
        result: anyhow::Result<ApplyResponse>,
    ) -> Result<Json<ApplyResponse>, ApiError> {
        match result {
            Ok(response) => {
                self.commit_effective(&response).await;
                if response.applied {
                    self.engine.metrics.record_config_apply();
                }
                info!(
                revision = response.revision,
                applied = response.applied,
                sha = %response.content_sha256,
                "config apply completed"
                );
                Ok(Json(response))
            }
            Err(err) => {
                if err.downcast_ref::<ValidationRejected>().is_some() {
                    self.engine.metrics.record_config_apply_rejected();
                }
                Err(validation_error(err))
            }
        }
    }

    /// Re-read desired state and hot-swap it (shared by `POST /api/v1/reload`
    /// and `SIGHUP`).
    ///
    /// Single-document mode refuses when a ledger revision is authoritative:
    /// `config::resolve` would boot from the ledger, so swapping in file
    /// contents here would leave the running config diverged from what a
    /// restart produces. Multi-org mode has no such refusal — reload
    /// re-resolves *every org from its own authority* (ledger stream or file),
    /// which converges the runtime to exactly what a restart would produce.
    pub async fn reload_from_disk(&self) -> Result<ReloadResponse, ApiError> {
        let bootstrap_snapshot = self.bootstrap.as_ref();
        if !bootstrap_snapshot.organizations.is_empty() {
            // Same lock as apply/rollback: a reload is a hot-swap too.
            let _guard = self.apply_lock.lock().await;
            let current_identity = self.desired.read().await.effective_desired_sha.clone();
            let response = reload_organizations(
                bootstrap_snapshot,
                &self.config_paths,
                &self.engine.store,
                current_identity.as_deref(),
                self,
            )
            .await
            .map_err(validation_error)?;
            if response.applied {
                self.desired.write().await.effective_desired_sha =
                    Some(response.content_sha256.clone());
            }
            return Ok(response);
        }

        let Some(path) = self.config_paths.app.clone() else {
            return Err(ApiError::BadRequest(
                "no app config file configured (--app / XRELEASE_APP_CONFIG); \
 desired state is pushed via POST /api/v1/config/apply"
                    .into(),
            ));
        };
        let raw = std::fs::read_to_string(&path)
            .map_err(|err| ApiError::BadRequest(format!("reading {}: {err}", path.display())))?;

        // Same lock as apply/rollback: a reload is a hot-swap too.
        let _guard = self.apply_lock.lock().await;

        // Gate on the AUTHORITATIVE ledger, not the in-memory
        // `effective_revision`: that field is written best-effort by
        // `commit_effective` (a DB blip during the post-apply metadata re-read
        // leaves it `None` while the ledger holds a row), so keying the
        // authority decision off it can fail *open* — a reload would swap in
        // file content that the next restart silently reverts from the ledger.
        // Querying the ledger here is both authoritative and fail-closed (a DB
        // error propagates rather than green-lighting the swap). Running it
        // under `apply_lock` keeps it atomic against a committing apply.
        // `source = "local"` pins authority to the file, so `ledger_is_bootable`
        // gates the query out entirely there.
        if self.config_api.ledger_is_bootable()
            && self
                .engine
                .store
                .latest_applied_config_revision(None)?
                .is_some()
        {
            return Err(ApiError::Conflict(
                "config ledger is authoritative for desired state; \
 use POST /api/v1/config/apply instead of reload"
                    .into(),
            ));
        }

        let current_sha = self.desired.read().await.effective_desired_sha.clone();
        let bootstrap = self.bootstrap.as_ref();

        let response = reload_document(
            bootstrap,
            current_sha.as_deref(),
            &path.display().to_string(),
            &raw,
            crate::config::format_from_path(&path),
            self,
        )
        .await
        .map_err(validation_error)?;

        if response.applied {
            self.desired.write().await.effective_desired_sha =
                Some(response.content_sha256.clone());
        }
        Ok(response)
    }
}

/// Map an apply/reload failure to its HTTP shape.
///
/// Both rejection kinds are the *client's* fault and must not surface as 500:
/// a document that parsed but is semantically wrong → 422 with the validation
/// report; a body that never parsed (or carried forbidden infra sections) →
/// 400. Anything else is a genuine server failure and keeps the default
/// mapping.
pub(super) fn validation_error(err: anyhow::Error) -> ApiError {
    if let Some(rejected) = err.downcast_ref::<ValidationRejected>() {
        return ApiError::validation(
            rejected.to_string(),
            &serde_json::to_value(&rejected.report).unwrap_or_else(|_| json!({})),
        );
    }
    if let Some(rejected) = err.downcast_ref::<DocumentRejected>() {
        return ApiError::BadRequest(rejected.to_string());
    }
    if let Some(nothing) = err.downcast_ref::<NothingToRollBackTo>() {
        return ApiError::Conflict(nothing.to_string());
    }
    err.into()
}
