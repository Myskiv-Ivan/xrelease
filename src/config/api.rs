//! HTTP API and webhook ingress settings (`[api]`, `[api.oidc]`, `[api.local_auth]`).

use serde::{Deserialize, Serialize};

fn default_api_listen() -> String {
    "127.0.0.1:8080".to_owned()
}

fn default_rate_limit() -> u32 {
    120
}

fn default_admin_username() -> String {
    "admin".to_owned()
}

fn default_session_ttl_secs() -> u64 {
    86_400
}

fn default_true() -> bool {
    true
}

fn default_role_claim() -> String {
    "groups".to_owned()
}

fn default_oidc_role_admin() -> Vec<String> {
    vec!["xrelease-admin".into(), "admin".into()]
}

fn default_oidc_role_operator() -> Vec<String> {
    vec!["xrelease-operator".into(), "operator".into()]
}

fn default_oidc_role_viewer() -> Vec<String> {
    vec!["xrelease-viewer".into(), "viewer".into()]
}

fn default_oidc_fallback_role() -> String {
    "viewer".to_owned()
}

/// HTTP API and webhook ingress settings.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ApiConfig {
    /// Bind address for the HTTP server (`xrelease serve`).
    #[serde(default = "default_api_listen")]
    pub listen: String,
    /// Shared secret for webhook signature verification (GitHub HMAC, GitLab token).
    #[serde(default)]
    pub webhook_secret: Option<String>,
    /// Bearer token required for management endpoints (`/api/v1/*`) — CLI / automation.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Max HTTP requests per minute for `/api/v1/*` (webhooks + management).
    /// `0` disables limiting. Default `120`.
    #[serde(default = "default_rate_limit")]
    pub rate_limit_per_minute: u32,
    /// Allowed browser origins for CORS (read-only UI dev/prod).
    /// Example: `["http://localhost:5173"]` for SvelteKit dev server.
    #[serde(default)]
    pub cors_origins: Vec<String>,
    /// OIDC / JWT bearer validation for management API (RFC 6750, RFC 7519).
    #[serde(default)]
    pub oidc: OidcConfig,
    /// Local username/password UI auth (first-boot admin seed + session JWT).
    #[serde(default)]
    pub local_auth: LocalAuthConfig,
    /// When true (default), `xrelease serve` refuses to start unless
    /// management-API authentication is configured (`api_key`, `oidc.issuer`,
    /// or local auth with a session secret). Set `false` only for trusted
    /// lab networks — a loud startup warning is still emitted when open.
    #[serde(default = "default_true")]
    pub require_auth: bool,
}

/// Local UI login settings (`[api.local_auth]`).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LocalAuthConfig {
    /// When true (default), password login + first-boot admin seed are active.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Username seeded on first start when `app_user` is empty.
    #[serde(default = "default_admin_username")]
    pub admin_username: String,
    /// Password for the first-boot admin (also `XRELEASE_ADMIN_PASSWORD`).
    /// No default: when unset the admin is **not** seeded (fail closed) rather
    /// than created with a guessable password.
    #[serde(default)]
    pub admin_password: Option<String>,
    /// HS256 secret for UI session JWTs (`XRELEASE_SESSION_SECRET`).
    /// Required for local login — it is **not** derived from `api_key`, which
    /// is a shared automation credential and must not double as a JWT key.
    #[serde(default)]
    pub session_secret: Option<String>,
    /// Session JWT lifetime in seconds (default 24h).
    #[serde(default = "default_session_ttl_secs")]
    pub session_ttl_secs: u64,
}

impl Default for LocalAuthConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            admin_username: default_admin_username(),
            admin_password: None,
            session_secret: None,
            session_ttl_secs: default_session_ttl_secs(),
        }
    }
}

impl LocalAuthConfig {
    /// Explicitly configured bootstrap password, if any. `None` means "do not
    /// seed an admin" — there is deliberately no default.
    #[must_use]
    pub fn admin_password(&self) -> Option<&str> {
        self.admin_password
            .as_deref()
            .filter(|value| !value.is_empty())
    }

    /// Secret used to sign session JWTs, if one is explicitly configured.
    #[must_use]
    pub fn session_secret(&self) -> Option<&str> {
        self.session_secret
            .as_deref()
            .filter(|value| !value.is_empty())
    }
}

