//! Release-source configuration (`[[sources]]`).
//!
//! [`SourceConfig`] is a serde-tagged enum mapping one-to-one onto
//! [`crate::sources::Provider`] variants, so adding a `[[sources]]` block
//! needs no glue code beyond the provider itself.

use serde::{Deserialize, Serialize};

use crate::pipeline::{NotifySchedule, Watch};
use crate::sources::{
    ArtifactHubSource, BitbucketEdition, BitbucketSource, ContainerRegistry, FeedSource,
    GiteaReleasesSource, GitlabSource, PackageRegistry, Provider,
};

use super::{Config, Defaults};

mod preset;

pub use preset::{
    builtin_preset_schemas, builtin_source_presets, effective_source_presets, BuiltinPresetInfo,
    BuiltinPresetSchema, SourceCommon, SourcePreset, BUILTIN_PRESET_INFO,
};

/// Default Docker registry when a source omits one.
const DEFAULT_REGISTRY: &str = "https://registry-1.docker.io";

/// Map a configured `registry` URL onto a [`ContainerRegistry`] preset.
///
/// Blank / Hub aliases → Docker Hub (so an empty UI `registry` field and common
/// mistakes like `docker.io` without a scheme do not become a relative URL that
/// reqwest rejects as `builder error`). Any other value gets an `https://`
/// scheme when missing.
fn resolve_docker_registry(registry: Option<&str>) -> ContainerRegistry {
    let Some(raw) = registry.map(str::trim).filter(|url| !url.is_empty()) else {
        return ContainerRegistry::DockerHub;
    };
    let with_scheme = if raw.contains("://") {
        raw.to_owned()
    } else {
        format!("https://{raw}")
    };
    let base = with_scheme.trim_end_matches('/').to_owned();
    if is_docker_hub_registry(&base) {
        ContainerRegistry::DockerHub
    } else {
        ContainerRegistry::Custom(base)
    }
}

fn is_docker_hub_registry(url: &str) -> bool {
    if url == DEFAULT_REGISTRY {
        return true;
    }
    let Some(host) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .and_then(|rest| rest.split('/').next())
        .map(|host| host.split(':').next().unwrap_or(host))
    else {
        return false;
    };
    matches!(
        host.to_ascii_lowercase().as_str(),
        "registry-1.docker.io" | "docker.io" | "index.docker.io" | "hub.docker.com"
    )
}

fn build_watch(
    id: String,
    provider: Provider,
    common: SourceCommon,
    defaults: &Defaults,
    presets: &std::collections::BTreeMap<String, SourcePreset>,
) -> anyhow::Result<Watch> {
    let common = common.with_preset_resolved(presets, &id)?;
    let filter = common.build_filter(&id)?;
    let (interval, jitter, poll_on_startup) = common.schedule(defaults);
    let notify_schedule = match resolve_notify_schedule(&common, defaults) {
        Ok(schedule) => schedule,
        Err(err) => anyhow::bail!("source {id}: {err}"),
    };
    Ok(Watch {
        provider,
        interval,
        jitter,
        poll_on_startup,
        filter,
        routing_tag: common.routing_tag,
        notify_schedule,
        organization_id: None,
    })
}

/// Effective notification schedule: per-source expression wins; an explicitly
/// empty per-source value opts out of the `[defaults]` one.
fn resolve_notify_schedule(
    common: &SourceCommon,
    defaults: &Defaults,
) -> Result<Option<NotifySchedule>, String> {
    let expr = match &common.notify_schedule {
        Some(expr) if expr.trim().is_empty() => return Ok(None),
        Some(expr) => Some(expr.as_str()),
        None => defaults
            .notify_schedule
            .as_deref()
            .filter(|expr| !expr.trim().is_empty()),
    };
    expr.map(NotifySchedule::parse).transpose()
}

/// Gitea-compatible forge preset for shared watch construction.
enum GiteaPreset {
    Github { repo: String },
    Codeberg { repo: String },
    Gitea { host: String, repo: String },
}

fn resolve_token(
    config_token: Option<String>,
    named_env: Option<&str>,
    global_env: &str,
) -> Option<String> {
    config_token
        .filter(|token| {
            let trimmed = token.trim();
            !trimmed.is_empty() && trimmed != "<redacted>" && !trimmed.contains("<redacted>")
        })
        .or_else(|| named_env.and_then(super::env_token))
        .or_else(|| super::env_token(global_env))
}

#[expect(
    clippy::too_many_arguments,
    reason = "thin adapter over build_watch; bundling would obscure call sites"
)]
fn build_gitea_watch(
    preset: GiteaPreset,
    id: Option<String>,
    token: Option<String>,
    token_env: Option<String>,
    global_token_env: &'static str,
    common: SourceCommon,
    defaults: &Defaults,
    presets: &std::collections::BTreeMap<String, SourcePreset>,
) -> anyhow::Result<Watch> {
    let token = resolve_token(token, token_env.as_deref(), global_token_env);
    let (id, provider) = match preset {
        GiteaPreset::Github { repo } => {
            let id = id.unwrap_or_else(|| format!("github:{repo}"));
            (
                id.clone(),
                Provider::Github(GiteaReleasesSource::github(id.clone(), repo, token)),
            )
        }
        GiteaPreset::Codeberg { repo } => {
            let id = id.unwrap_or_else(|| format!("codeberg:{repo}"));
            (
                id.clone(),
                Provider::Codeberg(GiteaReleasesSource::codeberg(id.clone(), repo, token)),
            )
        }
        GiteaPreset::Gitea { host, repo } => {
            let id = id.unwrap_or_else(|| format!("gitea:{host}:{repo}"));
            (
                id.clone(),
                Provider::Gitea(GiteaReleasesSource::gitea(id.clone(), host, repo, token)),
            )
        }
    };
    build_watch(id, provider, common, defaults, presets)
}

