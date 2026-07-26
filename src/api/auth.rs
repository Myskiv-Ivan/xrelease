//! API and webhook authentication helpers.
//!
//! Every check returns [`AuthResult`] (`Result<(), ApiError>`), so callers use
//! `?` directly. Webhook handlers call these inline (each forge verifies a
//! different signature/token shape); the management API instead runs
//! [`require_management_auth`] once as a router layer (see [`super::router`])
//! so the check can never be forgotten on a new route.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Request};
use axum::middleware::Next;
use axum::response::Response;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

use super::error::ApiError;
use super::role::AppRole;
use super::AppState;

type HmacSha256 = Hmac<Sha256>;

/// Result of an auth/signature check.
pub type AuthResult = Result<(), ApiError>;

/// Reject with 401 when the bearer API key does not match.
pub fn require_api_key(headers: &HeaderMap, expected: &Option<String>) -> AuthResult {
    let Some(expected) = expected else {
        return Ok(());
    };
    if expected.is_empty() {
        return Ok(());
    }

    let provided = bearer_token(headers).unwrap_or_default();
    if constant_time_eq(provided.as_bytes(), expected.as_bytes()) {
        Ok(())
    } else {
        Err(ApiError::Unauthorized("invalid or missing API key".into()))
    }
}

/// Validate management API bearer — API key, local session JWT, and/or OIDC.
pub fn require_bearer_auth(
    headers: &HeaderMap,
    api: &crate::config::ApiConfig,
    oidc: Option<&crate::api::OidcValidator>,
    store: &crate::store::Store,
) -> AuthResult {
    resolve_auth_principal(headers, api, oidc, store).map(|_| ())
}

/// Establish who is calling: API key, local session, OIDC, or anonymous.
///
/// `store` is consulted only for a local session, to enforce revocation: the
/// token's `ver` claim must still match the user's `session_version` (bumped on
/// logout / password / role change).
pub fn resolve_auth_principal(
    headers: &HeaderMap,
    api: &crate::config::ApiConfig,
    oidc: Option<&crate::api::OidcValidator>,
    store: &crate::store::Store,
) -> Result<AuthPrincipal, ApiError> {
    let api_key_configured = api.api_key.as_ref().is_some_and(|key| !key.is_empty());
    let oidc_configured = oidc.is_some();
    let session_secret = api
        .local_auth
        .enabled
        .then(|| api.local_auth.session_secret.clone())
        .flatten();
    let session_configured = session_secret.is_some();

    if !api_key_configured && !oidc_configured && !session_configured {
        return Ok(AuthPrincipal::Anonymous);
    }

    let Some(provided) = bearer_token(headers) else {
        return Err(ApiError::Unauthorized("missing bearer token".into()));
    };

    if api_key_configured && require_api_key(headers, &api.api_key).is_ok() {
        return Ok(AuthPrincipal::ApiKey);
    }

    if let Some(secret) = session_secret {
        if let Ok(claims) = super::session::validate_session_token(&secret, &provided) {
            // Revocation: the token is valid only while its epoch still matches
            // the user's current `session_version`. A logout / password / role
            // change bumps that version, instantly invalidating live tokens.
            // A missing user (deleted) also revokes. DB errors fail closed.
            let current = store
                .get_user_by_id(claims.uid)
                .map_err(ApiError::from)?
                .map(|user| user.session_version);
            if current == Some(claims.ver) {
                return Ok(AuthPrincipal::Local {
                    user_id: claims.uid,
                    username: claims.username,
                    role: claims.role,
                });
            }
            return Err(ApiError::Unauthorized("session has been revoked".into()));
        }
    }

    if let Some(validator) = oidc {
        if let Ok(claims) = validator.validate_token_claims(&provided) {
            let subject = claims
                .get("sub")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            // Resolve roles from the LIVE token claims (not a possibly-stale
            // DB row) so RBAC reflects the IdP's current group membership,
            // including per-organization `{role}:{org}` grants.
            let claimed = super::role::claim_strings(&claims, &api.oidc.role_claim);
            let fallback =
                super::role::AppRole::parse(&api.oidc.default_role).unwrap_or(AppRole::Viewer);
            let roles = super::role::resolve_resolved_roles(
                &claimed,
                &api.oidc.role_admin,
                &api.oidc.role_operator,
                &api.oidc.role_viewer,
                fallback,
            );
            return Ok(AuthPrincipal::Oidc { subject, roles });
        }
    }

    Err(ApiError::Unauthorized("invalid bearer token".into()))
}

