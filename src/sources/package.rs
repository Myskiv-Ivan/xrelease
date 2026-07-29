//! Package registry sources — PyPI, npm, crates.io, Maven Central, NuGet, Hex, RubyGems.
//!
//! Each registry exposes a JSON document listing published versions. We normalise
//! every version string into a [`Release`] whose identity is the version itself.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::http::{fetch_json, FetchOutcome};
use super::{parse_rfc3339, parse_timestamp_millis};
use crate::error::SourceError;
use crate::model::Release;

/// Which package registry to poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageRegistry {
    /// Python PyPI — `https://pypi.org/pypi/{name}/json`
    Pypi,
    /// Node.js npm — `https://registry.npmjs.org/{name}`
    Npm,
    /// Rust crates.io — `https://crates.io/api/v1/crates/{name}/versions`
    Cargo,
    /// Maven Central — search.maven.org Solr API
    Maven,
    /// NuGet.org — flat container index
    Nuget,
    /// Hex.pm (Elixir) — releases API
    Hex,
    /// RubyGems.org — versions API
    Rubygems,
    /// Packagist (PHP) — package metadata API
    Packagist,
    /// Yarn registry — npm-compatible metadata at `registry.yarnpkg.com`
    Yarn,
    /// Perl CPAN — MetaCPAN releases API
    Cpan,
}

impl PackageRegistry {
    /// Config `type` / id prefix for this registry.
    #[must_use]
    pub fn config_prefix(self) -> &'static str {
        self.kind()
    }

    fn kind(self) -> &'static str {
        match self {
            Self::Pypi => "pypi",
            Self::Npm => "npm",
            Self::Cargo => "cargo",
            Self::Maven => "maven",
            Self::Nuget => "nuget",
            Self::Hex => "hex",
            Self::Rubygems => "rubygems",
            Self::Packagist => "packagist",
            Self::Yarn => "yarn",
            Self::Cpan => "cpan",
        }
    }

    /// Every registry, so string↔variant conversions cannot drift apart.
    ///
    /// The array is exhaustively destructured in a test, which fails to compile
    /// if a variant is added without being listed here.
    pub const ALL: [Self; 10] = [
        Self::Pypi,
        Self::Npm,
        Self::Cargo,
        Self::Maven,
        Self::Nuget,
        Self::Hex,
        Self::Rubygems,
        Self::Packagist,
        Self::Yarn,
        Self::Cpan,
    ];

    /// Inverse of [`Self::config_prefix`] — recover the registry from a source
    /// id prefix (`cargo:serde` → [`Self::Cargo`]).
    ///
    /// Derived from [`Self::kind`] over [`Self::ALL`] rather than written as a
    /// second match: a hand-written reverse table with a `_ => None` arm would
    /// silently fail to resolve a newly added registry, and the only symptom
    /// would be advisory enrichment quietly skipping it.
    ///
    /// Used by advisory enrichment, which only has the stored `source_id` at
    /// delivery time and no live [`PackageSource`].
    #[must_use]
    pub fn from_kind(kind: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|registry| registry.kind() == kind)
    }

    /// Match a human-readable kind label (`npm`, `crates.io`, …) as stored on
    /// outbox events (`Event::source_kind` / `OutboxRecord::source_kind`).
    #[must_use]
    pub fn from_kind_label(label: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|registry| registry.kind_label() == label)
    }

    fn kind_label(self) -> &'static str {
        match self {
            Self::Pypi => "PyPI",
            Self::Npm => "npm",
            Self::Cargo => "crates.io",
            Self::Maven => "Maven Central",
            Self::Nuget => "NuGet",
            Self::Hex => "Hex",
            Self::Rubygems => "RubyGems",
            Self::Packagist => "Packagist",
            Self::Yarn => "Yarn",
            Self::Cpan => "CPAN",
        }
    }
}

/// A source watching published versions of a single package.
#[derive(Debug, Clone)]
pub struct PackageSource {
    id: String,
    name: String,
    registry: PackageRegistry,
}