fn build_package_watch(
    cfg: PackageCfg,
    registry: PackageRegistry,
    defaults: &Defaults,
    presets: &std::collections::BTreeMap<String, SourcePreset>,
) -> anyhow::Result<Watch> {
    let prefix = registry.config_prefix();
    let id = cfg.id.unwrap_or_else(|| format!("{prefix}:{}", cfg.name));
    build_watch(
        id.clone(),
        Provider::package_registry(id, cfg.name, registry),
        cfg.common,
        defaults,
        presets,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "thin adapter over build_watch; bundling would obscure call sites"
)]
fn build_container_watch(
    id: Option<String>,
    image: String,
    registry: ContainerRegistry,
    token: Option<String>,
    token_env: Option<String>,
    global_token_env: Option<&'static str>,
    common: SourceCommon,
    defaults: &Defaults,
    presets: &std::collections::BTreeMap<String, SourcePreset>,
) -> anyhow::Result<Watch> {
    let prefix = registry.config_prefix();
    let id = id.unwrap_or_else(|| format!("{prefix}:{image}"));
    let token = match global_token_env {
        Some(global) => resolve_token(token, token_env.as_deref(), global),
        None => resolve_token(token, token_env.as_deref(), ""),
    };
    build_watch(
        id.clone(),
        Provider::container_registry(id, image, registry, token),
        common,
        defaults,
        presets,
    )
}

/// One watched source. The `type` tag selects the variant.
///
/// `deny_unknown_fields` is intentionally absent: serde does not support it on
/// internally tagged enums.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SourceConfig {
    /// A GitHub repository's releases.
    Github(GithubCfg),
    /// A Codeberg repository's releases.
    Codeberg(GiteaRepoCfg),
    /// A self-hosted Gitea / Forgejo repository's releases.
    Gitea(GiteaHostCfg),
    /// A GitLab project's releases.
    Gitlab(GitlabCfg),
    /// Bitbucket Cloud repository tags.
    Bitbucket(BitbucketCfg),
    /// A container image on Docker Hub or any OCI registry.
    Docker(DockerCfg),
    /// GitHub Container Registry (`ghcr.io`).
    Ghcr(RegistryImageCfg),
    /// Quay.io container registry.
    Quay(RegistryImageCfg),
    /// AWS ECR Public gallery.
    Ecr(RegistryImageCfg),
    /// A generic RSS/Atom/JSON feed.
    Feed(FeedCfg),
    /// A Python PyPI package.
    Pypi(PackageCfg),
    /// A Node.js npm package.
    Npm(PackageCfg),
    /// A Rust crates.io crate.
    Cargo(PackageCfg),
    /// A Maven Central artifact (`group:artifact`).
    Maven(PackageCfg),
    /// A NuGet package.
    Nuget(PackageCfg),
    /// Hex.pm (Elixir) package.
    Hex(PackageCfg),
    /// A RubyGems.org gem.
    Rubygems(PackageCfg),
    /// A Packagist (PHP) package.
    Packagist(PackageCfg),
    /// A Yarn registry package (npm-compatible metadata).
    Yarn(PackageCfg),
    /// A Perl CPAN distribution.
    Cpan(PackageCfg),
    /// Artifact Hub catalog package (Helm chart by default).
    Artifacthub(ArtifactHubCfg),
}

/// Gitea-compatible repo source (GitHub / Codeberg share this shape).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GiteaRepoCfg {
    pub id: Option<String>,
    /// `owner/repo`.
    pub repo: String,
    /// Personal access token for REST API mode (release body + higher limits).
    pub token: Option<String>,
    /// Env var / vault name holding the token (GitOps / UI refs).
    #[serde(default)]
    pub token_env: Option<String>,
    #[serde(flatten)]
    pub common: SourceCommon,
}

/// Self-hosted Gitea / Forgejo repo source.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GiteaHostCfg {
    pub id: Option<String>,
    /// Instance base URL, e.g. `https://git.example.com`.
    pub host: String,
    /// `owner/repo`.
    pub repo: String,
    pub token: Option<String>,
    /// Env var / vault name holding the token (GitOps / UI refs).
    #[serde(default)]
    pub token_env: Option<String>,
    #[serde(flatten)]
    pub common: SourceCommon,
}

/// `type = "github"` source config.
pub type GithubCfg = GiteaRepoCfg;

/// `type = "gitlab"` source config.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GitlabCfg {
    pub id: Option<String>,
    /// Full project path, e.g. `gitlab-org/gitlab` or `group/sub/project`.
    pub project: String,
    /// Base URL of the GitLab instance. Defaults to `https://gitlab.com`.
    pub host: Option<String>,
    /// Personal Access Token or Deploy Token with `read_api` scope.
    pub token: Option<String>,
    /// Env var / vault name holding the token (GitOps / UI refs).
    #[serde(default)]
    pub token_env: Option<String>,
    #[serde(flatten)]
    pub common: SourceCommon,
}

/// `type = "bitbucket"` source config (Cloud or Server).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BitbucketCfg {
    pub id: Option<String>,
    /// Cloud: `workspace/repo_slug`. Server: `PROJECT/repo_slug`.
    pub repo: String,
    /// `cloud` (default) or `server` (Data Center / self-hosted).
    #[serde(default)]
    pub edition: BitbucketEditionCfg,
    /// Cloud: API base (`https://api.bitbucket.org/2.0`). Server: instance URL (required).
    pub host: Option<String>,
    /// App password / PAT (`BITBUCKET_TOKEN` env).
    pub token: Option<String>,
    /// Env var / vault name holding the token (GitOps / UI refs).
    #[serde(default)]
    pub token_env: Option<String>,
    #[serde(flatten)]
    pub common: SourceCommon,
}

