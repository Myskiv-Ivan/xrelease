//! GitLab releases source.
//!
//! Uses the GitLab Releases REST API:
//! `GET /api/v4/projects/{encoded_path}/releases?per_page=20`
//!
//! # Authentication
//!
//! For public projects on gitlab.com no token is required. For private projects
//! or self-hosted GitLab instances, set a **Personal Access Token** or
//! **Deploy Token** with the `read_api` scope in `token` (config) or
//! `GITLAB_TOKEN` (environment variable).
//!
//! # Self-hosted instances
//!
//! Set `host = "https://gitlab.example.com"` in the source config. Defaults to
//! `https://gitlab.com`.
//!
//! # Project path
//!
//! Use the full `owner/repo` path (e.g. `gitlab-org/gitlab`). Subgroups are
//! supported: `mygroup/subgroup/project`. Slashes are URL-encoded automatically.
//!
//! # Analogue
//!
//! This provider mirrors the GitLab fetcher in github-release-monitor, but
//! without tag fallback (use `type = "feed"` with the GitLab Atom tag feed if
//! you need tags from a project with no formal releases).

use serde::Deserialize;

use super::http::{fetch_json, FetchOutcome};
use super::parse_rfc3339;
use crate::error::SourceError;
use crate::model::Release;

/// Default GitLab SaaS host.
const DEFAULT_HOST: &str = "https://gitlab.com";

/// A source watching releases of a single GitLab project.
#[derive(Debug, Clone)]
pub struct GitlabSource {
    id: String,
    /// Full project path, e.g. `gitlab-org/gitlab` or `group/sub/project`.
    project: String,
    /// Base URL of the GitLab instance (no trailing slash).
    host: String,
    /// Optional Personal Access Token or Deploy Token with `read_api` scope.
    token: Option<String>,
}

/// GitLab Releases API response element.
///
/// Only the fields we actually use are declared. Serde silently ignores any
/// extra keys (e.g. `name`, `assets`) returned by the API.
///
/// <https://docs.gitlab.com/api/releases/#list-releases>
#[derive(Debug, Deserialize)]
struct GitlabReleaseApi {
    tag_name: String,
    /// Markdown description / changelog written by the author.
    description: Option<String>,
    /// Publication timestamp (ISO 8601).
    released_at: Option<String>,
    /// `_links` block contains a `self` link to the release page.
    #[serde(rename = "_links")]
    links: Option<GitlabLinks>,
}

#[derive(Debug, Deserialize)]
struct GitlabLinks {
    /// URL to the release page in the GitLab UI.
    #[serde(rename = "self")]
    self_link: Option<String>,
}

impl GitlabSource {
    /// Create a GitLab source.
    ///
    /// - `id` — stable internal identifier for this watch.
    /// - `host` — base URL, e.g. `https://gitlab.com` (defaults to gitlab.com when empty).
    /// - `project` — path like `gitlab-org/gitlab`.
    /// - `token` — optional access token for private projects.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        host: impl Into<String>,
        project: impl Into<String>,
        token: Option<String>,
    ) -> Self {
        let host = {
            let h = host.into();
            if h.is_empty() {
                DEFAULT_HOST.to_owned()
            } else {
                h.trim_end_matches('/').to_owned()
            }
        };
        Self {
            id: id.into(),
            host,
            project: project.into(),
            token,
        }
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn display_name(&self) -> &str {
        &self.project
    }

    pub(crate) fn source_page_url(&self) -> String {
        format!("{}/{}", self.host.trim_end_matches('/'), self.project)
    }

    pub(crate) fn release_page_url(&self, tag: &str) -> String {
        super::release_urls::gitlab_release_url(&self.host, &self.project, tag)
    }

    fn releases_url(&self) -> String {
        // GitLab API requires the project path to be URL-encoded: `/` → `%2F`.
        let encoded = self.project.replace('/', "%2F");
        format!(
            "{}/api/v4/projects/{}/releases?per_page=20",
            self.host, encoded
        )
    }

    /// Fetch the latest releases from the GitLab API.
    pub(crate) async fn fetch(
        &self,
        http: &reqwest::Client,
        if_none_match: Option<&str>,
    ) -> Result<FetchOutcome, SourceError> {
        let url = self.releases_url();
        let token = self.token.clone();
        let result = fetch_json::<Vec<GitlabReleaseApi>>(http, &url, if_none_match, |b| {
            if let Some(token) = &token {
                b.header("PRIVATE-TOKEN", token)
            } else {
                b
            }
        })
        .await?;

        if result.not_modified {
            return Ok(FetchOutcome::not_modified());
        }

        let raw = result.data.unwrap_or_default();
        let releases = raw
            .into_iter()
            .map(|r| {
                let published = r.released_at.as_deref().and_then(parse_rfc3339);
                let release_url = r.links.and_then(|l| l.self_link).filter(|s| !s.is_empty());
                Release::new(r.tag_name)
                    .with_url(release_url)
                    .with_published(published)
                    .with_body(r.description.filter(|d| !d.trim().is_empty()))
            })
            .collect();

        Ok(FetchOutcome::fresh(releases, result.etag))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releases_url_should_encode_slashes_in_project_path() {
        let s = GitlabSource::new("id", "https://gitlab.com", "group/sub/project", None);
        assert_eq!(
            s.releases_url(),
            "https://gitlab.com/api/v4/projects/group%2Fsub%2Fproject/releases?per_page=20"
        );
    }

    #[test]
    fn new_should_default_to_gitlab_com_when_host_is_empty() {
        let s = GitlabSource::new("id", "", "owner/repo", None);
        assert_eq!(s.host, DEFAULT_HOST);
    }

    #[test]
    fn new_should_strip_trailing_slash_from_host() {
        let s = GitlabSource::new("id", "https://gitlab.example.com/", "owner/repo", None);
        assert_eq!(s.host, "https://gitlab.example.com");
    }
}
