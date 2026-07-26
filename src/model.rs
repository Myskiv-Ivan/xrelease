//! The [`Release`] value type and version parsing helpers.
//!
//! Every provider, no matter how different its upstream API, normalizes its
//! output into a `Vec<Release>`. The rest of the pipeline only ever sees this
//! type.

use chrono::{DateTime, Utc};
use semver::Version;

/// A single release observed from some source.
///
/// `id` is the *stable identity* used for de-duplication (a Git tag, a registry
/// tag, an Atom entry GUID). `raw_tag` is what a human should read — usually the
/// same as `id`, but for feeds it is the entry title while `id` is the GUID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Stable identity used by the store to decide "have I seen this?".
    pub id: String,
    /// Human-facing tag/title/version string.
    pub raw_tag: String,
    /// Parsed semantic version, when the tag looks like one.
    pub version: Option<Version>,
    /// Canonical URL for the release, when known.
    pub url: Option<String>,
    /// Publication timestamp, when the source provides one.
    pub published_at: Option<DateTime<Utc>>,
    /// Whether this looks like a pre-release (rc/beta/alpha/semver pre-tag).
    pub prerelease: bool,
    /// Changelog / release notes body (Markdown), when provided by the source.
    ///
    /// GitHub and GitLab REST APIs return this; the Atom feed and Docker tags do not.
    pub body: Option<String>,
}

impl Release {
    /// Create a release whose identity and display tag are both `id`.
    ///
    /// Suitable for GitHub and Docker where the identity *is* the tag. For feeds
    /// use [`Release::display`] afterwards to set a nicer display string.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        let version = parse_version(&id);
        let prerelease = is_prerelease(&id, version.as_ref());
        Self {
            raw_tag: id.clone(),
            id,
            version,
            url: None,
            published_at: None,
            prerelease,
            body: None,
        }
    }

    /// Override the human-facing tag (and re-derive version/prerelease from it).
    ///
    /// The de-duplication identity ([`Release::id`]) is left untouched.
    #[must_use]
    pub fn display(mut self, tag: impl Into<String>) -> Self {
        let tag = tag.into();
        self.version = parse_version(&tag);
        self.prerelease = is_prerelease(&tag, self.version.as_ref());
        self.raw_tag = tag;
        self
    }

    /// Attach a canonical URL.
    #[must_use]
    pub fn with_url(mut self, url: Option<String>) -> Self {
        self.url = url;
        self
    }

    /// Attach a publication timestamp.
    #[must_use]
    pub fn with_published(mut self, at: Option<DateTime<Utc>>) -> Self {
        self.published_at = at;
        self
    }

    /// Attach a changelog body (Markdown text from GitHub/GitLab release notes).
    #[must_use]
    pub fn with_body(mut self, body: Option<String>) -> Self {
        self.body = body;
        self
    }

    /// Override the prerelease flag (used when the source provides an
    /// authoritative value — e.g. GitHub's `prerelease` JSON field).
    #[must_use]
    pub fn with_prerelease(mut self, prerelease: bool) -> Self {
        self.prerelease = prerelease;
        self
    }

    /// Fingerprint of mutable release content (body + URL).
    #[must_use]
    pub fn content_fingerprint(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        self.body.as_deref().unwrap_or("").hash(&mut hasher);
        self.url.as_deref().unwrap_or("").hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
}

/// Pick the newest release from a slice — prefers semver when both tags parse,
/// then `published_at`, then raw tag string.
#[must_use]
pub fn pick_latest<'a>(releases: impl IntoIterator<Item = &'a Release>) -> Option<&'a Release> {
    releases.into_iter().max_by(|a, b| compare_releases(a, b))
}

/// Sort release tags newest-first (semver when possible, else lexicographic).
pub fn sort_releases_newest_first(releases: &mut [Release]) {
    releases.sort_by(|a, b| compare_releases(b, a));
}

fn compare_releases(a: &Release, b: &Release) -> std::cmp::Ordering {
    match (&a.version, &b.version) {
        (Some(va), Some(vb)) => {
            let by_version = va.cmp(vb);
            if by_version != std::cmp::Ordering::Equal {
                return by_version;
            }
            // The 3-component `Version` approximation ties — compare every
            // numeric component of the raw tag (catches a 4th+ component,
            // e.g. NuGet's Revision, that `Version` can't represent).
            if let (Some(na), Some(nb)) = (numeric_parts(&a.raw_tag), numeric_parts(&b.raw_tag)) {
                let by_numeric = na.cmp(&nb);
                if by_numeric != std::cmp::Ordering::Equal {
                    return by_numeric;
                }
            }
        }
        (Some(_), None) => return std::cmp::Ordering::Greater,
        (None, Some(_)) => return std::cmp::Ordering::Less,
        (None, None) => {}
    }

    match (a.published_at, b.published_at) {
        (Some(pa), Some(pb)) => pa.cmp(&pb),
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (None, None) => a.raw_tag.cmp(&b.raw_tag),
    }
}

/// Best-effort parse of a tag into a [`semver::Version`].
///
/// Strips a leading `v`/`V`, accepts strict semver, and falls back to padding
/// short numeric versions (`1.2` → `1.2.0`). Returns `None` for anything that is
/// not recognizably numeric — callers fall back to string identity, so a `None`
/// here never loses a release, it only disables semver-aware ordering.
#[must_use]
pub fn parse_version(tag: &str) -> Option<Version> {
    let trimmed = tag.trim().trim_start_matches(['v', 'V']);
    Version::parse(trimmed)
        .ok()
        .or_else(|| lenient_parse(trimmed))
}

