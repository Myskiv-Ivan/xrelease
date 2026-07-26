//! Source presets + shared `[[sources]]` fields (`SourceCommon`).

use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::pipeline::Filter;

use crate::config::Defaults;

/// Named defaults for shared source fields — referenced via `preset: name`.
///
/// Every field is optional so a preset can contribute only the knobs an
/// operator wants to share (`routing_tag`, `interval_secs`, …). Per-source
/// values always win over the preset; unset source fields inherit.
///
/// Built-in names from [`builtin_source_presets`] are always available; a
/// same-named entry under top-level `presets` replaces the built-in entirely.
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourcePreset {
    pub include_prerelease: Option<bool>,
    pub prerelease_tags: Option<Vec<String>>,
    pub exclude_updated: Option<bool>,
    pub pattern: Option<String>,
    pub exclude_pattern: Option<String>,
    #[serde(default)]
    pub routing_tag: Option<String>,
    pub interval_secs: Option<u64>,
    pub jitter_secs: Option<u64>,
    pub poll_on_startup: Option<bool>,
    pub notify_schedule: Option<String>,
}

/// Human-readable description of a built-in preset (schema / docs).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct BuiltinPresetInfo {
    pub name: &'static str,
    pub description: &'static str,
}

/// Built-in preset as exposed on `GET /api/v1/config/schema` — description plus
/// concrete filter fields so clients do not hardcode patterns.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BuiltinPresetSchema {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_prerelease: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prerelease_tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_updated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_pattern: Option<String>,
}

/// Schema catalogue: [`BUILTIN_PRESET_INFO`] order with fields from
/// [`builtin_source_presets`].
#[must_use]
pub fn builtin_preset_schemas() -> Vec<BuiltinPresetSchema> {
    let presets = builtin_source_presets();
    BUILTIN_PRESET_INFO
        .iter()
        .map(|info| {
            let preset = presets.get(info.name).cloned().unwrap_or_default();
            BuiltinPresetSchema {
                name: info.name.to_owned(),
                description: info.description.to_owned(),
                include_prerelease: preset.include_prerelease,
                prerelease_tags: preset.prerelease_tags,
                exclude_updated: preset.exclude_updated,
                pattern: preset.pattern,
                exclude_pattern: preset.exclude_pattern,
            }
        })
        .collect()
}

/// Catalogue metadata for built-in presets (stable order for UI/docs).
pub const BUILTIN_PRESET_INFO: &[BuiltinPresetInfo] = &[
    BuiltinPresetInfo {
        name: "wildcard",
        description: "All tags including pre-releases (no pattern filter)",
    },
    BuiltinPresetInfo {
        name: "any-stable",
        description: "All non-prerelease tags (no pattern filter)",
    },
    BuiltinPresetInfo {
        name: "semver",
        description: "Stable semver tags with optional leading v (v1.2.3 or 1.2.3)",
    },
    BuiltinPresetInfo {
        name: "semver-v",
        description: "Stable semver tags that require a leading v (v1.2.3)",
    },
    BuiltinPresetInfo {
        name: "numeric",
        description: "Numeric semver without a v prefix — Docker Hub / PyPI style (1.2.3)",
    },
    BuiltinPresetInfo {
        name: "major-minor",
        description: "Major.minor tags only (v1.2 or 1.2)",
    },
    BuiltinPresetInfo {
        name: "calver",
        description: "Calendar versioning YYYY.M.D / YYYY.MM.DD (optional leading v)",
    },
    BuiltinPresetInfo {
        name: "semver-pre",
        description: "Semver including pre-release suffixes (v1.2.3 and v1.2.3-rc.1)",
    },
    BuiltinPresetInfo {
        name: "docker-semver",
        description: "Numeric semver tags; exclude latest / nightly / edge",
    },
    BuiltinPresetInfo {
        name: "prerelease",
        description: "Pre-release channels only (alpha / beta / rc) with matching tag pattern",
    },
    BuiltinPresetInfo {
        name: "stable",
        description: "Stable releases only; ignore changelog/URL edits on already-seen tags",
    },
];

