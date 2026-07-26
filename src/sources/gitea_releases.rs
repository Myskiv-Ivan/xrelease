//! Gitea-compatible releases API — shared by GitHub, Codeberg, Gitea, and Forgejo.
//!
//! All of these expose (nearly) the same REST shape:
//! `GET /repos/{owner}/{repo}/releases` and a public Atom feed at
//! `/{owner}/{repo}/releases.atom`. GitHub uses a separate API host
//! (`api.github.com`); Gitea-family hosts use `{host}/api/v1`.

use serde::Deserialize;

use super::http::{fetch_bytes, fetch_json, FetchOutcome};
use super::parse_rfc3339;
use crate::error::SourceError;
use crate::model::Release;

/// How to attach a token to REST requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthStyle {
    /// `Authorization: Bearer {token}` (GitHub).
    Bearer,
    /// `Authorization: token {token}` (Gitea / Codeberg).
    Token,
}

/// Static endpoints and labels for a Gitea-compatible host preset.
#[derive(Debug, Clone)]
pub(crate) struct GiteaPreset {
    pub kind: &'static str,
    pub kind_label: &'static str,
    /// REST API base including `/repos` prefix segment, e.g.
    /// `https://api.github.com/repos` or `https://codeberg.org/api/v1/repos`.
    pub api_repos_base: String,
    /// Web UI base, e.g. `https://github.com`.
    pub web_base: String,
    pub auth_style: AuthStyle,
    /// GitHub-only API version header value.
    pub github_api_version: Option<&'static str>,
}

/// A source using the Gitea/GitHub releases REST API (with Atom fallback).
#[derive(Debug, Clone)]
pub struct GiteaReleasesSource {
    id: String,
    repo: String,
    preset: GiteaPreset,
    token: Option<String>,
}

/// Raw releases API element (GitHub + Gitea share this shape).
#[derive(Debug, Deserialize)]
struct ReleaseApi {
    tag_name: String,
    html_url: String,
    body: Option<String>,
    published_at: Option<String>,
    prerelease: bool,
    draft: bool,
}

impl GiteaReleasesSource {
    /// GitHub.com preset.
    #[must_use]
    pub fn github(id: impl Into<String>, repo: impl Into<String>, token: Option<String>) -> Self {
        Self::new(
            id,
            repo,
            GiteaPreset {
                kind: "github",
                kind_label: "GitHub",
                api_repos_base: "https://api.github.com/repos".into(),
                web_base: "https://github.com".into(),
                auth_style: AuthStyle::Bearer,
                github_api_version: Some("2022-11-28"),
            },
            token,
        )
    }

    /// Codeberg.org preset.
    #[must_use]
    pub fn codeberg(id: impl Into<String>, repo: impl Into<String>, token: Option<String>) -> Self {
        Self::new(
            id,
            repo,
            GiteaPreset {
                kind: "codeberg",
                kind_label: "Codeberg",
                api_repos_base: "https://codeberg.org/api/v1/repos".into(),
                web_base: "https://codeberg.org".into(),
                auth_style: AuthStyle::Token,
                github_api_version: None,
            },
            token,
        )
    }

    /// Self-hosted Gitea / Forgejo instance.
    #[must_use]
    pub fn gitea(
        id: impl Into<String>,
        host: impl Into<String>,
        repo: impl Into<String>,
        token: Option<String>,
    ) -> Self {
        let host = host.into().trim_end_matches('/').to_owned();
        Self::new(
            id,
            repo,
            GiteaPreset {
                kind: "gitea",
                kind_label: "Gitea",
                api_repos_base: format!("{host}/api/v1/repos"),
                web_base: host,
                auth_style: AuthStyle::Token,
                github_api_version: None,
            },
            token,
        )
    }

    fn new(
        id: impl Into<String>,
        repo: impl Into<String>,
        preset: GiteaPreset,
        token: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            repo: repo.into(),
            preset,
            token,
        }
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn display_name(&self) -> &str {
        &self.repo
    }

