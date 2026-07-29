//! Docker / OCI registry source.
//!
//! Lists image tags via the Registry v2 API (`GET /v2/<name>/tags/list`).
//! Preset constructors cover Docker Hub, GHCR, and Quay; [`DockerSource::new`]
//! accepts any registry base URL.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use reqwest::header::{ETAG, IF_NONE_MATCH, LINK, WWW_AUTHENTICATE};
use reqwest::StatusCode;
use serde::Deserialize;

use super::http::FetchOutcome;
use super::parse_rfc3339;
use crate::error::SourceError;
use crate::model::Release;

/// Tags requested per `tags/list` page. Registries cap and paginate large lists,
/// so an explicit `n=` bounds each response instead of pulling an unbounded body.
const TAGS_PAGE_SIZE: usize = 100;
/// Safety bound on pages followed before giving up (≤ `TAGS_PAGE_SIZE` tags each).
const MAX_TAG_PAGES: usize = 50;

/// Docker Hub default registry.
pub const REGISTRY_DOCKER_HUB: &str = "https://registry-1.docker.io";
/// GitHub Container Registry.
pub const REGISTRY_GHCR: &str = "https://ghcr.io";
/// Red Hat Quay.
pub const REGISTRY_QUAY: &str = "https://quay.io";
/// AWS Elastic Container Registry (public gallery).
pub const REGISTRY_ECR_PUBLIC: &str = "https://public.ecr.aws";

/// Preset or custom OCI registry for [`DockerSource`].
#[derive(Debug, Clone)]
pub enum ContainerRegistry {
    /// Docker Hub (`registry-1.docker.io`).
    DockerHub,
    /// GitHub Container Registry (`ghcr.io`).
    Ghcr,
    /// Quay.io.
    Quay,
    /// AWS ECR Public (`public.ecr.aws`).
    EcrPublic,
    /// Any other registry base URL (`type = "docker"` with `registry`).
    Custom(String),
}

impl ContainerRegistry {
    /// Config `type` / id prefix for this registry preset.
    #[must_use]
    pub fn config_prefix(&self) -> &'static str {
        match self {
            Self::DockerHub | Self::Custom(_) => "docker",
            Self::Ghcr => "ghcr",
            Self::Quay => "quay",
            Self::EcrPublic => "ecr",
        }
    }

    /// Build a [`DockerSource`] for the given image.
    #[must_use]
    pub fn build_source(
        &self,
        id: impl Into<String>,
        image: impl Into<String>,
        token: Option<String>,
    ) -> DockerSource {
        let id = id.into();
        let image = image.into();
        match self {
            Self::DockerHub => DockerSource::docker_hub(id, image, token),
            Self::Ghcr => DockerSource::ghcr(id, image, token),
            Self::Quay => DockerSource::quay(id, image, token),
            Self::EcrPublic => DockerSource::ecr_public(id, image, token),
            Self::Custom(registry) => DockerSource::new(id, registry.clone(), image, token),
        }
    }
}

/// A source watching the tags of a single container image.
#[derive(Debug, Clone)]
pub struct DockerSource {
    id: String,
    kind: &'static str,
    kind_label: &'static str,
    registry: String,
    image: String,
    token: Option<String>,
}

