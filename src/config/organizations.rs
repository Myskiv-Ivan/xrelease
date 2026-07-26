//! Organization catalogue — bootstrap-only multi-tenant desired-state roots.
//!
//! Each [`OrganizationConfig`] may point at a desired-state file
//! (`app/<org>/releases.yaml`) used as GitOps authority (`source = local`) or as
//! a first-boot seed (`source = api`). With API/UI authoring the `app` path is
//! optional: an empty placeholder is used until the first ledger apply.
//! The catalogue lives in `bootstrap.toml` so a compromised apply cannot invent
//! a new tenant.

use serde::{Deserialize, Serialize};

/// One organization (tenant) declared in `[[organizations]]`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OrganizationConfig {
    /// Stable slug (`platform`, `security`). Used in API paths and DB keys.
    pub id: String,
    /// Human-readable label for the UI switcher.
    #[serde(default)]
    pub name: Option<String>,
    /// Path to this org's desired-state document (YAML or TOML).
    ///
    /// Required when `[config_api].source = "local"`. Optional when
    /// `source = "api"`: omit to start from an empty document and author via
    /// UI / `xrctl apply` into the org's ledger stream.
    #[serde(default)]
    pub app: Option<String>,
}

impl OrganizationConfig {
    /// Trimmed id; empty ids are invalid at resolve time.
    #[must_use]
    pub fn id(&self) -> &str {
        self.id.trim()
    }

    /// Display name, falling back to id.
    #[must_use]
    pub fn display_name(&self) -> &str {
        self.name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| &self.id)
    }

    /// Trimmed non-empty `app` path, if configured.
    #[must_use]
    pub fn app_path(&self) -> Option<&str> {
        self.app
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
    }
}

/// Separator between organization id and the inner source id / routing tag.
pub const ORGANIZATION_SEP: &str = "::";

/// Prefix `value` with `{organization_id}::` exactly once (idempotent).
fn namespace_scoped(organization_id: &str, value: &str) -> String {
    let value = value.trim();
    let prefix = format!("{organization_id}{ORGANIZATION_SEP}");
    if value.starts_with(&prefix) {
        value.to_owned()
    } else {
        format!("{prefix}{value}")
    }
}

/// Namespace a routing tag so two organizations cannot cross-route.
///
/// Already-prefixed tags (`{id}::…`) are left unchanged.
#[must_use]
pub fn namespace_routing_tag(organization_id: &str, tag: &str) -> String {
    namespace_scoped(organization_id, tag)
}

/// Namespace a source id for store keys / provider ids.
#[must_use]
pub fn namespace_source_id(organization_id: &str, source_id: &str) -> String {
    namespace_scoped(organization_id, source_id)
}

/// Namespace a user-defined preset name.
///
/// `Config::watches` resolves presets against the MERGED multi-org
/// catalogue; without per-org keys, two organizations defining the same
/// preset name would silently share one entry (later org wins). Built-ins
/// stay unprefixed and shared — only names the org itself declares under
/// `presets` are rewritten, on both the map key and each source's reference.
#[must_use]
pub fn namespace_preset_name(organization_id: &str, name: &str) -> String {
    namespace_scoped(organization_id, name)
}

/// Extract organization id from a namespaced source id (`platform::github:…`).
#[must_use]
pub fn organization_id_from_source_id(source_id: &str) -> Option<&str> {
    source_id
        .split_once(ORGANIZATION_SEP)
        .map(|(org, _)| org)
        .filter(|org| !org.is_empty())
}

