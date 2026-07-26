//! Infra (`bootstrap.toml`) vs app (`app/releases.yaml`) config paths.

use std::path::{Path, PathBuf};

/// Default infrastructure config path (repo root / container mount).
pub const DEFAULT_BOOTSTRAP_PATH: &str = "bootstrap.toml";

/// Default application config path when present on disk.
pub const DEFAULT_APP_PATH: &str = "app/releases.yaml";

/// Bootstrap (infra) + application (desired-state) file paths.
#[derive(Debug, Clone)]
pub struct ConfigPaths {
    /// Infrastructure config: `[database]`, `[api]`, `[log]`, `[config_api]`.
    pub bootstrap: PathBuf,
    /// Application config: sources, teams, notifiers, defaults, apprise structure.
    pub app: Option<PathBuf>,
}

impl ConfigPaths {
    /// Build from CLI flags, auto-discovering [`DEFAULT_APP_PATH`] when unset.
    #[must_use]
    pub fn resolve(bootstrap: PathBuf, app: Option<PathBuf>) -> Self {
        let app = app.or_else(|| {
            let default = PathBuf::from(DEFAULT_APP_PATH);
            default.exists().then_some(default)
        });
        Self { bootstrap, app }
    }

    /// Explicit bootstrap + app pair (no auto-discovery).
    #[must_use]
    pub fn new(bootstrap: PathBuf, app: Option<PathBuf>) -> Self {
        Self { bootstrap, app }
    }

    /// Whether an on-disk app config path is configured.
    #[must_use]
    pub fn has_app_file(&self) -> bool {
        self.app.is_some()
    }
}

impl AsRef<Path> for ConfigPaths {
    fn as_ref(&self) -> &Path {
        &self.bootstrap
    }
}
