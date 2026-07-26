//! Process-local secret vault backed by `app_secret` (and optionally process env).
//!
//! Desired documents store only `*_env` **names**. Values are sealed in Postgres
//! and cached here after [`crate::store::Store`] load / upsert so
//! [`crate::config::env_token`] resolves UI-managed secrets the same way as
//! Kubernetes-injected env vars.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

fn vault() -> &'static RwLock<HashMap<String, String>> {
    static VAULT: OnceLock<RwLock<HashMap<String, String>>> = OnceLock::new();
    VAULT.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Replace the in-memory vault (called after loading `app_secret` from Postgres).
pub fn vault_replace_all(entries: HashMap<String, String>) {
    if let Ok(mut guard) = vault().write() {
        *guard = entries;
    }
}

/// Upsert one plaintext secret into the in-memory vault.
pub fn vault_upsert(name: impl Into<String>, value: impl Into<String>) {
    let name = name.into();
    let value = value.into();
    if name.trim().is_empty() || value.trim().is_empty() {
        return;
    }
    if let Ok(mut guard) = vault().write() {
        guard.insert(name, value);
    }
}

/// Remove one entry from the in-memory vault (after `app_secret` delete).
pub fn vault_remove(name: &str) {
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    if let Ok(mut guard) = vault().write() {
        guard.remove(name);
    }
}

/// Look up a secret by env-var name.
#[must_use]
pub fn vault_get(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    vault()
        .read()
        .ok()
        .and_then(|guard| guard.get(name).cloned())
        .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_should_round_trip() {
        vault_replace_all(HashMap::new());
        vault_upsert("XRELEASE_TEST_VAULT", "secret-value");
        assert_eq!(
            vault_get("XRELEASE_TEST_VAULT").as_deref(),
            Some("secret-value")
        );
        vault_remove("XRELEASE_TEST_VAULT");
        assert!(vault_get("XRELEASE_TEST_VAULT").is_none());
        vault_replace_all(HashMap::new());
    }
}