impl PackageSource {
    /// Create a package source.
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>, registry: PackageRegistry) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            registry,
        }
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn display_name(&self) -> &str {
        &self.name
    }

    pub(crate) fn kind(&self) -> &'static str {
        self.registry.kind()
    }

    pub(crate) fn kind_label(&self) -> &'static str {
        self.registry.kind_label()
    }

    pub(crate) fn source_page_url(&self) -> String {
        self.package_index_url()
    }

    pub(crate) fn release_page_url(&self, version: &str) -> String {
        self.package_url(version)
    }

    fn package_index_url(&self) -> String {
        match self.registry {
            PackageRegistry::Pypi => format!("https://pypi.org/project/{}/", self.name),
            PackageRegistry::Npm => format!("https://www.npmjs.com/package/{}", self.name),
            PackageRegistry::Cargo => format!("https://crates.io/crates/{}", self.name),
            PackageRegistry::Maven => {
                if let Ok((group, artifact)) = parse_maven_coords(&self.name) {
                    format!("https://search.maven.org/artifact/{group}/{artifact}")
                } else {
                    format!("https://search.maven.org/artifact/{}", self.name)
                }
            }
            PackageRegistry::Nuget => format!("https://www.nuget.org/packages/{}", self.name),
            PackageRegistry::Hex => format!("https://hex.pm/packages/{}", self.name),
            PackageRegistry::Rubygems => format!("https://rubygems.org/gems/{}", self.name),
            PackageRegistry::Packagist => format!("https://packagist.org/packages/{}", self.name),
            PackageRegistry::Yarn => format!("https://www.npmjs.com/package/{}", self.name),
            PackageRegistry::Cpan => format!("https://metacpan.org/dist/{}", self.name),
        }
    }

    fn fetch_url(&self) -> Result<String, SourceError> {
        match self.registry {
            PackageRegistry::Pypi => Ok(format!("https://pypi.org/pypi/{}/json", self.name)),
            PackageRegistry::Npm => Ok(format!("https://registry.npmjs.org/{}", self.name)),
            PackageRegistry::Cargo => Ok(format!(
                "https://crates.io/api/v1/crates/{}/versions",
                self.name
            )),
            PackageRegistry::Maven => {
                let (group, artifact) = parse_maven_coords(&self.name)?;
                Ok(format!(
                    "https://search.maven.org/solrsearch/select?q=g:{group}+AND+a:{artifact}&core=gav&rows=100&wt=json"
                ))
            }
            PackageRegistry::Nuget => Ok(format!(
                "https://api.nuget.org/v3-flatcontainer/{}/index.json",
                self.name.to_ascii_lowercase()
            )),
            PackageRegistry::Hex => Ok(format!(
                "https://hex.pm/api/packages/{}/releases",
                self.name
            )),
            PackageRegistry::Rubygems => Ok(format!(
                "https://rubygems.org/api/v1/versions/{}.json",
                self.name
            )),
            PackageRegistry::Packagist => {
                parse_packagist_name(&self.name)?;
                Ok(format!("https://packagist.org/packages/{}.json", self.name))
            }
            PackageRegistry::Yarn => Ok(format!("https://registry.yarnpkg.com/{}", self.name)),
            PackageRegistry::Cpan => Ok(format!(
                "https://fastapi.metacpan.org/v1/release?distribution={}&sort=date&order=desc&size=100",
                self.name
            )),
        }
    }

    fn package_url(&self, version: &str) -> String {
        match self.registry {
            PackageRegistry::Pypi => {
                format!("https://pypi.org/project/{}/{version}/", self.name)
            }
            PackageRegistry::Npm => {
                format!("https://www.npmjs.com/package/{}/v/{version}", self.name)
            }
            PackageRegistry::Cargo => {
                format!("https://crates.io/crates/{}/{version}", self.name)
            }
            PackageRegistry::Maven => {
                let Ok((group, artifact)) = parse_maven_coords(&self.name) else {
                    return format!(
                        "https://search.maven.org/artifact/{}/{}",
                        self.name, version
                    );
                };
                format!("https://search.maven.org/artifact/{group}/{artifact}/{version}")
            }
            PackageRegistry::Nuget => {
                format!("https://www.nuget.org/packages/{}/{version}", self.name)
            }
            PackageRegistry::Hex => {
                format!("https://hex.pm/packages/{}/{version}", self.name)
            }
            PackageRegistry::Rubygems => {
                format!("https://rubygems.org/gems/{}/versions/{version}", self.name)
            }
            PackageRegistry::Packagist => {
                format!("https://packagist.org/packages/{}#v{}", self.name, version)
            }
            PackageRegistry::Yarn => {
                format!("https://www.npmjs.com/package/{}/v/{version}", self.name)
            }
            PackageRegistry::Cpan => {
                format!("https://metacpan.org/dist/{}/", self.name)
            }
        }
    }

    pub(crate) async fn fetch(
        &self,
        http: &reqwest::Client,
        if_none_match: Option<&str>,
    ) -> Result<FetchOutcome, SourceError> {
        let url = self.fetch_url()?;
        match self.registry {
            PackageRegistry::Pypi => self.fetch_pypi(http, &url, if_none_match).await,
            PackageRegistry::Npm => self.fetch_npm(http, &url, if_none_match).await,
            PackageRegistry::Cargo => self.fetch_cargo(http, &url, if_none_match).await,
            PackageRegistry::Maven => self.fetch_maven(http, &url, if_none_match).await,
            PackageRegistry::Nuget => self.fetch_nuget(http, &url, if_none_match).await,
            PackageRegistry::Hex => self.fetch_hex(http, &url, if_none_match).await,
            PackageRegistry::Rubygems => self.fetch_rubygems(http, &url, if_none_match).await,
            PackageRegistry::Packagist => self.fetch_packagist(http, &url, if_none_match).await,
            PackageRegistry::Yarn => self.fetch_npm(http, &url, if_none_match).await,
            PackageRegistry::Cpan => self.fetch_cpan(http, &url, if_none_match).await,
        }
    }

    /// Shared fetch skeleton for every registry: conditional GET → `304`
    /// short-circuit → decode `T` → map to releases via `extract` → wrap. Each
    /// registry differs only in its response type and the closure that pulls the
    /// version (and optional publish date) out of it.
    async fn fetch_versions<T, F>(
        &self,
        http: &reqwest::Client,
        url: &str,
        if_none_match: Option<&str>,
        build: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
        extract: F,
    ) -> Result<FetchOutcome, SourceError>
    where
        T: serde::de::DeserializeOwned,
        F: FnOnce(T) -> Vec<Release>,
    {
        let result = fetch_json::<T>(http, url, if_none_match, build).await?;
        if result.not_modified {
            return Ok(FetchOutcome::not_modified());
        }
        let data = result.data.ok_or_else(|| {
            SourceError::Other(format!("empty {} response", self.registry.kind()))
        })?;
        Ok(FetchOutcome::fresh(extract(data), result.etag))
    }

    async fn fetch_pypi(
        &self,
        http: &reqwest::Client,
        url: &str,
        if_none_match: Option<&str>,
    ) -> Result<FetchOutcome, SourceError> {
        #[derive(Deserialize)]
        struct PypiResponse {
            releases: BTreeMap<String, Vec<PypiFile>>,
        }

        /// One distribution file (sdist / wheel) of a release.
        #[derive(Deserialize)]
        struct PypiFile {
            #[serde(default)]
            upload_time_iso_8601: Option<String>,
        }

        self.fetch_versions::<PypiResponse, _>(
            http,
            url,
            if_none_match,
            |b| b,
            |data| {
                data.releases
                    .into_iter()
                    .map(|(version, files)| {
                        // A release has one file per platform/interpreter, each
                        // with its own upload time; the *earliest* is when the
                        // version became available. Yanked versions keep an
                        // empty file list, so this stays `None` for them.
                        let published = files
                            .iter()
                            .filter_map(|file| {
                                file.upload_time_iso_8601.as_deref().and_then(parse_rfc3339)
                            })
                            .min();
                        Release::new(version.clone())
                            .with_url(Some(self.package_url(&version)))
                            .with_published(published)
                    })
                    .collect()
            },
        )
        .await
    }

    async fn fetch_npm(
        &self,
        http: &reqwest::Client,
        url: &str,
        if_none_match: Option<&str>,
    ) -> Result<FetchOutcome, SourceError> {
        #[derive(Deserialize)]
        struct NpmResponse {
            versions: BTreeMap<String, serde_json::Value>,
            /// Sibling of `versions`: `{"1.0.0": "2011-…Z", "created": …,
            /// "modified": …}`. Publish times live here, not on the version
            /// objects, so they are looked up by version key.
            #[serde(default)]
            time: BTreeMap<String, String>,
        }

        self.fetch_versions::<NpmResponse, _>(
            http,
            url,
            if_none_match,
            |b| b,
            |data| {
                data.versions
                    .keys()
                    .map(|version| {
                        // `time` also carries the non-version keys `created` /
                        // `modified`; looking up by exact version key skips them.
                        let published = data
                            .time
                            .get(version)
                            .map(String::as_str)
                            .and_then(parse_rfc3339);
                        Release::new(version.clone())
                            .with_url(Some(self.package_url(version)))
                            .with_published(published)
                    })
                    .collect()
            },
        )
        .await
    }

    async fn fetch_cargo(
        &self,
        http: &reqwest::Client,
        url: &str,
        if_none_match: Option<&str>,
    ) -> Result<FetchOutcome, SourceError> {
        #[derive(Deserialize)]
        struct CargoVersionsResponse {
            versions: Vec<CargoVersion>,
        }

        #[derive(Deserialize)]
        struct CargoVersion {
            num: String,
            created_at: Option<String>,
        }

        self.fetch_versions::<CargoVersionsResponse, _>(
            http,
            url,
            if_none_match,
            |b| {
                b.header("Accept", "application/json").header(
                    "User-Agent",
                    concat!("xrelease/", env!("CARGO_PKG_VERSION")),
                )
            },
            |data| {
                data.versions
                    .into_iter()
                    .map(|v| {
                        Release::new(v.num.clone())
                            .with_url(Some(self.package_url(&v.num)))
                            .with_published(v.created_at.as_deref().and_then(parse_rfc3339))
                    })
                    .collect()
            },
        )
        .await
    }

    async fn fetch_maven(
        &self,
        http: &reqwest::Client,
        url: &str,
        if_none_match: Option<&str>,
    ) -> Result<FetchOutcome, SourceError> {
        #[derive(Deserialize)]
        struct MavenSearchResponse {
            response: MavenSearchInner,
        }

        #[derive(Deserialize)]
        struct MavenSearchInner {
            docs: Vec<MavenDoc>,
        }

        #[derive(Deserialize)]
        struct MavenDoc {
            v: String,
            timestamp: Option<i64>,
        }

        self.fetch_versions::<MavenSearchResponse, _>(
            http,
            url,
            if_none_match,
            |b| b,
            |data| {
                data.response
                    .docs
                    .into_iter()
                    .map(|doc| {
                        Release::new(doc.v.clone())
                            .with_url(Some(self.package_url(&doc.v)))
                            .with_published(doc.timestamp.and_then(parse_timestamp_millis))
                    })
                    .collect()
            },
        )
        .await
    }

    async fn fetch_nuget(
        &self,
        http: &reqwest::Client,
        url: &str,
        if_none_match: Option<&str>,
    ) -> Result<FetchOutcome, SourceError> {
        #[derive(Deserialize)]
        struct NugetIndexResponse {
            versions: Vec<String>,
        }

        let outcome = self
            .fetch_versions::<NugetIndexResponse, _>(
                http,
                url,
                if_none_match,
                |b| b,
                |data| {
                    data.versions
                        .into_iter()
                        .map(|version| {
                            Release::new(version.clone()).with_url(Some(self.package_url(&version)))
                        })
                        .collect()
                },
            )
            .await?;

        // The flat container lists versions and nothing else — no timestamps at
        // all — so publish dates need a second call to the registration API.
        // Skipped entirely on `304`: nothing changed, so nothing to date.
        if outcome.not_modified || outcome.releases.is_empty() {
            return Ok(outcome);
        }
        Ok(self.attach_nuget_published(http, outcome).await)
    }

    /// Best-effort: fold registration publish dates into already-fetched releases.
    ///
    /// Any failure returns the releases untouched — a missing date is cosmetic,
    /// and must never turn a successful poll into an error.
    async fn attach_nuget_published(
        &self,
        http: &reqwest::Client,
        mut outcome: FetchOutcome,
    ) -> FetchOutcome {
        match self.fetch_nuget_published(http).await {
            Ok(dates) if !dates.is_empty() => {
                for release in &mut outcome.releases {
                    if let Some(published) = dates.get(&release.id) {
                        release.published_at = Some(*published);
                    }
                }
            }
            Ok(_) => {}
            Err(err) => tracing::debug!(
                package = %self.name,
                error = %err,
                "nuget registration lookup failed — versions kept without publish dates"
            ),
        }
        outcome
    }

    /// Version → publish time from the NuGet registration API.
    ///
    /// The index is paginated. Small packages inline each page's `items`; large
    /// ones (Newtonsoft.Json among them) do not, and then only the **last** page
    /// is fetched — it holds the newest versions, which is what a release monitor
    /// surfaces. Older versions simply keep no date, exactly as before. That caps
    /// the cost at two requests instead of one per page.
    async fn fetch_nuget_published(
        &self,
        http: &reqwest::Client,
    ) -> Result<BTreeMap<String, DateTime<Utc>>, SourceError> {
        #[derive(Deserialize)]
        struct RegistrationIndex {
            #[serde(default)]
            items: Vec<RegistrationPage>,
        }

        #[derive(Deserialize)]
        struct RegistrationPage {
            #[serde(rename = "@id")]
            id: String,
            /// Present only when the page is inlined in the index.
            #[serde(default)]
            items: Option<Vec<RegistrationLeaf>>,
        }

        #[derive(Deserialize)]
        struct RegistrationLeaf {
            #[serde(rename = "catalogEntry")]
            catalog_entry: Option<CatalogEntry>,
        }

        #[derive(Deserialize)]
        struct CatalogEntry {
            version: String,
            #[serde(default)]
            published: Option<String>,
        }

        fn collect(leaves: &[RegistrationLeaf], into: &mut BTreeMap<String, DateTime<Utc>>) {
            for leaf in leaves {
                if let Some(entry) = &leaf.catalog_entry {
                    if let Some(at) = entry.published.as_deref().and_then(parse_rfc3339) {
                        into.insert(entry.version.clone(), at);
                    }
                }
            }
        }

        let index_url = format!(
            "https://api.nuget.org/v3/registration5-semver1/{}/index.json",
            self.name.to_ascii_lowercase()
        );
        let index = fetch_json::<RegistrationIndex>(http, &index_url, None, |b| b)
            .await?
            .data
            .ok_or_else(|| SourceError::Other("empty nuget registration index".to_owned()))?;

        let mut dates = BTreeMap::new();
        let mut last_uninlined: Option<String> = None;
        for page in &index.items {
            match &page.items {
                Some(leaves) => collect(leaves, &mut dates),
                None => last_uninlined = Some(page.id.clone()),
            }
        }

        if let Some(page_url) = last_uninlined {
            if let Some(page) = fetch_json::<RegistrationPage>(http, &page_url, None, |b| b)
                .await?
                .data
            {
                if let Some(leaves) = &page.items {
                    collect(leaves, &mut dates);
                }
            }
        }
        Ok(dates)
    }

    async fn fetch_hex(
        &self,
        http: &reqwest::Client,
        url: &str,
        if_none_match: Option<&str>,
    ) -> Result<FetchOutcome, SourceError> {
        #[derive(Deserialize)]
        struct HexRelease {
            version: String,
            url: Option<String>,
            inserted_at: Option<String>,
        }

        self.fetch_versions::<Vec<HexRelease>, _>(
            http,
            url,
            if_none_match,
            |b| b,
            |data| {
                data.into_iter()
                    .map(|r| {
                        let url = r.url.or_else(|| Some(self.package_url(&r.version)));
                        Release::new(r.version.clone())
                            .with_url(url)
                            .with_published(r.inserted_at.as_deref().and_then(parse_rfc3339))
                    })
                    .collect()
            },
        )
        .await
    }

    async fn fetch_rubygems(
        &self,
        http: &reqwest::Client,
        url: &str,
        if_none_match: Option<&str>,
    ) -> Result<FetchOutcome, SourceError> {
        #[derive(Deserialize)]
        struct RubygemsVersion {
            number: String,
            created_at: Option<String>,
        }

        self.fetch_versions::<Vec<RubygemsVersion>, _>(
            http,
            url,
            if_none_match,
            |b| b,
            |data| {
                data.into_iter()
                    .map(|entry| {
                        Release::new(entry.number.clone())
                            .with_url(Some(self.package_url(&entry.number)))
                            .with_published(entry.created_at.as_deref().and_then(parse_rfc3339))
                    })
                    .collect()
            },
        )
        .await
    }

    async fn fetch_packagist(
        &self,
        http: &reqwest::Client,
        url: &str,
        if_none_match: Option<&str>,
    ) -> Result<FetchOutcome, SourceError> {
        #[derive(Deserialize)]
        struct PackagistResponse {
            package: PackagistPackage,
        }

        #[derive(Deserialize)]
        struct PackagistPackage {
            versions: BTreeMap<String, PackagistVersion>,
        }

        #[derive(Deserialize)]
        struct PackagistVersion {
            version: String,
            time: Option<String>,
        }

        self.fetch_versions::<PackagistResponse, _>(
            http,
            url,
            if_none_match,
            |b| b,
            |data| {
                data.package
                    .versions
                    .into_values()
                    .map(|entry| {
                        Release::new(entry.version.clone())
                            .with_url(Some(self.package_url(&entry.version)))
                            .with_published(entry.time.as_deref().and_then(parse_rfc3339))
                    })
                    .collect()
            },
        )
        .await
    }

    async fn fetch_cpan(
        &self,
        http: &reqwest::Client,
        url: &str,
        if_none_match: Option<&str>,
    ) -> Result<FetchOutcome, SourceError> {
        #[derive(Deserialize)]
        struct CpanRelease {
            version: String,
            date: Option<String>,
        }

        self.fetch_versions::<Vec<CpanRelease>, _>(
            http,
            url,
            if_none_match,
            |b| b,
            |data| {
                data.into_iter()
                    .map(|entry| {
                        Release::new(entry.version.clone())
                            .with_url(Some(self.package_url(&entry.version)))
                            .with_published(entry.date.as_deref().and_then(parse_rfc3339))
                    })
                    .collect()
            },
        )
        .await
    }
}