/// Pad two-component numeric versions to three so semver can parse them.
fn lenient_parse(s: &str) -> Option<Version> {
    let mut nums = numeric_parts(s)?;
    while nums.len() < 3 {
        nums.push(0);
    }
    Version::parse(&format!("{}.{}.{}", nums[0], nums[1], nums[2])).ok()
}

/// Every dot-separated numeric component of a tag's core (before any
/// `-pre`/`+build` suffix), keeping *all* of them.
///
/// `semver::Version` (and [`lenient_parse`], which approximates one) only
/// models three numeric components, but some registries go further — NuGet's
/// `Revision` makes a fourth (`1.2.3.4`) routine. [`compare_releases`] uses
/// this as a tie-breaker so `1.2.3.4` still orders before `1.2.3.10` even
/// though both approximate to the same three-component `Version`.
fn numeric_parts(tag: &str) -> Option<Vec<u64>> {
    let trimmed = tag.trim().trim_start_matches(['v', 'V']);
    let core = trimmed.split(['-', '+']).next().unwrap_or(trimmed);
    let parts: Vec<&str> = core.split('.').collect();
    if parts.is_empty()
        || !parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let nums: Vec<u64> = parts.iter().filter_map(|p| p.parse().ok()).collect();
    (nums.len() == parts.len()).then_some(nums)
}

/// A version is a pre-release if semver says so, or the tag carries a common
/// pre-release keyword that survived the numeric-core stripping above.
fn is_prerelease(tag: &str, version: Option<&Version>) -> bool {
    if version.is_some_and(|v| !v.pre.is_empty()) {
        return true;
    }
    const MARKERS: [&str; 9] = [
        "alpha", "beta", "-rc", ".rc", "snapshot", "nightly", "preview", "canary", "-dev",
    ];
    let lower = tag.to_ascii_lowercase();
    MARKERS.iter().any(|m| lower.contains(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_should_strip_leading_v() {
        assert_eq!(parse_version("v1.2.3"), Some(Version::new(1, 2, 3)));
    }

    #[test]
    fn parse_version_should_pad_two_component_versions() {
        assert_eq!(parse_version("1.4"), Some(Version::new(1, 4, 0)));
    }

    #[test]
    fn parse_version_should_reject_non_numeric_tags() {
        assert_eq!(parse_version("latest"), None);
    }

    #[test]
    fn release_should_flag_rc_tags_as_prerelease() {
        assert!(Release::new("v2.0.0-rc1").prerelease);
    }

    #[test]
    fn release_should_not_flag_stable_tags_as_prerelease() {
        assert!(!Release::new("v2.0.0").prerelease);
    }

    #[test]
    fn pick_latest_should_prefer_semver_over_recent_atom_touch() {
        use chrono::TimeZone;
        let older_branch_touch = Release::new("v4.10.4").with_published(Some(
            chrono::Utc
                .with_ymd_and_hms(2026, 5, 19, 16, 49, 1)
                .unwrap(),
        ));
        let actual_latest = Release::new("v4.14.5").with_published(Some(
            chrono::Utc
                .with_ymd_and_hms(2026, 4, 23, 11, 46, 46)
                .unwrap(),
        ));
        assert_eq!(
            pick_latest([&older_branch_touch, &actual_latest]).map(|r| r.raw_tag.as_str()),
            Some("v4.14.5")
        );
    }

    #[test]
    fn pick_latest_should_prefer_published_at_without_semver() {
        use chrono::TimeZone;
        let older = Release::new("tokio-1.51.0").with_published(Some(
            chrono::Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
        ));
        let newer = Release::new("tokio-1.52.0").with_published(Some(
            chrono::Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap(),
        ));
        assert_eq!(
            pick_latest([&older, &newer]).map(|r| r.raw_tag.as_str()),
            Some("tokio-1.52.0")
        );
    }

    #[test]
    fn pick_latest_should_break_ties_on_fourth_version_component() {
        // Both approximate to semver `1.2.3` (Version has no 4th field), so
        // without the numeric_parts tie-breaker these compare Equal and the
        // raw_tag string fallback picks the wrong one ("...4" > "...10").
        let older = Release::new("1.2.3.4");
        let newer = Release::new("1.2.3.10");
        assert_eq!(
            pick_latest([&older, &newer]).map(|r| r.raw_tag.as_str()),
            Some("1.2.3.10")
        );
    }

    #[test]
    fn sort_releases_newest_first_should_order_fourth_component_numerically() {
        let mut releases = vec![
            Release::new("1.2.3.9"),
            Release::new("1.2.3.10"),
            Release::new("1.2.3.2"),
        ];
        sort_releases_newest_first(&mut releases);
        let tags: Vec<&str> = releases.iter().map(|r| r.raw_tag.as_str()).collect();
        assert_eq!(tags, vec!["1.2.3.10", "1.2.3.9", "1.2.3.2"]);
    }

    #[test]
    fn display_should_keep_identity_but_change_tag() {
        let r = Release::new("guid-123").display("v3.1.0");
        assert_eq!(r.id, "guid-123");
        assert_eq!(r.version, Some(Version::new(3, 1, 0)));
    }
}
