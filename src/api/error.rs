//! Unified API error type — one `IntoResponse` mapping for the whole HTTP
//! surface (management API + webhooks), replacing the ad-hoc `Response`
//! builders that used to be duplicated across `handlers.rs`, `webhook.rs`,
//! and `auth.rs`.
//!
//! Domain errors ([`crate::error::StoreError`], [`crate::error::SourceError`],
//! [`crate::error::PipelineError`]) stay HTTP-agnostic; this module is the one
//! place that decides which of them becomes a 404 vs a 502 vs a 500.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::error::{PipelineError, SourceError, StoreError};

/// Every error an API handler or webhook handler can return.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// 422 — semantically invalid config, optionally carrying the validation
    /// report so callers (CI) get the failing checks, not just a message.
    #[error("{message}")]
    UnprocessableEntity {
        message: String,
        /// Boxed: only this variant carries a payload, and an inline
        /// `serde_json::Value` would widen every other variant with it.
        report: Option<Box<serde_json::Value>>,
    },
    /// 401 — missing/invalid bearer token, API key, or webhook signature.
    #[error("{0}")]
    Unauthorized(String),
    /// 403 — authenticated, but the principal's role is below the level the
    /// route requires (RBAC). Distinct from 401: re-authenticating won't help.
    #[error("{0}")]
    Forbidden(String),
    /// 400 — malformed request body or path parameter.
    #[error("{0}")]
    BadRequest(String),
    /// 404 — the referenced source/resource does not exist.
    #[error("{0}")]
    NotFound(String),
    /// 409 — the request conflicts with current state (e.g. reloading from
    /// file while the config ledger is authoritative).
    #[error("{0}")]
    Conflict(String),
    /// 412 — an `If-Match` precondition did not hold: the config changed
    /// since the caller read it, so applying would silently clobber whoever
    /// wrote in between.
    #[error("{0}")]
    PreconditionFailed(String),
    /// 502 — an upstream registry or forge returned an error.
    #[error("{0}")]
    BadGateway(String),
    /// 503 — dependency temporarily unavailable (e.g. database pool exhausted).
    #[error("{0}")]
    ServiceUnavailable(String),
    /// 500 — store failure, notifier failure, or anything unexpected.
    #[error("{0}")]
    Internal(String),
}

impl ApiError {
    /// Wrap any displayable error as a 500. Used at handler boundaries where
    /// no more specific mapping applies.
    pub(crate) fn internal(err: impl std::fmt::Display) -> Self {
        Self::Internal(err.to_string())
    }

    /// 422 with a structured validation report attached.
    pub fn validation(message: impl Into<String>, report: &serde_json::Value) -> Self {
        Self::UnprocessableEntity {
            message: message.into(),
            report: Some(Box::new(report.clone())),
        }
    }

    /// HTTP status code for this error. `pub(crate)` so tests can assert on
    /// it without going through a full `IntoResponse` round-trip.
    pub(crate) fn status(&self) -> StatusCode {
        match self {
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::UnprocessableEntity { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::PreconditionFailed(_) => StatusCode::PRECONDITION_FAILED,
            Self::BadGateway(_) => StatusCode::BAD_GATEWAY,
            Self::ServiceUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            // Only 500s are unexpected; 4xx are routine (bad input, missing
            // auth) and would just be noise at error level.
            tracing::error!(error = %self, "API request failed");
        }
        let mut body = json!({ "error": self.to_string() });
        if let Self::UnprocessableEntity {
            report: Some(report),
            ..
        } = self
        {
            body["report"] = *report;
        }
        (status, Json(body)).into_response()
    }
}

impl From<StoreError> for ApiError {
    fn from(err: StoreError) -> Self {
        match &err {
            StoreError::PollerBusy => Self::Conflict(err.to_string()),
            StoreError::Pool(_) => Self::ServiceUnavailable(err.to_string()),
            StoreError::Postgres(_) | StoreError::Other(_) => Self::internal(err),
        }
    }
}

impl From<SourceError> for ApiError {
    /// Upstream registry failures surface their real status: a 404 from the
    /// registry means the image/package/repo is gone (404 to our caller
    /// too); any other non-2xx is an upstream problem, not ours (502).
    fn from(err: SourceError) -> Self {
        match &err {
            SourceError::Registry { status, .. } if *status == 404 => {
                Self::NotFound(err.to_string())
            }
            SourceError::Registry { .. } => Self::BadGateway(err.to_string()),
            _ => Self::internal(err),
        }
    }
}

impl From<PipelineError> for ApiError {
    fn from(err: PipelineError) -> Self {
        match err {
            PipelineError::Source(err) => err.into(),
            PipelineError::Store(err) => err.into(),
            PipelineError::Notify(err) => Self::BadGateway(err.to_string()),
            PipelineError::Other(message) => Self::internal(message),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    /// Pipeline / config-apply paths may still surface `anyhow`; downcast to
    /// the richer typed mappings when possible instead of flattening to 500.
    fn from(err: anyhow::Error) -> Self {
        match err.downcast::<PipelineError>() {
            Ok(pipeline) => pipeline.into(),
            Err(err) => match err.downcast::<SourceError>() {
                Ok(source_err) => source_err.into(),
                Err(err) => match err.downcast::<StoreError>() {
                    Ok(store_err) => store_err.into(),
                    Err(err) => Self::internal(err),
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unauthorized_should_map_to_401() {
        assert_eq!(
            ApiError::Unauthorized("x".into()).status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[test]
    fn not_found_should_map_to_404() {
        assert_eq!(
            ApiError::NotFound("x".into()).status(),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn source_error_registry_404_should_map_to_not_found() {
        let err = SourceError::Registry {
            status: 404,
            url: "https://example.test".into(),
        };
        assert_eq!(ApiError::from(err).status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn source_error_registry_other_should_map_to_bad_gateway() {
        let err = SourceError::Registry {
            status: 503,
            url: "https://example.test".into(),
        };
        assert_eq!(ApiError::from(err).status(), StatusCode::BAD_GATEWAY);
    }

    #[test]
    fn source_error_other_should_map_to_internal() {
        let err = SourceError::Other("boom".into());
        assert_eq!(
            ApiError::from(err).status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn store_error_poller_busy_should_map_to_conflict() {
        assert_eq!(
            ApiError::from(StoreError::PollerBusy).status(),
            StatusCode::CONFLICT
        );
    }

    #[test]
    fn pipeline_source_error_should_preserve_registry_mapping() {
        let err = PipelineError::Source(SourceError::Registry {
            status: 404,
            url: "https://example.test".into(),
        });
        assert_eq!(ApiError::from(err).status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn anyhow_wrapping_source_error_should_use_source_error_mapping() {
        let err: anyhow::Error = SourceError::Registry {
            status: 404,
            url: "https://example.test".into(),
        }
        .into();
        assert_eq!(ApiError::from(err).status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn anyhow_wrapping_store_error_should_use_store_mapping() {
        let err: anyhow::Error = StoreError::PollerBusy.into();
        assert_eq!(ApiError::from(err).status(), StatusCode::CONFLICT);
    }

    #[test]
    fn anyhow_wrapping_pipeline_error_should_use_pipeline_mapping() {
        let err: anyhow::Error = PipelineError::Store(StoreError::PollerBusy).into();
        assert_eq!(ApiError::from(err).status(), StatusCode::CONFLICT);
    }

    #[test]
    fn anyhow_wrapping_other_error_should_map_to_internal() {
        let err = anyhow::anyhow!("boom");
        assert_eq!(
            ApiError::from(err).status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
