//! AES-256-GCM envelope encryption for **app_secret** values.
//!
//! Wire format (UTF-8 text column `app_secret.ciphertext`):
//!
//! ```text
//! xrenc1:<base64(nonce || ciphertext || tag)>
//! ```
//!
//! Desired-document structure stays plaintext in `config_revision.content`
//! with `*_env` refs only. Secret **values** are sealed here.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use getrandom::getrandom;
use zeroize::ZeroizeOnDrop;

use crate::error::StoreError;

/// Prefix marking an encrypted secret blob (version 1).
pub const LEDGER_BLOB_PREFIX: &str = "xrenc1:";

/// Env var: 32-byte AES key (base64 or hex) for sealing `app_secret` values.
pub const ENCRYPTION_KEY_ENV: &str = "XRELEASE_CONFIG_ENCRYPTION_KEY";

/// Lab-only escape hatch when `source = api` must boot without a ledger key.
pub const ALLOW_PLAINTEXT_ENV: &str = "XRELEASE_ALLOW_PLAINTEXT_CONFIG_LEDGER";

const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

/// Process-wide cipher for sealing / opening per-secret ciphertext.
#[derive(Clone, ZeroizeOnDrop)]
pub struct LedgerCipher {
    key: [u8; KEY_LEN],
    #[zeroize(skip)]
    cipher: Aes256Gcm,
}

impl std::fmt::Debug for LedgerCipher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LedgerCipher")
            .field("key", &"<redacted>")
            .finish()
    }
}

impl LedgerCipher {
    /// Build from a raw 32-byte key.
    #[must_use]
    pub fn from_key(key: [u8; KEY_LEN]) -> Self {
        let cipher = Aes256Gcm::new_from_slice(&key).expect("AES-256-GCM accepts 32-byte keys");
        Self { key, cipher }
    }