    pub(crate) fn kind(&self) -> &'static str {
        self.preset.kind
    }

    pub(crate) fn kind_label(&self) -> &'static str {
        self.preset.kind_label
    }

    pub(crate) fn source_page_url(&self) -> String {
        format!(
            "{}/{}",
            self.preset.web_base.trim_end_matches('/'),
            self.repo
        )
    }

    pub(crate) fn release_page_url(&self, tag: &str) -> String {
        super::release_urls::gitea_release_url(&self.preset.web_base, &self.repo, tag)
    }

    fn rest_url(&self) -> String {
        format!(
            "{}/{}/releases?per_page=30",
            self.preset.api_repos_base.trim_end_matches('/'),
            self.repo
        )
    }

    fn atom_url(&self) -> String {
        format!(
            "{}/{}/releases.atom",
            self.preset.web_base.trim_end_matches('/'),
            self.repo
        )
    }

    fn apply_auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let Some(token) = &self.token else {
            return builder;
        };
        match self.preset.auth_style {
            AuthStyle::Bearer => builder.header("Authorization", format!("Bearer {token}")),
            AuthStyle::Token => builder.header("Authorization", format!("token {token}")),
        }
    }

    fn apply_rest_headers(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut builder = builder.header("Accept", "application/json");
        if let Some(version) = self.preset.github_api_version {
            builder = builder.header("X-GitHub-Api-Version", version);
        }
        self.apply_auth(builder)
    }

    async fn fetch_rest(
        &self,
        http: &reqwest::Client,
        if_none_match: Option<&str>,
    ) -> Result<FetchOutcome, SourceError> {
        let url = self.rest_url();
        let result = fetch_json::<Vec<ReleaseApi>>(http, &url, if_none_match, |b| {
            self.apply_rest_headers(b)
        })
        .await?;

        if result.not_modified {
            return Ok(FetchOutcome::not_modified());
        }

        let raw = result.data.unwrap_or_default();
        let releases = raw
            .into_iter()
            .filter(|r| !r.draft)
            .map(api_to_release)
            .collect();
        Ok(FetchOutcome::fresh(releases, result.etag))
    }

    async fn fetch_atom(
        &self,
        http: &reqwest::Client,
        if_none_match: Option<&str>,
    ) -> Result<FetchOutcome, SourceError> {
        let url = self.atom_url();
        let result = fetch_bytes(http, &url, if_none_match, |b| b).await?;

        if result.not_modified {
            return Ok(FetchOutcome::not_modified());
        }

        let bytes = result
            .bytes
            .ok_or_else(|| SourceError::Other("empty atom response".into()))?;
        let feed = feed_rs::parser::parse(&bytes[..])?;

        let releases = feed
            .entries
            .iter()
            .map(|entry| {
                let url = entry.links.first().map(|link| link.href.clone());
                let tag = url
                    .as_deref()
                    .and_then(tag_from_release_url)
                    .map(str::to_owned)
                    .or_else(|| entry.title.as_ref().map(|t| t.content.clone()))
                    .unwrap_or_else(|| entry.id.clone());
                let published = entry.published.or(entry.updated);

                Release::new(tag).with_url(url).with_published(published)
            })
            .collect();

        Ok(FetchOutcome::fresh(releases, result.etag))
    }

    /// Fetch releases. REST when a token is set; otherwise the public Atom feed.
    /// Falls back to Atom when REST auth fails (invalid/expired token).
    pub(crate) async fn fetch(
        &self,
        http: &reqwest::Client,
        if_none_match: Option<&str>,
    ) -> Result<FetchOutcome, SourceError> {
        if self.token.is_some() {
            match self.fetch_rest(http, if_none_match).await {
                Ok(outcome) => Ok(outcome),
                Err(SourceError::Registry { status, .. }) if status == 401 || status == 403 => {
                    tracing::warn!(
                        source = %self.id,
                        status,
                        "REST auth failed; falling back to Atom feed"
                    );
                    self.fetch_atom(http, if_none_match).await
                }
                Err(err) => Err(err),
            }
        } else {
            self.fetch_atom(http, if_none_match).await
        }
    }
}

