//! Artifact Hub catalog source — Helm charts and other package kinds.
//!
//! Polls package metadata from Artifact Hub (or a self-hosted instance):
//! `GET /api/v1/packages/{kind}/{repository}/{package}`
//!
//! Version list comes from `availableVersions` in the JSON response.

use chrono::DateTime;
use serde::Deserialize;

use super::http::{fetch_json, FetchOutcome};
use crate::error::SourceError;
use crate::model::Release;

/// Default Artifact Hub SaaS host.
const DEFAULT_HOST: &str = "https://artifacthub.io";
/// Default package kind (Helm chart).
const DEFAULT_KIND: &str = "helm";

/// A source watching published versions of a catalog package (e.g. Helm chart).
#[derive(Debug, Clone)]
pub struct ArtifactHubSource {
    id: String,
    /// `repository/chart` display path.
    name: String,
    repository: String,
    chart: String,
    package_kind: String,
    host: String,
}

#[derive(Debug, Deserialize)]
struct PackageResponse {
    #[serde(default, rename = "availableVersions")]
    available_versions: Option<Vec<AvailableVersion>>,
}

#[derive(Debug, Deserialize)]
struct AvailableVersion {
    version: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    ts: Option<i64>,
}

impl ArtifactHubSource {
    /// Artifact Hub preset (`repository/chart`, default SaaS host, Helm kind).
    pub fn helm(id: impl Into<String>, name: impl Into<String>) -> Result<Self, SourceError> {
        Self::with_options(id, name, DEFAULT_HOST, DEFAULT_KIND)
    }

    /// Custom host and package kind (e.g. self-hosted Artifact Hub, `helm`).
    pub fn with_options(
        id: impl Into<String>,
        name: impl Into<String>,
        host: impl Into<String>,
        package_kind: impl Into<String>,
    ) -> Result<Self, SourceError> {
        let name = name.into();
        let (repository, chart) = split_repo_chart(&name)?;
        Ok(Self {
            id: id.into(),
            name,
            repository,
            chart,
            package_kind: package_kind.into(),
            host: host.into().trim_end_matches('/').to_owned(),
        })
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn kind_label(&self) -> &'static str {
        "Artifact Hub"
    }

    pub(crate) fn display_name(&self) -> &str {
        &self.name
    }

    pub(crate) fn source_page_url(&self) -> String {
        format!(
            "{}/packages/{}/{}/{}",
            self.host, self.package_kind, self.repository, self.chart
        )
    }

    pub(crate) fn release_page_url(&self, version: &str) -> String {
        self.version_url(version)
    }

    fn fetch_url(&self) -> String {
        format!(
            "{}/api/v1/packages/{}/{}/{}",
            self.host, self.package_kind, self.repository, self.chart
        )
    }

    fn version_url(&self, version: &str) -> String {
        format!(
            "{}/packages/{}/{}/{}/{}",
            self.host, self.package_kind, self.repository, self.chart, version
        )
    }

    pub(crate) async fn fetch(
        &self,
        http: &reqwest::Client,
        if_none_match: Option<&str>,
    ) -> Result<FetchOutcome, SourceError> {
        let url = self.fetch_url();
        let result = fetch_json::<PackageResponse>(http, &url, if_none_match, |b| b).await?;

        if result.not_modified {
            return Ok(FetchOutcome::not_modified());
        }

        let data = result
            .data
            .ok_or_else(|| SourceError::Other("empty artifact hub response".into()))?;

        let releases = data
            .available_versions
            .unwrap_or_default()
            .into_iter()
            .map(|entry| self.version_to_release(entry))
            .collect();

        Ok(FetchOutcome::fresh(releases, result.etag))
    }

    fn version_to_release(&self, entry: AvailableVersion) -> Release {
        let published = entry.ts.and_then(|secs| DateTime::from_timestamp(secs, 0));

        Release::new(entry.version.clone())
            .with_prerelease(entry.prerelease)
            .with_url(Some(self.version_url(&entry.version)))
            .with_published(published)
    }
}

fn split_repo_chart(name: &str) -> Result<(String, String), SourceError> {
    let (repository, chart) = name.split_once('/').ok_or_else(|| {
        SourceError::Other(format!(
            "artifacthub name must be `repository/chart`, got `{name}`"
        ))
    })?;
    if repository.is_empty() || chart.is_empty() {
        return Err(SourceError::Other(format!(
            "artifacthub name must be `repository/chart`, got `{name}`"
        )));
    }
    Ok((repository.to_owned(), chart.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helm_preset_should_build_api_url() {
        let s = ArtifactHubSource::helm("id", "bitnami/nginx").expect("source");
        assert!(s.fetch_url().contains("/packages/helm/bitnami/nginx"));
    }

    #[test]
    fn split_repo_chart_should_require_slash() {
        assert!(split_repo_chart("bad").is_err());
        assert!(split_repo_chart("a/b").is_ok());
    }
}
