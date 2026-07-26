//! Bitbucket Cloud and Server (Data Center) tag sources.
//!
//! Bitbucket has no GitHub-style "releases" API; git tags are the practical
//! release signal.
//!
//! # Cloud
//!
//! `GET /2.0/repositories/{workspace}/{repo}/refs/tags`
//!
//! # Server / Data Center
//!
//! `GET /rest/api/1.0/projects/{projectKey}/repos/{slug}/tags`
//!
//! Set `edition = "server"` and `host = "https://bitbucket.example.com"`.

use chrono::{TimeZone, Utc};
use serde::Deserialize;

use super::http::{fetch_json, FetchOutcome};
use super::parse_rfc3339;
use crate::error::SourceError;
use crate::model::Release;

/// Bitbucket Cloud REST API base.
const DEFAULT_CLOUD_API: &str = "https://api.bitbucket.org/2.0";
/// Bitbucket Cloud web UI base.
const DEFAULT_CLOUD_WEB: &str = "https://bitbucket.org";

/// Cloud vs self-hosted Server / Data Center API shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BitbucketEdition {
    #[default]
    Cloud,
    Server,
}

/// A source watching git tags on Bitbucket Cloud or Server.
#[derive(Debug, Clone)]
pub struct BitbucketSource {
    id: String,
    edition: BitbucketEdition,
    /// Cloud: `workspace/repo`. Server: `PROJECT/repo_slug`.
    repo: String,
    project_key: String,
    repo_slug: String,
    api_base: String,
    web_base: String,
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CloudTagsPage {
    values: Option<Vec<CloudTagEntry>>,
}

#[derive(Debug, Deserialize)]
struct CloudTagEntry {
    name: String,
    target: Option<CloudTagTarget>,
    links: Option<CloudTagLinks>,
}

#[derive(Debug, Deserialize)]
struct CloudTagTarget {
    date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CloudTagLinks {
    html: Option<CloudTagLink>,
}

#[derive(Debug, Deserialize)]
struct CloudTagLink {
    href: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ServerTagsPage {
    values: Option<Vec<ServerTagEntry>>,
}

#[derive(Debug, Deserialize)]
struct ServerTagEntry {
    #[serde(rename = "displayId")]
    display_id: String,
    #[serde(rename = "latestCommit")]
    latest_commit: Option<ServerCommit>,
}

#[derive(Debug, Deserialize)]
struct ServerCommit {
    #[serde(rename = "authorTimestamp")]
    author_timestamp: Option<i64>,
}

impl BitbucketSource {
    /// Bitbucket Cloud preset (`workspace/repo_slug`).
    #[must_use]
    pub fn cloud(id: impl Into<String>, repo: impl Into<String>, token: Option<String>) -> Self {
        let repo = repo.into();
        let (project_key, repo_slug) = split_repo(&repo);
        Self {
            id: id.into(),
            edition: BitbucketEdition::Cloud,
            repo,
            project_key,
            repo_slug,
            api_base: DEFAULT_CLOUD_API.to_owned(),
            web_base: DEFAULT_CLOUD_WEB.to_owned(),
            token,
        }
    }

    /// Self-hosted Bitbucket Server / Data Center.
    #[must_use]
    pub fn server(
        id: impl Into<String>,
        host: impl Into<String>,
        repo: impl Into<String>,
        token: Option<String>,
    ) -> Self {
        let host = host.into().trim_end_matches('/').to_owned();
        let repo = repo.into();
        let (project_key, repo_slug) = split_repo(&repo);
        Self {
            id: id.into(),
            edition: BitbucketEdition::Server,
            repo,
            project_key,
            repo_slug,
            api_base: host.clone(),
            web_base: host,
            token,
        }
    }

