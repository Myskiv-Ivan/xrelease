//! Security-advisory enrichment settings (`[advisories]`).
//!
//! Bootstrap-only, like `[database]` and `[api]`: this is an outbound endpoint,
//! not desired state, so `ensure_desired_only` rejects it inside an applied
//! document.

use serde::{Deserialize, Serialize};

/// Public OSV API root.
pub const DEFAULT_OSV_ENDPOINT: &str = "https://api.osv.dev";

/// Default per-lookup timeout. Deliberately short: the lookup sits on the
/// delivery path and a notification must never wait long on enrichment.
pub const DEFAULT_ADVISORY_TIMEOUT_SECS: u32 = 5;

/// How long a `(ecosystem, name, version)` answer is reused.
///
/// Advisories for an *already-published* version change rarely, and the cost of
/// being an hour stale is far lower than re-querying on every retry.
pub const DEFAULT_ADVISORY_CACHE_TTL_SECS: u32 = 3_600;

/// Consecutive lookup failures before OSV is skipped entirely.
pub const DEFAULT_ADVISORY_BREAKER_THRESHOLD: u32 = 5;

/// How long OSV stays skipped after the breaker opens.
pub const DEFAULT_ADVISORY_BREAKER_COOLDOWN_SECS: u32 = 300;

/// How often the background sweep looks for never-checked versions.
///
/// Hourly rather than continuous: the sweep exists to close the gap left by
/// releases that never triggered a notification (everything a baseline poll
/// caught), and that backlog is static — nothing is gained by revisiting it
/// faster than upstream publishes.
pub const DEFAULT_ADVISORY_SWEEP_INTERVAL_SECS: u32 = 3_600;

/// Versions looked up per source per sweep round.
///
/// Deliberately small. A first sweep of a large instance would otherwise fire
/// `sources × seen releases` queries at a third-party API in one burst; at this
/// rate a source with 200 synced releases converges in under a day and then
/// costs nothing, because every verified answer is remembered.
pub const DEFAULT_ADVISORY_SWEEP_BATCH: u32 = 10;

/// `[advisories]` — annotate notifications with known CVEs / GHSAs.
///
/// **Disabled by default.** Enabling it sends the package names and versions
/// this instance watches to a third-party API. For a self-hosted tool that is a
/// disclosure the operator must opt into knowingly, not a default.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AdvisoryConfig {
    /// Look up advisories for newly released package versions.
    #[serde(default)]
    pub enabled: bool,
    /// OSV API root. Override to point at a self-hosted mirror.
    #[serde(default = "default_endpoint")]
    pub endpoint: String,
    /// Per-lookup timeout in seconds.
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u32,
    /// Cache lifetime for one `(ecosystem, name, version)` answer, in seconds.
    /// `0` disables caching.
    #[serde(default = "default_cache_ttl_secs")]
    pub cache_ttl_secs: u32,
    /// Consecutive failures before the breaker opens.
    #[serde(default = "default_breaker_threshold")]
    pub breaker_threshold: u32,
    /// Breaker cooldown in seconds.
    #[serde(default = "default_breaker_cooldown_secs")]
    pub breaker_cooldown_secs: u32,
    /// Seconds between background sweep rounds. `0` disables the sweep.
    ///
    /// With the sweep off, enrichment only ever covers versions that produced a
    /// notification, plus whatever a source-detail page fills in on demand —
    /// so everything a baseline poll caught stays permanently unchecked.
    #[serde(default = "default_sweep_interval_secs")]
    pub sweep_interval_secs: u32,
    /// Versions looked up per source per sweep round. `0` disables the sweep.
    #[serde(default = "default_sweep_batch")]
    pub sweep_batch: u32,
}

fn default_endpoint() -> String {
    DEFAULT_OSV_ENDPOINT.to_owned()
}

const fn default_timeout_secs() -> u32 {
    DEFAULT_ADVISORY_TIMEOUT_SECS
}

const fn default_cache_ttl_secs() -> u32 {
    DEFAULT_ADVISORY_CACHE_TTL_SECS
}

const fn default_breaker_threshold() -> u32 {
    DEFAULT_ADVISORY_BREAKER_THRESHOLD
}

const fn default_breaker_cooldown_secs() -> u32 {
    DEFAULT_ADVISORY_BREAKER_COOLDOWN_SECS
}

