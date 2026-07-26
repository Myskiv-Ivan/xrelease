//! Inbound webhook handlers (GitHub, GitLab, Gitea, Bitbucket, Docker, generic).
//!
//! Shared shape per forge route:
//! 1. Forge-specific auth (HMAC / token / secret)
//! 2. Idempotency claim (`webhook_delivery`)
//! 3. Parse JSON → [`Release`] (Ignored vs BadRequest)
//! 4. Resolve configured [`Watch`]
//! 5. [`process_pushed_release`] → same pipeline as poll

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use tracing::{debug, warn};

use super::auth;
use super::error::ApiError;
use super::handlers::WebhookResponse;
use super::webhook_id;
use super::webhook_payload::{
    BitbucketPushWebhook, DockerPushWebhook, GiteaCompatibleWebhook, GitlabWebhook,
    WebhookPayloadError,
};
use super::AppState;
use crate::metrics::WebhookOutcome;
use crate::model::Release;
use crate::pipeline::{deliver_new_releases, Watch};
use crate::watch_lookup::WatchIndex;

/// `POST /api/v1/webhooks/github`
pub async fn github(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<WebhookResponse>, ApiError> {
    auth::verify_github_signature(&headers, &state.api.webhook_secret, &body)?;
    if let Some(resp) = ensure_idempotent(&state, &headers, "github").await? {
        return Ok(resp);
    }
    handle_gitea_compatible(&state, body, &["github"]).await
}

/// `POST /api/v1/webhooks/gitea` — Gitea/Forgejo release hook (GitHub-compatible JSON).
pub async fn gitea(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<WebhookResponse>, ApiError> {
    auth::verify_webhook_secret(&headers, &state.api.webhook_secret)?;
    if let Some(resp) = ensure_idempotent(&state, &headers, "gitea").await? {
        return Ok(resp);
    }
    handle_gitea_compatible(&state, body, &["gitea", "codeberg"]).await
}

async fn handle_gitea_compatible(
    state: &Arc<AppState>,
    body: Bytes,
    kinds: &[&str],
) -> Result<Json<WebhookResponse>, ApiError> {
    let payload: GiteaCompatibleWebhook = parse_json(&body)?;
    ingest_mapped_release(
        state,
        payload.into_release(),
        |index, repo| index.find_repos_any(kinds, repo),
        |repo| {
            format!(
                "no configured source for {repo} (kinds: {})",
                kinds.join(", ")
            )
        },
    )
    .await
}

/// `POST /api/v1/webhooks/gitlab`
pub async fn gitlab(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<WebhookResponse>, ApiError> {
    auth::verify_gitlab_token(&headers, &state.api.webhook_secret)?;
    if let Some(resp) = ensure_idempotent(&state, &headers, "gitlab").await? {
        return Ok(resp);
    }

    let payload: GitlabWebhook = parse_json(&body)?;
    ingest_mapped_release(
        &state,
        payload.into_release(),
        |index, repo| index.find_repos("gitlab", repo),
        |repo| format!("no configured gitlab source for {repo}"),
    )
    .await
}

/// `POST /api/v1/webhooks/bitbucket` — Bitbucket Cloud / Server tag push (`repo:push`).
pub async fn bitbucket(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<WebhookResponse>, ApiError> {
    auth::verify_bitbucket_signature(&headers, &state.api.webhook_secret, &body)?;
    if let Some(resp) = ensure_idempotent(&state, &headers, "bitbucket").await? {
        return Ok(resp);
    }

    if let Some(event_key) = headers
        .get("X-Event-Key")
        .and_then(|value| value.to_str().ok())
    {
        if event_key != "repo:push" {
            state.engine.metrics.record_webhook(WebhookOutcome::Ignored);
            return Ok(Json(WebhookResponse::ignored(format!(
                "ignored X-Event-Key {event_key}"
            ))));
        }
    }

    let payload: BitbucketPushWebhook = parse_json(&body)?;
    ingest_mapped_release(
        &state,
        payload.into_release(),
        |index, repo| index.find_repos("bitbucket", repo),
        |repo| format!("no configured bitbucket source for {repo}"),
    )
    .await
}

/// `POST /api/v1/webhooks/docker` — Docker Hub registry push events.
pub async fn docker(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<WebhookResponse>, ApiError> {
    auth::verify_webhook_secret(&headers, &state.api.webhook_secret)?;
    if let Some(resp) = ensure_idempotent(&state, &headers, "docker").await? {
        return Ok(resp);
    }

    let payload: DockerPushWebhook = parse_json(&body)?;
    ingest_mapped_release(
        &state,
        payload.into_release(),
        |index, image| index.find_images_any(crate::watch_lookup::CONTAINER_KINDS, image),
        |image| {
            format!(
                "no configured container source for image {image} (kinds: docker, ghcr, quay, ecr)"
            )
        },
    )
    .await
}

/// Generic push webhook for custom integrations.
#[derive(Debug, Deserialize)]
pub struct GenericWebhook {
    pub source_id: String,
    pub tag: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub prerelease: bool,
}

/// `POST /api/v1/webhooks/generic`
pub async fn generic(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<WebhookResponse>, ApiError> {
    auth::verify_webhook_secret(&headers, &state.api.webhook_secret)?;
    if let Some(resp) = ensure_idempotent(&state, &headers, "generic").await? {
        return Ok(resp);
    }

    // `body: Bytes` + manual decode (verify-then-parse) — matches every other
    // webhook handler. Previously this was the only route using the `Json<T>`
    // extractor, which parses the body *before* the secret check runs.
    let payload: GenericWebhook = parse_json(&body)?;

    // Resolve + clone the watch under the read guard, then drop the guard
    // before delivery (below): holding `desired.read` across the delivery's
    // network I/O would block a concurrent config apply's `desired.write`.
    let watch = {
        let desired = state.desired.read().await;
        let watch = desired.index.find_by_id(&payload.source_id).cloned();
        watch.ok_or_else(|| {
            state.engine.metrics.record_webhook(WebhookOutcome::Error);
            ApiError::NotFound(format!("unknown source_id {}", payload.source_id))
        })?
    };

    let mut release = Release::new(payload.tag.clone()).with_prerelease(payload.prerelease);
    release = release.with_url(payload.url);
    release = release.with_body(payload.body.filter(|b| !b.trim().is_empty()));

    process_pushed_release(&state, std::slice::from_ref(&watch), release).await
}

/// Map forge `into_release` → watch lookup → shared delivery path.
///
/// `find_watches` takes the [`WatchIndex`] by reference (not a captured read
/// guard) so the guard's scope ends the moment the resolved [`Watch`]es are
/// cloned — the delivery below then runs with owned `Watch`es and no lock
/// held. Holding `index.read` across `process_pushed_release`'s network I/O
/// would let a slow sink block a concurrent config apply's `index.write`,
/// and tokio's write-preferring `RwLock` would then queue every reader behind
/// it (`get_status`/`list_sources`/webhooks) — a brief global read-API stall.
///
/// The lookup is **multi-match**: with `[[organizations]]` several tenants can
/// watch the same upstream, and one inbound event must reach every one of them
/// ('s isolation is between orgs' notifications, not their upstreams).
async fn ingest_mapped_release<F, N>(
    state: &Arc<AppState>,
    mapped: Result<(String, Release), WebhookPayloadError>,
    find_watches: F,
    not_found: N,
) -> Result<Json<WebhookResponse>, ApiError>
where
    F: for<'i> FnOnce(&'i WatchIndex, &str) -> Vec<&'i Watch>,
    N: FnOnce(&str) -> String,
{
    let (key, release) = match map_payload(state.as_ref(), mapped)? {
        MappedPayload::Respond(response) => return Ok(response),
        MappedPayload::Deliver { key, release } => (key, release),
    };
    let watches: Vec<Watch> = {
        let desired = state.desired.read().await;
        find_watches(&desired.index, &key)
            .into_iter()
            .cloned()
            .collect()
    };
    if watches.is_empty() {
        state.engine.metrics.record_webhook(WebhookOutcome::Error);
        return Err(ApiError::NotFound(not_found(&key)));
    }
    process_pushed_release(state, &watches, release).await
}

enum MappedPayload {
    Deliver { key: String, release: Release },
    Respond(Json<WebhookResponse>),
}

fn map_payload(
    state: &AppState,
    result: Result<(String, Release), WebhookPayloadError>,
) -> Result<MappedPayload, ApiError> {
    match result {
        Ok((key, release)) => Ok(MappedPayload::Deliver { key, release }),
        Err(WebhookPayloadError::Ignored(message)) => {
            state.engine.metrics.record_webhook(WebhookOutcome::Ignored);
            Ok(MappedPayload::Respond(Json(WebhookResponse::ignored(
                message,
            ))))
        }
        Err(WebhookPayloadError::BadRequest(message)) => {
            state.engine.metrics.record_webhook(WebhookOutcome::Error);
            Err(bad_request(message))
        }
    }
}

fn parse_json<T: DeserializeOwned>(body: &Bytes) -> Result<T, ApiError> {
    serde_json::from_slice(body).map_err(|err| bad_request(err.to_string()))
}

/// Deliver one pushed release to every matched watch (usually one; several
/// when multiple organizations track the same upstream).
///
/// Each watch runs the full poll pipeline independently — its own filter,
/// seen-state, and routing — so one org's dedup cannot suppress another's
/// first sighting. A mid-loop failure returns 500 after some orgs delivered:
/// acceptable, because delivery itself is outbox-backed (the failure here is
/// enqueue-level) and the ledger claim already made a sender retry a no-op.
async fn process_pushed_release(
    state: &Arc<AppState>,
    watches: &[Watch],
    release: Release,
) -> Result<Json<WebhookResponse>, ApiError> {
    let tag = release.raw_tag.clone();
    let first_source_id = watches
        .first()
        .map(|watch| watch.provider.id().to_owned())
        .unwrap_or_default();

    let mut sent = 0usize;
    for watch in watches {
        sent += deliver_new_releases(watch, vec![release.clone()], &state.engine)
            .await
            .map_err(|err| {
                state.engine.metrics.record_webhook(WebhookOutcome::Error);
                ApiError::from(err)
            })?;
    }

    if sent == 0 {
        debug!(
        source = %first_source_id,
        matched = watches.len(),
        tag = %tag,
        "webhook release ignored (filtered or already seen)"
        );
        state.engine.metrics.record_webhook(WebhookOutcome::Ignored);
        return Ok(Json(WebhookResponse {
            accepted: true,
            source_id: Some(first_source_id),
            tag: Some(tag),
            notifications_sent: 0,
            message: "accepted but no notification sent (filtered, duplicate, or baseline)".into(),
        }));
    }

    state
        .engine
        .metrics
        .record_webhook(WebhookOutcome::Accepted);
    let mut response = WebhookResponse::delivered(first_source_id, tag, sent);
    if watches.len() > 1 {
        response.message = format!("notification delivered ({} matched sources)", watches.len());
    }
    Ok(Json(response))
}

async fn ensure_idempotent(
    state: &AppState,
    headers: &HeaderMap,
    kind: &str,
) -> Result<Option<Json<WebhookResponse>>, ApiError> {
    let Some(delivery_id) = webhook_id::delivery_id(headers, kind) else {
        return Ok(None);
    };
    match state.engine.store.claim_webhook_delivery(&delivery_id) {
        Ok(true) => Ok(None),
        Ok(false) => {
            state
                .engine
                .metrics
                .record_webhook(WebhookOutcome::Duplicate);
            Ok(Some(Json(WebhookResponse::duplicate(delivery_id))))
        }
        Err(err) => Err(err.into()),
    }
}

/// Build a 400 for a malformed webhook body, logging the cause. The one
/// remaining local helper: unlike `NotFound`/`Internal`, a bad request is
/// specific to webhook ingress and always worth a log line.
fn bad_request(message: String) -> ApiError {
    warn!(%message, "webhook bad request");
    ApiError::BadRequest(message)
}