impl DockerSource {
    /// Arbitrary OCI registry (`type = "docker"` with custom `registry` URL).
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        registry: impl Into<String>,
        image: impl Into<String>,
        token: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            kind: "docker",
            kind_label: "Docker registry",
            registry: registry.into(),
            image: image.into(),
            token,
        }
    }

    /// Docker Hub (`library/` namespace for official images).
    #[must_use]
    pub fn docker_hub(
        id: impl Into<String>,
        image: impl Into<String>,
        token: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            kind: "docker",
            kind_label: "Docker Hub",
            registry: REGISTRY_DOCKER_HUB.to_owned(),
            image: image.into(),
            token,
        }
    }

    /// GitHub Container Registry (`ghcr.io`).
    #[must_use]
    pub fn ghcr(id: impl Into<String>, image: impl Into<String>, token: Option<String>) -> Self {
        Self {
            id: id.into(),
            kind: "ghcr",
            kind_label: "GHCR",
            registry: REGISTRY_GHCR.to_owned(),
            image: image.into(),
            token,
        }
    }

    /// Quay.io registry.
    #[must_use]
    pub fn quay(id: impl Into<String>, image: impl Into<String>, token: Option<String>) -> Self {
        Self {
            id: id.into(),
            kind: "quay",
            kind_label: "Quay",
            registry: REGISTRY_QUAY.to_owned(),
            image: image.into(),
            token,
        }
    }

    /// AWS ECR Public gallery (`public.ecr.aws`).
    #[must_use]
    pub fn ecr_public(
        id: impl Into<String>,
        image: impl Into<String>,
        token: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            kind: "ecr",
            kind_label: "AWS ECR Public",
            registry: REGISTRY_ECR_PUBLIC.to_owned(),
            image: image.into(),
            token,
        }
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn kind(&self) -> &'static str {
        self.kind
    }

    pub(crate) fn kind_label(&self) -> &'static str {
        self.kind_label
    }

    pub(crate) fn display_name(&self) -> &str {
        &self.image
    }

    pub(crate) fn source_page_url(&self) -> String {
        match self.kind {
            "docker" => format!("https://hub.docker.com/r/{}", self.image),
            "ghcr" => ghcr_catalog_url(&self.image),
            "quay" => format!("https://quay.io/repository/{}", self.image),
            "ecr" => format!("https://gallery.ecr.aws/{}", self.image),
            _ => format!("{}/v2/{}/tags/list", self.registry, self.image),
        }
    }

    pub(crate) fn release_page_url(&self, tag: &str) -> String {
        let encoded = super::release_urls::encode_path_segment(tag);
        match self.kind {
            "docker" => format!(
                "https://hub.docker.com/r/{}/tags?name={encoded}",
                self.image
            ),
            "ghcr" => ghcr_release_url(&self.image, tag),
            "quay" => format!(
                "https://quay.io/repository/{}?tab=tags&tag={encoded}",
                self.image
            ),
            "ecr" => format!("https://gallery.ecr.aws/{}?tag={encoded}", self.image),
            _ => self.source_page_url(),
        }
    }

    pub(crate) async fn fetch(
        &self,
        http: &reqwest::Client,
        if_none_match: Option<&str>,
    ) -> Result<FetchOutcome, SourceError> {
        let base = format!(
            "{}/v2/{}/tags/list",
            self.registry.trim_end_matches('/'),
            self.image
        );
        let first_url = format!("{base}?n={TAGS_PAGE_SIZE}");

        // Only the first page carries the conditional request and yields the
        // ETag we persist; a `304` means the tag list is unchanged, so we skip
        // re-parsing entirely (the whole point of storing the ETag).
        let Some(first) = self.get_page(http, &first_url, if_none_match).await? else {
            return Ok(FetchOutcome::not_modified());
        };
        let etag = first.etag;
        let mut tags = first.body.tags.unwrap_or_default();
        let mut next = first.link.as_deref().and_then(parse_next_link);

        // Follow RFC 5988 `Link: rel="next"` pages. `tags/list` has no defined
        // ordering, so the full list is needed to reliably spot new tags — but
        // each page is now bounded and a runaway is capped.
        let mut pages = 1usize;
        while let Some(rel) = next {
            if pages >= MAX_TAG_PAGES {
                tracing::warn!(
                    source = %self.id,
                    pages,
                    "docker tag pagination hit page cap; some tags may be missed"
                );
                break;
            }
            let page_url = join_registry_url(&self.registry, &rel);
            let Some(page) = self.get_page(http, &page_url, None).await? else {
                break;
            };
            if let Some(mut more) = page.body.tags {
                tags.append(&mut more);
            }
            next = page.link.as_deref().and_then(parse_next_link);
            pages += 1;
        }

        let releases = tags
            .into_iter()
            .map(|tag| {
                let url = self.release_page_url(&tag);
                Release::new(tag).with_url(Some(url))
            })
            .collect();
        let outcome = FetchOutcome::fresh(releases, etag);

        // The Registry v2 `tags/list` response is names only — no timestamps
        // exist anywhere in that API, which is why docker sources rendered an
        // empty "Released" column. Docker Hub exposes push dates through its
        // own Hub API, so the Hub preset folds them in; other registries have
        // no anonymous date endpoint (GHCR none at all) and keep dates unknown.
        if self.registry == REGISTRY_DOCKER_HUB {
            return Ok(self.attach_hub_pushed(http, outcome).await);
        }
        Ok(outcome)
    }

    /// Best-effort: fold Docker Hub tag push dates into already-fetched tags.
    ///
    /// Any failure returns the tags untouched — a missing date is cosmetic, and
    /// must never turn a successful poll into an error (same contract as the
    /// NuGet registration lookup).
    async fn attach_hub_pushed(
        &self,
        http: &reqwest::Client,
        mut outcome: FetchOutcome,
    ) -> FetchOutcome {
        if outcome.releases.is_empty() {
            return outcome;
        }
        match self.fetch_hub_pushed(http).await {
            Ok(dates) if !dates.is_empty() => {
                for release in &mut outcome.releases {
                    if let Some(pushed) = dates.get(&release.id) {
                        release.published_at = Some(*pushed);
                    }
                }
            }
            Ok(_) => {}
            Err(err) => tracing::debug!(
                source = %self.id,
                error = %err,
                "docker hub tag-date lookup failed — tags kept without publish dates"
            ),
        }
        outcome
    }

    /// Tag → last-push time from the Docker Hub API.
    ///
    /// One page only, newest pushes first (the API's default order): that dates
    /// the tags a release monitor actually surfaces, and the store's
    /// `COALESCE(seen_release.published_at, …)` upsert keeps filling older tags
    /// in on later polls as they rotate through the newest page. A tag's push
    /// time is the closest thing OCI has to a publish date — re-pushing a tag
    /// moves it, but the store only records the first value it sees.
    async fn fetch_hub_pushed(
        &self,
        http: &reqwest::Client,
    ) -> Result<BTreeMap<String, DateTime<Utc>>, SourceError> {
        #[derive(Deserialize)]
        struct HubTagsResponse {
            #[serde(default)]
            results: Vec<HubTag>,
        }

        #[derive(Deserialize)]
        struct HubTag {
            name: String,
            /// When this tag last received a push (the date the UI wants).
            #[serde(default)]
            tag_last_pushed: Option<String>,
            /// Older field; kept as fallback — some mirrors omit `tag_last_pushed`.
            #[serde(default)]
            last_updated: Option<String>,
        }

        let url = format!(
            "https://hub.docker.com/v2/repositories/{}/tags?page_size={TAGS_PAGE_SIZE}",
            self.image
        );
        let response = http.get(&url).send().await?;
        if !response.status().is_success() {
            return Err(SourceError::Registry {
                status: response.status().as_u16(),
                url,
            });
        }
        let body: HubTagsResponse = response.json().await?;
        Ok(body
            .results
            .into_iter()
            .filter_map(|tag| {
                let pushed = tag
                    .tag_last_pushed
                    .as_deref()
                    .or(tag.last_updated.as_deref())
                    .and_then(parse_rfc3339)?;
                Some((tag.name, pushed))
            })
            .collect())
    }

    /// Fetch one `tags/list` page. Returns `None` on `304 Not Modified`.
    async fn get_page(
        &self,
        http: &reqwest::Client,
        url: &str,
        if_none_match: Option<&str>,
    ) -> Result<Option<TagsPage>, SourceError> {
        let response = self.send_authenticated(http, url, if_none_match).await?;
        if response.status() == StatusCode::NOT_MODIFIED {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(SourceError::Registry {
                status: response.status().as_u16(),
                url: url.to_owned(),
            });
        }
        let etag = header_string(&response, ETAG);
        let link = header_string(&response, LINK);
        let body: TagsList = response.json().await?;
        Ok(Some(TagsPage { etag, link, body }))
    }

    /// Send a registry GET, transparently performing the Docker v2 token
    /// handshake (`401` + `WWW-Authenticate` → bearer).
    ///
    /// The handshake runs even when a static token is configured: sending a Hub
    /// PAT / wrong `DOCKER_TOKEN` as `Authorization: Bearer` on the registry
    /// itself is rejected with 401, and skipping the challenge left public
    /// `library/*` pulls permanently broken. Credentials (when set) are offered
    /// to the *token realm* as HTTP Basic, which is how Docker Hub expects them.
    async fn send_authenticated(
        &self,
        http: &reqwest::Client,
        url: &str,
        if_none_match: Option<&str>,
    ) -> Result<reqwest::Response, SourceError> {
        let make = |bearer: Option<&str>| {
            let mut req = http.get(url);
            if let Some(token) = bearer {
                req = req.bearer_auth(token);
            }
            if let Some(inm) = if_none_match {
                req = req.header(IF_NONE_MATCH, inm);
            }
            req
        };

        let response = make(None).send().await?;
        if response.status() != StatusCode::UNAUTHORIZED {
            return Ok(response);
        }
        let challenge = header_string(&response, WWW_AUTHENTICATE).unwrap_or_default();
        if challenge.is_empty() {
            return Ok(response);
        }
        let token = obtain_token(http, &challenge, self.token.as_deref()).await?;
        Ok(make(Some(token.as_str())).send().await?)
    }
}

