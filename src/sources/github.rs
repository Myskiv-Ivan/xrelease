//! GitHub releases source (Gitea-compatible API preset).
//!
//! Alias for [`GiteaReleasesSource`] configured with GitHub.com endpoints.
//! Construct via [`GiteaReleasesSource::github`].

pub use super::gitea_releases::GiteaReleasesSource as GithubSource;
