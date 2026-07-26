//! Shared HTTP helpers for release sources.
//!
//! Centralises conditional requests (`If-None-Match`) so providers can skip
//! parsing on `304 Not Modified` responses and persist ETags in the store.

use reqwest::header::{ETAG, IF_NONE_MATCH};
use reqwest::StatusCode;
use serde::de::DeserializeOwned;

use crate::error::SourceError;
use crate::model::Release;

/// Result of a provider fetch, including optional cache metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchOutcome {
    /// Parsed releases when the upstream returned fresh content.
    pub releases: Vec<Release>,
    /// New ETag from the upstream, when present.
    pub etag: Option<String>,
    /// `true` when the upstream answered `304 Not Modified`.
    pub not_modified: bool,
}

impl FetchOutcome {
    /// A successful fetch with parsed releases.
    #[must_use]
    pub fn fresh(releases: Vec<Release>, etag: Option<String>) -> Self {
        Self {
            releases,
            etag,
            not_modified: false,
        }
    }

    /// Upstream reported nothing changed since the stored ETag.
    #[must_use]
    pub fn not_modified() -> Self {
        Self {
            releases: Vec::new(),
            etag: None,
            not_modified: true,
        }
    }
}

/// JSON GET with optional `If-None-Match`.
pub(crate) async fn fetch_json<T>(
    http: &reqwest::Client,
    url: &str,
    if_none_match: Option<&str>,
    build: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
) -> Result<JsonFetchResult<T>, SourceError>
where
    T: DeserializeOwned,
{
    let mut builder = http.get(url);
    if let Some(etag) = if_none_match {
        builder = builder.header(IF_NONE_MATCH, etag);
    }
    let response = build(builder).send().await?;
    let status = response.status();
    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    if status == StatusCode::NOT_MODIFIED {
        return Ok(JsonFetchResult {
            data: None,
            etag,
            not_modified: true,
        });
    }

    if !status.is_success() {
        return Err(SourceError::Registry {
            status: status.as_u16(),
            url: url.to_owned(),
        });
    }

    let data = response.json::<T>().await?;
    Ok(JsonFetchResult {
        data: Some(data),
        etag,
        not_modified: false,
    })
}

/// Raw bytes GET with optional `If-None-Match`.
pub(crate) async fn fetch_bytes(
    http: &reqwest::Client,
    url: &str,
    if_none_match: Option<&str>,
    build: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
) -> Result<BytesFetchResult, SourceError> {
    let mut builder = http.get(url);
    if let Some(etag) = if_none_match {
        builder = builder.header(IF_NONE_MATCH, etag);
    }
    let response = build(builder).send().await?;
    let status = response.status();
    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    if status == StatusCode::NOT_MODIFIED {
        return Ok(BytesFetchResult {
            bytes: None,
            etag,
            not_modified: true,
        });
    }

    if !status.is_success() {
        return Err(SourceError::Registry {
            status: status.as_u16(),
            url: url.to_owned(),
        });
    }

    let bytes = response.bytes().await?.to_vec();
    Ok(BytesFetchResult {
        bytes: Some(bytes),
        etag,
        not_modified: false,
    })
}

/// Parsed JSON response metadata.
pub(crate) struct JsonFetchResult<T> {
    pub data: Option<T>,
    pub etag: Option<String>,
    pub not_modified: bool,
}

/// Raw bytes response metadata.
pub(crate) struct BytesFetchResult {
    pub bytes: Option<Vec<u8>>,
    pub etag: Option<String>,
    pub not_modified: bool,
}
