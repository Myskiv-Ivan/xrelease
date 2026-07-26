//! `app_secret` — encrypted UI / API secret store keyed by env-var name.

use sha2::{Digest, Sha256};
use std::collections::HashMap;

use super::PostgresStore;
use crate::config::{vault_remove, vault_replace_all, vault_upsert, SecretWrite};
use crate::crypto::LedgerCipher;
use crate::store::StoreError;

impl PostgresStore {
    /// Load every `app_secret` into the process vault (call after schema apply).
    pub(crate) fn load_app_secrets_into_vault(&self) -> Result<(), StoreError> {
        let mut client = self.conn()?;
        let rows = client.query("SELECT name, ciphertext FROM app_secret", &[])?;
        let mut entries = HashMap::with_capacity(rows.len());
        for row in rows {
            let name: String = row.get(0);
            let ciphertext: String = row.get(1);
            let value = self.open_secret_ciphertext(&name, &ciphertext)?;
            entries.insert(name, value);
        }
        vault_replace_all(entries);
        Ok(())
    }

    /// Upsert sealed secrets and refresh the process vault entries.
    pub(crate) fn upsert_app_secrets(&self, writes: &[SecretWrite]) -> Result<(), StoreError> {
        if writes.is_empty() {
            return Ok(());
        }
        if self.ledger_cipher.is_none() && !LedgerCipher::plaintext_ledger_allowed() {
            return Err(StoreError::Other(
                "inline config secrets require XRELEASE_CONFIG_ENCRYPTION_KEY \
                 (or XRELEASE_ALLOW_PLAINTEXT_CONFIG_LEDGER=1 for lab only)"
                    .into(),
            ));
        }

        let mut client = self.conn()?;
        let mut tx = client.transaction()?;
        for write in writes {
            let name = write.name.trim();
            if name.is_empty() || write.value.trim().is_empty() {
                continue;
            }
            let digest = format!("{:x}", Sha256::digest(write.value.as_bytes()));
            // Skip seal+write when the plaintext digest is unchanged.
            let unchanged: bool = tx
                .query_opt(
                    "SELECT 1 FROM app_secret WHERE name = $1 AND value_sha256 = $2",
                    &[&name, &digest],
                )?
                .is_some();
            if unchanged {
                vault_upsert(name, write.value.clone());
                continue;
            }
            let ciphertext = match self.ledger_cipher.as_deref() {
                Some(cipher) => cipher.seal(&write.value)?,
                None => write.value.clone(),
            };
            tx.execute(
                "INSERT INTO app_secret (name, ciphertext, value_sha256, updated_at) \
                 VALUES ($1, $2, $3, now()) \
                 ON CONFLICT (name) DO UPDATE SET \
                   ciphertext = EXCLUDED.ciphertext, \
                   value_sha256 = EXCLUDED.value_sha256, \
                   updated_at = now()",
                &[&name, &ciphertext, &digest],
            )?;
            vault_upsert(name, write.value.clone());
        }
        tx.commit()?;
        Ok(())
    }

    /// Delete sealed secrets and drop them from the process vault (orphan GC).
    pub(crate) fn delete_app_secrets(&self, names: &[String]) -> Result<usize, StoreError> {
        if names.is_empty() {
            return Ok(0);
        }
        let mut client = self.conn()?;
        let mut tx = client.transaction()?;
        let mut deleted = 0usize;
        for name in names {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            let n = tx.execute("DELETE FROM app_secret WHERE name = $1", &[&name])?;
            if n > 0 {
                deleted += n as usize;
                vault_remove(name);
            }
        }
        tx.commit()?;
        Ok(deleted)
    }

    fn open_secret_ciphertext(&self, name: &str, ciphertext: &str) -> Result<String, StoreError> {
        if LedgerCipher::is_encrypted(ciphertext) {
            let Some(cipher) = self.ledger_cipher.as_deref() else {
                return Err(StoreError::Other(format!(
                    "app_secret `{name}` is encrypted but XRELEASE_CONFIG_ENCRYPTION_KEY is unset"
                )));
            };
            return cipher.open(ciphertext);
        }
        Ok(ciphertext.to_owned())
    }
}
