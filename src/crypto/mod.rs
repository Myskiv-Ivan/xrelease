//! Cryptographic helpers for at-rest protection of persisted secrets.
//!
//! [`ledger`] seals `app_secret` values with AES-256-GCM. Desired-document
//! structure in `config_revision.content` keeps only `*_env` refs.

pub mod ledger;

pub use ledger::LedgerCipher;
