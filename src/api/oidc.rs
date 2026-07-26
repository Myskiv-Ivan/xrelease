//! OIDC JWT bearer validation — RFC 7519 (JWT), RFC 8414 (discovery), RFC 8725 (BCP).

use std::sync::RwLock;
use std::time::{Duration, Instant};

use jsonwebtoken::jwk::{Jwk, JwkSet};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use reqwest::Client;
use serde::Deserialize;
use tracing::warn;

use crate::config::OidcConfig;

const JWKS_REFRESH_INTERVAL: Duration = Duration::from_secs(300);
/// Poll cadence for the background refresh task. Shorter than
/// [`JWKS_REFRESH_INTERVAL`] so an *empty* cache (IdP unreachable at startup)
/// recovers in seconds instead of minutes; a populated cache is still only
/// re-fetched once it goes stale.
pub const JWKS_RETRY_INTERVAL: Duration = Duration::from_secs(30);
const FORBIDDEN_ALGORITHMS: &[&str] = &["none"];

#[derive(Debug, Deserialize)]
struct DiscoveryDocument {
    jwks_uri: String,
}

/// Cached JWKS validator for OIDC access tokens presented as Bearer credentials.
pub struct OidcValidator {
    issuer: String,
    audiences: Vec<String>,
    /// Scope required in `scope`/`scp` claims (authorization gate); `None` = authN only.
    required_scope: Option<String>,
    /// RFC 8414 discovery endpoint, used when `jwks_uri` is not configured
    /// explicitly and to re-resolve it if discovery was unreachable at startup.
    discovery_url: String,
    /// `None` until discovery resolves it (or it is configured directly).
    jwks_uri: RwLock<Option<String>>,
    http: Client,
    jwks: RwLock<CachedJwks>,
}

struct CachedJwks {
    set: JwkSet,
    /// `None` until the first *successful* fetch, which makes the cache
    /// permanently stale so the background task keeps retrying.
    fetched_at: Option<Instant>,
}

impl OidcValidator {
    /// Build the validator from config and try to warm the JWKS cache.
    ///
    /// An unreachable identity provider is **not** a startup failure: the
    /// validator is returned with an empty cache and the background refresh
    /// task (see [`JWKS_RETRY_INTERVAL`]) keeps retrying. Until keys arrive,
    /// JWT validation fails closed (`401`) while polling, delivery, and
    /// api-key auth keep working — a transient IdP outage must not take the
    /// whole backend down.
    pub async fn try_from_config(
        config: &OidcConfig,
        http: Client,
    ) -> anyhow::Result<Option<Self>> {
        let Some(issuer) = config.issuer.as_ref().filter(|value| !value.is_empty()) else {
            return Ok(None);
        };

        let issuer = issuer.trim_end_matches('/').to_owned();
        let discovery_base = config
            .discovery_url
            .as_ref()
            .filter(|value| !value.is_empty())
            .map(|value| value.trim_end_matches('/').to_owned())
            .unwrap_or_else(|| issuer.clone());

        let validator = Self {
            issuer,
            audiences: config.audience.clone(),
            required_scope: config
                .required_scope
                .clone()
                .filter(|scope| !scope.trim().is_empty()),
            discovery_url: format!("{discovery_base}/.well-known/openid-configuration"),
            jwks_uri: RwLock::new(
                config
                    .jwks_uri
                    .as_ref()
                    .filter(|value| !value.is_empty())
                    .map(|uri| uri.trim().to_owned()),
            ),
            http,
            jwks: RwLock::new(CachedJwks {
                set: JwkSet { keys: Vec::new() },
                fetched_at: None,
            }),
        };

        validator.refresh_now().await;
        if !validator.has_keys() {
            warn!(
                issuer = %validator.issuer,
                retry_secs = JWKS_RETRY_INTERVAL.as_secs(),
                "OIDC JWKS unavailable at startup — JWT auth will reject tokens \
                 until the identity provider is reachable; retrying in background"
            );
        }
        Ok(Some(validator))
    }

    /// Whether the cache currently holds any verification key.
    #[must_use]
    pub fn has_keys(&self) -> bool {
        self.jwks
            .read()
            .map(|cache| !cache.set.keys.is_empty())
            .unwrap_or(false)
    }

