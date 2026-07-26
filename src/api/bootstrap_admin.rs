//! First-boot local admin seed.

use tracing::{info, warn};

use super::password::hash_password;
use crate::config::ApiConfig;
use crate::error::StoreError;
use crate::store::{AppUserInsert, Store};

/// Ensure exactly one bootstrap local admin exists when the user table is empty.
///
/// Fails **closed**: an admin is seeded only when local auth is fully
/// configured — a dedicated session secret AND an explicit admin password.
/// There is no default password, so a half-configured instance can never boot
/// with guessable `admin`/`admin` credentials.
pub fn ensure_bootstrap_admin(store: &Store, api: &ApiConfig) -> Result<(), StoreError> {
    if !api.local_auth.enabled {
        return Ok(());
    }
    if api.local_auth.session_secret().is_none() {
        // No signing secret ⇒ login can't work anyway; don't seed a user.
        return Ok(());
    }
    if store.count_users()? > 0 {
        return Ok(());
    }

    let username = api.local_auth.admin_username.trim();
    if username.is_empty() {
        return Err(StoreError::Other(
            "api.local_auth.admin_username must not be empty when seeding the first admin".into(),
        ));
    }
    let Some(password) = api.local_auth.admin_password() else {
        warn!(
            user = %username,
            "local auth is enabled with a session secret but no admin password — set \
             XRELEASE_ADMIN_PASSWORD (or [api.local_auth].admin_password) to seed the first admin"
        );
        return Ok(());
    };
    let password_hash = hash_password(password).map_err(StoreError::Other)?;

    store.insert_user(&AppUserInsert {
        username: Some(username),
        password_hash: Some(&password_hash),
        oidc_sub: None,
        email: None,
        display_name: Some(username),
        role: "admin",
        auth_source: "local",
    })?;

    info!(user = %username, "seeded local admin user on first start");
    Ok(())
}