#[derive(Debug, Deserialize)]
struct TagsList {
    tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    token: Option<String>,
    access_token: Option<String>,
}

/// One page of a paginated `tags/list` response.
struct TagsPage {
    etag: Option<String>,
    link: Option<String>,
    body: TagsList,
}

/// Read a response header as an owned `String`, if present and valid UTF-8.
fn header_string(
    response: &reqwest::Response,
    name: reqwest::header::HeaderName,
) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

/// Extract the `rel="next"` target from an RFC 5988 `Link` header, if present.
fn parse_next_link(header: &str) -> Option<String> {
    header.split(',').find_map(|part| {
        let part = part.trim();
        if !part.contains("rel=\"next\"") && !part.contains("rel=next") {
            return None;
        }
        let start = part.find('<')?;
        let rest = &part[start + 1..];
        let end = rest.find('>')?;
        Some(rest[..end].to_owned())
    })
}

/// Resolve a pagination `Link` target against the registry base. Targets are
/// usually root-relative paths; some registries return absolute URLs.
fn join_registry_url(registry: &str, target: &str) -> String {
    if target.starts_with("http://") || target.starts_with("https://") {
        target.to_owned()
    } else {
        format!("{}{}", registry.trim_end_matches('/'), target)
    }
}

fn ghcr_catalog_url(image: &str) -> String {
    let parts: Vec<&str> = image.split('/').collect();
    match parts.as_slice() {
        [owner, name] => format!("https://github.com/{owner}/{name}/pkgs/container/{name}"),
        [owner, repo, name] => {
            format!("https://github.com/{owner}/{repo}/pkgs/container/{name}")
        }
        _ => format!("https://ghcr.io/{image}"),
    }
}

