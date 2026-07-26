//! Index configured [`Watch`]es for O(1) webhook and API resolution.
//!
//! Centralises repo→watch mapping that was duplicated in API handlers.
//! Matches github-release-monitor's repo id resolution (`owner/repo` → source).

use std::collections::HashMap;

use crate::pipeline::Watch;

/// Container registry kinds indexed for image webhooks (`POST …/webhooks/docker`).
pub const CONTAINER_KINDS: &[&str] = &["docker", "ghcr", "quay", "ecr"];

/// Fast lookup table built once at startup from configured watches.
///
/// Repo/image lookups are **multi-match**: with `[[organizations]]` two tenants
/// legitimately watch the same upstream (`platform::github:tokio-rs/tokio` and
/// `security::github:tokio-rs/tokio`), and a single inbound webhook must reach
/// both. A single-slot map here silently dropped every org but the last one
/// indexed — deliveries would arrive only via the next poll.
#[derive(Debug, Clone)]
pub struct WatchIndex {
    watches: Vec<Watch>,
    by_id: HashMap<String, usize>,
    /// `(kind, normalised_repo)` → watch indices (multi-org fan-out)
    by_repo: HashMap<(String, String), Vec<usize>>,
    /// `kind` → (`normalised_image` → watch indices) for container sources
    by_image: HashMap<String, HashMap<String, Vec<usize>>>,
}

impl WatchIndex {
    /// Build an index from the resolved watch list.
    #[must_use]
    pub fn new(watches: Vec<Watch>) -> Self {
        let mut by_id = HashMap::new();
        let mut by_repo: HashMap<(String, String), Vec<usize>> = HashMap::new();
        let mut by_image: HashMap<String, HashMap<String, Vec<usize>>> = HashMap::new();

        for (idx, watch) in watches.iter().enumerate() {
            by_id.insert(watch.provider.id().to_owned(), idx);
            let kind = watch.provider.kind().to_owned();
            let repo = normalise_repo(watch.provider.display_name());
            by_repo.entry((kind.clone(), repo)).or_default().push(idx);

            if CONTAINER_KINDS.contains(&kind.as_str()) {
                let image = normalise_image(watch.provider.display_name());
                by_image
                    .entry(kind)
                    .or_default()
                    .entry(image)
                    .or_default()
                    .push(idx);
            }
        }

        Self {
            watches,
            by_id,
            by_repo,
            by_image,
        }
    }

    /// All watches (for iteration / scheduler).
    #[must_use]
    pub fn watches(&self) -> &[Watch] {
        &self.watches
    }

    /// Into inner vec (for scheduler ownership transfer).
    #[must_use]
    pub fn into_watches(self) -> Vec<Watch> {
        self.watches
    }

    /// Exact match on stable source id (`github:owner/repo`,
    /// `platform::github:owner/repo`, `pypi:requests`, …).
    #[must_use]
    pub fn find_by_id(&self, source_id: &str) -> Option<&Watch> {
        self.by_id.get(source_id).map(|&i| &self.watches[i])
    }

    /// Every watch of `repo` for a specific provider kind.
    #[must_use]
    pub fn find_repos(&self, kind: &str, repo: &str) -> Vec<&Watch> {
        let key = (kind.to_owned(), normalise_repo(repo));
        self.indices_to_watches(self.by_repo.get(&key))
    }

    /// Every watch of `repo` across several kinds (same repo path).
    ///
    /// Used by webhook routes that serve sibling forges (Gitea route also
    /// answers for Codeberg, GitHub route for `github`-kind only).
    #[must_use]
    pub fn find_repos_any(&self, kinds: &[&str], repo: &str) -> Vec<&Watch> {
        let normalised = normalise_repo(repo);
        let mut found = Vec::new();
        for kind in kinds {
            if let Some(indices) = self.by_repo.get(&((*kind).to_owned(), normalised.clone())) {
                found.extend(indices.iter().map(|&i| &self.watches[i]));
            }
        }
        found
    }

    /// Every watch of a container image across registry kinds.
    #[must_use]
    pub fn find_images_any(&self, kinds: &[&str], image: &str) -> Vec<&Watch> {
        let normalised = normalise_image(image);
        let mut found = Vec::new();
        for kind in kinds {
            if let Some(indices) = self
                .by_image
                .get(*kind)
                .and_then(|images| images.get(&normalised))
            {
                found.extend(indices.iter().map(|&i| &self.watches[i]));
            }
        }
        found
    }

