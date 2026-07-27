//! Public auth endpoints: local login, OIDC user sync, session introspection,
//! and admin user directory.

use std::sync::{Arc, OnceLock};

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::{Deserialize, Serialize};

use std::collections::BTreeMap;

use super::auth::{bearer_token, resolve_auth_principal, AuthPrincipal};
use super::error::ApiError;
use super::password::{hash_password, verify_password};
use super::role::{claim_strings, resolve_resolved_roles, AppRole};
use super::session::issue_session_token;
use super::AppState;
use crate::store::{AppUser, AppUserInsert, AppUserUpsertOidc};

/// Minimum length for passwords created via the admin UI / API.
const MIN_PASSWORD_LEN: usize = 8;

/// A valid Argon2 PHC hash of a random string, computed once. Verifying a
/// submitted password against this when no eligible user exists keeps the login
/// path's timing uniform, so an attacker cannot distinguish "no such user" from
/// "wrong password" (username enumeration via a timing oracle).
fn decoy_password_hash() -> &'static str {
    static DECOY: OnceLock<String> = OnceLock::new();
    DECOY.get_or_init(|| hash_password("xrelease-timing-equalizer").unwrap_or_default())
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub role: AppRole,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
}

/// Admin body for `POST /api/v1/auth/users/{id}/oidc`.
///
/// `email` is the SSO link key: the address the IdP will assert for this
/// person. Blank / null clears both the link key and any bound subject.
///
/// Email rather than `oidc_sub` because an admin provisioning an account ahead
/// of first login knows the person's address but not the IdP's opaque subject
/// — that only exists once they have signed in at least once. The subject is
/// bound automatically on their first OIDC sign-in, provided the token carries
/// `email_verified`.
///
/// `oidc_sub` stays accepted so an admin can still pin or clear a subject
/// directly (and so existing automation keeps working).
#[derive(Debug, Deserialize)]
pub struct LinkOidcRequest {
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub oidc_sub: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AuthUserView {
    pub id: i64,
    pub username: Option<String>,
    pub email: Option<String>,
    pub display_name: Option<String>,
    /// Instance-wide (global) role. Per-org grants live in [`Self::organization_roles`].
    pub role: AppRole,
    /// `local` | `oidc` — how the row was created / how it authenticates.
    pub auth_source: String,
    /// IdP subject when the user was created or linked via OIDC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oidc_sub: Option<String>,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
    /// RFC 3339 last successful login, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_login_at: Option<String>,
    /// OIDC `{role}:{org}` grants. Empty for local / api-key principals.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub organization_roles: BTreeMap<String, AppRole>,
}

#[derive(Debug, Serialize)]
pub struct UserListResponse {
    pub users: Vec<AuthUserView>,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub token_type: &'static str,
    pub expires_in: u64,
    pub role: AppRole,
    pub user: AuthUserView,
}

#[derive(Debug, Serialize)]
pub struct AuthMethodsResponse {
    pub local: bool,
    pub oidc: bool,
    pub api_key: bool,
    /// `[api.oidc] auto_create_users`. Surfaced so the UI can explain a denied
    /// SSO sign-in ("an admin must create your account first") instead of
    /// showing a bare 403, and so Settings can display the policy.
    pub oidc_auto_create_users: bool,
}

#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub method: &'static str,
    /// Instance-wide role. Api-key principals are always `admin`.
    pub role: Option<AppRole>,
    /// Per-organization grants from OIDC scoped aliases (`admin:platform`).
    /// Empty when the principal has no org-scoped claims (local, api-key, bare OIDC).
    #[serde(default)]
    pub organization_roles: BTreeMap<String, AppRole>,
    pub user: Option<AuthUserView>,
}

/// `GET /api/v1/auth/methods` — which login paths the UI should offer.
pub async fn auth_methods(State(state): State<Arc<AppState>>) -> Json<AuthMethodsResponse> {
    Json(AuthMethodsResponse {
        local: state.api.local_auth_configured(),
        oidc: state.oidc.is_some(),
        api_key: state
            .api
            .api_key
            .as_ref()
            .is_some_and(|key| !key.is_empty()),
        oidc_auto_create_users: state.api.oidc.auto_create_users,
    })
}

