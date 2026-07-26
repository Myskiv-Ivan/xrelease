//! Desired-state document formats (TOML and YAML).

use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use super::Config;

/// Wire format for app / desired-state documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DesiredFormat {
    Toml,
    Yaml,
}

impl DesiredFormat {
    /// Map HTTP `Content-Type` (without parameters) to a format.
    #[must_use]
    pub fn from_content_type(value: &str) -> Option<Self> {
        let mime = value.split(';').next()?.trim().to_ascii_lowercase();
        match mime.as_str() {
            "application/yaml" | "application/x-yaml" | "text/yaml" => Some(Self::Yaml),
            "application/toml" | "text/plain" => Some(Self::Toml),
            _ => None,
        }
    }
}

/// Guess format from a file extension.
#[must_use]
pub fn format_from_path(path: &Path) -> Option<DesiredFormat> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("yaml") | Some("yml") => Some(DesiredFormat::Yaml),
        Some("toml") => Some(DesiredFormat::Toml),
        _ => None,
    }
}

/// Heuristic when neither path nor `Content-Type` is available (ledger replay).
///
/// Only *structural* markers count — markers that can appear at the start of a
/// line, never inside a value. An earlier version keyed on `text.contains('=')`
/// to mean TOML, which misread any YAML carrying `=` in a value (Apprise and
/// webhook URLs routinely do: `?format=markdown`). Detection is still a guess;
/// [`parse_desired_document_with_hint`] falls back to the other parser so a
/// wrong guess cannot fail an otherwise valid document.
#[must_use]
pub fn detect_desired_format(text: &str) -> DesiredFormat {
    if text.trim_start().starts_with("---") {
        return DesiredFormat::Yaml;
    }

    let mut toml_markers = 0usize;
    let mut yaml_markers = 0usize;
    for line in text.lines() {
        let line = line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // `[table]` / `[[array-of-tables]]` headers and `key = …` are TOML-only.
        if line.starts_with('[') || is_toml_assignment(line) {
            toml_markers += 1;
        } else if line.starts_with("- ") || line == "-" || is_yaml_mapping_key(line) {
            yaml_markers += 1;
        }
    }

    if yaml_markers > toml_markers {
        DesiredFormat::Yaml
    } else {
        DesiredFormat::Toml
    }
}

/// `key = …` at the start of a line — a TOML assignment, not a value containing `=`.
fn is_toml_assignment(line: &str) -> bool {
    let Some((key, _)) = line.split_once('=') else {
        return false;
    };
    let key = key.trim_end();
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '"' | '\''))
}

/// `key:` or `key: value` at the start of a line — a YAML mapping entry.
fn is_yaml_mapping_key(line: &str) -> bool {
    let Some((key, rest)) = line.split_once(':') else {
        return false;
    };
    if !(rest.is_empty() || rest.starts_with(' ')) {
        return false; // `http://…` and other colon-in-value cases
    }
    let key = key.trim_end();
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// Parse a desired-state document (TOML or YAML) without env overrides.
pub fn parse_desired_document(text: &str) -> anyhow::Result<Config> {
    parse_desired_document_with_hint(text, None)
}

/// Parse with an explicit or inferred format, falling back to the other one.
///
/// The ledger stores a revision's bytes but not its format, so boot
/// (`config::resolve`) and rollback re-parse with no hint at all. A misdetected
/// format there would strand an already-applied revision — the process would
/// refuse to boot on a config it accepted minutes earlier. Trying the other
/// parser makes detection advisory instead of load-bearing; the primary error
/// is still what surfaces when the document is genuinely invalid.
pub fn parse_desired_document_with_hint(
    text: &str,
    hint: Option<DesiredFormat>,
) -> anyhow::Result<Config> {
    let format = hint.unwrap_or_else(|| detect_desired_format(text));
    let primary = parse_as(text, format);
    if primary.is_ok() {
        return primary;
    }

    let fallback = match format {
        DesiredFormat::Toml => DesiredFormat::Yaml,
        DesiredFormat::Yaml => DesiredFormat::Toml,
    };
    if let Ok(config) = parse_as(text, fallback) {
        tracing::debug!(
            detected = ?format,
            actual = ?fallback,
            "desired-state document parsed as the fallback format"
        );
        return Ok(config);
    }
    primary
}

fn parse_as(text: &str, format: DesiredFormat) -> anyhow::Result<Config> {
    reject_removed_desired_keys(text)?;
    match format {
        DesiredFormat::Toml => toml::from_str(text).context("parsing desired-state TOML document"),
        DesiredFormat::Yaml => {
            serde_yaml::from_str(text).context("parsing desired-state YAML document")
        }
    }
}

/// Reject removed config keys that serde would otherwise silently drop.
///
/// Internally tagged `SourceConfig` cannot use `deny_unknown_fields` with
/// `flatten`, so obsolete names like `apprise_tag` would parse as "no routing
/// tag" and strand notifications. Fail loudly instead (pre-release; no alias).
fn reject_removed_desired_keys(text: &str) -> anyhow::Result<()> {
    /// Keys removed from the desired-state surface (pre-release; no compat).
    const REMOVED: &[(&str, &str)] = &[
        (
            "apprise_tag",
            "use `routing_tag` (team routing is not Apprise-specific)",
        ),
        (
            "bot_id",
            "express notifiers use `access_token` / `access_token_env` only",
        ),
        (
            "secret_key",
            "express notifiers use `access_token` / `access_token_env` only",
        ),
        (
            "secret_key_env",
            "express notifiers use `access_token` / `access_token_env` only",
        ),
    ];

    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // Strip a leading `- ` so YAML list items are checked the same way.
        let trimmed = trimmed.strip_prefix("- ").unwrap_or(trimmed).trim_start();
        for &(key, hint) in REMOVED {
            if let Some(rest) = trimmed.strip_prefix(key) {
                if rest.is_empty()
                    || matches!(rest.as_bytes().first(), Some(b'=' | b':' | b' ' | b'\t'))
                {
                    anyhow::bail!("line {}: `{key}` was removed — {hint}", idx + 1);
                }
            }
        }
    }
    Ok(())
}

