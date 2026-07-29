//! Webhook payload parsers for Git forges and registries.

use crate::model::Release;
// The shared parser also rejects placeholder dates (1900-01-01 & co.), which a
// local `DateTime::parse_from_rfc3339` wrapper silently accepted.
use crate::sources::parse_rfc3339;

/// GitHub / Gitea / Codeberg release webhook body.
#[derive(Debug, serde::Deserialize)]
pub struct GiteaCompatibleWebhook {
    pub action: Option<String>,
    pub release: Option<GiteaCompatibleRelease>,
    pub repository: Option<GiteaCompatibleRepository>,
}

impl GiteaCompatibleWebhook {
    /// Whether this is a published release event we should process.
    #[must_use]
    pub fn is_published_release(&self) -> bool {
        self.action.as_deref() == Some("published")
    }

    /// Extract repo path and release, rejecting drafts.
    pub fn into_release(self) -> Result<(String, Release), WebhookPayloadError> {
        if !self.is_published_release() {
            return Err(WebhookPayloadError::Ignored(format!(
                "ignored action {:?}",
                self.action
            )));
        }

        let release = self
            .release
            .ok_or(WebhookPayloadError::BadRequest("missing release".into()))?;
        if release.draft.unwrap_or(false) {
            return Err(WebhookPayloadError::Ignored("draft release".into()));
        }

        let repo =
            self.repository
                .and_then(|r| r.full_name)
                .ok_or(WebhookPayloadError::BadRequest(
                    "missing repository.full_name".into(),
                ))?;

        Ok((repo, release.into_release()))
    }
}

/// Errors while parsing or validating a webhook payload.
#[derive(Debug)]
pub enum WebhookPayloadError {
    /// Malformed request.
    BadRequest(String),
    /// Valid payload we intentionally skip.
    Ignored(String),
}

#[derive(Debug, serde::Deserialize)]
pub struct GiteaCompatibleRelease {
    tag_name: String,
    html_url: Option<String>,
    body: Option<String>,
    prerelease: Option<bool>,
    draft: Option<bool>,
    published_at: Option<String>,
}

