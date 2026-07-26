//! Canonical catalog and per-release page URLs for observability UI.
//!
//! Poll-time [`crate::model::Release::url`] from upstream APIs is preferred when
//! available (exact `html_url`, feed entry link, …). These builders are used as
//! fallbacks for older `seen_release` rows and providers that only return tags.

/// Percent-encode a path segment (release tag) for use in URLs.
#[must_use]
pub(crate) fn encode_path_segment(segment: &str) -> String {
    segment
        .bytes()
        .map(|byte| match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// Gitea-compatible release page (`/{owner}/{repo}/releases/tag/{tag}`).
#[must_use]
pub(crate) fn gitea_release_url(web_base: &str, repo: &str, tag: &str) -> String {
    format!(
        "{}/{}/releases/tag/{}",
        web_base.trim_end_matches('/'),
        repo,
        encode_path_segment(tag)
    )
}

/// GitLab release page (`/{project}/-/releases/{tag}`).
#[must_use]
pub(crate) fn gitlab_release_url(host: &str, project: &str, tag: &str) -> String {
    format!(
        "{}/{}/-/releases/{}",
        host.trim_end_matches('/'),
        project,
        encode_path_segment(tag)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_path_segment_should_percent_encode_spaces() {
        assert_eq!(encode_path_segment("v1.0.0"), "v1.0.0");
        assert_eq!(encode_path_segment("v 1"), "v%201");
    }

    #[test]
    fn gitea_release_url_should_match_github_shape() {
        assert_eq!(
            gitea_release_url("https://github.com", "tokio-rs/tokio", "tokio-1.38.0"),
            "https://github.com/tokio-rs/tokio/releases/tag/tokio-1.38.0"
        );
    }
}