/// Built-in source presets always available without declaring them in config.
///
/// User `presets` with the same name replace these entirely (no field-wise merge).
#[must_use]
pub fn builtin_source_presets() -> std::collections::BTreeMap<String, SourcePreset> {
    let mut map = std::collections::BTreeMap::new();
    map.insert(
        "wildcard".into(),
        SourcePreset {
            include_prerelease: Some(true),
            ..SourcePreset::default()
        },
    );
    map.insert(
        "any-stable".into(),
        SourcePreset {
            include_prerelease: Some(false),
            ..SourcePreset::default()
        },
    );
    map.insert(
        "semver".into(),
        SourcePreset {
            include_prerelease: Some(false),
            pattern: Some(r"^v?\d+\.\d+\.\d+$".into()),
            ..SourcePreset::default()
        },
    );
    map.insert(
        "semver-v".into(),
        SourcePreset {
            include_prerelease: Some(false),
            pattern: Some(r"^v\d+\.\d+\.\d+$".into()),
            ..SourcePreset::default()
        },
    );
    map.insert(
        "numeric".into(),
        SourcePreset {
            include_prerelease: Some(false),
            pattern: Some(r"^\d+\.\d+\.\d+$".into()),
            ..SourcePreset::default()
        },
    );
    map.insert(
        "major-minor".into(),
        SourcePreset {
            include_prerelease: Some(false),
            pattern: Some(r"^v?\d+\.\d+$".into()),
            ..SourcePreset::default()
        },
    );
    map.insert(
        "calver".into(),
        SourcePreset {
            include_prerelease: Some(false),
            pattern: Some(r"^v?\d{4}\.\d{1,2}\.\d{1,2}$".into()),
            ..SourcePreset::default()
        },
    );
    map.insert(
        "semver-pre".into(),
        SourcePreset {
            include_prerelease: Some(true),
            pattern: Some(r"^v?\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$".into()),
            ..SourcePreset::default()
        },
    );
    map.insert(
        "docker-semver".into(),
        SourcePreset {
            include_prerelease: Some(false),
            pattern: Some(r"^\d+\.\d+\.\d+$".into()),
            exclude_pattern: Some(r"^(latest|nightly|edge)$".into()),
            ..SourcePreset::default()
        },
    );
    map.insert(
        "prerelease".into(),
        SourcePreset {
            include_prerelease: Some(true),
            prerelease_tags: Some(vec!["alpha".into(), "beta".into(), "rc".into()]),
            pattern: Some(r"^v?\d+\.\d+\.\d+-(alpha|beta|rc)".into()),
            ..SourcePreset::default()
        },
    );
    map.insert(
        "stable".into(),
        SourcePreset {
            include_prerelease: Some(false),
            exclude_updated: Some(true),
            pattern: Some(r"^v?\d+\.\d+\.\d+$".into()),
            ..SourcePreset::default()
        },
    );
    debug_assert_eq!(
        map.len(),
        BUILTIN_PRESET_INFO.len(),
        "BUILTIN_PRESET_INFO must list every built-in preset"
    );
    map
}

/// Effective preset catalogue: built-ins, then user overrides by name.
#[must_use]
pub fn effective_source_presets(
    user: &std::collections::BTreeMap<String, SourcePreset>,
) -> std::collections::BTreeMap<String, SourcePreset> {
    let mut map = builtin_source_presets();
    for (name, preset) in user {
        map.insert(name.clone(), preset.clone());
    }
    map
}

/// Shared optional settings applied to every source type.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SourceCommon {
    /// Named entry under top-level `presets` — merged before per-source fields.
    #[serde(default)]
    pub preset: Option<String>,
    /// Include pre-releases? Omitted = inherit preset, else `false`.
    #[serde(default)]
    pub include_prerelease: Option<bool>,
    /// When `include_prerelease = true`, optionally restrict to sub-channels
    /// (`alpha`, `beta`, `rc`, …). Empty / omitted = all pre-releases.
    #[serde(default)]
    pub prerelease_tags: Option<Vec<String>>,
    /// When true, ignore changelog/URL edits on already-seen tags (cli-go
    /// `--exclude-updated`). Omitted = inherit preset, else `false`.
    #[serde(default)]
    pub exclude_updated: Option<bool>,
    /// Optional regex the tag must match (inclusion filter).
    pub pattern: Option<String>,
    /// Optional regex the tag must **not** match (exclusion filter).
    pub exclude_pattern: Option<String>,
    /// Team routing tag — matches notifier `tags` / `[[teams]].tag`.
    #[serde(default)]
    pub routing_tag: Option<String>,
    /// Override base interval (seconds).
    pub interval_secs: Option<u64>,
    /// Override jitter (seconds).
    pub jitter_secs: Option<u64>,
    /// Override immediate poll on backend start (`true` = poll once right after launch).
    pub poll_on_startup: Option<bool>,
    /// Notification delivery schedule (crontab expression, UTC): hold
    /// notifications in the outbox until the next matching moment. Overrides
    /// `[defaults].notify_schedule`; an empty string opts this source out of
    /// the default. Unset here + unset default = deliver immediately.
    pub notify_schedule: Option<String>,
}