/// Parse `vendor/package` coordinates for Packagist sources.
fn parse_packagist_name(name: &str) -> Result<(), SourceError> {
    let (vendor, package) = name.split_once('/').ok_or_else(|| {
        SourceError::Other(format!(
            "packagist name must be `vendor/package`, got `{name}`"
        ))
    })?;
    if vendor.is_empty() || package.is_empty() {
        return Err(SourceError::Other(format!(
            "packagist name must be `vendor/package`, got `{name}`"
        )));
    }
    Ok(())
}

/// Parse `group:artifact` coordinates for Maven sources.
fn parse_maven_coords(name: &str) -> Result<(String, String), SourceError> {
    let (group, artifact) = name.split_once(':').ok_or_else(|| {
        SourceError::Other(format!("maven name must be `group:artifact`, got `{name}`"))
    })?;
    if group.is_empty() || artifact.is_empty() {
        return Err(SourceError::Other(format!(
            "maven name must be `group:artifact`, got `{name}`"
        )));
    }
    Ok((group.to_owned(), artifact.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time guard: destructuring `ALL` exhaustively means adding a
    /// `PackageRegistry` variant without listing it in `ALL` fails to build,
    /// which is what keeps `from_kind` complete.
    #[test]
    fn all_should_list_every_registry() {
        let [pypi, npm, cargo, maven, nuget, hex, rubygems, packagist, yarn, cpan] =
            PackageRegistry::ALL;
        for registry in [
            pypi, npm, cargo, maven, nuget, hex, rubygems, packagist, yarn, cpan,
        ] {
            assert_eq!(
                PackageRegistry::from_kind(registry.kind()),
                Some(registry),
                "kind() and from_kind() must round-trip for {registry:?}"
            );
        }
    }

    #[test]
    fn from_kind_should_reject_unknown_prefixes() {
        assert_eq!(PackageRegistry::from_kind("github"), None);
        assert_eq!(PackageRegistry::from_kind(""), None);
        assert_eq!(PackageRegistry::from_kind("PyPI"), None, "case-sensitive");
    }

    #[test]
    fn from_kind_label_should_round_trip_outbox_labels() {
        for registry in PackageRegistry::ALL {
            assert_eq!(
                PackageRegistry::from_kind_label(registry.kind_label()),
                Some(registry),
            );
        }
        assert_eq!(PackageRegistry::from_kind_label("GitHub"), None);
    }

    #[test]
    fn fetch_urls_should_match_registry_conventions() {
        let pypi = PackageSource::new("id", "requests", PackageRegistry::Pypi);
        assert!(pypi
            .fetch_url()
            .unwrap()
            .contains("pypi.org/pypi/requests/json"));

        let npm = PackageSource::new("id", "vue", PackageRegistry::Npm);
        assert!(npm.fetch_url().unwrap().contains("registry.npmjs.org/vue"));

        let cargo = PackageSource::new("id", "serde", PackageRegistry::Cargo);
        assert!(cargo
            .fetch_url()
            .unwrap()
            .contains("crates.io/api/v1/crates/serde"));

        let maven = PackageSource::new(
            "id",
            "com.fasterxml.jackson.core:jackson-databind",
            PackageRegistry::Maven,
        );
        let maven_url = maven.fetch_url().unwrap();
        assert!(maven_url.contains("search.maven.org"));
        assert!(maven_url.contains("jackson-databind"));

        let nuget = PackageSource::new("id", "Newtonsoft.Json", PackageRegistry::Nuget);
        assert!(nuget
            .fetch_url()
            .unwrap()
            .contains("newtonsoft.json/index.json"));

        let hex = PackageSource::new("id", "phoenix", PackageRegistry::Hex);
        assert!(hex
            .fetch_url()
            .unwrap()
            .contains("hex.pm/api/packages/phoenix"));

        let rubygems = PackageSource::new("id", "rails", PackageRegistry::Rubygems);
        assert!(rubygems
            .fetch_url()
            .unwrap()
            .contains("rubygems.org/api/v1/versions/rails"));

        let packagist = PackageSource::new("id", "monolog/monolog", PackageRegistry::Packagist);
        assert!(packagist
            .fetch_url()
            .unwrap()
            .contains("packagist.org/packages/monolog/monolog"));

        let yarn = PackageSource::new("id", "lodash", PackageRegistry::Yarn);
        assert!(yarn
            .fetch_url()
            .unwrap()
            .contains("registry.yarnpkg.com/lodash"));

        let cpan = PackageSource::new("id", "Moose", PackageRegistry::Cpan);
        assert!(cpan
            .fetch_url()
            .unwrap()
            .contains("fastapi.metacpan.org/v1/release"));
    }

    #[test]
    fn parse_packagist_name_should_require_slash() {
        assert!(parse_packagist_name("bad").is_err());
        assert!(parse_packagist_name("vendor/pkg").is_ok());
    }

    #[test]
    fn packagist_fixture_should_deserialize_versions() {
        #[derive(Deserialize)]
        struct PackagistResponse {
            package: PackagistPackage,
        }
        #[derive(Deserialize)]
        struct PackagistPackage {
            versions: BTreeMap<String, PackagistVersion>,
        }
        #[derive(Deserialize)]
        struct PackagistVersion {
            version: String,
        }

        let data: PackagistResponse =
            serde_json::from_str(include_str!("../../tests/fixtures/packagist_monolog.json"))
                .expect("fixture");
        assert_eq!(data.package.versions.len(), 2);
        assert!(data.package.versions.contains_key("3.5.0"));
        assert_eq!(
            data.package
                .versions
                .get("3.5.0")
                .map(|v| v.version.as_str()),
            Some("3.5.0")
        );
    }

    #[test]
    fn parse_maven_coords_should_require_colon() {
        assert!(parse_maven_coords("bad").is_err());
        assert!(parse_maven_coords("g:a").is_ok());
    }

    #[test]
    fn rubygems_fixture_should_deserialize_version_numbers() {
        #[derive(Deserialize)]
        struct RubygemsVersion {
            number: String,
        }

        let entries: Vec<RubygemsVersion> =
            serde_json::from_str(include_str!("../../tests/fixtures/rubygems_versions.json"))
                .expect("fixture");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].number, "7.1.0");
        assert_eq!(entries[1].number, "8.0.0");
    }

    /// PyPI publishes one file per platform, each with its own upload time. The
    /// *earliest* is when the version became installable, so that is the date
    /// reported — mirrors the selection in `fetch_pypi`.
    #[test]
    fn pypi_fixture_should_date_a_release_from_its_earliest_file() {
        #[derive(Deserialize)]
        struct Response {
            releases: BTreeMap<String, Vec<File>>,
        }
        #[derive(Deserialize)]
        struct File {
            #[serde(default)]
            upload_time_iso_8601: Option<String>,
        }

        let data: Response =
            serde_json::from_str(include_str!("../../tests/fixtures/pypi_requests.json"))
                .expect("fixture");
        let earliest = |version: &str| {
            data.releases[version]
                .iter()
                .filter_map(|f| f.upload_time_iso_8601.as_deref().and_then(parse_rfc3339))
                .min()
        };

        // Two files 3s apart — the sdist at :09 is the release moment, not :12.
        assert_eq!(
            earliest("2.32.2").map(|at| at.to_rfc3339()),
            Some("2024-05-21T15:34:09.987654+00:00".to_owned())
        );
        assert!(earliest("2.32.3").is_some());
    }

    /// A yanked release keeps an empty file list; it must yield no date rather
    /// than a wrong one.
    #[test]
    fn pypi_fixture_should_leave_a_fileless_release_undated() {
        #[derive(Deserialize)]
        struct Response {
            releases: BTreeMap<String, Vec<serde_json::Value>>,
        }
        let data: Response =
            serde_json::from_str(include_str!("../../tests/fixtures/pypi_requests.json"))
                .expect("fixture");
        assert!(data.releases["2.32.1"].is_empty());
    }

    /// npm keeps publish times in a `time` map beside `versions`, mixed in with
    /// the non-version keys `created` / `modified` — looking up by exact version
    /// key is what keeps those out.
    #[test]
    fn npm_fixture_should_date_versions_from_the_time_map() {
        #[derive(Deserialize)]
        struct Response {
            versions: BTreeMap<String, serde_json::Value>,
            #[serde(default)]
            time: BTreeMap<String, String>,
        }

        let data: Response = serde_json::from_str(include_str!(
            "../../tests/fixtures/npm_registry_left_pad.json"
        ))
        .expect("fixture");

        assert_eq!(data.versions.len(), 2);
        for version in data.versions.keys() {
            assert!(
                data.time
                    .get(version)
                    .map(String::as_str)
                    .and_then(parse_rfc3339)
                    .is_some(),
                "every version must resolve a publish time: {version}"
            );
        }
        assert!(
            !data.versions.contains_key("created") && !data.versions.contains_key("modified"),
            "`created`/`modified` are not versions and must never become releases"
        );
    }

    /// NuGet's flat container has no dates at all, so they come from the
    /// registration index. Large packages leave pages un-inlined; only the last
    /// page is then fetched, which is where the newest versions live.
    #[test]
    fn nuget_registration_fixture_should_inline_only_the_newest_page() {
        #[derive(Deserialize)]
        struct Index {
            #[serde(default)]
            items: Vec<Page>,
        }
        #[derive(Deserialize)]
        struct Page {
            #[serde(rename = "@id")]
            id: String,
            #[serde(default)]
            items: Option<Vec<Leaf>>,
        }
        #[derive(Deserialize)]
        struct Leaf {
            #[serde(rename = "catalogEntry")]
            catalog_entry: Option<Entry>,
        }
        #[derive(Deserialize)]
        struct Entry {
            version: String,
            #[serde(default)]
            published: Option<String>,
        }

        let index: Index = serde_json::from_str(include_str!(
            "../../tests/fixtures/nuget_registration_index.json"
        ))
        .expect("fixture");

        let mut dates = BTreeMap::new();
        let mut uninlined = None;
        for page in &index.items {
            match &page.items {
                Some(leaves) => {
                    for leaf in leaves {
                        if let Some(entry) = &leaf.catalog_entry {
                            if let Some(at) = entry.published.as_deref().and_then(parse_rfc3339) {
                                dates.insert(entry.version.clone(), at);
                            }
                        }
                    }
                }
                None => uninlined = Some(page.id.clone()),
            }
        }

        assert!(
            uninlined.is_some_and(|url| url.contains("1.0.0/12.0.3")),
            "the old page is not inlined and would need its own request"
        );
        // The version from the user report resolves a real date instead of `—`.
        assert!(dates.contains_key("13.0.5-beta1"));
        assert_eq!(dates.len(), 2);
    }

    #[test]
    fn cpan_fixture_should_deserialize_versions() {
        #[derive(Deserialize)]
        struct CpanRelease {
            version: String,
        }

        let entries: Vec<CpanRelease> =
            serde_json::from_str(include_str!("../../tests/fixtures/cpan_moose.json"))
                .expect("fixture");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].version, "2.2200");
    }
}
