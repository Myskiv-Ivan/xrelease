//! Generic RSS / Atom / JSON-Feed source.
//!
//! This is the extensibility escape hatch: anything that publishes a feed can be
//! watched without a bespoke provider. GitLab tag feeds
//! (`https://gitlab.com/<group>/<project>/-/tags?format=atom`), project blogs,
//! and most release pages work here.

use feed_rs::model::Entry;

use super::http::{fetch_bytes, FetchOutcome};
use crate::error::SourceError;
use crate::model::Release;

/// A source backed by a single RSS/Atom/JSON feed URL.
#[derive(Debug, Clone)]
pub struct FeedSource {
    id: String,
    url: String,
}

impl FeedSource {
    /// Create a feed source with an explicit id and feed URL.
    #[must_use]
    pub fn new(id: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            url: url.into(),
        }
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn display_name(&self) -> &str {
        &self.url
    }

    pub(crate) fn source_page_url(&self) -> String {
        self.url.clone()
    }

    pub(crate) async fn fetch(
        &self,
        http: &reqwest::Client,
        if_none_match: Option<&str>,
    ) -> Result<FetchOutcome, SourceError> {
        let url = self.url.clone();
        let result = fetch_bytes(http, &url, if_none_match, |b| b).await?;

        if result.not_modified {
            return Ok(FetchOutcome::not_modified());
        }

        let bytes = result
            .bytes
            .ok_or_else(|| SourceError::Other("empty feed response".into()))?;
        let feed = feed_rs::parser::parse(&bytes[..])?;
        let releases = feed.entries.iter().map(entry_to_release).collect();
        Ok(FetchOutcome::fresh(releases, result.etag))
    }
}

/// Map a feed entry to a [`Release`].
///
/// The entry GUID/`id` is the stable de-dup identity; the title is the friendly
/// display tag (falling back to the id when a feed omits titles).
fn entry_to_release(entry: &Entry) -> Release {
    let url = entry.links.first().map(|link| link.href.clone());
    let title = entry
        .title
        .as_ref()
        .map(|text| text.content.clone())
        .unwrap_or_else(|| entry.id.clone());
    let published = entry.published.or(entry.updated);

    Release::new(entry.id.clone())
        .display(title)
        .with_url(url)
        .with_published(published)
}