impl GiteaCompatibleRelease {
    fn into_release(self) -> Release {
        let published = self.published_at.as_deref().and_then(parse_rfc3339);
        Release::new(self.tag_name)
            .with_prerelease(self.prerelease.unwrap_or(false))
            .with_url(self.html_url)
            .with_published(published)
            .with_body(self.body.filter(|b| !b.trim().is_empty()))
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct GiteaCompatibleRepository {
    pub full_name: Option<String>,
}

/// GitLab release hook payload.
#[derive(Debug, serde::Deserialize)]
pub struct GitlabWebhook {
    pub object_kind: Option<String>,
    pub tag: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    pub released_at: Option<String>,
    pub project: Option<GitlabProject>,
}

impl GitlabWebhook {
    /// Parse a GitLab release webhook into repo path + release.
    pub fn into_release(self) -> Result<(String, Release), WebhookPayloadError> {
        if self.object_kind.as_deref() != Some("release") {
            return Err(WebhookPayloadError::Ignored(format!(
                "ignored object_kind {:?}",
                self.object_kind
            )));
        }

        let project = self
            .project
            .ok_or(WebhookPayloadError::BadRequest("missing project".into()))?;
        let tag = self
            .tag
            .or(self.name)
            .ok_or(WebhookPayloadError::BadRequest("missing tag".into()))?;

        let repo = project.path_with_namespace.unwrap_or(project.name);

        let mut release = Release::new(tag);
        if let Some(url) = self.url {
            release = release.with_url(Some(url));
        }
        if let Some(desc) = self.description.filter(|d| !d.trim().is_empty()) {
            release = release.with_body(Some(desc));
        }
        if let Some(at) = self.released_at.as_deref().and_then(parse_rfc3339) {
            release = release.with_published(Some(at));
        }

        Ok((repo, release))
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct GitlabProject {
    pub path_with_namespace: Option<String>,
    pub name: String,
}

/// Bitbucket Cloud / Server `repo:push` webhook (tag pushes).
#[derive(Debug, serde::Deserialize)]
pub struct BitbucketPushWebhook {
    #[serde(default)]
    push: Option<BitbucketPush>,
    #[serde(default)]
    repository: Option<BitbucketRepository>,
}

#[derive(Debug, serde::Deserialize)]
struct BitbucketPush {
    #[serde(default)]
    changes: Vec<BitbucketChange>,
}

#[derive(Debug, serde::Deserialize)]
struct BitbucketChange {
    #[serde(default)]
    new: Option<BitbucketRef>,
}

#[derive(Debug, serde::Deserialize)]
struct BitbucketRef {
    #[serde(rename = "type")]
    ref_type: Option<String>,
    name: Option<String>,
    #[serde(default)]
    target: Option<BitbucketTarget>,
}

#[derive(Debug, serde::Deserialize)]
struct BitbucketTarget {
    #[serde(default)]
    links: Option<BitbucketLinks>,
}

#[derive(Debug, serde::Deserialize)]
struct BitbucketLinks {
    html: Option<BitbucketLink>,
}

#[derive(Debug, serde::Deserialize)]
struct BitbucketLink {
    href: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct BitbucketRepository {
    full_name: Option<String>,
}

impl BitbucketPushWebhook {
    /// Parse a tag push from `repo:push` into workspace/repo + tag release.
    pub fn into_release(self) -> Result<(String, Release), WebhookPayloadError> {
        let repo =
            self.repository
                .and_then(|r| r.full_name)
                .ok_or(WebhookPayloadError::BadRequest(
                    "missing repository.full_name".into(),
                ))?;

        let changes = self.push.map(|push| push.changes).unwrap_or_default();

        for change in changes {
            let Some(new_ref) = change.new else {
                continue;
            };
            if new_ref.ref_type.as_deref() != Some("tag") {
                continue;
            }
            let tag = new_ref.name.filter(|name| !name.trim().is_empty()).ok_or(
                WebhookPayloadError::BadRequest("missing tag name in push change".into()),
            )?;
            let url = new_ref
                .target
                .and_then(|target| target.links)
                .and_then(|links| links.html)
                .and_then(|link| link.href);
            return Ok((repo, Release::new(tag).with_url(url)));
        }

        Err(WebhookPayloadError::Ignored(
            "no tag push in repo:push changes".into(),
        ))
    }
}

/// Docker Hub automated build / registry push webhook.
#[derive(Debug, serde::Deserialize)]
pub struct DockerPushWebhook {
    #[serde(default)]
    push_data: Option<DockerPushData>,
    #[serde(default)]
    repository: Option<DockerRepository>,
}

#[derive(Debug, serde::Deserialize)]
struct DockerPushData {
    tag: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct DockerRepository {
    repo_name: Option<String>,
    name: Option<String>,
}

impl DockerPushWebhook {
    /// Parse a Docker Hub push event into image path + tag release.
    pub fn into_release(self) -> Result<(String, Release), WebhookPayloadError> {
        let push = self
            .push_data
            .ok_or(WebhookPayloadError::BadRequest("missing push_data".into()))?;
        let tag =
            push.tag
                .filter(|t| !t.trim().is_empty())
                .ok_or(WebhookPayloadError::BadRequest(
                    "missing push_data.tag".into(),
                ))?;

        let repo = self.repository.and_then(|r| r.repo_name.or(r.name)).ok_or(
            WebhookPayloadError::BadRequest("missing repository.repo_name".into()),
        )?;

        let url = Some(format!("https://hub.docker.com/r/{repo}/tags?name={tag}"));
        Ok((repo, Release::new(tag).with_url(url)))
    }
}

#[cfg(test)]
mod docker_tests {
    use super::*;

    #[test]
    fn docker_hub_push_should_parse_tag() {
        let payload: DockerPushWebhook = serde_json::from_str(
            r#"{
                "push_data": { "tag": "1.2.3" },
                "repository": { "repo_name": "library/nginx" }
            }"#,
        )
        .expect("parse");
        let (image, release) = payload.into_release().expect("release");
        assert_eq!(image, "library/nginx");
        assert_eq!(release.raw_tag, "1.2.3");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_fixture_should_parse_published_release() {
        let json = include_str!("../../tests/fixtures/github_release_published.json");
        let payload: GiteaCompatibleWebhook = serde_json::from_str(json).expect("parse");
        let (repo, release) = payload.into_release().expect("release");
        assert_eq!(repo, "tokio-rs/tokio");
        assert_eq!(release.raw_tag, "v1.2.3");
        assert!(!release.prerelease);
        assert!(release.body.is_some());
    }

    #[test]
    fn github_fixture_should_ignore_non_published_action() {
        let payload: GiteaCompatibleWebhook = serde_json::from_str(
            r#"{"action":"created","release":{"tag_name":"v1.0.0","draft":false},"repository":{"full_name":"a/b"}}"#,
        )
        .expect("parse");
        assert!(matches!(
            payload.into_release(),
            Err(WebhookPayloadError::Ignored(_))
        ));
    }

    #[test]
    fn bitbucket_push_tag_should_parse_release() {
        let json = include_str!("../../tests/fixtures/bitbucket_push_tag.json");
        let payload: BitbucketPushWebhook = serde_json::from_str(json).expect("parse");
        let (repo, release) = payload.into_release().expect("release");
        assert_eq!(repo, "atlassian/python-bitbucket");
        assert_eq!(release.raw_tag, "1.2.3");
    }

    #[test]
    fn bitbucket_branch_push_should_be_ignored() {
        let payload: BitbucketPushWebhook = serde_json::from_str(
            r#"{
                "push": {
                    "changes": [{
                        "new": { "type": "branch", "name": "main" }
                    }]
                },
                "repository": { "full_name": "ws/repo" }
            }"#,
        )
        .expect("parse");
        assert!(matches!(
            payload.into_release(),
            Err(WebhookPayloadError::Ignored(_))
        ));
    }

    #[test]
    fn gitlab_fixture_should_parse_release() {
        let json = include_str!("../../tests/fixtures/gitlab_release.json");
        let payload: GitlabWebhook = serde_json::from_str(json).expect("parse");
        let (repo, release) = payload.into_release().expect("release");
        assert_eq!(repo, "gitlab-org/gitlab-runner");
        assert_eq!(release.raw_tag, "v15.0.0");
    }
}