    /// Refresh JWKS when stale, or on every tick while the cache is empty.
    pub async fn refresh_if_stale(&self) {
        let should_refresh = self
            .jwks
            .read()
            .map(|cache| {
                cache
                    .fetched_at
                    .is_none_or(|at| at.elapsed() >= JWKS_REFRESH_INTERVAL)
            })
            .unwrap_or(true);

        if !should_refresh {
            return;
        }
        self.refresh_now().await;
    }

    /// Fetch JWKS unconditionally, resolving the URI via discovery when needed.
    async fn refresh_now(&self) {
        let Some(uri) = self.resolve_jwks_uri().await else {
            return;
        };
        match self.http.get(&uri).send().await {
            Ok(response) => match response.json::<JwkSet>().await {
                Ok(jwks) => {
                    let keys = jwks.keys.len();
                    if let Ok(mut guard) = self.jwks.write() {
                        guard.set = jwks;
                        guard.fetched_at = Some(Instant::now());
                    }
                    tracing::debug!(keys, uri = %uri, "JWKS refreshed");
                }
                Err(err) => warn!(error = %err, "failed to parse JWKS"),
            },
            Err(err) => warn!(error = %err, "failed to refresh JWKS"),
        }
    }

    /// Current `jwks_uri`, resolving it through RFC 8414 discovery on first use.
    ///
    /// Discovery is retried here (not only at startup) so an IdP that was down
    /// when the process booted still converges without a restart.
    async fn resolve_jwks_uri(&self) -> Option<String> {
        {
            // Guard dropped before any await — std locks must not cross one.
            let guard = self.jwks_uri.read().ok()?;
            if let Some(uri) = guard.as_ref() {
                return Some(uri.clone());
            }
        }

        let discovered = match self.http.get(&self.discovery_url).send().await {
            Ok(response) => match response.json::<DiscoveryDocument>().await {
                Ok(document) => document.jwks_uri,
                Err(err) => {
                    warn!(error = %err, url = %self.discovery_url, "failed to parse OIDC discovery document");
                    return None;
                }
            },
            Err(err) => {
                warn!(error = %err, url = %self.discovery_url, "OIDC discovery unreachable");
                return None;
            }
        };

        if let Ok(mut guard) = self.jwks_uri.write() {
            *guard = Some(discovered.clone());
        }
        Some(discovered)
    }

    /// Validate bearer JWT per RFC 7519 + RFC 8725 (reject `none`).
    pub fn validate_token(&self, token: &str) -> Result<(), String> {
        self.validate_token_subject(token).map(|_| ())
    }

    /// Validate as [`Self::validate_token`] and return decoded claims.
    pub fn validate_token_claims(&self, token: &str) -> Result<serde_json::Value, String> {
        let claims = self.decode_and_validate(token)?;
        if let Some(required) = &self.required_scope {
            if !scope_satisfied(&claims, required) {
                return Err(format!("token missing required scope `{required}`"));
            }
        }
        Ok(claims)
    }

    /// Validate as [`Self::validate_token`] and return the token's `sub` claim.
    ///
    /// Used for audit provenance: a config apply's `applied_by` must be
    /// derived from the verified token, never from a client-supplied header.
    /// `None` means the token verified but carried no `sub`.
    pub fn validate_token_subject(&self, token: &str) -> Result<Option<String>, String> {
        let claims = self.decode_and_validate(token)?;
        if let Some(required) = &self.required_scope {
            if !scope_satisfied(&claims, required) {
                return Err(format!("token missing required scope `{required}`"));
            }
        }
        Ok(subject_of(&claims))
    }

    /// Validate token and require an additional scope (config apply).
    pub fn validate_token_with_extra_scope(
        &self,
        token: &str,
        extra_scope: &str,
    ) -> Result<(), String> {
        let claims = self.decode_and_validate(token)?;
        if let Some(required) = &self.required_scope {
            if !scope_satisfied(&claims, required) {
                return Err(format!("token missing required scope `{required}`"));
            }
        }
        if !scope_satisfied(&claims, extra_scope) {
            return Err(format!("token missing required scope `{extra_scope}`"));
        }
        Ok(())
    }