/// Namespace teams / notifiers / sources for one organization document.
///
/// The transformation must preserve the document's STANDALONE semantics: in a
/// single-document instance a sink without `tags` is a catch-all for every
/// event of that config. Merged into one process, "catch-all" would mean
/// receiving *other organizations'* events — so an untagged sink instead gets
/// the full set of routing tags its own organization can emit (each source's
/// namespaced tag plus the org broadcast tag). Giving it only the broadcast
/// tag — an earlier draft — silently dropped every notification from a source
/// with an explicit `routing_tag`, because a tagged sink matches nothing else.
pub fn namespace_organization_desired(organization_id: &str, config: &mut crate::config::Config) {
    use crate::config::sources::{prepare_organization_sources, source_routing_tag};

    let broadcast = namespace_routing_tag(organization_id, "__org__");

    // Every routing tag this organization's events can carry — computed from
    // the ORIGINAL document each time it is (re)composed, so per-org applies
    // keep untagged sinks in sync with new sources automatically.
    let mut org_event_tags: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    org_event_tags.insert(broadcast.clone());
    for source in &config.sources {
        match source_routing_tag(source)
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
        {
            Some(tag) => {
                org_event_tags.insert(namespace_routing_tag(organization_id, tag));
            }
            None => {
                // Untagged sources emit the broadcast tag (already inserted).
            }
        }
    }
    // Ops meta-alerts use defaults.ops_routing_tag; include it so catch-all
    // sinks still receive them after tags are rewritten to an explicit set.
    if let Some(tag) = config
        .defaults
        .ops_routing_tag
        .as_deref()
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
    {
        org_event_tags.insert(namespace_routing_tag(organization_id, tag));
    }
    let catch_all: Vec<String> = org_event_tags.into_iter().collect();

    for team in &mut config.teams {
        team.tag = namespace_routing_tag(organization_id, &team.tag);
    }

    if let Some(tag) = &mut config.defaults.ops_routing_tag {
        let trimmed = tag.trim();
        if !trimmed.is_empty() {
            *tag = namespace_routing_tag(organization_id, trimmed);
        }
    }

    for notifier in &mut config.notifiers {
        let tags = notifier.routing_tags_mut();
        if tags.is_empty() {
            *tags = catch_all.clone();
        } else {
            *tags = tags
                .iter()
                .map(|tag| namespace_routing_tag(organization_id, tag))
                .collect();
        }
    }

    prepare_organization_sources(organization_id, config);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_routing_tag_should_prefix_once() {
        assert_eq!(namespace_routing_tag("platform", "core"), "platform::core");
        assert_eq!(
            namespace_routing_tag("platform", "platform::core"),
            "platform::core"
        );
    }

    #[test]
    fn namespace_source_id_should_not_collide_with_github_paths() {
        assert_eq!(
            namespace_source_id("security", "github:org/repo"),
            "security::github:org/repo"
        );
        assert_eq!(
            organization_id_from_source_id("security::github:org/repo"),
            Some("security")
        );
        assert_eq!(organization_id_from_source_id("github:org/repo"), None);
    }

    #[test]
    fn untagged_sink_should_stay_a_catch_all_within_its_org() {
        // One tagged + one untagged source; apprise carries no tags of its own.
        // Standalone, that apprise is a catch-all — after namespacing it must
        // still hear BOTH the explicit team tag and the org broadcast, not just
        // the broadcast (which silently dropped tagged sources' events).
        let mut config: crate::config::Config = serde_yaml::from_str(
            r#"
notifiers:
  - type: apprise
    urls: ["mailto://a@b.c"]
sources:
  - type: github
    repo: org/tagged
    routing_tag: core
  - type: github
    repo: org/untagged
"#,
        )
        .expect("parse");

        namespace_organization_desired("platform", &mut config);

        let tags = config.notifiers[0].routing_tags();
        assert!(tags.contains(&"platform::core".to_owned()));
        assert!(tags.contains(&"platform::__org__".to_owned()));
    }

    #[test]
    fn only_org_declared_preset_names_should_be_namespaced() {
        let mut config: crate::config::Config = serde_yaml::from_str(
            r#"
presets:
  mine:
    pattern: "^v"
sources:
  - type: github
    repo: org/a
    preset: mine
  - type: github
    repo: org/b
    preset: major-only
"#,
        )
        .expect("parse");

        namespace_organization_desired("platform", &mut config);

        assert!(config.presets.contains_key("platform::mine"));
        let refs: Vec<_> = config
            .sources
            .iter()
            .map(|source| crate::config::sources::source_common(source).preset.clone())
            .collect();
        assert_eq!(
            refs,
            vec![
                Some("platform::mine".to_owned()),
                // Not declared by THIS org — stays unprefixed: it resolves to a
                // built-in (or errors), never to another organization's entry.
                Some("major-only".to_owned())
            ]
        );
    }

    #[test]
    fn explicitly_tagged_sink_should_only_be_namespaced() {
        let mut config: crate::config::Config = serde_yaml::from_str(
            r#"
notifiers:
  - type: apprise
    urls: ["mailto://a@b.c"]
    tags: [core]
sources:
  - type: github
    repo: org/tagged
    routing_tag: core
"#,
        )
        .expect("parse");

        namespace_organization_desired("platform", &mut config);

        assert_eq!(
            config.notifiers[0].routing_tags(),
            vec!["platform::core".to_owned()]
        );
    }

    #[test]
    fn ops_routing_tag_should_be_namespaced_and_reach_catch_all_sinks() {
        let mut config: crate::config::Config = serde_yaml::from_str(
            r#"
defaults:
  ops_routing_tag: ops
notifiers:
  - type: apprise
    urls: ["mailto://a@b.c"]
teams:
  - tag: ops
sources:
  - type: github
    repo: org/app
    routing_tag: core
"#,
        )
        .expect("parse");

        namespace_organization_desired("platform", &mut config);

        assert_eq!(
            config.defaults.ops_routing_tag.as_deref(),
            Some("platform::ops")
        );
        let tags = config.notifiers[0].routing_tags();
        assert!(tags.contains(&"platform::ops".to_owned()));
        assert!(tags.contains(&"platform::core".to_owned()));
        assert_eq!(config.teams[0].tag, "platform::ops");
    }
}