fn ghcr_release_url(image: &str, tag: &str) -> String {
    let encoded = super::release_urls::encode_path_segment(tag);
    let parts: Vec<&str> = image.split('/').collect();
    match parts.as_slice() {
        [owner, name] => {
            format!("https://github.com/{owner}/{name}/pkgs/container/{name}/versions/{encoded}")
        }
        [owner, repo, name] => {
            format!("https://github.com/{owner}/{repo}/pkgs/container/{name}/versions/{encoded}")
        }
        _ => format!("https://ghcr.io/{image}:{encoded}"),
    }
}

async fn obtain_token(
    http: &reqwest::Client,
    challenge: &str,
    identity: Option<&str>,
) -> Result<String, SourceError> {
    let challenge = challenge
        .trim()
        .strip_prefix("Bearer ")
        .or_else(|| challenge.trim().strip_prefix("bearer "))
        .unwrap_or(challenge);

    let mut realm: Option<String> = None;
    let mut query: Vec<(String, String)> = Vec::new();
    for part in challenge.split(',') {
        let mut kv = part.splitn(2, '=');
        let key = kv.next().unwrap_or_default().trim();
        let value = kv.next().unwrap_or_default().trim().trim_matches('"');
        match key {
            "realm" => realm = Some(value.to_owned()),
            "" => {}
            other => query.push((other.to_owned(), value.to_owned())),
        }
    }

    let realm =
        realm.ok_or_else(|| SourceError::Other("registry auth challenge missing realm".into()))?;

    // Anonymous first — covers every public Hub image. Only if that fails and
    // the source has credentials do we retry with HTTP Basic against the realm
    // (Docker Hub PATs: `username:PAT` in `token` / `DOCKER_TOKEN`).
    let anonymous = http.get(&realm).query(&query).send().await?;
    if anonymous.status().is_success() {
        return token_from_response(anonymous).await;
    }

    if let Some(secret) = identity.map(str::trim).filter(|value| !value.is_empty()) {
        let mut req = http.get(&realm).query(&query);
        req = match secret.split_once(':') {
            Some((user, pass)) => req.basic_auth(user, Some(pass)),
            None => req.basic_auth(secret, Some("")),
        };
        let authed = req.send().await?;
        if authed.status().is_success() {
            return token_from_response(authed).await;
        }
        return Err(SourceError::Registry {
            status: authed.status().as_u16(),
            url: realm,
        });
    }

    Err(SourceError::Registry {
        status: anonymous.status().as_u16(),
        url: realm,
    })
}