const fn default_sweep_interval_secs() -> u32 {
    DEFAULT_ADVISORY_SWEEP_INTERVAL_SECS
}

const fn default_sweep_batch() -> u32 {
    DEFAULT_ADVISORY_SWEEP_BATCH
}

impl Default for AdvisoryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: default_endpoint(),
            timeout_secs: default_timeout_secs(),
            cache_ttl_secs: default_cache_ttl_secs(),
            breaker_threshold: default_breaker_threshold(),
            breaker_cooldown_secs: default_breaker_cooldown_secs(),
            sweep_interval_secs: default_sweep_interval_secs(),
            sweep_batch: default_sweep_batch(),
        }
    }
}

impl AdvisoryConfig {
    /// Whether lookups should run: explicitly enabled with a usable endpoint.
    ///
    /// A blank endpoint disables rather than fails — a half-configured mirror
    /// must not take the delivery path down with it.
    #[must_use]
    pub fn active(&self) -> bool {
        self.enabled && !self.endpoint.trim().is_empty()
    }

    /// Endpoint without a trailing slash, so path joins stay canonical.
    #[must_use]
    pub fn endpoint_base(&self) -> &str {
        self.endpoint.trim().trim_end_matches('/')
    }

    /// Whether the background sweep should run: enrichment active, with a
    /// non-zero interval **and** a non-zero per-round batch.
    ///
    /// Either knob at `0` is a complete off switch — a sweep that wakes up to
    /// do nothing, or one scheduled to never wake, are both just "off", and
    /// treating them differently would leave a task spinning for no reason.
    #[must_use]
    pub fn sweep_active(&self) -> bool {
        self.active() && self.sweep_interval_secs > 0 && self.sweep_batch > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_should_be_disabled() {
        let config = AdvisoryConfig::default();
        assert!(
            !config.enabled,
            "must be opt-in — it discloses package names"
        );
        assert!(!config.active());
    }

    #[test]
    fn active_should_require_both_flag_and_endpoint() {
        let mut config = AdvisoryConfig {
            enabled: true,
            ..AdvisoryConfig::default()
        };
        assert!(config.active());

        config.endpoint = "   ".into();
        assert!(
            !config.active(),
            "a blank endpoint must disable, not fail the delivery path"
        );
    }

    #[test]
    fn endpoint_base_should_strip_trailing_slashes() {
        let config = AdvisoryConfig {
            endpoint: " https://osv.example/ ".into(),
            ..AdvisoryConfig::default()
        };
        assert_eq!(config.endpoint_base(), "https://osv.example");
    }

    #[test]
    fn deserialize_should_fill_defaults_from_bare_table() {
        let config: AdvisoryConfig = toml::from_str("enabled = true").expect("parse");
        assert!(config.enabled);
        assert_eq!(config.endpoint, DEFAULT_OSV_ENDPOINT);
        assert_eq!(config.timeout_secs, DEFAULT_ADVISORY_TIMEOUT_SECS);
        assert_eq!(config.cache_ttl_secs, DEFAULT_ADVISORY_CACHE_TTL_SECS);
    }

    #[test]
    fn sweep_should_be_on_by_default_once_enrichment_is_enabled() {
        // Enabling `[advisories]` already discloses the watched package names;
        // the sweep only adds more versions of those same packages, so it does
        // not need a second opt-in.
        let config = AdvisoryConfig {
            enabled: true,
            ..AdvisoryConfig::default()
        };
        assert!(config.sweep_active());
    }

    #[test]
    fn sweep_should_be_off_while_enrichment_is() {
        assert!(
            !AdvisoryConfig::default().sweep_active(),
            "the sweep must never query OSV for an instance that opted out"
        );
    }

    #[test]
    fn either_zero_knob_should_disable_the_sweep() {
        let base = AdvisoryConfig {
            enabled: true,
            ..AdvisoryConfig::default()
        };
        assert!(!AdvisoryConfig {
            sweep_interval_secs: 0,
            ..base.clone()
        }
        .sweep_active());
        assert!(
            !AdvisoryConfig {
                sweep_batch: 0,
                ..base
            }
            .sweep_active(),
            "a round that would look up nothing is just 'off'"
        );
    }

    #[test]
    fn deserialize_should_reject_unknown_keys() {
        assert!(
            toml::from_str::<AdvisoryConfig>("enabled = true\nendpiont = \"x\"").is_err(),
            "a typo must fail loudly rather than silently disable enrichment"
        );
    }
}
