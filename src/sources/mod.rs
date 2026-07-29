//! Release sources.
//!
//! # Dispatch choice (Best Practices, Chapter 6)
//!
//! The set of source kinds is closed and maps one-to-one onto config variants,
//! so we use an **enum ([`Provider`]) with static dispatch** rather than
//! `Box<dyn Source>`. Shared fetch logic for Gitea-compatible hosts and package
//! registries lives in dedicated modules to avoid duplication.

mod artifacthub;
mod bitbucket;
mod docker;
mod feed;
mod gitea_releases;
mod github;
mod gitlab;
mod http;
mod package;
mod release_urls;
mod time;

pub use artifacthub::ArtifactHubSource;
pub use bitbucket::{BitbucketEdition, BitbucketSource};
pub use docker::{ContainerRegistry, DockerSource};
pub use feed::FeedSource;
pub use gitea_releases::GiteaReleasesSource;
pub use github::GithubSource;
pub use gitlab::GitlabSource;
pub use http::FetchOutcome;
pub use package::{PackageRegistry, PackageSource};

pub(crate) use time::{parse_rfc3339, parse_timestamp_millis};

use crate::error::SourceError;

/// A configured release source. One variant per supported upstream kind.
#[derive(Debug, Clone)]
pub enum Provider {
    /// GitHub.com releases (Gitea-compatible REST + Atom fallback).
    Github(GiteaReleasesSource),
    /// Codeberg.org releases (Gitea API).
    Codeberg(GiteaReleasesSource),
    /// Self-hosted Gitea / Forgejo releases.
    Gitea(GiteaReleasesSource),
    /// GitLab project releases via the GitLab REST API.
    Gitlab(GitlabSource),
    /// Bitbucket Cloud repository tags.
    Bitbucket(BitbucketSource),
    /// Container image tags via the Registry v2 `tags/list` API.
    Docker(DockerSource),
    /// GitHub Container Registry (`ghcr.io`).
    Ghcr(DockerSource),
    /// Quay.io container registry.
    Quay(DockerSource),
    /// AWS ECR Public gallery.
    Ecr(DockerSource),
    /// Any RSS / Atom / JSON feed.
    Feed(FeedSource),
    /// Python PyPI package versions.
    Pypi(PackageSource),
    /// Node.js npm package versions.
    Npm(PackageSource),
    /// Rust crates.io versions.
    Cargo(PackageSource),
    /// Maven Central artifact versions.
    Maven(PackageSource),
    /// NuGet.org package versions.
    Nuget(PackageSource),
    /// Hex.pm (Elixir) package versions.
    Hex(PackageSource),
    /// RubyGems.org gem versions.
    Rubygems(PackageSource),
    /// Packagist (PHP) package versions.
    Packagist(PackageSource),
    /// Yarn registry package versions.
    Yarn(PackageSource),
    /// Perl CPAN distribution versions.
    Cpan(PackageSource),
    /// Artifact Hub catalog package (Helm chart by default).
    ArtifactHub(ArtifactHubSource),
}

impl Provider {
    /// Build a container-registry variant through the shared [`DockerSource`] helper.
    #[must_use]
    pub fn container_registry(
        id: impl Into<String>,
        image: impl Into<String>,
        registry: ContainerRegistry,
        token: Option<String>,
    ) -> Self {
        let id = id.into();
        let source = registry.build_source(id.clone(), image, token);
        match source.kind() {
            "ghcr" => Self::Ghcr(source),
            "quay" => Self::Quay(source),
            "ecr" => Self::Ecr(source),
            _ => Self::Docker(source),
        }
    }

    /// Build a package-registry variant through the shared [`PackageSource`] helper.
    #[must_use]
    pub fn package_registry(
        id: impl Into<String>,
        name: impl Into<String>,
        registry: PackageRegistry,
    ) -> Self {
        let id = id.into();
        let name = name.into();
        match registry {
            PackageRegistry::Pypi => Self::Pypi(PackageSource::new(id, name, registry)),
            PackageRegistry::Npm => Self::Npm(PackageSource::new(id, name, registry)),
            PackageRegistry::Cargo => Self::Cargo(PackageSource::new(id, name, registry)),
            PackageRegistry::Maven => Self::Maven(PackageSource::new(id, name, registry)),
            PackageRegistry::Nuget => Self::Nuget(PackageSource::new(id, name, registry)),
            PackageRegistry::Hex => Self::Hex(PackageSource::new(id, name, registry)),
            PackageRegistry::Rubygems => Self::Rubygems(PackageSource::new(id, name, registry)),
            PackageRegistry::Packagist => Self::Packagist(PackageSource::new(id, name, registry)),
            PackageRegistry::Yarn => Self::Yarn(PackageSource::new(id, name, registry)),
            PackageRegistry::Cpan => Self::Cpan(PackageSource::new(id, name, registry)),
        }
    }

