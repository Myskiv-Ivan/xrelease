//! Shared timestamp parsing for release sources.

use chrono::{DateTime, Utc};

/// Parse an upstream RFC 3339 / ISO 8601 timestamp (`…Z` or `…+00:00`) into UTC.
///
/// Tries [`DateTime<Utc>`]'s own parser first (which accepts a bare `Z`), then
/// falls back to [`DateTime::parse_from_rfc3339`] for explicit offsets. Returns
/// `None` for anything unparseable — callers treat a missing timestamp as
/// "unknown", so a `None` here never drops a release.
#[must_use]
pub(crate) fn parse_rfc3339(value: &str) -> Option<DateTime<Utc>> {
    value.parse::<DateTime<Utc>>().ok().or_else(|| {
        DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rfc3339_should_accept_zulu_suffix() {
        let at = parse_rfc3339("2026-04-23T11:46:46Z").expect("parse");
        assert_eq!(at.to_rfc3339(), "2026-04-23T11:46:46+00:00");
    }

    #[test]
    fn parse_rfc3339_should_accept_explicit_offset() {
        let at = parse_rfc3339("2026-04-23T13:46:46+02:00").expect("parse");
        assert_eq!(at.to_rfc3339(), "2026-04-23T11:46:46+00:00");
    }

    #[test]
    fn parse_rfc3339_should_reject_garbage() {
        assert!(parse_rfc3339("not-a-date").is_none());
    }
}