/// Serde-facing edition tag for [`BitbucketCfg`].
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BitbucketEditionCfg {
    #[default]
    Cloud,
    Server,
}

impl From<BitbucketEditionCfg> for BitbucketEdition {
    fn from(value: BitbucketEditionCfg) -> Self {
        match value {
            BitbucketEditionCfg::Cloud => Self::Cloud,
            BitbucketEditionCfg::Server => Self::Server,
        }
    }
}

/// `type = "docker"` source config.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DockerCfg {
    pub id: Option<String>,
    /// Repository path, e.g. `library/nginx`.
    pub image: String,
    /// Registry base URL; defaults to Docker Hub.
    pub registry: Option<String>,
    /// Optional static bearer token for private registries.
    pub token: Option<String>,
    /// Env var / vault name holding the token (GitOps / UI refs).
    #[serde(default)]
    pub token_env: Option<String>,
    #[serde(flatten)]
    pub common: SourceCommon,
}

/// `type = "feed"` source config.
/// Preset registry image (`ghcr`, `quay`) — registry URL is implicit.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RegistryImageCfg {
    pub id: Option<String>,
    /// Repository path, e.g. `org/image`.
    pub image: String,
    /// Optional bearer token for private images.
    pub token: Option<String>,
    /// Env var / vault name holding the token (GitOps / UI refs).
    #[serde(default)]
    pub token_env: Option<String>,
    #[serde(flatten)]
    pub common: SourceCommon,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FeedCfg {
    pub id: Option<String>,
    /// Feed URL.
    pub url: String,
    #[serde(flatten)]
    pub common: SourceCommon,
}

/// Package registry source (`pypi`, `npm`, `cargo`, …).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PackageCfg {
    pub id: Option<String>,
    /// Package / crate name.
    pub name: String,
    #[serde(flatten)]
    pub common: SourceCommon,
}

/// `type = "artifacthub"` source config.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ArtifactHubCfg {
    pub id: Option<String>,
    /// `repository/chart`, e.g. `bitnami/nginx`.
    pub name: String,
    /// Artifact Hub base URL. Default `https://artifacthub.io`.
    pub host: Option<String>,
    /// Package kind segment in the API path. Default `helm`.
    #[serde(default = "default_artifacthub_kind")]
    pub package_kind: String,
    #[serde(flatten)]
    pub common: SourceCommon,
}

fn default_artifacthub_kind() -> String {
    "helm".to_owned()
}