/// `POST /api/v1/auth/login` — local username/password → session JWT.
pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, ApiError> {
    if !state.api.local_auth.enabled {
        return Err(ApiError::Unauthorized("local auth is disabled".into()));
    }
    let secret = state.api.local_auth.session_secret().ok_or_else(|| {
        ApiError::Unauthorized(
            "local auth has no session secret — set XRELEASE_SESSION_SECRET".into(),
        )
    })?;

    let username = body.username.trim();
    if username.is_empty() || body.password.is_empty() {
        return Err(ApiError::Unauthorized(
            "invalid username or password".into(),
        ));
    }

    // Resolve the eligible password hash, then ALWAYS run one Argon2 verify —
    // against a decoy when the user is absent or not a local account — so every
    // failure path takes comparable time (no username enumeration via timing).
    let user_row = state.engine.store.get_user_by_username(username)?;
    let (has_local_hash, password_ok) = {
        let stored_hash = user_row
            .as_ref()
            .filter(|user| user.auth_source == "local")
            .and_then(|user| user.password_hash.as_deref());
        let ok = verify_password(
            &body.password,
            stored_hash.unwrap_or_else(|| decoy_password_hash()),
        );
        (stored_hash.is_some(), ok)
    };
    let Some(user) = user_row.filter(|_| has_local_hash && password_ok) else {
        return Err(ApiError::Unauthorized(
            "invalid username or password".into(),
        ));
    };

    let role = AppRole::parse(&user.role).unwrap_or(AppRole::Viewer);
    let (token, expires_in) = issue_session_token(
        secret,
        user.id,
        username,
        role,
        user.session_version,
        state.api.local_auth.session_ttl_secs,
    )
    .map_err(ApiError::internal)?;

    let _ = state.engine.store.touch_user_last_login(user.id);

    Ok(Json(LoginResponse {
        access_token: token,
        token_type: "Bearer",
        expires_in,
        role,
        user: user_view(&user, role, BTreeMap::new()),
    }))
}

/// `POST /api/v1/auth/oidc/sync` — validate OIDC bearer, upsert user + role.
pub async fn oidc_sync(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<AuthUserView>, ApiError> {
    let validator = state
        .oidc
        .as_ref()
        .ok_or_else(|| ApiError::Unauthorized("OIDC is not configured".into()))?;
    let token = bearer_token(&headers)
        .ok_or_else(|| ApiError::Unauthorized("missing bearer token".into()))?;

    let claims = validator
        .validate_token_claims(&token)
        .map_err(ApiError::Unauthorized)?;

    let oidc_sub = claims
        .get("sub")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::Unauthorized("OIDC token missing sub".into()))?;

    let email = claims
        .get("email")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    // Adopting a pre-created local account by email is an account-takeover
    // vector when the IdP never proved the address, so the claim is required
    // (and must be the boolean `true` — some IdPs send the string "true",
    // which we accept, but a missing/false claim blocks linking).
    let email_verified = claims
        .get("email_verified")
        .map(|v| v.as_bool() == Some(true) || v.as_str() == Some("true"))
        .unwrap_or(false);
    let display_name = claims
        .get("name")
        .and_then(|v| v.as_str())
        .or_else(|| claims.get("preferred_username").and_then(|v| v.as_str()))
        .filter(|s| !s.is_empty())
        .or(email);

    let claimed = claim_strings(&claims, &state.api.oidc.role_claim);
    let fallback = AppRole::parse(&state.api.oidc.default_role).unwrap_or(AppRole::Viewer);
    let roles = resolve_resolved_roles(
        &claimed,
        &state.api.oidc.role_admin,
        &state.api.oidc.role_operator,
        &state.api.oidc.role_viewer,
        fallback,
    );

    // Persist the global role only — per-org grants are live-claim and enforced
    // at request time; the DB row is for audit / local listing, not authZ.
    let user = state
        .engine
        .store
        .upsert_oidc_user(&AppUserUpsertOidc {
            oidc_sub,
            email,
            display_name,
            role: roles.global.as_str(),
            link_by_email: email_verified,
            allow_create: state.api.oidc.auto_create_users,
        })?
        .ok_or_else(|| {
            ApiError::Forbidden(
                "no account for this identity and OIDC auto-provisioning is disabled — \
                 ask an admin to create a local user with your email address"
                    .into(),
            )
        })?;

    Ok(Json(user_view(&user, roles.global, roles.per_org.clone())))
}

