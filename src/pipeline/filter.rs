//! Release acceptance filters for a watch.

use regex::Regex;

use crate::model::Release;

/// Which releases a watch cares about.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// When false (default), pre-release tags are ignored.
    include_prerelease: bool,
    /// When `include_prerelease` is true, optionally restrict to tags containing
    /// these markers (`alpha`, `beta`, `rc`, …). Mirrors gh-monitor
    /// `preReleaseSubChannels`.
    prerelease_tags: Option<Vec<String>>,
    /// Optional regular expression the tag must match (inclusion filter).
    ///
    /// Analogue of `includeRegex` / `--regex-exclude-inverse` in reference tooling.
    pattern: Option<Regex>,
    /// Optional regular expression the tag must **not** match (exclusion filter).
    ///
    /// Applied after `pattern`. Useful for blacklisting specific build variants,
    /// e.g. `'^nightly$'` or `'\.linux\.'`.
    /// Analogue of `excludeRegex` / `--regex-exclude` in reference tooling.
    exclude_pattern: Option<Regex>,
    /// When false (default), re-notify if release body/URL changes on a seen tag.
    /// When true, only genuinely new tag identities trigger notifications
    /// (cli-go `--exclude-updated`).
    exclude_updated: bool,
}

impl Filter {
    /// Build a filter, compiling the optional regexes.
    ///
    /// # Errors
    /// Returns `Err` if either regex fails to compile.
    pub fn new(
        include_prerelease: bool,
        pattern: Option<&str>,
        exclude_pattern: Option<&str>,
    ) -> Result<Self, regex::Error> {
        Self::with_options(include_prerelease, None, pattern, exclude_pattern, false)
    }

    /// Build a filter with optional pre-release sub-channel markers.
    pub fn with_prerelease_tags(
        include_prerelease: bool,
        prerelease_tags: Option<Vec<String>>,
        pattern: Option<&str>,
        exclude_pattern: Option<&str>,
    ) -> Result<Self, regex::Error> {
        Self::with_options(
            include_prerelease,
            prerelease_tags,
            pattern,
            exclude_pattern,
            false,
        )
    }

    /// Build a filter with all optional gates.
    pub fn with_options(
        include_prerelease: bool,
        prerelease_tags: Option<Vec<String>>,
        pattern: Option<&str>,
        exclude_pattern: Option<&str>,
        exclude_updated: bool,
    ) -> Result<Self, regex::Error> {
        let pattern = pattern.map(Regex::new).transpose()?;
        let exclude_pattern = exclude_pattern.map(Regex::new).transpose()?;
        let prerelease_tags = prerelease_tags.filter(|tags| !tags.is_empty());
        Ok(Self {
            include_prerelease,
            prerelease_tags,
            pattern,
            exclude_pattern,
            exclude_updated,
        })
    }

    /// Whether body/URL edits on an already-seen tag should be skipped.
    #[must_use]
    pub fn excludes_updated(&self) -> bool {
        self.exclude_updated
    }

    /// Whether a release passes the filter.
    ///
    /// Evaluation order: prerelease gate → prerelease sub-tags → exclude → include.
    #[must_use]
    pub fn accepts(&self, release: &Release) -> bool {
        if release.prerelease && !self.include_prerelease {
            return false;
        }
        if release.prerelease {
            if let Some(tags) = &self.prerelease_tags {
                if !matches_prerelease_marker(&release.raw_tag, tags) {
                    return false;
                }
            }
        }
        if let Some(re) = &self.exclude_pattern {
            if re.is_match(&release.raw_tag) {
                return false;
            }
        }
        match &self.pattern {
            Some(re) => re.is_match(&release.raw_tag),
            None => true,
        }
    }
}

/// Match gh-monitor `isPreReleaseByTagName` — boundary-aware marker search.
fn matches_prerelease_marker(tag: &str, markers: &[String]) -> bool {
    let tag_lower = tag.to_ascii_lowercase();
    markers.iter().any(|marker| {
        let m = marker.to_ascii_lowercase();
        if m.is_empty() {
            return false;
        }
        // Word-boundary-ish: marker preceded by start/non-alpha, followed by non-alpha/end.
        tag_lower
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|part| part == m.as_str())
            || tag_lower.contains(&format!("-{m}"))
            || tag_lower.contains(&format!("_{m}"))
            || tag_lower.contains(&format!(".{m}"))
    })
}
