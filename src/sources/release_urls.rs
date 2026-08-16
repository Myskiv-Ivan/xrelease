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

/// Percent-decode a path segment extracted from a release URL.
///
/// Atom / web links encode `/` and other reserved characters (`backup/foo` →
/// `backup%2Ffoo`). Display tags and store identities must match the REST
/// `tag_name` form, so callers decode after splitting the URL.
///
/// Invalid `%` sequences are left verbatim (best-effort).
#[must_use]
pub(crate) fn decode_path_segment(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Extract and percent-decode `<TAG>` from a `.../releases/tag/<TAG>` URL.
///
/// GitHub/Gitea web + Atom links encode reserved characters in the tag segment
/// (`/` → `%2F`). Decoding keeps Atom identities aligned with REST `tag_name`.
#[must_use]
pub(crate) fn tag_from_release_url(url: &str) -> Option<String> {
    url.split("/releases/tag/")
        .nth(1)
        .map(|rest| rest.split(['/', '?', '#']).next().unwrap_or(rest))
        .map(decode_path_segment)
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
    fn decode_path_segment_should_roundtrip_slash_and_space() {
        assert_eq!(decode_path_segment("backup%2F38320"), "backup/38320");
        assert_eq!(decode_path_segment("v%201"), "v 1");
        assert_eq!(
            decode_path_segment(&encode_path_segment("backup/38320-pre")),
            "backup/38320-pre"
        );
    }

    #[test]
    fn decode_path_segment_should_leave_invalid_escapes() {
        assert_eq!(decode_path_segment("a%ZZb"), "a%ZZb");
        assert_eq!(decode_path_segment("trailing%"), "trailing%");
    }

    #[test]
    fn tag_from_release_url_should_percent_decode_slash() {
        assert_eq!(
            tag_from_release_url(
                "https://github.com/wazuh/wazuh/releases/tag/backup%2F38320-pre-rebase3-20260812"
            )
            .as_deref(),
            Some("backup/38320-pre-rebase3-20260812")
        );
        assert_eq!(
            tag_from_release_url("https://github.com/tokio-rs/tokio/releases/tag/tokio-1.38.0")
                .as_deref(),
            Some("tokio-1.38.0")
        );
    }

    #[test]
    fn gitea_release_url_should_match_github_shape() {
        assert_eq!(
            gitea_release_url("https://github.com", "tokio-rs/tokio", "tokio-1.38.0"),
            "https://github.com/tokio-rs/tokio/releases/tag/tokio-1.38.0"
        );
        assert_eq!(
            gitea_release_url(
                "https://github.com",
                "wazuh/wazuh",
                "backup/38320-pre-rebase3-20260812"
            ),
            "https://github.com/wazuh/wazuh/releases/tag/backup%2F38320-pre-rebase3-20260812"
        );
    }
}
