//! Tracing/log initialization.

use tracing_subscriber::{fmt, EnvFilter};

const FALLBACK_FILTER: &str = "info";

/// Initialize the global tracing subscriber.
///
/// `default_level` comes from `[log].level` after env overlays (`XRELEASE_LOG`).
///
/// - Empty / whitespace directives fall back to [`FALLBACK_FILTER`].
/// - Invalid EnvFilter syntax falls back to [`FALLBACK_FILTER`] (never panics).
/// - A second call is a no-op (`try_init`) so tests / miswired binaries do not abort.
pub fn init(default_level: &str) {
    let filter = build_env_filter(default_level);
    // `with_target(false)` keeps operator logs compact; EnvFilter still matches
    // crate targets (`xrelease=debug,reqwest=warn`) even when targets are hidden.
    let _ = fmt().with_env_filter(filter).with_target(false).try_init();
}

/// Parse an EnvFilter directive, falling back to `info` on empty/invalid input.
fn build_env_filter(default_level: &str) -> EnvFilter {
    let directive = default_level.trim();
    let directive = if directive.is_empty() {
        FALLBACK_FILTER
    } else {
        directive
    };
    match EnvFilter::try_new(directive) {
        Ok(filter) => filter,
        Err(err) => {
            // Subscriber is not up yet — stderr is the only channel.
            eprintln!(
                "xrelease: invalid log filter `{directive}` ({err}); falling back to {FALLBACK_FILTER}"
            );
            EnvFilter::new(FALLBACK_FILTER)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_env_filter_should_accept_simple_level() {
        let filter = build_env_filter("debug");
        assert_eq!(filter.to_string(), "debug");
    }

    #[test]
    fn build_env_filter_should_accept_crate_directives() {
        let filter = build_env_filter("xrelease=debug,reqwest=warn");
        let rendered = filter.to_string();
        assert!(rendered.contains("xrelease=debug"), "{rendered}");
        assert!(rendered.contains("reqwest=warn"), "{rendered}");
    }

    #[test]
    fn build_env_filter_should_fallback_on_empty() {
        assert_eq!(build_env_filter("").to_string(), FALLBACK_FILTER);
        assert_eq!(build_env_filter("   ").to_string(), FALLBACK_FILTER);
    }

    #[test]
    fn build_env_filter_should_fallback_on_invalid_syntax() {
        // A lone `=` is not a valid EnvFilter directive.
        assert_eq!(build_env_filter("=").to_string(), FALLBACK_FILTER);
    }
}