    fn decode_and_validate(&self, token: &str) -> Result<serde_json::Value, String> {
        let header = match decode_header(token) {
            Ok(header) => header,
            Err(err) => {
                let msg = err.to_string();
                if forbidden_alg_in_decode_error(&msg) {
                    return Err("forbidden JWT algorithm: none".to_owned());
                }
                return Err(msg);
            }
        };
        let alg = format!("{:?}", header.alg);
        if FORBIDDEN_ALGORITHMS
            .iter()
            .any(|forbidden| alg.eq_ignore_ascii_case(forbidden))
        {
            return Err(format!("forbidden JWT algorithm: {alg}"));
        }

        let algorithm = match header.alg {
            jsonwebtoken::Algorithm::RS256 => Algorithm::RS256,
            jsonwebtoken::Algorithm::RS384 => Algorithm::RS384,
            jsonwebtoken::Algorithm::RS512 => Algorithm::RS512,
            other => return Err(format!("unsupported JWT algorithm: {other:?}")),
        };

        let jwks = self
            .jwks
            .read()
            .map_err(|_| "JWKS lock poisoned".to_owned())?;
        if jwks.set.keys.is_empty() {
            // Distinguish "IdP never reached" from a genuine kid mismatch —
            // otherwise an unreachable provider looks like a bad token.
            return Err(
                "JWKS cache is empty (identity provider unreachable); retrying in background"
                    .to_owned(),
            );
        }
        let kid = header.kid.as_deref();
        let jwk = find_verification_jwk(&jwks.set, kid, &alg)
            .ok_or_else(|| "no matching JWK".to_owned())?;

        let decoding_key =
            DecodingKey::from_jwk(jwk).map_err(|err| format!("invalid JWK: {err}"))?;

        let mut validation = Validation::new(algorithm);
        validation.set_issuer(&[self.issuer.as_str()]);
        validation.validate_exp = true;
        validation.validate_nbf = true;
        if self.audiences.is_empty() {
            // No audience configured → do not reject tokens that carry an `aud`
            // claim. `Validation::new` defaults `validate_aud = true`, which
            // would otherwise fail every token containing an audience.
            validation.validate_aud = false;
        } else {
            let aud: Vec<&str> = self.audiences.iter().map(String::as_str).collect();
            validation.set_audience(&aud);
        }

        let decoded = decode::<serde_json::Value>(token, &decoding_key, &validation)
            .map_err(|err| err.to_string())?;

        Ok(decoded.claims)
    }
}

/// Whether the token claims carry `required` as a scope — in the OAuth 2
/// `scope` claim (space-separated string, RFC 6749 §3.3 / RFC 8693) or the
/// `scp` claim (JSON array of strings, Azure AD style).
/// Best-effort human-identifying claim, preferring `sub` (the stable subject
/// identifier every OIDC provider issues) and falling back to the friendlier
/// `preferred_username`/`email` only when `sub` is absent.
fn subject_of(claims: &serde_json::Value) -> Option<String> {
    for key in ["sub", "preferred_username", "email"] {
        if let Some(value) = claims.get(key).and_then(|v| v.as_str()) {
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        }
    }
    None
}

fn scope_satisfied(claims: &serde_json::Value, required: &str) -> bool {
    if let Some(scope) = claims.get("scope").and_then(|v| v.as_str()) {
        if scope.split_ascii_whitespace().any(|s| s == required) {
            return true;
        }
    }
    if let Some(scp) = claims.get("scp").and_then(|v| v.as_array()) {
        if scp.iter().any(|v| v.as_str() == Some(required)) {
            return true;
        }
    }
    false
}

fn find_verification_jwk<'a>(jwks: &'a JwkSet, kid: Option<&str>, alg: &str) -> Option<&'a Jwk> {
    if let Some(kid) = kid {
        // RFC 8725: when the token names a key, only that key may verify it.
        // Falling back to an alg-match / first key would mask rotation and
        // `kid` mismatches instead of surfacing them as auth failures.
        return jwks
            .keys
            .iter()
            .find(|key| key.common.key_id.as_deref() == Some(kid));
    }
    jwks.keys
        .iter()
        .find(|key| {
            key.common
                .key_algorithm
                .as_ref()
                .is_some_and(|a| a.to_string() == alg)
        })
        .or_else(|| jwks.keys.first())
}