/// Read and parse an on-disk app config file (`.yaml`/`.yml`/`.toml`).
pub fn load_desired_file(path: &Path) -> anyhow::Result<Config> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading app config {}", path.display()))?;
    let hint = format_from_path(path);
    parse_desired_document_with_hint(&text, hint)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_YAML: &str = r#"
defaults:
  interval_secs: 3600
sources:
  - type: github
    repo: org/app
"#;

    const SAMPLE_TOML: &str = r#"
[[sources]]
type = "github"
repo = "org/app"
"#;

    #[test]
    fn yaml_should_parse_sources() {
        let config =
            parse_desired_document_with_hint(SAMPLE_YAML, Some(DesiredFormat::Yaml)).expect("yaml");
        assert_eq!(config.sources.len(), 1);
    }

    #[test]
    fn detect_should_prefer_toml_for_array_tables() {
        assert_eq!(detect_desired_format(SAMPLE_TOML), DesiredFormat::Toml);
    }

    /// YAML whose values carry `=` (Apprise / webhook URLs with query strings).
    /// Previously sniffed as TOML, which made an applied revision unparseable
    /// on the next boot and on rollback.
    const YAML_WITH_EQUALS: &str = r#"
notifiers:
  - type: apprise
    endpoint: http://apprise:8000
    urls: ["tgram://token/chat?format=markdown"]
  - type: webhook
    name: n8n
    url: https://n8n.example.com/hook?token=abc123
sources:
  - type: github
    repo: org/app
"#;

    #[test]
    fn detect_should_not_treat_equals_in_values_as_toml() {
        assert_eq!(detect_desired_format(YAML_WITH_EQUALS), DesiredFormat::Yaml);
    }

    #[test]
    fn hintless_parse_should_accept_yaml_containing_equals() {
        // The ledger boot path (`config::resolve`) parses with no hint.
        let config = parse_desired_document(YAML_WITH_EQUALS).expect("yaml with '='");
        assert_eq!(config.sources.len(), 1);
        assert_eq!(config.notifiers.len(), 2);
    }

    #[test]
    fn parse_should_fall_back_when_the_hint_is_wrong() {
        // A wrong Content-Type must not strand a valid document.
        let config = parse_desired_document_with_hint(SAMPLE_YAML, Some(DesiredFormat::Toml))
            .expect("fallback to yaml");
        assert_eq!(config.sources.len(), 1);
    }

    #[test]
    fn parse_should_report_the_primary_error_when_both_formats_fail() {
        let err = parse_desired_document_with_hint("{{{ not a config", Some(DesiredFormat::Toml))
            .unwrap_err();
        assert!(
            err.to_string().contains("TOML"),
            "expected the hinted format's error, got: {err}"
        );
    }

    #[test]
    fn toml_with_url_values_should_still_detect_as_toml() {
        let toml = r#"
[[notifiers]]
type = "apprise"
endpoint = "http://apprise:8000"
urls = ["tgram://token/chat?format=markdown"]
[[sources]]
type = "github"
repo = "org/app"
"#;
        assert_eq!(detect_desired_format(toml), DesiredFormat::Toml);
        assert_eq!(parse_desired_document(toml).expect("toml").sources.len(), 1);
    }

    #[test]
    fn content_type_should_map_yaml() {
        assert_eq!(
            DesiredFormat::from_content_type("application/yaml; charset=utf-8"),
            Some(DesiredFormat::Yaml)
        );
    }

    #[test]
    fn apprise_tag_should_be_rejected_in_toml() {
        let err = parse_desired_document(
            r#"
[[sources]]
type = "github"
repo = "org/app"
apprise_tag = "platform-team"
"#,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("routing_tag"),
            "expected migration hint, got: {err}"
        );
    }

    #[test]
    fn apprise_tag_should_be_rejected_in_yaml() {
        let err = parse_desired_document(
            r#"
sources:
  - type: github
    repo: org/app
    apprise_tag: platform-team
"#,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("apprise_tag"),
            "expected reject, got: {err}"
        );
    }

    #[test]
    fn express_secret_key_should_be_rejected_in_desired_document() {
        let err = parse_desired_document(
            r#"
notifiers:
  - type: express
    base_url: https://cts.example.com
    access_token: token
    group_chat_id: dec60c05-77b7-0d78-159e-b4fbee4d48f6
    secret_key: legacy
"#,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("secret_key"),
            "expected reject, got: {err}"
        );
    }
}