impl SourceConfig {
    pub(super) fn into_watch(
        self,
        defaults: &Defaults,
        presets: &std::collections::BTreeMap<String, SourcePreset>,
    ) -> anyhow::Result<Watch> {
        match self {
            SourceConfig::Github(c) => build_gitea_watch(
                GiteaPreset::Github { repo: c.repo },
                c.id,
                c.token,
                c.token_env,
                "GITHUB_TOKEN",
                c.common,
                defaults,
                presets,
            ),
            SourceConfig::Codeberg(c) => build_gitea_watch(
                GiteaPreset::Codeberg { repo: c.repo },
                c.id,
                c.token,
                c.token_env,
                "CODEBERG_TOKEN",
                c.common,
                defaults,
                presets,
            ),
            SourceConfig::Gitea(c) => build_gitea_watch(
                GiteaPreset::Gitea {
                    host: c.host,
                    repo: c.repo,
                },
                c.id,
                c.token,
                c.token_env,
                "GITEA_TOKEN",
                c.common,
                defaults,
                presets,
            ),
            SourceConfig::Gitlab(c) => {
                let id = c.id.unwrap_or_else(|| format!("gitlab:{}", c.project));
                let token = resolve_token(c.token, c.token_env.as_deref(), "GITLAB_TOKEN");
                let host = c.host.unwrap_or_default();
                build_watch(
                    id.clone(),
                    Provider::Gitlab(GitlabSource::new(id, host, c.project, token)),
                    c.common,
                    defaults,
                    presets,
                )
            }
            SourceConfig::Bitbucket(c) => {
                let id = c.id.unwrap_or_else(|| format!("bitbucket:{}", c.repo));
                let token = resolve_token(c.token, c.token_env.as_deref(), "BITBUCKET_TOKEN");
                let edition: BitbucketEdition = c.edition.into();
                let provider = match edition {
                    BitbucketEdition::Cloud => {
                        if let Some(host) = c.host {
                            let web = host
                                .replace("api.bitbucket.org/2.0", "bitbucket.org")
                                .trim_end_matches('/')
                                .to_owned();
                            Provider::Bitbucket(BitbucketSource::with_host(
                                id.clone(),
                                c.repo,
                                host,
                                web,
                                token,
                            ))
                        } else {
                            Provider::Bitbucket(BitbucketSource::cloud(id.clone(), c.repo, token))
                        }
                    }
                    BitbucketEdition::Server => {
                        let host = c.host.ok_or_else(|| {
                            anyhow::anyhow!(
                                "bitbucket source `{}`: edition = \"server\" requires `host`",
                                id
                            )
                        })?;
                        Provider::Bitbucket(BitbucketSource::server(
                            id.clone(),
                            host,
                            c.repo,
                            token,
                        ))
                    }
                };
                build_watch(id.clone(), provider, c.common, defaults, presets)
            }
            SourceConfig::Docker(c) => {
                let registry = resolve_docker_registry(c.registry.as_deref());
                build_container_watch(
                    c.id,
                    c.image,
                    registry,
                    c.token,
                    c.token_env,
                    Some("DOCKER_TOKEN"),
                    c.common,
                    defaults,
                    presets,
                )
            }
            SourceConfig::Ghcr(c) => build_container_watch(
                c.id,
                c.image,
                ContainerRegistry::Ghcr,
                c.token,
                c.token_env,
                Some("GHCR_TOKEN"),
                c.common,
                defaults,
                presets,
            ),
            SourceConfig::Quay(c) => build_container_watch(
                c.id,
                c.image,
                ContainerRegistry::Quay,
                c.token,
                c.token_env,
                Some("QUAY_TOKEN"),
                c.common,
                defaults,
                presets,
            ),
            SourceConfig::Ecr(c) => build_container_watch(
                c.id,
                c.image,
                ContainerRegistry::EcrPublic,
                c.token,
                c.token_env,
                Some("ECR_TOKEN"),
                c.common,
                defaults,
                presets,
            ),
            SourceConfig::Feed(c) => {
                let id = c.id.unwrap_or_else(|| format!("feed:{}", c.url));
                build_watch(
                    id.clone(),
                    Provider::Feed(FeedSource::new(id, c.url)),
                    c.common,
                    defaults,
                    presets,
                )
            }
            SourceConfig::Pypi(c) => {
                build_package_watch(c, PackageRegistry::Pypi, defaults, presets)
            }
            SourceConfig::Npm(c) => build_package_watch(c, PackageRegistry::Npm, defaults, presets),
            SourceConfig::Cargo(c) => {
                build_package_watch(c, PackageRegistry::Cargo, defaults, presets)
            }
            SourceConfig::Maven(c) => {
                build_package_watch(c, PackageRegistry::Maven, defaults, presets)
            }
            SourceConfig::Nuget(c) => {
                build_package_watch(c, PackageRegistry::Nuget, defaults, presets)
            }
            SourceConfig::Hex(c) => build_package_watch(c, PackageRegistry::Hex, defaults, presets),
            SourceConfig::Rubygems(c) => {
                build_package_watch(c, PackageRegistry::Rubygems, defaults, presets)
            }
            SourceConfig::Packagist(c) => {
                build_package_watch(c, PackageRegistry::Packagist, defaults, presets)
            }
            SourceConfig::Yarn(c) => {
                build_package_watch(c, PackageRegistry::Yarn, defaults, presets)
            }
            SourceConfig::Cpan(c) => {
                build_package_watch(c, PackageRegistry::Cpan, defaults, presets)
            }
            SourceConfig::Artifacthub(c) => {
                let id = c.id.unwrap_or_else(|| format!("artifacthub:{}", c.name));
                let host = c
                    .host
                    .unwrap_or_else(|| "https://artifacthub.io".to_owned());
                let source =
                    ArtifactHubSource::with_options(id.clone(), c.name, host, c.package_kind)?;
                build_watch(
                    id.clone(),
                    Provider::ArtifactHub(source),
                    c.common,
                    defaults,
                    presets,
                )
            }
        }
    }

    /// Non-fatal hints about suboptimal source configuration.
    ///
    /// Resolves `preset` first so a shared `pattern` on the preset suppresses
    /// the "noisy registry" warning.
    pub fn lint(
        &self,
        presets: &std::collections::BTreeMap<String, SourcePreset>,
    ) -> Option<String> {
        let id = source_label(self);
        let pattern = match source_common(self)
            .clone()
            .with_preset_resolved(presets, &id)
        {
            Ok(common) => common.pattern,
            Err(_) => source_common(self).pattern.clone(),
        };
        match self {
            Self::Docker(_) | Self::Ghcr(_) | Self::Quay(_) | Self::Ecr(_) => {
                if pattern.is_none() {
                    Some(format!(
                        "source `{id}`: container registries are noisy without a `pattern` filter"
                    ))
                } else {
                    None
                }
            }
            Self::Maven(c) if !c.name.contains(':') => Some(format!(
                "source `{id}`: maven name should be `group:artifact`, got `{}`",
                c.name
            )),
            Self::Packagist(c) if !c.name.contains('/') => Some(format!(
                "source `{id}`: packagist name should be `vendor/package`, got `{}`",
                c.name
            )),
            Self::Github(c)
                if resolve_token(c.token.clone(), c.token_env.as_deref(), "GITHUB_TOKEN")
                    .is_none() =>
            {
                Some(format!(
 "source `{id}`: no GitHub token — using Atom feed (no release body, lower rate limit)"
 ))
            }
            Self::Bitbucket(c) if c.edition == BitbucketEditionCfg::Server && c.host.is_none() => {
                Some(format!(
                    "source `{id}`: bitbucket edition = server requires `host`"
                ))
            }
            Self::Artifacthub(c) if !c.name.contains('/') => Some(format!(
                "source `{id}`: artifacthub name should be `repository/chart`, got `{}`",
                c.name
            )),
            _ => None,
        }
    }
}

pub(crate) fn source_routing_tag(source: &SourceConfig) -> Option<&str> {
    source_common(source).routing_tag.as_deref()
}