fn api_to_release(r: ReleaseApi) -> Release {
    let published = r.published_at.as_deref().and_then(parse_rfc3339);
    Release::new(r.tag_name)
        .with_prerelease(r.prerelease)
        .with_url(Some(r.html_url))
        .with_published(published)
        .with_body(r.body.filter(|b| !b.trim().is_empty()))
}

/// Extract `<TAG>` from a `.../releases/tag/<TAG>` URL.
fn tag_from_release_url(url: &str) -> Option<&str> {
    url.split("/releases/tag/")
        .nth(1)
        .map(|rest| rest.split(['/', '?', '#']).next().unwrap_or(rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_urls_should_use_api_and_web_hosts() {
        let s = GiteaReleasesSource::github("id", "owner/repo", None);
        assert!(s.rest_url().contains("api.github.com"));
        assert!(s.atom_url().contains("github.com/owner/repo/releases.atom"));
        assert_eq!(s.kind(), "github");
    }

    #[test]
    fn codeberg_urls_should_use_gitea_api_path() {
        let s = GiteaReleasesSource::codeberg("id", "owner/repo", None);
        assert!(s.rest_url().contains("codeberg.org/api/v1/repos"));
        assert!(s
            .atom_url()
            .contains("codeberg.org/owner/repo/releases.atom"));
        assert_eq!(s.kind(), "codeberg");
    }

    #[test]
    fn gitea_urls_should_use_custom_host() {
        let s = GiteaReleasesSource::gitea("id", "https://git.example.com/", "o/r", None);
        assert!(s.rest_url().contains("git.example.com/api/v1/repos"));
        assert_eq!(s.kind(), "gitea");
    }

    #[test]
    fn tag_from_release_url_should_extract_the_tag() {
        let url = "https://github.com/tokio-rs/tokio/releases/tag/tokio-1.38.0";
        assert_eq!(tag_from_release_url(url), Some("tokio-1.38.0"));
    }

    #[test]
    fn api_to_release_should_parse_github_zulu_published_at() {
        let release = api_to_release(ReleaseApi {
            tag_name: "v4.14.5".into(),
            html_url: "https://github.com/wazuh/wazuh/releases/tag/v4.14.5".into(),
            body: None,
            published_at: Some("2026-04-23T11:46:46Z".into()),
            prerelease: false,
            draft: false,
        });
        let published = release.published_at.expect("published_at");
        assert_eq!(published.to_rfc3339(), "2026-04-23T11:46:46+00:00");
    }

    #[test]
    fn atom_entry_should_use_updated_when_published_missing() {
        let atom = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>releases</title>
  <id>tag:github.com,2008:releases</id>
  <updated>2026-04-23T11:46:46Z</updated>
  <entry>
    <id>tag:github.com,2008:Repository/1/v4.14.5</id>
    <updated>2026-04-23T11:46:46Z</updated>
    <link rel="alternate" type="text/html" href="https://github.com/wazuh/wazuh/releases/tag/v4.14.5"/>
    <title>Wazuh v4.14.5</title>
  </entry>
</feed>"#;
        let feed = feed_rs::parser::parse(atom.as_bytes()).expect("parse atom");
        let entry = feed.entries.first().expect("entry");
        let published = entry.published.or(entry.updated);
        let release = Release::new("v4.14.5")
            .with_url(entry.links.first().map(|link| link.href.clone()))
            .with_published(published);
        let at = release.published_at.expect("published");
        assert_eq!(at.to_rfc3339(), "2026-04-23T11:46:46+00:00");
    }
}
