//! Source-set diff for apply responses.

use std::collections::{HashMap, HashSet};

use crate::config::Config;

/// Added / removed / changed source ids between two configs.
#[derive(Debug, Clone, Default)]
pub struct ConfigDiff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<String>,
}

impl ConfigDiff {
    /// Diff `current` against `next` at the level of individual sources.
    ///
    /// `changed` reports sources present in **both** configs whose spec actually
    /// differs — not merely every retained id. Comparing the serialized
    /// [`crate::config::SourceConfig`] (both sides in memory, never emitted)
    /// makes "changed" truthful, so a UI does not label untouched sources as
    /// modified after an apply that only added one.
    #[must_use]
    pub fn compute(current: &Config, next: &Config) -> Self {
        let current = source_content(current);
        let next = source_content(next);

        let current_by_id: HashMap<&str, &serde_json::Value> = current
            .iter()
            .map(|(id, spec)| (id.as_str(), spec))
            .collect();
        let next_ids: HashSet<&str> = next.iter().map(|(id, _)| id.as_str()).collect();

        // Walk `next` (then `current`) in declaration order so the output is
        // stable and diffable rather than HashMap-random.
        let mut added = Vec::new();
        let mut changed = Vec::new();
        for (id, spec) in &next {
            match current_by_id.get(id.as_str()) {
                None => added.push(id.clone()),
                Some(old) if *old != spec => changed.push(id.clone()),
                Some(_) => {}
            }
        }
        let removed = current
            .iter()
            .filter(|(id, _)| !next_ids.contains(id.as_str()))
            .map(|(id, _)| id.clone())
            .collect();

        Self {
            added,
            removed,
            changed,
        }
    }
}

/// Pair each source's stable id with a canonical serialization of its spec.
///
/// `into_watches` is 1:1 and order-preserving with `sources`, so the provider
/// id (the same id used everywhere else) lines up with the `SourceConfig` it
/// was built from. On the rare `into_watches` error (already validated by the
/// time a diff runs) an empty map degrades to "everything added/removed", which
/// is only ever used to fill an advisory response field.
fn source_content(config: &Config) -> Vec<(String, serde_json::Value)> {
    let Ok(watches) = config.to_watches() else {
        return Vec::new();
    };
    watches
        .iter()
        .zip(&config.sources)
        .map(|(watch, source)| {
            (
                watch.provider.id().to_owned(),
                serde_json::to_value(source).unwrap_or(serde_json::Value::Null),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn config_with(sources: &str) -> Config {
        toml::from_str(&format!(
            "[[notifiers]]\ntype = \"apprise\"\nurls = [\"mailto://a@b.c\"]\n{sources}"
        ))
        .expect("parse")
    }

    #[test]
    fn diff_should_detect_added_and_removed_sources() {
        let current = config_with("[[sources]]\ntype = \"github\"\nrepo = \"org/old\"\n");
        let next = config_with("[[sources]]\ntype = \"github\"\nrepo = \"org/new\"\n");

        let diff = ConfigDiff::compute(&current, &next);
        assert_eq!(diff.removed, vec!["github:org/old"]);
        assert_eq!(diff.added, vec!["github:org/new"]);
        assert!(diff.changed.is_empty());
    }

    #[test]
    fn diff_should_flag_only_the_source_whose_spec_changed() {
        let current = config_with(
            "[[sources]]\ntype = \"github\"\nrepo = \"org/a\"\n\
             [[sources]]\ntype = \"github\"\nrepo = \"org/b\"\n",
        );
        // `org/a` gains a prerelease filter; `org/b` is byte-identical.
        let next = config_with(
            "[[sources]]\ntype = \"github\"\nrepo = \"org/a\"\ninclude_prerelease = true\n\
             [[sources]]\ntype = \"github\"\nrepo = \"org/b\"\n",
        );

        let diff = ConfigDiff::compute(&current, &next);
        assert_eq!(diff.changed, vec!["github:org/a"]);
        assert!(diff.added.is_empty());
        assert!(diff.removed.is_empty());
    }
}