pub(crate) fn source_common(source: &SourceConfig) -> &SourceCommon {
    match source {
        SourceConfig::Github(c) | SourceConfig::Codeberg(c) => &c.common,
        SourceConfig::Gitea(c) => &c.common,
        SourceConfig::Gitlab(c) => &c.common,
        SourceConfig::Bitbucket(c) => &c.common,
        SourceConfig::Docker(c) => &c.common,
        SourceConfig::Ghcr(c) | SourceConfig::Quay(c) | SourceConfig::Ecr(c) => &c.common,
        SourceConfig::Feed(c) => &c.common,
        SourceConfig::Pypi(c)
        | SourceConfig::Npm(c)
        | SourceConfig::Cargo(c)
        | SourceConfig::Maven(c)
        | SourceConfig::Nuget(c)
        | SourceConfig::Hex(c)
        | SourceConfig::Rubygems(c)
        | SourceConfig::Packagist(c)
        | SourceConfig::Yarn(c)
        | SourceConfig::Cpan(c) => &c.common,
        SourceConfig::Artifacthub(c) => &c.common,
    }
}

fn source_common_mut(source: &mut SourceConfig) -> &mut SourceCommon {
    match source {
        SourceConfig::Github(c) | SourceConfig::Codeberg(c) => &mut c.common,
        SourceConfig::Gitea(c) => &mut c.common,
        SourceConfig::Gitlab(c) => &mut c.common,
        SourceConfig::Bitbucket(c) => &mut c.common,
        SourceConfig::Docker(c) => &mut c.common,
        SourceConfig::Ghcr(c) | SourceConfig::Quay(c) | SourceConfig::Ecr(c) => &mut c.common,
        SourceConfig::Feed(c) => &mut c.common,
        SourceConfig::Pypi(c)
        | SourceConfig::Npm(c)
        | SourceConfig::Cargo(c)
        | SourceConfig::Maven(c)
        | SourceConfig::Nuget(c)
        | SourceConfig::Hex(c)
        | SourceConfig::Rubygems(c)
        | SourceConfig::Packagist(c)
        | SourceConfig::Yarn(c)
        | SourceConfig::Cpan(c) => &mut c.common,
        SourceConfig::Artifacthub(c) => &mut c.common,
    }
}

fn source_explicit_id_mut(source: &mut SourceConfig) -> &mut Option<String> {
    match source {
        SourceConfig::Github(c) | SourceConfig::Codeberg(c) => &mut c.id,
        SourceConfig::Gitea(c) => &mut c.id,
        SourceConfig::Gitlab(c) => &mut c.id,
        SourceConfig::Bitbucket(c) => &mut c.id,
        SourceConfig::Docker(c) => &mut c.id,
        SourceConfig::Ghcr(c) | SourceConfig::Quay(c) | SourceConfig::Ecr(c) => &mut c.id,
        SourceConfig::Feed(c) => &mut c.id,
        SourceConfig::Pypi(c)
        | SourceConfig::Npm(c)
        | SourceConfig::Cargo(c)
        | SourceConfig::Maven(c)
        | SourceConfig::Nuget(c)
        | SourceConfig::Hex(c)
        | SourceConfig::Rubygems(c)
        | SourceConfig::Packagist(c)
        | SourceConfig::Yarn(c)
        | SourceConfig::Cpan(c) => &mut c.id,
        SourceConfig::Artifacthub(c) => &mut c.id,
    }
}

/// Bake org defaults into each source and namespace ids / routing tags /
/// user preset names.
pub(crate) fn prepare_organization_sources(organization_id: &str, config: &mut Config) {
    use super::organizations::{namespace_preset_name, namespace_routing_tag, namespace_source_id};

    // Org-scope user presets BEFORE merging: `watches` resolves names against
    // the merged catalogue, so unprefixed keys from two orgs would collide
    // (later org wins) and one org's preset fields would leak into another's
    // sources. Only names this org declares are rewritten — a reference to a
    // built-in stays as-is, and a reference to a name defined in a DIFFERENT
    // org now fails resolution instead of silently borrowing it.
    let user_presets: std::collections::BTreeSet<String> = config.presets.keys().cloned().collect();
    if !user_presets.is_empty() {
        config.presets = std::mem::take(&mut config.presets)
            .into_iter()
            .map(|(name, preset)| (namespace_preset_name(organization_id, &name), preset))
            .collect();
    }

    let defaults = config.defaults.clone();
    for source in &mut config.sources {
        let default_id = default_source_id(source);
        let id_slot = source_explicit_id_mut(source);
        // Prefer an explicit custom id, but rewrite the legacy shared
        // `package:` / `registry:` prefixes that multi-org prepare used to bake
        // in — those break advisory coordinate parsing and webhook lookups.
        let base = match id_slot.as_deref().map(bare_source_id) {
            Some(bare) if is_legacy_shared_prefix_id(bare) => default_id,
            Some(bare) => bare.to_owned(),
            None => default_id,
        };
        *id_slot = Some(namespace_source_id(organization_id, &base));

        let common = source_common_mut(source);
        if common.interval_secs.is_none() {
            common.interval_secs = Some(defaults.interval_secs);
        }
        if common.jitter_secs.is_none() {
            common.jitter_secs = Some(defaults.jitter_secs);
        }
        if common.poll_on_startup.is_none() {
            common.poll_on_startup = Some(defaults.poll_on_startup);
        }
        if common.notify_schedule.is_none() {
            common.notify_schedule = defaults.notify_schedule.clone();
        }
        match &common.routing_tag {
            Some(tag) if !tag.trim().is_empty() => {
                common.routing_tag = Some(namespace_routing_tag(organization_id, tag));
            }
            _ => {
                common.routing_tag = Some(namespace_routing_tag(organization_id, "__org__"));
            }
        }
        let org_preset = common
            .preset
            .as_deref()
            .map(str::trim)
            .filter(|name| user_presets.contains(*name))
            .map(|name| namespace_preset_name(organization_id, name));
        if org_preset.is_some() {
            common.preset = org_preset;
        }
    }
}