/// `GET /api/v1/auth/me` — current principal (behind management auth).
pub async fn me(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<MeResponse>, ApiError> {
    let principal = resolve_auth_principal(
        &headers,
        &state.api,
        state.oidc.as_deref(),
        &state.engine.store,
    )?;
    match principal {
        // Api-key is admin everywhere (no per-org map). Surface `admin` so the
        // UI does not depend on a Vite default that can drift from the server.
        AuthPrincipal::ApiKey => Ok(Json(MeResponse {
            method: "api_key",
            role: Some(AppRole::Admin),
            organization_roles: BTreeMap::new(),
            user: None,
        })),
        AuthPrincipal::Local {
            user_id,
            username: _,
            role,
        } => {
            let user = state.engine.store.get_user_by_id(user_id)?;
            Ok(Json(MeResponse {
                method: "local",
                role: Some(role),
                organization_roles: BTreeMap::new(),
                user: user.map(|u| user_view(&u, role, BTreeMap::new())),
            }))
        }
        AuthPrincipal::Oidc { subject, roles, .. } => {
            // Roles come from the live token claims (resolved at auth time),
            // not the possibly-stale DB row. Expose both global and per-org so
            // the UI can gate org-scoped config write without guessing claims.
            let role = roles.global;
            let organization_roles = roles.per_org.clone();
            let user = match subject.as_deref() {
                Some(sub) => state.engine.store.get_user_by_oidc_sub(sub)?,
                None => None,
            };
            Ok(Json(MeResponse {
                method: "oidc",
                role: Some(role),
                organization_roles: organization_roles.clone(),
                user: user.map(|u| user_view(&u, role, organization_roles)),
            }))
        }
        AuthPrincipal::Anonymous => Ok(Json(MeResponse {
            method: "anonymous",
            role: None,
            organization_roles: BTreeMap::new(),
            user: None,
        })),
    }
}

/// `POST /api/v1/auth/logout` — revoke the caller's live sessions.
///
/// Bumps the local user's `session_version`, so **every** JWT issued to them
/// (this device and any others) is rejected on its next request. A no-op for
/// api-key/OIDC callers (their credentials are not server-session-backed).
pub async fn logout(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let principal = resolve_auth_principal(
        &headers,
        &state.api,
        state.oidc.as_deref(),
        &state.engine.store,
    )?;
    if let AuthPrincipal::Local { user_id, .. } = principal {
        state.engine.store.bump_session_version(user_id)?;
        return Ok(Json(serde_json::json!({ "revoked": true })));
    }
    Ok(Json(serde_json::json!({ "revoked": false })))
}

/// `GET /api/v1/auth/users` — admin directory of local + OIDC users.
pub async fn list_users(
    State(state): State<Arc<AppState>>,
) -> Result<Json<UserListResponse>, ApiError> {
    let users = state
        .engine
        .store
        .list_users()?
        .iter()
        .map(|user| {
            let role = AppRole::parse(&user.role).unwrap_or(AppRole::Viewer);
            user_view(user, role, BTreeMap::new())
        })
        .collect();
    Ok(Json(UserListResponse { users }))
}

/// `POST /api/v1/auth/users` — create a local username/password user (admin).
pub async fn create_user(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateUserRequest>,
) -> Result<Json<AuthUserView>, ApiError> {
    if !state.api.local_auth.enabled {
        return Err(ApiError::BadRequest(
            "local auth is disabled — cannot create password users".into(),
        ));
    }
    if state.api.local_auth.session_secret.is_none() {
        return Err(ApiError::BadRequest(
            "local auth has no session secret — set XRELEASE_SESSION_SECRET".into(),
        ));
    }

    let username = body.username.trim();
    if username.is_empty() {
        return Err(ApiError::BadRequest("username is required".into()));
    }
    if username.len() > 64 || username.chars().any(char::is_whitespace) {
        return Err(ApiError::BadRequest(
            "username must be 1–64 characters without whitespace".into(),
        ));
    }
    if body.password.len() < MIN_PASSWORD_LEN {
        return Err(ApiError::BadRequest(format!(
            "password must be at least {MIN_PASSWORD_LEN} characters"
        )));
    }

    if state.engine.store.get_user_by_username(username)?.is_some() {
        return Err(ApiError::Conflict(format!(
            "username `{username}` already exists"
        )));
    }

    let password_hash = hash_password(&body.password).map_err(ApiError::internal)?;
    let email = body
        .email
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let display_name = body
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or(Some(username));

    let user = state.engine.store.insert_user(&AppUserInsert {
        username: Some(username),
        password_hash: Some(&password_hash),
        oidc_sub: None,
        email,
        display_name,
        role: body.role.as_str(),
        auth_source: "local",
    })?;

    Ok(Json(user_view(&user, body.role, BTreeMap::new())))
}

/// `POST /api/v1/auth/users/{id}/oidc` — link or unlink an IdP subject on a
/// **local** user so the same row authenticates via password and SSO.
pub async fn link_user_oidc(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<i64>,
    Json(body): Json<LinkOidcRequest>,
) -> Result<Json<AuthUserView>, ApiError> {
    let sub = body
        .oidc_sub
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    // Email path: the admin names the address the IdP will assert, and the
    // subject binds itself on first sign-in. Only when no explicit subject was
    // given — a caller pinning `oidc_sub` still wins.
    if sub.is_none() && body.oidc_sub.is_none() {
        let email = body
            .email
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let user = state
            .engine
            .store
            .set_user_oidc_link_email(user_id, email)
            .map_err(|err| {
                let message = err.to_string();
                if message.contains("no local app_user") {
                    ApiError::NotFound(message)
                } else if message.contains("already used by user") {
                    ApiError::Conflict(message)
                } else {
                    ApiError::from(err)
                }
            })?;
        let role = AppRole::parse(&user.role).unwrap_or(AppRole::Viewer);
        return Ok(Json(user_view(&user, role, BTreeMap::new())));
    }

    if let Some(sub) = sub {
        if let Some(owner) = state.engine.store.get_user_by_oidc_sub(sub)? {
            if owner.id != user_id {
                return Err(ApiError::Conflict(format!(
                    "oidc_sub `{sub}` is already linked to user {}",
                    owner.id
                )));
            }
        }
    }

    let user = state
        .engine
        .store
        .link_user_oidc_sub(user_id, sub)
        .map_err(|err| {
            let message = err.to_string();
            if message.contains("no local app_user") {
                ApiError::NotFound(message)
            } else if message.contains("already linked") {
                ApiError::Conflict(message)
            } else {
                ApiError::from(err)
            }
        })?;
    let role = AppRole::parse(&user.role).unwrap_or(AppRole::Viewer);
    Ok(Json(user_view(&user, role, BTreeMap::new())))
}

fn user_view(
    user: &AppUser,
    role: AppRole,
    organization_roles: BTreeMap<String, AppRole>,
) -> AuthUserView {
    AuthUserView {
        id: user.id,
        username: user.username.clone(),
        email: user.email.clone(),
        display_name: user.display_name.clone(),
        role,
        auth_source: user.auth_source.clone(),
        oidc_sub: user.oidc_sub.clone(),
        created_at: user.created_at.to_rfc3339(),
        last_login_at: user.last_login_at.map(|ts| ts.to_rfc3339()),
        organization_roles,
    }
}