/// OpenID Connect JWT validation settings for `/api/v1/*` bearer tokens.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OidcConfig {
    /// OIDC issuer URL (must match JWT `iss` claim).
    #[serde(default)]
    pub issuer: Option<String>,
    /// Discovery base URL when it differs from `issuer` (e.g. internal
    /// Keycloak service URL while `issuer` stays the browser-facing HTTPS URL).
    #[serde(default)]
    pub discovery_url: Option<String>,
    /// JWKS endpoint override when discovery returns a host unreachable from this process.
    #[serde(default)]
    pub jwks_uri: Option<String>,
    /// Accepted `aud` values (client IDs). When empty, audience is not checked.
    #[serde(default)]
    pub audience: Vec<String>,
    /// Scope the token must carry to access the management API (authorization,
    /// not just authentication). Checked against the `scope` claim
    /// (space-separated string, RFC 6749/8693) or the `scp` claim (array,
    /// Azure AD style). When unset, any valid token from the issuer is
    /// accepted.
    #[serde(default)]
    pub required_scope: Option<String>,
    /// JWT claim path for roles/groups (default `groups`). Dot path supported.
    #[serde(default = "default_role_claim")]
    pub role_claim: String,
    /// IdP role/group values that map to `admin`.
    #[serde(default = "default_oidc_role_admin")]
    pub role_admin: Vec<String>,
    /// IdP role/group values that map to `operator`.
    #[serde(default = "default_oidc_role_operator")]
    pub role_operator: Vec<String>,
    /// IdP role/group values that map to `viewer`.
    #[serde(default = "default_oidc_role_viewer")]
    pub role_viewer: Vec<String>,
    /// Role assigned when no mapped claim matches (default `viewer`).
    #[serde(default = "default_oidc_fallback_role")]
    pub default_role: String,
    /// Create an `app_user` row the first time an unknown IdP subject signs in.
    ///
    /// Default `true` keeps the historical behaviour (any valid token from the
    /// issuer provisions an account). Set `false` to make the user directory an
    /// allow-list: a token then only authenticates if it already resolves to a
    /// row — by `oidc_sub`, or by a verified email matching a local account an
    /// admin created up front. Unknown subjects are rejected instead of
    /// silently provisioned.
    #[serde(default = "default_true")]
    pub auto_create_users: bool,
}

impl Default for OidcConfig {
    fn default() -> Self {
        Self {
            issuer: None,
            discovery_url: None,
            jwks_uri: None,
            audience: Vec::new(),
            required_scope: None,
            role_claim: default_role_claim(),
            role_admin: default_oidc_role_admin(),
            role_operator: default_oidc_role_operator(),
            role_viewer: default_oidc_role_viewer(),
            default_role: default_oidc_fallback_role(),
            auto_create_users: default_true(),
        }
    }
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            listen: default_api_listen(),
            webhook_secret: None,
            api_key: None,
            rate_limit_per_minute: default_rate_limit(),
            cors_origins: Vec::new(),
            oidc: OidcConfig::default(),
            local_auth: LocalAuthConfig::default(),
            require_auth: true,
        }
    }
}

impl ApiConfig {
    /// Whether any management-API authentication is configured — a static
    /// `api_key`, an OIDC issuer, or enabled local UI auth with a session secret.
    /// When false, `/api/v1/*` is served unauthenticated and only the network
    /// boundary protects it.
    #[must_use]
    pub fn auth_configured(&self) -> bool {
        self.api_key.as_ref().is_some_and(|key| !key.is_empty())
            || self
                .oidc
                .issuer
                .as_ref()
                .is_some_and(|issuer| !issuer.is_empty())
            || self.local_auth_configured()
    }

    /// Local password login is active and a dedicated session signing secret is
    /// configured (never derived from `api_key`).
    #[must_use]
    pub fn local_auth_configured(&self) -> bool {
        self.local_auth.enabled && self.local_auth.session_secret().is_some()
    }

    /// Whether webhook ingress is protected by a shared secret / HMAC. When
    /// false, `/api/v1/webhooks/*` accepts unauthenticated POSTs.
    #[must_use]
    pub fn webhook_auth_configured(&self) -> bool {
        self.webhook_secret
            .as_ref()
            .is_some_and(|secret| !secret.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_auth_configured_should_be_false_by_default() {
        // Local auth is enabled by default but has no session secret until
        // XRELEASE_SESSION_SECRET / [api.local_auth].session_secret is set.
        assert!(!ApiConfig::default().auth_configured());
    }

    #[test]
    fn api_require_auth_should_default_true() {
        assert!(ApiConfig::default().require_auth);
        let config: crate::config::Config = toml::from_str("[api]\n").expect("empty api table");
        assert!(config.api.require_auth);
    }

    #[test]
    fn api_auth_configured_should_detect_api_key() {
        let api = ApiConfig {
            api_key: Some("secret".into()),
            ..ApiConfig::default()
        };
        assert!(api.auth_configured());
        // `api_key` alone does NOT enable local login: the session secret is
        // never derived from it (a shared automation credential must not double
        // as the JWT signing key).
        assert!(!api.local_auth_configured());
    }

    #[test]
    fn api_auth_configured_should_ignore_empty_api_key() {
        let api = ApiConfig {
            api_key: Some(String::new()),
            ..ApiConfig::default()
        };
        assert!(!api.auth_configured());
    }

    #[test]
    fn api_auth_configured_should_detect_oidc_issuer() {
        let mut api = ApiConfig::default();
        api.oidc.issuer = Some("https://issuer.example".into());
        assert!(api.auth_configured());
    }

    #[test]
    fn api_auth_configured_should_detect_session_secret() {
        let mut api = ApiConfig::default();
        api.local_auth.session_secret = Some("session-secret".into());
        assert!(api.auth_configured());
        assert!(api.local_auth_configured());
    }

    #[test]
    fn api_webhook_auth_configured_should_track_secret_presence() {
        let with_secret = ApiConfig {
            webhook_secret: Some("s3cr3t".into()),
            ..ApiConfig::default()
        };
        assert!(with_secret.webhook_auth_configured());
    }

    #[test]
    fn api_webhook_auth_configured_should_be_false_when_unset() {
        assert!(!ApiConfig::default().webhook_auth_configured());
    }
}