pub(crate) fn source_label(source: &SourceConfig) -> String {
    source_explicit_id(source)
        .cloned()
        .unwrap_or_else(|| default_source_id(source))
}

/// Stable default id derived from type + primary field (ignores any explicit `id`).
///
/// Multi-org prepare previously used a shared `package:` / `registry:` prefix for
/// every package/container variant, which broke advisory enrichment
/// (`coordinate_from_source_id` only recognises `npm:`, `pypi:`, …). Keep this in
/// lockstep with [`build_package_watch`] / [`build_container_watch`].
pub(crate) fn default_source_id(source: &SourceConfig) -> String {
    match source {
        SourceConfig::Github(c) | SourceConfig::Codeberg(c) => format!("github:{}", c.repo),
        SourceConfig::Gitea(c) => format!("gitea:{}:{}", c.host, c.repo),
        SourceConfig::Gitlab(c) => format!("gitlab:{}", c.project),
        SourceConfig::Bitbucket(c) => format!("bitbucket:{}", c.repo),
        SourceConfig::Docker(c) => format!("docker:{}", c.image),
        SourceConfig::Ghcr(c) => format!("ghcr:{}", c.image),
        SourceConfig::Quay(c) => format!("quay:{}", c.image),
        SourceConfig::Ecr(c) => format!("ecr:{}", c.image),
        SourceConfig::Feed(c) => format!("feed:{}", c.url),
        SourceConfig::Pypi(c) => format!("pypi:{}", c.name),
        SourceConfig::Npm(c) => format!("npm:{}", c.name),
        SourceConfig::Cargo(c) => format!("cargo:{}", c.name),
        SourceConfig::Maven(c) => format!("maven:{}", c.name),
        SourceConfig::Nuget(c) => format!("nuget:{}", c.name),
        SourceConfig::Hex(c) => format!("hex:{}", c.name),
        SourceConfig::Rubygems(c) => format!("rubygems:{}", c.name),
        SourceConfig::Packagist(c) => format!("packagist:{}", c.name),
        SourceConfig::Yarn(c) => format!("yarn:{}", c.name),
        SourceConfig::Cpan(c) => format!("cpan:{}", c.name),
        SourceConfig::Artifacthub(c) => format!("artifacthub:{}", c.name),
    }
}

fn source_explicit_id(source: &SourceConfig) -> Option<&String> {
    match source {
        SourceConfig::Github(c) | SourceConfig::Codeberg(c) => c.id.as_ref(),
        SourceConfig::Gitea(c) => c.id.as_ref(),
        SourceConfig::Gitlab(c) => c.id.as_ref(),
        SourceConfig::Bitbucket(c) => c.id.as_ref(),
        SourceConfig::Docker(c) => c.id.as_ref(),
        SourceConfig::Ghcr(c) | SourceConfig::Quay(c) | SourceConfig::Ecr(c) => c.id.as_ref(),
        SourceConfig::Feed(c) => c.id.as_ref(),
        SourceConfig::Pypi(c)
        | SourceConfig::Npm(c)
        | SourceConfig::Cargo(c)
        | SourceConfig::Maven(c)
        | SourceConfig::Nuget(c)
        | SourceConfig::Hex(c)
        | SourceConfig::Rubygems(c)
        | SourceConfig::Packagist(c)
        | SourceConfig::Yarn(c)
        | SourceConfig::Cpan(c) => c.id.as_ref(),
        SourceConfig::Artifacthub(c) => c.id.as_ref(),
    }
}

/// Bare id after stripping a possible `org::` prefix.
fn bare_source_id(source_id: &str) -> &str {
    source_id
        .split_once(super::organizations::ORGANIZATION_SEP)
        .map_or(source_id, |(_, rest)| rest)
}