    /// Container-registry variant, if any. All registry kinds share [`DockerSource`].
    fn container(&self) -> Option<&DockerSource> {
        match self {
            Self::Docker(s) | Self::Ghcr(s) | Self::Quay(s) | Self::Ecr(s) => Some(s),
            _ => None,
        }
    }

    /// Package-registry variant, if any. All package kinds share [`PackageSource`].
    fn package(&self) -> Option<&PackageSource> {
        match self {
            Self::Pypi(s)
            | Self::Npm(s)
            | Self::Cargo(s)
            | Self::Maven(s)
            | Self::Nuget(s)
            | Self::Hex(s)
            | Self::Rubygems(s)
            | Self::Packagist(s)
            | Self::Yarn(s)
            | Self::Cpan(s) => Some(s),
            _ => None,
        }
    }

    /// Forge / feed sources after [`Self::package`] / [`Self::container`] miss.
    ///
    /// Collapses the repeated match arms for id/kind/fetch onto one enum so
    /// registry variants stay unreachable only in this helper.
    fn primary(&self) -> PrimarySource<'_> {
        match self {
            Self::Github(s) | Self::Codeberg(s) | Self::Gitea(s) => PrimarySource::Gitea(s),
            Self::Gitlab(s) => PrimarySource::Gitlab(s),
            Self::Bitbucket(s) => PrimarySource::Bitbucket(s),
            Self::ArtifactHub(s) => PrimarySource::ArtifactHub(s),
            Self::Feed(s) => PrimarySource::Feed(s),
            Self::Docker(_)
            | Self::Ghcr(_)
            | Self::Quay(_)
            | Self::Ecr(_)
            | Self::Pypi(_)
            | Self::Npm(_)
            | Self::Cargo(_)
            | Self::Maven(_)
            | Self::Nuget(_)
            | Self::Hex(_)
            | Self::Rubygems(_)
            | Self::Packagist(_)
            | Self::Yarn(_)
            | Self::Cpan(_) => unreachable!("handled by package() or container()"),
        }
    }

    /// Stable per-source identifier, used as the de-duplication namespace.
    #[must_use]
    pub fn id(&self) -> &str {
        if let Some(s) = self.package() {
            return s.id();
        }
        if let Some(s) = self.container() {
            return s.id();
        }
        match self.primary() {
            PrimarySource::Gitea(s) => s.id(),
            PrimarySource::Gitlab(s) => s.id(),
            PrimarySource::Bitbucket(s) => s.id(),
            PrimarySource::ArtifactHub(s) => s.id(),
            PrimarySource::Feed(s) => s.id(),
        }
    }

    /// Machine-readable source kind.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        if let Some(s) = self.package() {
            return s.kind();
        }
        if let Some(s) = self.container() {
            return s.kind();
        }
        match self.primary() {
            PrimarySource::Gitea(s) => s.kind(),
            PrimarySource::Gitlab(_) => "gitlab",
            PrimarySource::Bitbucket(_) => "bitbucket",
            PrimarySource::ArtifactHub(_) => "artifacthub",
            PrimarySource::Feed(_) => "feed",
        }
    }

    /// Human-readable source kind, for notification titles.
    #[must_use]
    pub fn kind_label(&self) -> &'static str {
        if let Some(s) = self.package() {
            return s.kind_label();
        }
        if let Some(s) = self.container() {
            return s.kind_label();
        }
        match self.primary() {
            PrimarySource::Gitea(s) => s.kind_label(),
            PrimarySource::Gitlab(_) => "GitLab",
            PrimarySource::Bitbucket(_) => "Bitbucket",
            PrimarySource::ArtifactHub(s) => s.kind_label(),
            PrimarySource::Feed(_) => "Feed",
        }
    }

    /// Human-readable subject (the repo, image, package, or feed name).
    #[must_use]
    pub fn display_name(&self) -> &str {
        if let Some(s) = self.package() {
            return s.display_name();
        }
        if let Some(s) = self.container() {
            return s.display_name();
        }
        match self.primary() {
            PrimarySource::Gitea(s) => s.display_name(),
            PrimarySource::Gitlab(s) => s.display_name(),
            PrimarySource::Bitbucket(s) => s.display_name(),
            PrimarySource::ArtifactHub(s) => s.display_name(),
            PrimarySource::Feed(s) => s.display_name(),
        }
    }

    /// Human-facing catalog page for this source (repo, package index, feed URL).
    #[must_use]
    pub fn source_page_url(&self) -> Option<String> {
        if let Some(s) = self.package() {
            return Some(s.source_page_url());
        }
        if let Some(s) = self.container() {
            return Some(s.source_page_url());
        }
        match self.primary() {
            PrimarySource::Gitea(s) => Some(s.source_page_url()),
            PrimarySource::Gitlab(s) => Some(s.source_page_url()),
            PrimarySource::Bitbucket(s) => Some(s.source_page_url()),
            PrimarySource::ArtifactHub(s) => Some(s.source_page_url()),
            PrimarySource::Feed(s) => Some(s.source_page_url()),
        }
    }

    /// Best-effort release page URL for a stored identity (usually the tag).
    #[must_use]
    pub fn release_page_url(&self, identity: &str) -> Option<String> {
        if let Some(s) = self.package() {
            return Some(s.release_page_url(identity));
        }
        if let Some(s) = self.container() {
            return Some(s.release_page_url(identity));
        }
        match self.primary() {
            PrimarySource::Gitea(s) => Some(s.release_page_url(identity)),
            PrimarySource::Gitlab(s) => Some(s.release_page_url(identity)),
            PrimarySource::Bitbucket(s) => Some(s.release_page_url(identity)),
            PrimarySource::ArtifactHub(s) => Some(s.release_page_url(identity)),
            PrimarySource::Feed(_) => None,
        }
    }

    /// Fetch the current set of releases from the upstream source.
    pub async fn fetch(
        &self,
        http: &reqwest::Client,
        if_none_match: Option<&str>,
    ) -> Result<FetchOutcome, SourceError> {
        if let Some(s) = self.package() {
            return s.fetch(http, if_none_match).await;
        }
        if let Some(s) = self.container() {
            return s.fetch(http, if_none_match).await;
        }
        match self.primary() {
            PrimarySource::Gitea(s) => s.fetch(http, if_none_match).await,
            PrimarySource::Gitlab(s) => s.fetch(http, if_none_match).await,
            PrimarySource::Bitbucket(s) => s.fetch(http, if_none_match).await,
            PrimarySource::ArtifactHub(s) => s.fetch(http, if_none_match).await,
            PrimarySource::Feed(s) => s.fetch(http, if_none_match).await,
        }
    }
}