    /// Custom Cloud API host override.
    #[must_use]
    pub fn with_host(
        id: impl Into<String>,
        repo: impl Into<String>,
        api_base: impl Into<String>,
        web_base: impl Into<String>,
        token: Option<String>,
    ) -> Self {
        let repo = repo.into();
        let (project_key, repo_slug) = split_repo(&repo);
        Self {
            id: id.into(),
            edition: BitbucketEdition::Cloud,
            repo,
            project_key,
            repo_slug,
            api_base: api_base.into().trim_end_matches('/').to_owned(),
            web_base: web_base.into().trim_end_matches('/').to_owned(),
            token,
        }
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn display_name(&self) -> &str {
        &self.repo
    }

    pub(crate) fn source_page_url(&self) -> String {
        format!("{}/{}", self.web_base.trim_end_matches('/'), self.repo)
    }

    pub(crate) fn release_page_url(&self, tag: &str) -> String {
        format!(
            "{}/{}/src/{}",
            self.web_base.trim_end_matches('/'),
            self.repo,
            super::release_urls::encode_path_segment(tag)
        )
    }

    pub(crate) async fn fetch(
        &self,
        http: &reqwest::Client,
        if_none_match: Option<&str>,
    ) -> Result<FetchOutcome, SourceError> {
        match self.edition {
            BitbucketEdition::Cloud => self.fetch_cloud(http, if_none_match).await,
            BitbucketEdition::Server => self.fetch_server(http, if_none_match).await,
        }
    }

    async fn fetch_cloud(
        &self,
        http: &reqwest::Client,
        if_none_match: Option<&str>,
    ) -> Result<FetchOutcome, SourceError> {
        let url = format!(
            "{}/repositories/{}/refs/tags?pagelen=100&sort=-target.date",
            self.api_base, self.repo
        );

        let result = fetch_json::<CloudTagsPage>(http, &url, if_none_match, |builder| {
            self.apply_auth(builder)
        })
        .await?;

        if result.not_modified {
            return Ok(FetchOutcome::not_modified());
        }

        let page = result
            .data
            .ok_or_else(|| SourceError::Other("empty Bitbucket tags response".into()))?;

        let releases = page
            .values
            .unwrap_or_default()
            .into_iter()
            .map(|entry| self.cloud_tag_to_release(entry))
            .collect();

        Ok(FetchOutcome::fresh(releases, result.etag))
    }

    async fn fetch_server(
        &self,
        http: &reqwest::Client,
        if_none_match: Option<&str>,
    ) -> Result<FetchOutcome, SourceError> {
        // `orderBy=MODIFICATION` sorts by commit date (newest first) — the
        // default ALPHABETICAL order could push the newest tag past the page
        // limit on repos with >100 tags. Mirrors Cloud's `sort=-target.date`.
        let url = format!(
            "{}/rest/api/1.0/projects/{}/repos/{}/tags?limit=100&orderBy=MODIFICATION",
            self.api_base, self.project_key, self.repo_slug
        );

        let result = fetch_json::<ServerTagsPage>(http, &url, if_none_match, |builder| {
            self.apply_auth(builder)
        })
        .await?;

        if result.not_modified {
            return Ok(FetchOutcome::not_modified());
        }

        let page = result
            .data
            .ok_or_else(|| SourceError::Other("empty Bitbucket Server tags response".into()))?;

        let releases = page
            .values
            .unwrap_or_default()
            .into_iter()
            .map(|entry| self.server_tag_to_release(entry))
            .collect();

        Ok(FetchOutcome::fresh(releases, result.etag))
    }

    fn apply_auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(token) = &self.token {
            builder.bearer_auth(token)
        } else {
            builder
        }
    }

    fn cloud_tag_to_release(&self, entry: CloudTagEntry) -> Release {
        let published = entry
            .target
            .as_ref()
            .and_then(|t| t.date.as_deref())
            .and_then(parse_rfc3339);

        let url = entry
            .links
            .and_then(|l| l.html)
            .and_then(|h| h.href)
            .or_else(|| {
                Some(format!(
                    "{}/{}/src/{}",
                    self.web_base, self.repo, entry.name
                ))
            });

        Release::new(entry.name)
            .with_url(url)
            .with_published(published)
    }

    fn server_tag_to_release(&self, entry: ServerTagEntry) -> Release {
        let published = entry
            .latest_commit
            .and_then(|c| c.author_timestamp)
            .and_then(|ms| Utc.timestamp_millis_opt(ms).single());

        let url = Some(format!(
            "{}/projects/{}/repos/{}/browse?at=refs/tags/{}",
            self.web_base, self.project_key, self.repo_slug, entry.display_id
        ));

        Release::new(entry.display_id)
            .with_url(url)
            .with_published(published)
    }
}

fn split_repo(repo: &str) -> (String, String) {
    let mut parts = repo.splitn(2, '/');
    let project = parts.next().unwrap_or_default().to_owned();
    let slug = parts.next().unwrap_or_default().to_owned();
    (project, slug)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitbucket_tags_should_use_the_shared_prerelease_policy() {
        // Bitbucket's API carries no prerelease flag, so classification is
        // delegated entirely to `Release::new`'s shared marker/semver logic
        // (`crate::model::is_prerelease`) — no Bitbucket-specific override,
        // so this can never drift from every other provider's policy again.
        assert!(Release::new("v1.0.0-rc1").prerelease);
        assert!(Release::new("1.0.0-beta.2").prerelease);
        assert!(!Release::new("v1.0.0").prerelease);
    }

    #[test]
    fn split_repo_should_separate_project_and_slug() {
        assert_eq!(
            split_repo("PROJ/my-repo"),
            ("PROJ".into(), "my-repo".into())
        );
    }

    #[test]
    fn server_preset_should_build_api_path() {
        let s = BitbucketSource::server("id", "https://bb.example.com", "PROJ/app", None);
        assert_eq!(s.edition, BitbucketEdition::Server);
        assert_eq!(s.project_key, "PROJ");
        assert_eq!(s.repo_slug, "app");
    }
}
