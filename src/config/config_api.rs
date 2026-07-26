//! Push-applied config API settings (`[config_api]`).

use serde::{Deserialize, Serialize};

/// Which store is authoritative for desired state at boot.
///
/// No sticky/`auto` mode: operators must declare `local` (disk YAML) or `api`
/// (PostgreSQL `config_revision` ledger).
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigSource {
    /// On-disk app YAML/TOML is authoritative; pushes are refused (409).
    #[default]
    Local,
    /// Ledger is authoritative; the app file is only a first-boot seed.
    Api,
}

/// Controls `POST /api/v1/config/apply` and `POST /api/v1/config/rollback`.
///
/// Default-off: endpoints return 404 until `api_config = true`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConfigApiConfig {
    /// When false, config mutation endpoints are not registered (404).
    #[serde(default)]
    pub api_config: bool,
    /// OIDC scope required on top of the management bearer for apply/rollback.
    /// Unset = management scope only (still requires HMAC when secret is set).
    #[serde(default)]
    pub apply_scope: Option<String>,
    /// Declares which store owns desired state. Defaults to [`ConfigSource::Local`].
    #[serde(default)]
    pub source: ConfigSource,
    /// Whether the dashboard may offer a config editor.
    ///
    /// Separate from [`Self::api_config`] on purpose: pipelines/`xrctl` can author
    /// config while browser sessions cannot. Implies `api_config` — editing goes
    /// through the same `/config/apply`, so this alone does nothing.
    #[serde(default)]
    pub ui_config: bool,
}

impl ConfigApiConfig {
    /// HMAC signing secret from `XRELEASE_CONFIG_APPLY_SECRET`.
    #[must_use]
    pub fn apply_secret(&self) -> Option<String> {
        crate::config::env_token("XRELEASE_CONFIG_APPLY_SECRET")
    }

    /// Whether apply/rollback routes should be mounted.
    #[must_use]
    pub fn routes_enabled(&self) -> bool {
        self.api_config
    }

    /// Whether HMAC body verification is configured.
    #[must_use]
    pub fn hmac_configured(&self) -> bool {
        self.apply_secret().is_some()
    }

    /// Whether a pushed document may become the new desired state.
    ///
    /// `Local` mode refuses: accepting a push there would apply config the next
    /// boot silently discards, since boot reads the file.
    #[must_use]
    pub fn accepts_pushed_config(&self) -> bool {
        matches!(self.source, ConfigSource::Api)
    }

    /// Whether a ledger revision may win over the app file at boot.
    #[must_use]
    pub fn ledger_is_bootable(&self) -> bool {
        matches!(self.source, ConfigSource::Api)
    }

    /// Whether the dashboard should expose config editing.
    ///
    /// `ui_config` without `api_config` cannot work (there is no apply endpoint
    /// to submit to), so it is reported as off rather than advertising an
    /// editor whose save button would 404. `source = local` also keeps the
    /// editor off — saves would 409.
    #[must_use]
    pub fn ui_config_enabled(&self) -> bool {
        self.api_config && self.ui_config && self.accepts_pushed_config()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_api_should_default_to_disabled_local() {
        let cfg = ConfigApiConfig::default();
        assert!(!cfg.routes_enabled());
        assert_eq!(cfg.source, ConfigSource::Local);
        assert!(!cfg.accepts_pushed_config());
        assert!(!cfg.ledger_is_bootable());
        assert!(!cfg.ui_config_enabled());
    }

    #[test]
    fn local_source_should_refuse_pushes_and_ignore_the_ledger() {
        let cfg = ConfigApiConfig {
            api_config: true,
            source: ConfigSource::Local,
            ..ConfigApiConfig::default()
        };
        assert!(cfg.routes_enabled());
        assert!(!cfg.accepts_pushed_config());
        assert!(!cfg.ledger_is_bootable());
        assert!(!cfg.ui_config_enabled());
    }

    #[test]
    fn ui_config_should_require_api_config_and_api_source() {
        let orphan = ConfigApiConfig {
            ui_config: true,
            api_config: false,
            source: ConfigSource::Api,
            ..ConfigApiConfig::default()
        };
        assert!(!orphan.ui_config_enabled());

        let wired = ConfigApiConfig {
            ui_config: true,
            api_config: true,
            source: ConfigSource::Api,
            ..ConfigApiConfig::default()
        };
        assert!(wired.ui_config_enabled());

        // API-only (no UI) — mode 2.
        let api_only = ConfigApiConfig {
            api_config: true,
            source: ConfigSource::Api,
            ..ConfigApiConfig::default()
        };
        assert!(!api_only.ui_config_enabled());
    }

    #[test]
    fn api_source_should_accept_pushes() {
        let cfg = ConfigApiConfig {
            source: ConfigSource::Api,
            ..ConfigApiConfig::default()
        };
        assert!(cfg.accepts_pushed_config());
        assert!(cfg.ledger_is_bootable());
    }

    #[test]
    fn current_field_names_should_deserialize() {
        let cfg: ConfigApiConfig = toml::from_str(
            r#"
 api_config = true
 source = "api"
 ui_config = true
 "#,
        )
        .expect("current field names");
        assert!(cfg.api_config);
        assert_eq!(cfg.source, ConfigSource::Api);
        assert!(cfg.ui_config);
        assert!(cfg.ui_config_enabled());
    }

    #[test]
    fn removed_legacy_field_names_should_be_rejected() {
        // Pre-release: the old `enabled` / `ui_editing` names carry no compat
        // burden and are gone (`deny_unknown_fields` rejects them loudly).
        for legacy in ["enabled = true", "ui_editing = true"] {
            assert!(
                toml::from_str::<ConfigApiConfig>(legacy).is_err(),
                "legacy field `{legacy}` should be rejected"
            );
        }
    }

    #[test]
    fn removed_source_aliases_should_be_rejected() {
        // Only `local` / `api` are valid; the old `file` / `ledger` / `auto`
        // spellings are unknown variants now.
        for source in ["file", "ledger", "auto"] {
            let toml = format!("source = \"{source}\"");
            assert!(
                toml::from_str::<ConfigApiConfig>(&toml).is_err(),
                "source = \"{source}\" should be rejected"
            );
        }
    }
}
