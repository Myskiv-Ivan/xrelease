//! PostgreSQL connection settings (`[database]`).

use serde::{Deserialize, Serialize};

/// How the PostgreSQL client negotiates TLS.
///
/// Can also be set via the URL query (`?sslmode=require`) or
/// `XRELEASE_DATABASE_SSL_MODE`. Explicit config / env wins over the URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum DatabaseSslMode {
    /// Plain TCP (Compose / private CNI default).
    #[default]
    Disable,
    /// Encrypt if the server offers TLS. Implemented like `require` (TLS connector
    /// only — no plaintext fallback), matching libpq's practical use for managed DBs.
    Prefer,
    /// Require TLS; do not verify the server certificate.
    Require,
    /// Require TLS and verify the server certificate against the CA store /
    /// [`DatabaseConfig::ssl_root_cert`].
    VerifyCa,
    /// Like `verify-ca`, plus hostname verification against the certificate.
    VerifyFull,
}

impl DatabaseSslMode {
    /// Parse libpq-style `sslmode` values (case-insensitive).
    pub fn parse_libpq(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "disable" => Some(Self::Disable),
            "allow" | "prefer" => Some(Self::Prefer),
            "require" => Some(Self::Require),
            "verify-ca" => Some(Self::VerifyCa),
            "verify-full" => Some(Self::VerifyFull),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disable => "disable",
            Self::Prefer => "prefer",
            Self::Require => "require",
            Self::VerifyCa => "verify-ca",
            Self::VerifyFull => "verify-full",
        }
    }

    /// Whether a TLS connector is used (anything except `disable`).
    #[must_use]
    pub fn uses_tls(self) -> bool {
        !matches!(self, Self::Disable)
    }
}

/// Storage backend for release state (PostgreSQL only).
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConfig {
    /// PostgreSQL connection URL.
    #[serde(default)]
    pub postgres_url: String,
    /// TLS mode. When unset, falls back to `sslmode` in the URL, else `disable`.
    #[serde(default)]
    pub ssl_mode: Option<DatabaseSslMode>,
    /// PEM CA bundle for `verify-ca` / `verify-full` (optional — system CAs used otherwise).
    #[serde(default)]
    pub ssl_root_cert: Option<String>,
    /// Maximum pooled connections (`None` = built-in default of 16). Tune to the
    /// number of concurrent sources/replicas without exceeding the server's
    /// `max_connections`.
    #[serde(default)]
    pub max_connections: Option<u32>,
    /// Seconds to wait for a connection before failing fast (`None` = 5). Keeps
    /// startup probes and tests from hanging when the database is unreachable.
    #[serde(default)]
    pub connect_timeout_secs: Option<u32>,
    /// Drop `seen_release` rows older than N days (`0` = disabled).
    #[serde(default)]
    pub prune_seen_after_days: u32,
    /// Drop `webhook_delivery` rows older than N days (`0` = disabled).
    #[serde(default)]
    pub prune_webhooks_after_days: u32,
    /// Drop sent `notification_outbox` rows older than N days (`0` = disabled).
    #[serde(default)]
    pub prune_outbox_sent_after_days: u32,
    /// Drop `release_advisory` rows older than N days (`0` = disabled).
    ///
    /// Independent of `prune_seen_after_days`: advisories are keyed by package
    /// coordinate, not by source, so they do not share `seen_release`'s
    /// lifecycle — several sources across organizations can reference the same
    /// `(ecosystem, package, version)` cache entry.
    #[serde(default)]
    pub prune_advisories_after_days: u32,
    /// Run [`Store::prune`] on this interval while `serve` is up (`0` = startup only).
    #[serde(default)]
    pub prune_interval_hours: u32,
}

impl DatabaseConfig {
    /// Effective TLS mode: explicit field → URL `sslmode` → `disable`.
    #[must_use]
    pub fn effective_ssl_mode(&self) -> DatabaseSslMode {
        if let Some(mode) = self.ssl_mode {
            return mode;
        }
        ssl_mode_from_url(&self.postgres_url).unwrap_or(DatabaseSslMode::Disable)
    }
}

/// Read `sslmode` from a postgres URL query string (if present).
fn ssl_mode_from_url(url: &str) -> Option<DatabaseSslMode> {
    let query = url.split_once('?')?.1;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=')?;
        if key.eq_ignore_ascii_case("sslmode") {
            return DatabaseSslMode::parse_libpq(value);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_ssl_mode_prefers_explicit_over_url() {
        let cfg = DatabaseConfig {
            postgres_url: "postgres://u:p@h/db?sslmode=require".into(),
            ssl_mode: Some(DatabaseSslMode::Disable),
            ..Default::default()
        };
        assert_eq!(cfg.effective_ssl_mode(), DatabaseSslMode::Disable);
    }

    #[test]
    fn effective_ssl_mode_reads_url_when_unset() {
        let cfg = DatabaseConfig {
            postgres_url: "postgres://u:p@h/db?sslmode=verify-full".into(),
            ..Default::default()
        };
        assert_eq!(cfg.effective_ssl_mode(), DatabaseSslMode::VerifyFull);
    }

    #[test]
    fn effective_ssl_mode_defaults_to_disable() {
        let cfg = DatabaseConfig {
            postgres_url: "postgres://u:p@h/db".into(),
            ..Default::default()
        };
        assert_eq!(cfg.effective_ssl_mode(), DatabaseSslMode::Disable);
    }
}