/// Whether a stored bare id is the old shared `package:` / `registry:` prefix
/// that must be rewritten to the type-specific default.
fn is_legacy_shared_prefix_id(bare: &str) -> bool {
    bare.starts_with("package:") || bare.starts_with("registry:")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn config_should_parse_new_provider_types() {
        let toml = r#"
 [[sources]]
 type = "codeberg"
 repo = "Forgejo/Forgejo"

 [[sources]]
 type = "pypi"
 name = "requests"

 [[sources]]
 type = "cargo"
 name = "serde"
 "#;
        let config: Config = toml::from_str(toml).expect("parse");
        assert_eq!(config.sources.len(), 3);
        let watches = config.into_watches().expect("watches");
        assert_eq!(watches[0].provider.kind(), "codeberg");
        assert_eq!(watches[1].provider.kind(), "pypi");
        assert_eq!(watches[2].provider.kind(), "cargo");
    }

    #[test]
    fn config_should_parse_registry_presets() {
        let toml = r#"
 [[sources]]
 type = "ghcr"
 image = "org/app"

 [[sources]]
 type = "quay"
 image = "org/app"

 [[sources]]
 type = "ecr"
 image = "docker/library/nginx"
 "#;
        let config: Config = toml::from_str(toml).expect("parse");
        let watches = config.into_watches().expect("watches");
        assert_eq!(watches[0].provider.kind(), "ghcr");
        assert_eq!(watches[1].provider.kind(), "quay");
        assert_eq!(watches[2].provider.kind(), "ecr");
    }

    #[test]
    fn config_should_parse_yarn_and_cpan() {
        let toml = r#"
 [[sources]]
 type = "yarn"
 name = "lodash"

 [[sources]]
 type = "cpan"
 name = "Moose"
 "#;
        let config: Config = toml::from_str(toml).expect("parse");
        let watches = config.into_watches().expect("watches");
        assert_eq!(watches[0].provider.kind(), "yarn");
        assert_eq!(watches[1].provider.kind(), "cpan");
    }

    #[test]
    fn config_should_parse_maven_and_nuget() {
        let toml = r#"
 [[sources]]
 type = "maven"
 name = "com.google.guava:guava"

 [[sources]]
 type = "nuget"
 name = "Newtonsoft.Json"
 "#;
        let config: Config = toml::from_str(toml).expect("parse");
        let watches = config.into_watches().expect("watches");
        assert_eq!(watches[0].provider.kind(), "maven");
        assert_eq!(watches[1].provider.kind(), "nuget");
    }

    #[test]
    fn config_should_map_prerelease_tags_to_filter() {
        let toml = r#"
 [[sources]]
 type = "github"
 repo = "org/app"
 include_prerelease = true
 prerelease_tags = ["rc", "beta"]
 "#;
        let config: Config = toml::from_str(toml).expect("parse");
        let watches = config.into_watches().expect("watches");
        assert!(watches[0]
            .filter
            .accepts(&crate::model::Release::new("v1.0.0-rc1")));
        assert!(!watches[0]
            .filter
            .accepts(&crate::model::Release::new("v1.0.0-alpha1")));
    }

    #[test]
    fn config_should_map_exclude_updated_to_filter() {
        let toml = r#"
 [[sources]]
 type = "github"
 repo = "org/app"
 exclude_updated = true
 "#;
        let config: Config = toml::from_str(toml).expect("parse");
        let watches = config.into_watches().expect("watches");
        assert!(watches[0].filter.excludes_updated());
    }

    #[test]
    fn source_routing_tag_should_map_to_the_watch() {
        let toml = r#"
 [[notifiers]]
 type = "apprise"
 urls = ["mailto://a@b.c"]

 [[sources]]
 type = "github"
 repo = "org/app"
 routing_tag = "platform-team"
 "#;
        let config: Config = toml::from_str(toml).expect("parse");
        let watches = config.into_watches().expect("watches");
        assert_eq!(watches[0].routing_tag.as_deref(), Some("platform-team"));
    }

    #[test]
    fn source_should_prefer_canonical_routing_tag() {
        let toml = r#"
 [[notifiers]]
 type = "apprise"
 urls = ["mailto://a@b.c"]

 [[sources]]
 type = "github"
 repo = "org/app"
 routing_tag = "security-team"
 "#;
        let config: Config = toml::from_str(toml).expect("parse");
        let watches = config.into_watches().expect("watches");
        assert_eq!(watches[0].routing_tag.as_deref(), Some("security-team"));
    }

    #[test]
    fn resolve_token_should_ignore_blank_config_and_env() {
        let key = "XRELEASE_TEST_RESOLVE_TOKEN";
        std::env::set_var(key, "");
        assert!(resolve_token(Some(" ".into()), None, key).is_none());
        assert!(resolve_token(None, None, key).is_none());
        std::env::set_var(key, "secret");
        assert_eq!(resolve_token(None, None, key).as_deref(), Some("secret"));
        assert_eq!(
            resolve_token(Some("cfg".into()), None, key).as_deref(),
            Some("cfg")
        );
        std::env::remove_var(key);
    }

    #[test]
    fn resolve_docker_registry_should_treat_blank_and_hub_aliases_as_docker_hub() {
        assert!(matches!(
            resolve_docker_registry(None),
            ContainerRegistry::DockerHub
        ));
        assert!(matches!(
            resolve_docker_registry(Some("")),
            ContainerRegistry::DockerHub
        ));
        assert!(matches!(
            resolve_docker_registry(Some("docker.io")),
            ContainerRegistry::DockerHub
        ));
        assert!(matches!(
            resolve_docker_registry(Some("https://hub.docker.com")),
            ContainerRegistry::DockerHub
        ));
        assert!(matches!(
            resolve_docker_registry(Some("https://registry-1.docker.io")),
            ContainerRegistry::DockerHub
        ));
        match resolve_docker_registry(Some("ghcr.example.com")) {
            ContainerRegistry::Custom(url) => assert_eq!(url, "https://ghcr.example.com"),
            other => panic!("expected custom registry, got {other:?}"),
        }
        match resolve_docker_registry(Some("https://registry.example.com/")) {
            ContainerRegistry::Custom(url) => assert_eq!(url, "https://registry.example.com"),
            other => panic!("expected custom registry, got {other:?}"),
        }
    }

    #[test]
    fn into_watches_should_derive_ids() {
        let toml = r#"
 [[notifiers]]
 type = "apprise"
 endpoint = "http://apprise:8000"
 urls = ["tgram://token/chat"]

 [[sources]]
 type = "github"
 repo = "tokio-rs/tokio"
 "#;
        let config: Config = toml::from_str(toml).expect("parse");
        let watches = config.into_watches().expect("watches");
        assert_eq!(watches[0].provider.id(), "github:tokio-rs/tokio");
    }

    #[test]
    fn default_source_id_should_use_registry_specific_prefixes() {
        let npm: SourceConfig = toml::from_str(
            r#"
 type = "npm"
 name = "axios"
 "#,
        )
        .expect("parse");
        assert_eq!(default_source_id(&npm), "npm:axios");

        let docker: SourceConfig = toml::from_str(
            r#"
 type = "docker"
 image = "library/nginx"
 "#,
        )
        .expect("parse");
        assert_eq!(default_source_id(&docker), "docker:library/nginx");

        let ghcr: SourceConfig = toml::from_str(
            r#"
 type = "ghcr"
 image = "org/app"
 "#,
        )
        .expect("parse");
        assert_eq!(default_source_id(&ghcr), "ghcr:org/app");
    }

    #[test]
    fn prepare_organization_sources_should_rewrite_legacy_package_prefix() {
        let mut config: Config = toml::from_str(
            r#"
 [[sources]]
 type = "npm"
 name = "axios"
 id = "package:axios"
 "#,
        )
        .expect("parse");
        prepare_organization_sources("platform", &mut config);
        assert_eq!(source_label(&config.sources[0]), "platform::npm:axios");
    }

    #[test]
    fn prepare_organization_sources_should_rewrite_legacy_registry_prefix() {
        let mut config: Config = toml::from_str(
            r#"
 [[sources]]
 type = "docker"
 image = "library/nginx"
 id = "registry:library/nginx"
 "#,
        )
        .expect("parse");
        prepare_organization_sources("platform", &mut config);
        assert_eq!(
            source_label(&config.sources[0]),
            "platform::docker:library/nginx"
        );
    }

    #[test]
    fn notify_schedule_should_flow_from_source_config_to_watch() {
        let toml = r#"
 [[sources]]
 type = "github"
 repo = "org/app"
 notify_schedule = "0 9 * * MON-FRI"
 "#;
        let config: Config = toml::from_str(toml).expect("parse");
        let watches = config.into_watches().expect("watches");
        assert_eq!(
            watches[0].notify_schedule.as_ref().map(|s| s.expr()),
            Some("0 9 * * MON-FRI")
        );
    }

    #[test]
    fn empty_source_notify_schedule_should_opt_out_of_default() {
        let toml = r#"
 [defaults]
 notify_schedule = "0 9 * * *"

 [[sources]]
 type = "github"
 repo = "org/scheduled"

 [[sources]]
 type = "github"
 repo = "org/immediate"
 notify_schedule = ""
 "#;
        let config: Config = toml::from_str(toml).expect("parse");
        let watches = config.into_watches().expect("watches");
        assert!(watches[0].notify_schedule.is_some(), "default applies");
        assert!(watches[1].notify_schedule.is_none(), "empty opts out");
    }

    #[test]
    fn invalid_notify_schedule_should_fail_watch_construction() {
        let toml = r#"
 [[sources]]
 type = "github"
 repo = "org/app"
 notify_schedule = "definitely not cron"
 "#;
        let config: Config = toml::from_str(toml).expect("parse");
        let err = config.into_watches().expect_err("must fail");
        assert!(err.to_string().contains("invalid cron expression"));
    }

    #[test]
    fn builtin_preset_should_apply_without_user_catalogue() {
        let toml = r#"
 [[sources]]
 type = "github"
 repo = "org/app"
 preset = "semver-v"
 "#;
        let config: Config = toml::from_str(toml).expect("parse");
        let watches = config.into_watches().expect("watches");
        assert!(watches[0]
            .filter
            .accepts(&crate::model::Release::new("v1.2.3")));
        assert!(!watches[0]
            .filter
            .accepts(&crate::model::Release::new("1.2.3")));
        assert!(!watches[0]
            .filter
            .accepts(&crate::model::Release::new("latest")));
    }

    #[test]
    fn wildcard_builtin_preset_should_accept_any_tag() {
        let toml = r#"
 [[sources]]
 type = "github"
 repo = "org/app"
 preset = "wildcard"
 "#;
        let config: Config = toml::from_str(toml).expect("parse");
        let watches = config.into_watches().expect("watches");
        assert!(watches[0]
            .filter
            .accepts(&crate::model::Release::new("v1.2.3")));
        assert!(watches[0]
            .filter
            .accepts(&crate::model::Release::new("latest")));
        let pre = crate::model::Release::new("v2.0.0-rc.1").with_prerelease(true);
        assert!(watches[0].filter.accepts(&pre));
    }

    #[test]
    fn user_preset_should_override_builtin_by_name() {
        let yaml = r#"
presets:
  semver-v:
    pattern: '^custom-\d+$'
sources:
  - type: github
    repo: org/a
    preset: semver-v
"#;
        let config = crate::config::parse_desired_document(yaml).expect("parse");
        let watches = config.into_watches().expect("watches");
        assert!(watches[0]
            .filter
            .accepts(&crate::model::Release::new("custom-9")));
        assert!(!watches[0]
            .filter
            .accepts(&crate::model::Release::new("v1.2.3")));
    }

    #[test]
    fn preset_should_apply_shared_fields_with_source_override() {
        let yaml = r#"
presets:
  security:
    routing_tag: security-team
    interval_secs: 3600
    pattern: '^v?\d+\.\d+\.\d+$'
sources:
  - type: github
    repo: org/a
    preset: security
  - type: github
    repo: org/b
    preset: security
    interval_secs: 900
"#;
        let config = crate::config::parse_desired_document(yaml).expect("parse");
        let watches = config.into_watches().expect("watches");
        assert_eq!(watches[0].routing_tag.as_deref(), Some("security-team"));
        assert_eq!(watches[0].interval.as_secs(), 3600);
        assert!(watches[0]
            .filter
            .accepts(&crate::model::Release::new("v1.2.3")));
        assert!(!watches[0]
            .filter
            .accepts(&crate::model::Release::new("latest")));
        assert_eq!(watches[1].interval.as_secs(), 900);
        assert_eq!(watches[1].routing_tag.as_deref(), Some("security-team"));
    }

    #[test]
    fn unknown_preset_should_fail_watch_construction() {
        let toml = r#"
 [[sources]]
 type = "github"
 repo = "org/app"
 preset = "missing"
 "#;
        let config: Config = toml::from_str(toml).expect("parse");
        let err = config.into_watches().expect_err("must fail");
        assert!(err.to_string().contains("unknown preset"));
    }
}