async fn token_from_response(response: reqwest::Response) -> Result<String, SourceError> {
    let token: TokenResponse = response.error_for_status()?.json().await?;
    token
        .token
        .or(token.access_token)
        .ok_or_else(|| SourceError::Other("registry auth response contained no token".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn ghcr_preset_should_use_ghcr_registry() {
        let s = DockerSource::ghcr("id", "org/app", None);
        assert_eq!(s.kind(), "ghcr");
        assert!(s.registry.contains("ghcr.io"));
    }

    #[test]
    fn parse_next_link_should_extract_next_target() {
        let header = "</v2/app/tags/list?last=v1&n=100>; rel=\"next\"";
        assert_eq!(
            parse_next_link(header).as_deref(),
            Some("/v2/app/tags/list?last=v1&n=100")
        );
    }

    #[test]
    fn parse_next_link_should_ignore_non_next_relations() {
        assert!(parse_next_link("</v2/app>; rel=\"prev\"").is_none());
    }

    #[tokio::test]
    async fn fetch_should_follow_link_pagination() {
        let server = MockServer::start().await;
        // Page 1: one tag + a `Link` header pointing at page 2.
        Mock::given(method("GET"))
            .and(path("/v2/lib/app/tags/list"))
            .and(query_param("n", "100"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Link", "</v2/lib/app/tags/list?last=v1.0.0>; rel=\"next\"")
                    .set_body_json(serde_json::json!({ "tags": ["v1.0.0"] })),
            )
            .mount(&server)
            .await;
        // Page 2: identified by `last`, one more tag, no further `Link`.
        Mock::given(method("GET"))
            .and(path("/v2/lib/app/tags/list"))
            .and(query_param("last", "v1.0.0"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "tags": ["v2.0.0"] })),
            )
            .mount(&server)
            .await;

        let source = DockerSource::new("docker:lib/app", server.uri(), "lib/app", Some("t".into()));
        let outcome = source
            .fetch(&reqwest::Client::new(), None)
            .await
            .expect("fetch");

        let tags: Vec<&str> = outcome
            .releases
            .iter()
            .map(|r| r.raw_tag.as_str())
            .collect();
        assert!(tags.contains(&"v1.0.0"), "page 1 tag missing: {tags:?}");
        assert!(tags.contains(&"v2.0.0"), "page 2 tag missing: {tags:?}");
    }

    #[tokio::test]
    async fn fetch_should_report_not_modified_on_304() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/lib/app/tags/list"))
            .and(header("if-none-match", "\"etag-1\""))
            .respond_with(ResponseTemplate::new(304))
            .mount(&server)
            .await;

        let source = DockerSource::new("docker:lib/app", server.uri(), "lib/app", Some("t".into()));
        let outcome = source
            .fetch(&reqwest::Client::new(), Some("\"etag-1\""))
            .await
            .expect("fetch");

        assert!(outcome.not_modified);
    }

    #[tokio::test]
    async fn fetch_should_capture_response_etag() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/lib/app/tags/list"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("ETag", "\"etag-2\"")
                    .set_body_json(serde_json::json!({ "tags": ["v1.0.0"] })),
            )
            .mount(&server)
            .await;

        let source = DockerSource::new("docker:lib/app", server.uri(), "lib/app", None);
        let outcome = source
            .fetch(&reqwest::Client::new(), None)
            .await
            .expect("fetch");
        assert_eq!(outcome.etag.as_deref(), Some("\"etag-2\""));
    }

    /// A configured (but useless) bearer must not skip the anonymous Hub-style
    /// challenge — that is what left public `library/*` pulls stuck on 401 when
    /// `DOCKER_TOKEN` was set to something the registry does not accept as Bearer.
    #[tokio::test]
    async fn fetch_should_handshake_even_when_a_static_token_is_configured() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/library/nginx/tags/list"))
            .respond_with(
                ResponseTemplate::new(401).insert_header(
                    "WWW-Authenticate",
                    &format!(
                        "Bearer realm=\"{}/token\",service=\"test\",scope=\"repository:library/nginx:pull\"",
                        server.uri()
                    ),
                ),
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "exchange-ok"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v2/library/nginx/tags/list"))
            .and(header("authorization", "Bearer exchange-ok"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "tags": ["1.27.0"] })),
            )
            .mount(&server)
            .await;

        let source = DockerSource::new(
            "docker:library/nginx",
            server.uri(),
            "library/nginx",
            Some("bogus-static-token".into()),
        );
        let outcome = source
            .fetch(&reqwest::Client::new(), None)
            .await
            .expect("fetch after handshake");
        assert_eq!(outcome.releases.len(), 1);
        assert_eq!(outcome.releases[0].raw_tag, "1.27.0");
    }
}