/// Shared handle for forge/feed variants (everything that is not package/container).
enum PrimarySource<'a> {
    Gitea(&'a GiteaReleasesSource),
    Gitlab(&'a GitlabSource),
    Bitbucket(&'a BitbucketSource),
    ArtifactHub(&'a ArtifactHubSource),
    Feed(&'a FeedSource),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_registry_should_map_presets_to_provider_variants() {
        let ghcr = Provider::container_registry("id", "org/app", ContainerRegistry::Ghcr, None);
        assert!(matches!(ghcr, Provider::Ghcr(_)));
        assert_eq!(ghcr.kind(), "ghcr");

        let quay = Provider::container_registry("id", "org/app", ContainerRegistry::Quay, None);
        assert!(matches!(quay, Provider::Quay(_)));

        let ecr = Provider::container_registry(
            "id",
            "docker/library/nginx",
            ContainerRegistry::EcrPublic,
            None,
        );
        assert!(matches!(ecr, Provider::Ecr(_)));
        assert_eq!(ecr.kind(), "ecr");

        let hub =
            Provider::container_registry("id", "library/nginx", ContainerRegistry::DockerHub, None);
        assert!(matches!(hub, Provider::Docker(_)));
    }

    #[test]
    fn package_registry_should_map_yarn_and_cpan() {
        let yarn = Provider::package_registry("id", "lodash", PackageRegistry::Yarn);
        assert!(matches!(yarn, Provider::Yarn(_)));
        assert_eq!(yarn.kind(), "yarn");

        let cpan = Provider::package_registry("id", "Moose", PackageRegistry::Cpan);
        assert!(matches!(cpan, Provider::Cpan(_)));
        assert_eq!(cpan.kind(), "cpan");
    }
}
