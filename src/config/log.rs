//! Logging settings (`[log]`).

use serde::{Deserialize, Serialize};

/// Logging settings.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LogConfig {
    /// Tracing EnvFilter directive (overridden by `XRELEASE_LOG`).
    ///
    /// Examples: `info`, `xrelease=debug,reqwest=warn`.
    #[serde(default = "default_log_level")]
    pub level: String,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
        }
    }
}

fn default_log_level() -> String {
    "info".to_owned()
}

/// Canonical env key for the log filter.
const LOG_ENV_KEY: &str = "XRELEASE_LOG";

/// Non-blank `XRELEASE_LOG`, if set.
///
/// Blank/`""` does **not** win: operators sometimes export an empty
/// `XRELEASE_LOG` in templates, and that must not wipe `[log].level`.
#[must_use]
pub(crate) fn log_level_from_env() -> Option<String> {
    let Ok(value) = std::env::var(LOG_ENV_KEY) else {
        return None;
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// Apply env overlay onto `[log]` (used by [`crate::config::apply_env_overrides`]).
pub(crate) fn apply_log_env_overlay(log: &mut LogConfig) {
    if let Some(level) = log_level_from_env() {
        log.level = level;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Process env is global — serialize log-env tests against each other.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn clear_log_env() {
        std::env::remove_var("XRELEASE_LOG");
        // Stale aliases must not be read even if still exported in the process.
        std::env::remove_var("XRELEASE_LOG_LEVEL");
        std::env::remove_var("RUST_LOG");
    }

    #[test]
    fn log_level_from_env_should_read_xrelease_log() {
        let _guard = ENV_LOCK.lock().expect("log env lock");
        clear_log_env();
        std::env::set_var("XRELEASE_LOG", "xrelease=debug");
        std::env::set_var("RUST_LOG", "error");
        assert_eq!(log_level_from_env().as_deref(), Some("xrelease=debug"));
        clear_log_env();
    }

    #[test]
    fn log_level_from_env_should_ignore_blank_and_aliases() {
        let _guard = ENV_LOCK.lock().expect("log env lock");
        clear_log_env();
        std::env::set_var("XRELEASE_LOG", "  ");
        std::env::set_var("XRELEASE_LOG_LEVEL", "warn");
        std::env::set_var("RUST_LOG", "warn");
        assert_eq!(log_level_from_env(), None);
        clear_log_env();
    }

    #[test]
    fn apply_log_env_overlay_should_leave_file_value_when_env_unset() {
        let _guard = ENV_LOCK.lock().expect("log env lock");
        clear_log_env();
        let mut log = LogConfig {
            level: "debug".into(),
        };
        apply_log_env_overlay(&mut log);
        assert_eq!(log.level, "debug");
    }
}