impl SourceCommon {
    /// Resolve `preset` against the catalogue, then drop the name so runtime
    /// state only carries concrete fields.
    pub(crate) fn with_preset_resolved(
        mut self,
        presets: &std::collections::BTreeMap<String, SourcePreset>,
        source_id: &str,
    ) -> anyhow::Result<Self> {
        let Some(name) = self.preset.take() else {
            return Ok(self);
        };
        let name = name.trim();
        if name.is_empty() {
            anyhow::bail!("source `{source_id}`: `preset` must not be empty");
        }
        let preset = presets.get(name).with_context(|| {
            format!(
                "source `{source_id}`: unknown preset `{name}` \
                 (not a built-in and not listed under `presets`)"
            )
        })?;
        Ok(preset.merge_into(self))
    }

    pub(super) fn build_filter(&self, source_id: &str) -> anyhow::Result<Filter> {
        let tags = self
            .prerelease_tags
            .as_ref()
            .filter(|tags| !tags.is_empty())
            .cloned();
        Filter::with_options(
            self.include_prerelease.unwrap_or(false),
            tags,
            self.pattern.as_deref(),
            self.exclude_pattern.as_deref(),
            self.exclude_updated.unwrap_or(false),
        )
        .with_context(|| format!("invalid pattern for source {source_id}"))
    }

    pub(super) fn schedule(&self, defaults: &Defaults) -> (Duration, Duration, bool) {
        (
            Duration::from_secs(self.interval_secs.unwrap_or(defaults.interval_secs)),
            Duration::from_secs(self.jitter_secs.unwrap_or(defaults.jitter_secs)),
            self.poll_on_startup.unwrap_or(defaults.poll_on_startup),
        )
    }
}

impl SourcePreset {
    /// Overlay preset values under source-local fields (source wins when set).
    fn merge_into(&self, mut common: SourceCommon) -> SourceCommon {
        if common.include_prerelease.is_none() {
            common.include_prerelease = self.include_prerelease;
        }
        if common.prerelease_tags.is_none() {
            common.prerelease_tags = self.prerelease_tags.clone();
        }
        if common.exclude_updated.is_none() {
            common.exclude_updated = self.exclude_updated;
        }
        if common.pattern.is_none() {
            common.pattern = self.pattern.clone();
        }
        if common.exclude_pattern.is_none() {
            common.exclude_pattern = self.exclude_pattern.clone();
        }
        if common.routing_tag.is_none() {
            common.routing_tag = self.routing_tag.clone();
        }
        if common.interval_secs.is_none() {
            common.interval_secs = self.interval_secs;
        }
        if common.jitter_secs.is_none() {
            common.jitter_secs = self.jitter_secs;
        }
        if common.poll_on_startup.is_none() {
            common.poll_on_startup = self.poll_on_startup;
        }
        if common.notify_schedule.is_none() {
            common.notify_schedule = self.notify_schedule.clone();
        }
        common
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_preset_schemas_should_include_filter_fields() {
        let schemas = builtin_preset_schemas();
        assert_eq!(schemas.len(), BUILTIN_PRESET_INFO.len());
        let docker = schemas
            .iter()
            .find(|entry| entry.name == "docker-semver")
            .expect("docker-semver");
        assert_eq!(docker.pattern.as_deref(), Some(r"^\d+\.\d+\.\d+$"));
        assert_eq!(
            docker.exclude_pattern.as_deref(),
            Some(r"^(latest|nightly|edge)$")
        );
        assert_eq!(docker.include_prerelease, Some(false));
        let pre = schemas
            .iter()
            .find(|entry| entry.name == "prerelease")
            .expect("prerelease");
        assert_eq!(
            pre.prerelease_tags,
            Some(vec!["alpha".into(), "beta".into(), "rc".into()])
        );
    }
}
