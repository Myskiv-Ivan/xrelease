//! Integration tests against a local WireMock upstream (no real network).

use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xrelease::sources::{ArtifactHubSource, BitbucketSource, GiteaReleasesSource, Provider};

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("xrelease-test")
        .build()
        .expect("client")
}

#[tokio::test]
async fn gitea_rest_auth_failure_should_fallback_to_atom() {
    let atom = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <entry>
    <title>v1.0.0</title>
    <id>tag:v1.0.0</id>
    <link href="https://example.com/o/r/releases/tag/v1.0.0" rel="alternate"/>
  </entry>
</feed>"#;
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/repos/o/r/releases"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/o/r/releases.atom"))
        .respond_with(ResponseTemplate::new(200).set_body_string(atom))
        .mount(&server)
        .await;

    let source = GiteaReleasesSource::gitea("test", server.uri(), "o/r", Some("bad-token".into()));
    let provider = Provider::Gitea(source);
    let outcome = provider.fetch(&http_client(), None).await.expect("fetch");

    assert_eq!(outcome.releases.len(), 1);
    assert_eq!(outcome.releases[0].raw_tag, "v1.0.0");
}

#[tokio::test]
async fn gitea_compatible_api_should_parse_releases() {
    let body = include_str!("fixtures/github_releases.json");
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/repos/o/r/releases"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&server)
        .await;

    let source = GiteaReleasesSource::gitea("test", server.uri(), "o/r", Some("test-token".into()));
    let provider = Provider::Gitea(source);
    let outcome = provider.fetch(&http_client(), None).await.expect("fetch");

    assert!(!outcome.not_modified);
    assert_eq!(outcome.releases.len(), 2);
    assert_eq!(outcome.releases[0].raw_tag, "v1.2.0");
    assert!(outcome.releases[0].body.as_deref().is_some());
}

#[tokio::test]
async fn bitbucket_cloud_should_parse_tags() {
    let body = include_str!("fixtures/bitbucket_cloud_tags.json");
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repositories/ws/repo/refs/tags"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&server)
        .await;

    let source = BitbucketSource::with_host(
        "test",
        "ws/repo",
        server.uri(),
        "https://bitbucket.org",
        None,
    );
    let provider = Provider::Bitbucket(source);
    let outcome = provider.fetch(&http_client(), None).await.expect("fetch");

    assert_eq!(outcome.releases.len(), 1);
    assert_eq!(outcome.releases[0].raw_tag, "v2.0.0");
}

#[tokio::test]
async fn bitbucket_server_should_parse_tags() {
    let body = include_str!("fixtures/bitbucket_server_tags.json");
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/1.0/projects/PROJ/repos/app/tags"))
        // Pin newest-first ordering — ALPHABETICAL default could push the
        // newest tag past the page limit on repos with >100 tags.
        .and(query_param("orderBy", "MODIFICATION"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&server)
        .await;

    let source = BitbucketSource::server("test", server.uri(), "PROJ/app", None);
    let provider = Provider::Bitbucket(source);
    let outcome = provider.fetch(&http_client(), None).await.expect("fetch");

    assert_eq!(outcome.releases.len(), 1);
    assert_eq!(outcome.releases[0].raw_tag, "3.1.0");
}

#[tokio::test]
async fn upstream_should_honor_if_none_match_304() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/repos/o/r/releases"))
        .respond_with(ResponseTemplate::new(304))
        .mount(&server)
        .await;

    let source = GiteaReleasesSource::gitea("test", server.uri(), "o/r", Some("test-token".into()));
    let provider = Provider::Gitea(source);
    let outcome = provider
        .fetch(&http_client(), Some("\"cached\""))
        .await
        .expect("fetch");

    assert!(outcome.not_modified);
    assert!(outcome.releases.is_empty());
}

#[tokio::test]
async fn artifacthub_should_parse_helm_versions() {
    let body = include_str!("fixtures/artifacthub_helm.json");
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/packages/helm/bitnami/nginx"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&server)
        .await;

    let source = ArtifactHubSource::with_options("test", "bitnami/nginx", server.uri(), "helm")
        .expect("source");
    let provider = Provider::ArtifactHub(source);
    let outcome = provider.fetch(&http_client(), None).await.expect("fetch");

    assert_eq!(outcome.releases.len(), 2);
    assert_eq!(outcome.releases[0].raw_tag, "15.3.4");
    assert!(!outcome.releases[0].prerelease);
}