fn forbidden_alg_in_decode_error(msg: &str) -> bool {
    FORBIDDEN_ALGORITHMS
        .iter()
        .any(|alg| msg.contains(&format!("unknown variant `{alg}`")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subject_of_should_prefer_sub_then_fall_back() {
        let claims = serde_json::json!({ "sub": "u-123", "email": "a@b.c" });
        assert_eq!(subject_of(&claims), Some("u-123".to_owned()));

        let no_sub = serde_json::json!({ "preferred_username": "alice" });
        assert_eq!(subject_of(&no_sub), Some("alice".to_owned()));

        let empty_sub = serde_json::json!({ "sub": "", "email": "a@b.c" });
        assert_eq!(subject_of(&empty_sub), Some("a@b.c".to_owned()));

        assert_eq!(subject_of(&serde_json::json!({})), None);
    }

    #[test]
    fn forbidden_none_algorithm_is_rejected() {
        let validator = test_validator(JwkSet { keys: vec![] });
        let header = "eyJhbGciOiJub25lIn0.eyJzdWIiOiIxIn0.";
        let err = validator
            .validate_token(&format!("{header}signature"))
            .unwrap_err();
        assert!(err.contains("forbidden") || err.contains("Invalid"));
    }

    fn test_validator(set: JwkSet) -> OidcValidator {
        OidcValidator {
            issuer: "https://issuer.example".to_owned(),
            audiences: vec!["client".to_owned()],
            required_scope: None,
            discovery_url: "https://issuer.example/.well-known/openid-configuration".to_owned(),
            jwks_uri: RwLock::new(Some("https://issuer.example/jwks".to_owned())),
            http: Client::new(),
            jwks: RwLock::new(CachedJwks {
                set,
                fetched_at: None,
            }),
        }
    }

    #[test]
    fn validator_with_empty_jwks_should_report_provider_unreachable() {
        // Regression: an IdP that was down at startup must surface as its own
        // error, not as a generic "no matching JWK" token failure.
        let validator = test_validator(JwkSet { keys: vec![] });
        assert!(!validator.has_keys());
        // RS256 header so the check reaches the JWKS lookup.
        let token = "eyJhbGciOiJSUzI1NiIsImtpZCI6ImsxIn0.eyJzdWIiOiIxIn0.sig";
        let err = validator.validate_token(token).unwrap_err();
        assert!(
            err.contains("JWKS cache is empty"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn scope_satisfied_should_match_space_separated_scope_claim() {
        let claims = serde_json::json!({ "scope": "openid xrelease:manage profile" });
        assert!(scope_satisfied(&claims, "xrelease:manage"));
    }

    #[test]
    fn scope_satisfied_should_not_match_substring_of_another_scope() {
        let claims = serde_json::json!({ "scope": "xrelease:manage-all" });
        assert!(!scope_satisfied(&claims, "xrelease:manage"));
    }

    #[test]
    fn scope_satisfied_should_match_scp_array_claim() {
        let claims = serde_json::json!({ "scp": ["openid", "xrelease:manage"] });
        assert!(scope_satisfied(&claims, "xrelease:manage"));
    }

    #[test]
    fn scope_satisfied_should_fail_when_claims_carry_no_scopes() {
        let claims = serde_json::json!({ "sub": "user-1" });
        assert!(!scope_satisfied(&claims, "xrelease:manage"));
    }

    #[test]
    fn find_verification_jwk_should_not_fall_back_when_kid_is_unmatched() {
        // One key with kid "other" in the set; token asks for kid "missing".
        let jwks: JwkSet = serde_json::from_value(serde_json::json!({
            "keys": [{
                "kty": "RSA",
                "kid": "other",
                "alg": "RS256",
                "n": "AQAB",
                "e": "AQAB"
            }]
        }))
        .expect("jwks");
        assert!(find_verification_jwk(&jwks, Some("missing"), "RS256").is_none());
    }

    #[test]
    fn find_verification_jwk_should_match_exact_kid() {
        let jwks: JwkSet = serde_json::from_value(serde_json::json!({
            "keys": [{
                "kty": "RSA",
                "kid": "key-1",
                "alg": "RS256",
                "n": "AQAB",
                "e": "AQAB"
            }]
        }))
        .expect("jwks");
        assert!(find_verification_jwk(&jwks, Some("key-1"), "RS256").is_some());
    }
}