/// Bearer-auth gate for the management API (`/api/v1/status`, `/sources`,
/// `/outbox`, `/teams`, `/check*`), applied once as a router layer
/// (`middleware::from_fn_with_state` in [`super::router`]) instead of being
/// copy-pasted into every handler. Fails open when no `api_key`/OIDC/local
/// session is configured — see [`crate::config::ApiConfig::auth_configured`].
pub async fn require_management_auth(
    State(state): State<Arc<AppState>>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, ApiError> {
    require_bearer_auth(
        request.headers(),
        &state.api,
        state.oidc.as_deref(),
        &state.engine.store,
    )?;
    Ok(next.run(request).await)
}

/// Router layer for **mutating** management routes (`/check*`,
/// `/outbox/requeue`, `/notifiers/test`, `/reload`, `/config/validate`): a
/// valid bearer is not enough — the principal must be at least `operator`.
///
/// Reads stay `viewer`-accessible via [`require_management_auth`]; config
/// apply/rollback demand `admin` inside [`require_config_apply_auth`].
pub async fn require_operator_auth(
    State(state): State<Arc<AppState>>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, ApiError> {
    let principal = resolve_auth_principal(
        request.headers(),
        &state.api,
        state.oidc.as_deref(),
        &state.engine.store,
    )?;
    authorize(&principal, AppRole::Operator)?;
    Ok(next.run(request).await)
}

/// Router layer for instance-admin surfaces (user directory / create local user).
pub async fn require_admin_auth(
    State(state): State<Arc<AppState>>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, ApiError> {
    let principal = resolve_auth_principal(
        request.headers(),
        &state.api,
        state.oidc.as_deref(),
        &state.engine.store,
    )?;
    authorize(&principal, AppRole::Admin)?;
    Ok(next.run(request).await)
}

/// Enforce a minimum role. A principal with no role is only ever [`AuthPrincipal::Anonymous`],
/// which occurs solely when no auth is configured (the lab-only when require_auth is false
/// management surface) — so it is allowed; every configured principal carries a
/// concrete role and is compared against `minimum`.
pub fn authorize(principal: &AuthPrincipal, minimum: AppRole) -> AuthResult {
    authorize_for_org(principal, minimum, None)
}

/// [`authorize`] scoped to one organization (`None` = instance-wide). The
/// principal's effective role *for that org* must meet `minimum` — so an OIDC
/// user who is `admin` of `platform` passes an admin check on `platform` even
/// if their global role is lower.
pub fn authorize_for_org(
    principal: &AuthPrincipal,
    minimum: AppRole,
    organization: Option<&str>,
) -> AuthResult {
    match principal.role_for_org(organization) {
        None => Ok(()),
        Some(role) if role.allows(minimum) => Ok(()),
        Some(role) => Err(ApiError::Forbidden(match organization {
            Some(org) => format!(
                "role `{role}` on organization `{org}` is not permitted here \
 (requires `{minimum}` or higher)"
            ),
            None => format!("role `{role}` is not permitted here (requires `{minimum}` or higher)"),
        })),
    }
}

/// Verify `X-Config-Signature: sha256=…` HMAC when a secret is configured.
pub fn verify_config_signature(
    headers: &HeaderMap,
    secret: &Option<String>,
    body: &[u8],
) -> AuthResult {
    verify_hmac_header(headers, secret, body, "X-Config-Signature", true)
}

/// Who performed an authenticated config mutation, as established by the
/// server — never by a client-supplied header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthPrincipal {
    /// Authenticated with the static `api.api_key` — a full-privilege
    /// automation credential, treated as `admin` for RBAC.
    ApiKey,
    /// Authenticated with a local UI session JWT.
    Local {
        user_id: i64,
        username: String,
        role: AppRole,
    },
    /// Authenticated with an OIDC token; carries its `sub` and the roles
    /// (global + per-organization) resolved from the token's group claims.
    Oidc {
        subject: Option<String>,
        roles: super::role::ResolvedRoles,
    },
    /// No authentication is configured at all (unauthenticated management API (require_auth = false)).
    Anonymous,
}

impl AuthPrincipal {
    /// Effective role for a target organization (`None` = whole-document /
    /// instance-wide). `api_key` is admin everywhere; a local user's role is
    /// instance-wide (no per-org grants); an OIDC principal resolves the higher
    /// of its global role and any `{role}:{org}` grant.
    #[must_use]
    pub fn role_for_org(&self, organization: Option<&str>) -> Option<AppRole> {
        match self {
            Self::ApiKey => Some(AppRole::Admin),
            Self::Local { role, .. } => Some(*role),
            Self::Oidc { roles, .. } => Some(roles.for_org(organization)),
            Self::Anonymous => None,
        }
    }

    /// Audit label for `config_revision.applied_by`.
    #[must_use]
    pub fn audit_label(&self) -> Option<String> {
        match self {
            Self::ApiKey => Some("api_key".to_owned()),
            Self::Local { username, .. } => Some(format!("local:{username}")),
            Self::Oidc {
                subject: Some(subject),
                ..
            } => Some(format!("oidc:{subject}")),
            Self::Oidc { subject: None, .. } => Some("oidc".to_owned()),
            Self::Anonymous => None,
        }
    }
}

/// Config apply/rollback auth: management bearer + optional OIDC apply scope + HMAC body.
///
/// Returns the established principal so callers can record *verified*
/// provenance in the ledger rather than trusting `X-Config-Applied-By`.
pub fn require_config_apply_auth(
    state: &AppState,
    headers: &HeaderMap,
    body: &[u8],
    organization: Option<&str>,
) -> Result<AuthPrincipal, ApiError> {
    if !state.config_api.routes_enabled() {
        return Err(ApiError::NotFound("config apply API is disabled".into()));
    }

    // Declared file authority: accepting a push here would hot-swap config the
    // next boot silently discards, since boot reads the app file. Refusing is
    // the honest answer — the caller should edit the file (and `POST /reload`).
    if !state.config_api.accepts_pushed_config() {
        return Err(ApiError::Conflict(
            "config_api.source = \"local\": the app config file is authoritative, \
 so pushed config would be discarded on the next boot — edit the file \
 and POST /api/v1/reload instead"
                .into(),
        ));
    }

    let principal = resolve_auth_principal(
        headers,
        &state.api,
        state.oidc.as_deref(),
        &state.engine.store,
    )?;

    // Config apply/rollback rewrites the running desired state — the highest
    // privilege the API grants. Require `admin` *for the target organization*
    // (`None` = the whole-document instance). ApiKey is admin everywhere; an
    // unconfigured/Anonymous instance stays unauthenticated when require_auth is false, matched by `authorize`.
    authorize_for_org(&principal, AppRole::Admin, organization)?;

    // `apply_scope` is an OIDC-claims concept — it must only gate requests
    // that actually authenticated via an OIDC token. The static `api_key` and
    // local session JWT have no IdP claims to check.
    if let AuthPrincipal::Oidc { .. } = &principal {
        if let Some(scope) = state
            .config_api
            .apply_scope
            .as_ref()
            .filter(|s| !s.is_empty())
        {
            let token = bearer_token(headers)
                .ok_or_else(|| ApiError::Unauthorized("missing bearer token".into()))?;
            let validator = state
                .oidc
                .as_ref()
                .ok_or_else(|| ApiError::Unauthorized("OIDC required for apply_scope".into()))?;
            validator
                .validate_token_with_extra_scope(&token, scope)
                .map_err(ApiError::Unauthorized)?;
        }
    }

    verify_config_signature(headers, &state.config_api.apply_secret(), body)?;
    Ok(principal)
}

/// Whether *some* real authentication guards `POST /api/v1/config/apply` and
/// `/config/rollback` — bearer (api_key or OIDC) and/or the HMAC body
/// signature. Unlike the general management API (which may run without
/// credentials only when `require_auth = false`), config-apply is an opt-in,
/// mutation-only surface: there is no legitimate reason to enable it with zero
/// protection, so the caller of this check fails closed instead.
#[must_use]
pub fn config_apply_auth_configured(bearer_configured: bool, hmac_configured: bool) -> bool {
    bearer_configured || hmac_configured
}

/// Verify GitHub `X-Hub-Signature-256` HMAC when a secret is configured.
pub fn verify_github_signature(
    headers: &HeaderMap,
    secret: &Option<String>,
    body: &[u8],
) -> AuthResult {
    verify_hmac_header(headers, secret, body, "X-Hub-Signature-256", true)
}

/// Verify Bitbucket `X-Hub-Signature` / `X-Hub-Signature-256` HMAC when configured.
///
/// Bitbucket Cloud sends plain hex in `X-Hub-Signature`; some installs also send
/// the GitHub-style `sha256=` prefix on `X-Hub-Signature-256`.
pub fn verify_bitbucket_signature(
    headers: &HeaderMap,
    secret: &Option<String>,
    body: &[u8],
) -> AuthResult {
    if headers.contains_key("X-Hub-Signature") {
        return verify_hmac_header(headers, secret, body, "X-Hub-Signature", false);
    }
    if headers.contains_key("X-Hub-Signature-256") {
        return verify_hmac_header(headers, secret, body, "X-Hub-Signature-256", true);
    }
    verify_hmac_header(headers, secret, body, "X-Hub-Signature", false)
}

/// Verify GitLab `X-Gitlab-Token` when a secret is configured.
pub fn verify_gitlab_token(headers: &HeaderMap, secret: &Option<String>) -> AuthResult {
    let Some(secret) = secret else {
        return Ok(());
    };
    if secret.is_empty() {
        return Ok(());
    }

    let provided = headers
        .get("X-Gitlab-Token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();

    if constant_time_eq(provided.as_bytes(), secret.as_bytes()) {
        Ok(())
    } else {
        Err(ApiError::Unauthorized("invalid X-Gitlab-Token".into()))
    }
}

/// Verify generic webhook secret via `X-Webhook-Secret` or `Authorization: Bearer`.
pub fn verify_webhook_secret(headers: &HeaderMap, secret: &Option<String>) -> AuthResult {
    let Some(secret) = secret else {
        return Ok(());
    };
    if secret.is_empty() {
        return Ok(());
    }

    let header_secret = headers
        .get("X-Webhook-Secret")
        .and_then(|v| v.to_str().ok());
    let bearer = bearer_token(headers);

    let provided = header_secret.or(bearer.as_deref()).unwrap_or_default();
    if constant_time_eq(provided.as_bytes(), secret.as_bytes()) {
        Ok(())
    } else {
        Err(ApiError::Unauthorized("invalid webhook secret".into()))
    }
}

fn verify_hmac_header(
    headers: &HeaderMap,
    secret: &Option<String>,
    body: &[u8],
    header_name: &str,
    sha256_prefix: bool,
) -> AuthResult {
    let Some(secret) = secret else {
        return Ok(());
    };
    if secret.is_empty() {
        return Ok(());
    }

    let Some(header) = headers.get(header_name) else {
        return Err(ApiError::Unauthorized(format!("missing {header_name}")));
    };
    let header = header
        .to_str()
        .map_err(|_| ApiError::Unauthorized("invalid signature header".into()))?;
    let digest_hex = if sha256_prefix {
        header
            .strip_prefix("sha256=")
            .ok_or_else(|| ApiError::Unauthorized("invalid signature format".into()))?
    } else {
        header
    };

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|_| ApiError::Unauthorized("invalid webhook secret".into()))?;
    mac.update(body);
    let expected_hex = hex::encode(mac.finalize().into_bytes());

    if constant_time_eq(digest_hex.as_bytes(), expected_hex.as_bytes()) {
        Ok(())
    } else {
        Err(ApiError::Unauthorized("signature mismatch".into()))
    }
}

pub(crate) fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_owned)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderValue, StatusCode};

    /// Regression: `config_api.api_config = true` with neither bearer auth
    /// (api_key/OIDC) nor an HMAC secret configured left `/config/apply` and
    /// `/config/rollback` completely unauthenticated — and the startup
    /// warning that existed at the time claimed they were still "bearer-only"
    /// in exactly that case. This predicate is what `api::serve` now checks
    /// to refuse starting instead.
    #[test]
    fn audit_label_should_identify_the_verified_principal() {
        assert_eq!(
            AuthPrincipal::Oidc {
                subject: Some("alice@example.com".into()),
                roles: super::super::role::ResolvedRoles::flat(AppRole::Admin),
            }
            .audit_label(),
            Some("oidc:alice@example.com".to_owned())
        );
        assert_eq!(
            AuthPrincipal::ApiKey.audit_label(),
            Some("api_key".to_owned())
        );
        assert_eq!(
            AuthPrincipal::Oidc {
                subject: None,
                roles: super::super::role::ResolvedRoles::flat(AppRole::Viewer),
            }
            .audit_label(),
            Some("oidc".into())
        );
        assert_eq!(AuthPrincipal::Anonymous.audit_label(), None);
    }

    #[test]
    fn oidc_org_scoped_role_should_authorize_that_org_only() {
        // Global viewer, but admin of `platform` via `xrelease-admin:platform`.
        let admin = vec!["xrelease-admin".to_owned()];
        let operator = vec!["xrelease-operator".to_owned()];
        let viewer = vec!["xrelease-viewer".to_owned()];
        let roles = super::super::role::resolve_resolved_roles(
            &["xrelease-admin:platform".to_owned()],
            &admin,
            &operator,
            &viewer,
            AppRole::Viewer,
        );
        let principal = AuthPrincipal::Oidc {
            subject: Some("bob".into()),
            roles,
        };
        // Admin on `platform`…
        assert!(authorize_for_org(&principal, AppRole::Admin, Some("platform")).is_ok());
        // …but only viewer elsewhere and instance-wide.
        assert!(authorize_for_org(&principal, AppRole::Operator, Some("security")).is_err());
        assert!(authorize_for_org(&principal, AppRole::Operator, None).is_err());
    }

    #[test]
    fn authorize_should_enforce_the_role_ladder() {
        // Viewer session cannot reach an operator route; api_key (admin) can.
        let viewer = AuthPrincipal::Local {
            user_id: 1,
            username: "v".into(),
            role: AppRole::Viewer,
        };
        assert!(authorize(&viewer, AppRole::Operator).is_err());
        assert!(authorize(&viewer, AppRole::Viewer).is_ok());
        assert!(authorize(&AuthPrincipal::ApiKey, AppRole::Admin).is_ok());
        // Unconfigured (no credentials) instance: no role, everything allowed.
        assert!(authorize(&AuthPrincipal::Anonymous, AppRole::Admin).is_ok());
    }

    #[test]
    fn config_apply_auth_configured_should_require_bearer_or_hmac() {
        assert!(!config_apply_auth_configured(false, false));
        assert!(config_apply_auth_configured(true, false));
        assert!(config_apply_auth_configured(false, true));
        assert!(config_apply_auth_configured(true, true));
    }

    #[test]
    fn github_signature_should_verify_valid_hmac() {
        let secret = "test-secret";
        let body = br#"{"action":"published"}"#;
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac");
        mac.update(body);
        let sig = hex::encode(mac.finalize().into_bytes());

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Hub-Signature-256",
            HeaderValue::from_str(&format!("sha256={sig}")).expect("header"),
        );

        verify_github_signature(&headers, &Some(secret.into()), body).expect("valid signature");
    }

    #[test]
    fn bitbucket_signature_should_verify_plain_hex() {
        let secret = "test-secret";
        let body = br#"{"push":{}}"#;
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac");
        mac.update(body);
        let sig = hex::encode(mac.finalize().into_bytes());

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Hub-Signature",
            HeaderValue::from_str(&sig).expect("header"),
        );

        verify_bitbucket_signature(&headers, &Some(secret.into()), body).expect("valid signature");
    }

    #[test]
    fn api_key_should_reject_missing_bearer() {
        let headers = HeaderMap::new();
        let err = require_api_key(&headers, &Some("secret".into())).expect_err("should fail");
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn github_signature_should_reject_tampered_body() {
        let secret = "test-secret";
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac");
        mac.update(br#"{"action":"published"}"#);
        let sig = hex::encode(mac.finalize().into_bytes());

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Hub-Signature-256",
            HeaderValue::from_str(&format!("sha256={sig}")).expect("header"),
        );

        let err =
            verify_github_signature(&headers, &Some(secret.into()), br#"{"action":"tampered"}"#)
                .expect_err("tampered body must fail");
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn github_signature_should_reject_missing_header() {
        let headers = HeaderMap::new();
        let err = verify_github_signature(&headers, &Some("secret".into()), b"body")
            .expect_err("missing header must fail");
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn gitlab_token_should_accept_match_and_reject_mismatch() {
        let mut headers = HeaderMap::new();
        headers.insert("X-Gitlab-Token", HeaderValue::from_static("wh-secret"));
        verify_gitlab_token(&headers, &Some("wh-secret".into())).expect("matching token passes");

        let mut wrong = HeaderMap::new();
        wrong.insert("X-Gitlab-Token", HeaderValue::from_static("nope"));
        let err = verify_gitlab_token(&wrong, &Some("wh-secret".into()))
            .expect_err("mismatched token fails");
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn webhook_secret_should_skip_when_unset_and_enforce_when_set() {
        let headers = HeaderMap::new();
        // No secret configured → always passes (opt-in verification).
        verify_webhook_secret(&headers, &None).expect("unset secret skips");

        // Secret configured but header absent → reject.
        let err = verify_webhook_secret(&headers, &Some("s3cr3t".into()))
            .expect_err("missing secret fails");
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);

        // Correct secret via header → pass.
        let mut ok = HeaderMap::new();
        ok.insert("X-Webhook-Secret", HeaderValue::from_static("s3cr3t"));
        verify_webhook_secret(&ok, &Some("s3cr3t".into())).expect("matching secret passes");
    }
}
