//! REST API handlers.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::info;

use super::error::ApiError;
use super::AppState;
use crate::pipeline::poll_once;

/// Webhook handler response body.
#[derive(Debug, Serialize)]
pub struct WebhookResponse {
    pub accepted: bool,
    pub source_id: Option<String>,
    pub tag: Option<String>,
    pub notifications_sent: usize,
    pub message: String,
}

impl WebhookResponse {
    pub fn ignored(message: impl Into<String>) -> Self {
        Self {
            accepted: false,
            source_id: None,
            tag: None,
            notifications_sent: 0,
            message: message.into(),
        }
    }

    pub fn delivered(source_id: String, tag: String, notifications_sent: usize) -> Self {
        Self {
            accepted: true,
            source_id: Some(source_id),
            tag: Some(tag),
            notifications_sent,
            message: "notification delivered".into(),
        }
    }

    pub fn duplicate(delivery_id: String) -> Self {
        Self {
            accepted: true,
            source_id: None,
            tag: None,
            notifications_sent: 0,
            message: format!("duplicate delivery `{delivery_id}` ignored"),
        }
    }
}

/// `GET /health` — liveness (process up). Dependency checks belong on `/ready`.
///
/// Outbox counters are best-effort: when PostgreSQL is briefly unreachable the
/// probe still returns 200 so orchestrators do not kill a healthy process.
pub async fn health(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let sources = state.desired.read().await.index.watches().len();
    let (outbox_pending, outbox_dead) = match state.engine.store.outbox_counts() {
        Ok(outbox) => (outbox.pending + outbox.failed, outbox.dead),
        Err(_) => (0, 0),
    };
    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "uptime_secs": state.started_at.elapsed().as_secs(),
            "sources": sources,
            "outbox_pending": outbox_pending,
            "outbox_dead": outbox_dead,
        })),
    )
}

/// `GET /ready` — readiness (PostgreSQL + notification sinks reachable).
pub async fn ready(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let check = state.engine.ready_check().await;
    let status = if check.is_ready() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(json!({
            "status": if check.is_ready() { "ready" } else { "not_ready" },
            "database_ok": check.database_ok,
            "notifiers_ok": check.notifiers_ok,
            "outbox_pending": check.outbox.pending + check.outbox.failed,
            "outbox_dead": check.outbox.dead,
        })),
    )
}

/// `GET /metrics` — Prometheus text exposition.
pub async fn metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let outbox = state.engine.store.outbox_counts().unwrap_or_default();
    let breakers = state.engine.notifier_snapshot().await.breaker_states();
    let body = state.engine.metrics.render_prometheus(
        state.desired.read().await.index.watches().len(),
        &outbox,
        state.started_at.elapsed().as_secs(),
        &breakers,
    );
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
}

/// `GET /openapi.json`
pub async fn openapi_spec() -> impl IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "application/json; charset=utf-8",
        )],
        include_str!("../../api/openapi.json"),
    )
}

/// Request body for `POST /api/v1/check`.
#[derive(Debug, Deserialize)]
pub struct CheckRequest {
    #[serde(default)]
    pub dry_run: bool,
}

/// Response body for `POST /api/v1/check`.
#[derive(Debug, Serialize)]
pub struct CheckResponse {
    pub notifications_sent: usize,
    pub sources_checked: usize,
}

/// `POST /api/v1/check`
pub async fn trigger_check(
    State(state): State<Arc<AppState>>,
    body: Option<Json<CheckRequest>>,
) -> Result<Json<CheckResponse>, ApiError> {
    let dry_run = body.is_some_and(|b| b.dry_run);
    let mut notifications_sent = 0usize;

    let watches: Vec<_> = state.desired.read().await.index.watches().to_vec();
    let sources_checked = watches.len();

    if dry_run {
        for watch in &watches {
            watch.provider.fetch(&state.engine.http, None).await?;
        }
    } else {
        notifications_sent += state.engine.flush_outbox(50).await?;
        for watch in &watches {
            notifications_sent += poll_once(&state.engine, watch, true).await?;
        }
    }

    info!(
        dry_run,
        notifications_sent,
        sources = sources_checked,
        "API check triggered"
    );

    Ok(Json(CheckResponse {
        notifications_sent,
        sources_checked,
    }))
}