    /// Parse a key from Base64 (32 decoded bytes) or hex (64 hex chars).
    ///
    /// # Errors
    /// Returns [`StoreError::Other`] when the encoding or length is wrong.
    pub fn parse_key(raw: &str) -> Result<[u8; KEY_LEN], StoreError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(StoreError::Other(format!("{ENCRYPTION_KEY_ENV} is empty")));
        }
        if trimmed.len() == KEY_LEN * 2 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
            let mut key = [0_u8; KEY_LEN];
            hex::decode_to_slice(trimmed, &mut key).map_err(|err| {
                StoreError::Other(format!("{ENCRYPTION_KEY_ENV} hex decode: {err}"))
            })?;
            return Ok(key);
        }
        let decoded = B64.decode(trimmed).map_err(|err| {
            StoreError::Other(format!(
                "{ENCRYPTION_KEY_ENV} must be 32-byte base64 or 64-char hex: {err}"
            ))
        })?;
        let key: [u8; KEY_LEN] = decoded.try_into().map_err(|bytes: Vec<u8>| {
            StoreError::Other(format!(
                "{ENCRYPTION_KEY_ENV} must decode to {KEY_LEN} bytes, got {}",
                bytes.len()
            ))
        })?;
        Ok(key)
    }

    /// Load from [`ENCRYPTION_KEY_ENV`] when set.
    ///
    /// # Errors
    /// Returns an error when the env var is set but invalid. `Ok(None)` when unset.
    pub fn from_env() -> Result<Option<Self>, StoreError> {
        match std::env::var(ENCRYPTION_KEY_ENV) {
            Ok(value) if !value.trim().is_empty() => {
                Ok(Some(Self::from_key(Self::parse_key(&value)?)))
            }
            Ok(_) | Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => Err(StoreError::Other(format!(
                "{ENCRYPTION_KEY_ENV} is not valid UTF-8"
            ))),
        }
    }

    /// Whether [`ENCRYPTION_KEY_ENV`] is set to a non-empty value (not validated).
    #[must_use]
    pub fn key_env_present() -> bool {
        std::env::var(ENCRYPTION_KEY_ENV)
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
    }

    /// Whether operators opted into a plaintext secret ledger (lab escape hatch).
    #[must_use]
    pub fn plaintext_ledger_allowed() -> bool {
        match std::env::var(ALLOW_PLAINTEXT_ENV) {
            Ok(value) => matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            ),
            Err(_) => false,
        }
    }

    /// Whether `stored` looks like an encrypted blob.
    #[must_use]
    pub fn is_encrypted(stored: &str) -> bool {
        stored.starts_with(LEDGER_BLOB_PREFIX)
    }

    /// Seal a secret value into the `xrenc1:` wire format.
    ///
    /// # Errors
    /// Returns [`StoreError::Other`] on AEAD failure.
    pub fn seal(&self, plaintext: &str) -> Result<String, StoreError> {
        let mut nonce_bytes = [0_u8; NONCE_LEN];
        getrandom(&mut nonce_bytes)
            .map_err(|err| StoreError::Other(format!("ledger nonce: {err}")))?;
        let nonce = Nonce::from(nonce_bytes);
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|err| StoreError::Other(format!("ledger encrypt: {err}")))?;
        let mut packed = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        packed.extend_from_slice(&nonce_bytes);
        packed.extend_from_slice(&ciphertext);
        Ok(format!("{LEDGER_BLOB_PREFIX}{}", B64.encode(packed)))
    }

    /// Open a sealed secret blob.
    ///
    /// # Errors
    /// Returns [`StoreError::Other`] when the blob is corrupt, unprefixed, or the key is wrong.
    pub fn open(&self, stored: &str) -> Result<String, StoreError> {
        let Some(payload) = stored.strip_prefix(LEDGER_BLOB_PREFIX) else {
            return Err(StoreError::Other(
                "app_secret ciphertext missing xrenc1: prefix".into(),
            ));
        };
        let packed = B64
            .decode(payload.trim())
            .map_err(|err| StoreError::Other(format!("ledger ciphertext base64: {err}")))?;
        if packed.len() <= NONCE_LEN {
            return Err(StoreError::Other(
                "ledger ciphertext truncated (missing nonce)".into(),
            ));
        }
        let (nonce_bytes, ciphertext) = packed.split_at(NONCE_LEN);
        let nonce_arr: [u8; NONCE_LEN] = nonce_bytes
            .try_into()
            .map_err(|_| StoreError::Other("ledger ciphertext truncated (nonce length)".into()))?;
        let nonce = Nonce::from(nonce_arr);
        let plain = self.cipher.decrypt(&nonce, ciphertext).map_err(|_| {
            StoreError::Other(format!(
                "ledger decrypt failed — wrong {ENCRYPTION_KEY_ENV} or corrupt secret"
            ))
        })?;
        String::from_utf8(plain)
            .map_err(|err| StoreError::Other(format!("ledger plaintext is not UTF-8: {err}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_key() -> [u8; KEY_LEN] {
        *b"0123456789abcdef0123456789abcdef"
    }

    #[test]
    fn seal_open_should_round_trip() {
        let cipher = LedgerCipher::from_key(sample_key());
        let sealed = cipher.seal("xoxb-secret").expect("seal");
        assert!(sealed.starts_with(LEDGER_BLOB_PREFIX));
        assert!(!sealed.contains("xoxb"));
        assert_eq!(cipher.open(&sealed).expect("open"), "xoxb-secret");
    }

    #[test]
    fn open_should_reject_plaintext() {
        let cipher = LedgerCipher::from_key(sample_key());
        assert!(cipher.open("not-encrypted").is_err());
    }

    #[test]
    fn wrong_key_should_fail_open() {
        let a = LedgerCipher::from_key(sample_key());
        let mut other = sample_key();
        other[0] ^= 0xff;
        let b = LedgerCipher::from_key(other);
        let sealed = a.seal("secret").expect("seal");
        assert!(b.open(&sealed).is_err());
    }

    #[test]
    fn parse_key_should_accept_hex_and_base64() {
        let hex_key = "00".repeat(KEY_LEN);
        assert_eq!(
            LedgerCipher::parse_key(&hex_key).expect("hex"),
            [0_u8; KEY_LEN]
        );
        let b64 = B64.encode(sample_key());
        assert_eq!(LedgerCipher::parse_key(&b64).expect("b64"), sample_key());
    }
}
