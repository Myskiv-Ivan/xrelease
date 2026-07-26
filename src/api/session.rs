//! HS256 session JWTs for local UI login.

use std::time::{SystemTime, UNIX_EPOCH};

use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use super::role::AppRole;

const SESSION_ISS: &str = "xrelease";
const SESSION_TYP: &str = "session";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionClaims {
    pub sub: String,
    pub iss: String,
    /// Discriminator so OIDC tokens are never accepted as sessions.
    pub typ: String,
    pub uid: i64,
    pub role: AppRole,
    pub username: String,
    /// Session epoch the token was issued under; revoked when it no longer
    /// matches the user's `app_user.session_version`.
    pub ver: i64,
    pub exp: u64,
    pub iat: u64,
}

/// Issue a session JWT for a local user.
pub fn issue_session_token(
    secret: &str,
    user_id: i64,
    username: &str,
    role: AppRole,
    session_version: i64,
    ttl_secs: u64,
) -> Result<(String, u64), String> {
    let now = now_secs();
    let ttl = ttl_secs.max(60);
    let claims = SessionClaims {
        sub: format!("local:{user_id}"),
        iss: SESSION_ISS.to_owned(),
        typ: SESSION_TYP.to_owned(),
        uid: user_id,
        role,
        username: username.to_owned(),
        ver: session_version,
        iat: now,
        exp: now.saturating_add(ttl),
    };
    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|err| err.to_string())?;
    Ok((token, ttl))
}

/// Validate a session JWT and return its claims.
pub fn validate_session_token(secret: &str, token: &str) -> Result<SessionClaims, String> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_issuer(&[SESSION_ISS]);
    validation.validate_exp = true;
    let decoded = decode::<SessionClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|err| err.to_string())?;
    if decoded.claims.typ != SESSION_TYP {
        return Err("not a session token".into());
    }
    if decoded.claims.username.is_empty() || decoded.claims.uid <= 0 {
        return Err("invalid session claims".into());
    }
    Ok(decoded.claims)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_token_should_round_trip() {
        let (token, ttl) =
            issue_session_token("test-secret", 7, "admin", AppRole::Admin, 3, 3600).expect("issue");
        assert_eq!(ttl, 3600);
        let claims = validate_session_token("test-secret", &token).expect("validate");
        assert_eq!(claims.uid, 7);
        assert_eq!(claims.username, "admin");
        assert_eq!(claims.role, AppRole::Admin);
        assert_eq!(claims.ver, 3);
    }

    #[test]
    fn session_token_should_reject_wrong_secret() {
        let (token, _) =
            issue_session_token("test-secret", 1, "admin", AppRole::Admin, 0, 3600).expect("issue");
        assert!(validate_session_token("other", &token).is_err());
    }
}