/// `POST /api/v1/check/{id}` — poll one configured source on demand.
pub async fn trigger_check_source(
    State(state): State<Arc<AppState>>,
    Path(source_id): Path<String>,
    body: Option<Json<CheckRequest>>,
) -> Result<Json<CheckResponse>, ApiError> {
    let watch = {
        let desired = state.desired.read().await;
        desired
            .index
            .find_by_id(&source_id)
            .cloned()
            .ok_or_else(|| ApiError::NotFound(format!("source `{source_id}` not found")))?
    };

    let dry_run = body.is_some_and(|b| b.dry_run);
    let notifications_sent = if dry_run {
        watch.provider.fetch(&state.engine.http, None).await?;
        0
    } else {
        let mut sent = state.engine.flush_outbox(50).await?;
        sent += poll_once(&state.engine, &watch, true).await?;
        sent
    };

    info!(
        source = %source_id,
        dry_run,
        notifications_sent,
        "API single-source check triggered"
    );

    Ok(Json(CheckResponse {
        notifications_sent,
        sources_checked: 1,
    }))
}

/// One live notification sink (running config).
#[derive(Debug, Serialize)]
pub struct NotifierView {
    pub index: usize,
    pub kind: String,
    pub name: String,
    pub tags: Vec<String>,
}

/// `GET /api/v1/notifiers` — configured delivery channels (runtime snapshot).
#[derive(Debug, Serialize)]
pub struct NotifierListResponse {
    pub notifiers: Vec<NotifierView>,
}

/// Request body for `POST /api/v1/notifiers/test`.
#[derive(Debug, Deserialize)]
pub struct NotifierTestRequest {
    /// When set, only this sink index is probed. When omitted, every sink is tested.
    #[serde(default)]
    pub index: Option<usize>,
    /// Optional override for the synthetic event title.
    #[serde(default)]
    pub title: Option<String>,
    /// Optional override for the synthetic event body.
    #[serde(default)]
    pub body: Option<String>,
}

/// Per-sink outcome of a notification test.
#[derive(Debug, Serialize)]
pub struct NotifierTestResult {
    pub index: usize,
    pub kind: String,
    pub name: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `POST /api/v1/notifiers/test` — send a synthetic event through live sinks.
#[derive(Debug, Serialize)]
pub struct NotifierTestResponse {
    pub results: Vec<NotifierTestResult>,
}

fn test_notification_event(title: Option<&str>, body: Option<&str>) -> crate::notify::Event {
    crate::notify::Event {
        source_id: "xrelease:test".into(),
        source_kind: "Test".into(),
        title: title
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("xrelease notification test")
            .to_owned(),
        body: body
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(
                "This is a test notification from the xrelease management API. \
                 If you received it, this delivery channel is working.",
            )
            .to_owned(),
        url: None,
        routing_tag: None,
    }
}

/// `GET /api/v1/notifiers`
pub async fn list_notifiers(
    State(state): State<Arc<AppState>>,
) -> Result<Json<NotifierListResponse>, ApiError> {
    let notifier = state.engine.notifier_snapshot().await;
    let notifiers = notifier
        .describe()
        .into_iter()
        .map(|sink| NotifierView {
            index: sink.index,
            kind: sink.kind.to_owned(),
            name: sink.name,
            tags: sink.tags,
        })
        .collect();
    Ok(Json(NotifierListResponse { notifiers }))
}

/// `POST /api/v1/notifiers/test` — force-deliver a synthetic event (bypasses routing tags).
pub async fn test_notifiers(
    State(state): State<Arc<AppState>>,
    body: Option<Json<NotifierTestRequest>>,
) -> Result<Json<NotifierTestResponse>, ApiError> {
    let req = body
        .map(|Json(value)| value)
        .unwrap_or(NotifierTestRequest {
            index: None,
            title: None,
            body: None,
        });
    let event = test_notification_event(req.title.as_deref(), req.body.as_deref());
    let notifier = state.engine.notifier_snapshot().await;
    let described = notifier.describe();

    if let Some(index) = req.index {
        if index >= notifier.len() {
            return Err(ApiError::NotFound(format!(
                "notifier index {index} not found ({} configured)",
                notifier.len()
            )));
        }
        let view = &described[index];
        let outcome = notifier.test_at(index, &event).await;
        let result = NotifierTestResult {
            index,
            kind: view.kind.to_owned(),
            name: view.name.clone(),
            ok: outcome.is_ok(),
            error: outcome.err().map(|err| err.to_string()),
        };
        info!(
            index,
            kind = %result.kind,
            ok = result.ok,
            "API notifier test triggered"
        );
        return Ok(Json(NotifierTestResponse {
            results: vec![result],
        }));
    }

    let outcomes = notifier.test_all(&event).await;
    let results: Vec<NotifierTestResult> = outcomes
        .into_iter()
        .map(|(index, outcome)| {
            let view = &described[index];
            NotifierTestResult {
                index,
                kind: view.kind.to_owned(),
                name: view.name.clone(),
                ok: outcome.is_ok(),
                error: outcome.err().map(|err| err.to_string()),
            }
        })
        .collect();

    info!(
        sinks = results.len(),
        ok = results.iter().filter(|r| r.ok).count(),
        "API notifier test (all) triggered"
    );

    Ok(Json(NotifierTestResponse { results }))
}