    fn indices_to_watches(&self, indices: Option<&Vec<usize>>) -> Vec<&Watch> {
        indices
            .map(|indices| indices.iter().map(|&i| &self.watches[i]).collect())
            .unwrap_or_default()
    }
}

/// Normalise `owner/repo` for case-insensitive matching (gh-monitor lowercases ids).
fn normalise_repo(repo: &str) -> String {
    repo.trim().to_ascii_lowercase()
}

/// Normalise container image paths (`library/nginx`, `org/app`).
fn normalise_image(image: &str) -> String {
    image.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::pipeline::{Filter, Watch};
    use crate::sources::{GiteaReleasesSource, Provider};
    use crate::watch_lookup::WatchIndex;

    fn sample_watch(kind: &str, repo: &str) -> Watch {
        let provider = match kind {
            "github" => Provider::Github(GiteaReleasesSource::github(
                format!("github:{repo}"),
                repo,
                None,
            )),
            "codeberg" => Provider::Codeberg(GiteaReleasesSource::codeberg(
                format!("codeberg:{repo}"),
                repo,
                None,
            )),
            _ => panic!("unsupported kind in test"),
        };
        Watch {
            provider,
            interval: Duration::from_secs(900),
            jitter: Duration::from_secs(0),
            poll_on_startup: true,
            filter: Filter::new(false, None, None).expect("filter"),
            routing_tag: None,
            notify_schedule: None,
            organization_id: None,
        }
    }

    #[test]
    fn find_repos_should_be_case_insensitive() {
        let index = WatchIndex::new(vec![sample_watch("github", "Tokio-rs/Tokio")]);
        assert_eq!(index.find_repos("github", "tokio-rs/tokio").len(), 1);
    }

    #[test]
    fn find_repos_should_not_cross_kinds() {
        let index = WatchIndex::new(vec![sample_watch("github", "Forgejo/Forgejo")]);
        assert_eq!(index.find_repos("github", "Forgejo/Forgejo").len(), 1);
        assert!(index.find_repos("codeberg", "Forgejo/Forgejo").is_empty());
    }

    fn org_watch(org: &str, repo: &str) -> Watch {
        let mut watch = sample_watch("github", repo);
        watch.provider = Provider::Github(GiteaReleasesSource::github(
            format!("{org}::github:{repo}"),
            repo,
            None,
        ));
        watch.organization_id = Some(org.to_owned());
        watch
    }

    #[test]
    fn find_repos_should_fan_out_across_organizations() {
        // Two organizations watching the same upstream repo: one inbound
        // webhook must resolve to BOTH watches, not whichever indexed last.
        let index = WatchIndex::new(vec![
            org_watch("platform", "tokio-rs/tokio"),
            org_watch("security", "tokio-rs/tokio"),
        ]);
        let found = index.find_repos_any(&["github"], "tokio-rs/tokio");
        assert_eq!(found.len(), 2);
    }

    fn docker_watch(image: &str) -> Watch {
        Watch {
            provider: Provider::Docker(crate::sources::DockerSource::docker_hub(
                format!("docker:{image}"),
                image,
                None,
            )),
            interval: Duration::from_secs(900),
            jitter: Duration::from_secs(0),
            poll_on_startup: true,
            filter: Filter::new(false, None, None).expect("filter"),
            routing_tag: None,
            notify_schedule: None,
            organization_id: None,
        }
    }

    #[test]
    fn find_images_should_match_docker_sources() {
        let index = WatchIndex::new(vec![docker_watch("library/nginx")]);
        assert_eq!(index.find_images_any(&["docker"], "Library/Nginx").len(), 1);
        assert!(index.find_images_any(&["docker"], "other/image").is_empty());
    }

    #[test]
    fn find_image_any_should_include_ecr_kind() {
        let index = WatchIndex::new(vec![Watch {
            provider: Provider::Ecr(crate::sources::DockerSource::ecr_public(
                "ecr:aws-containers/amazon-eks-ami",
                "aws-containers/amazon-eks-ami",
                None,
            )),
            interval: Duration::from_secs(900),
            jitter: Duration::from_secs(0),
            poll_on_startup: true,
            filter: Filter::new(false, None, None).expect("filter"),
            routing_tag: None,
            notify_schedule: None,
            organization_id: None,
        }]);
        assert_eq!(
            index
                .find_images_any(super::CONTAINER_KINDS, "aws-containers/amazon-eks-ami")
                .len(),
            1
        );
    }
}
