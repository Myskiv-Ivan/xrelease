//! Shared timestamp parsing for release sources.

use chrono::{DateTime, Datelike, Utc};

/// Oldest year an upstream publish date can plausibly carry.
///
/// Registries stamp a placeholder rather than omitting the field when a version
/// has no real publish date, and every placeholder parses cleanly:
///
/// - NuGet marks **unlisted** packages `1900-01-01T00:00:00Z` — that is what put
///   "Jan 1, 1900" in the release table for every unlisted `Newtonsoft.Json`
///   prerelease;
/// - .NET-backed APIs leak `DateTime.MinValue` as `0001-01-01T00:00:00Z`;
/// - a zero-valued timestamp struct serialises as the Unix epoch.
///
/// Treating those as real dates is worse than having none: they sort to the top
/// of an ascending "Released" column and read as fact. 1980 sits above all three
/// and below the oldest genuine date any supported registry can return (CPAN's
/// archive starts in 1995), so no real release is discarded.
const EARLIEST_PLAUSIBLE_YEAR: i32 = 1980;

/// Parse an upstream RFC 3339 / ISO 8601 timestamp (`…Z` or `…+00:00`) into UTC.
///
/// Tries [`DateTime<Utc>`]'s own parser first (which accepts a bare `Z`), then
/// falls back to [`DateTime::parse_from_rfc3339`] for explicit offsets. Returns
/// `None` for anything unparseable **or for a placeholder older than
/// [`EARLIEST_PLAUSIBLE_YEAR`]** — callers treat a missing timestamp as
/// "unknown", so a `None` here never drops a release, it only stops a sentinel
/// from being rendered as a publication date.
#[must_use]
pub(crate) fn parse_rfc3339(value: &str) -> Option<DateTime<Utc>> {
    let parsed = value.parse::<DateTime<Utc>>().ok().or_else(|| {
        DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
    })?;
    (parsed.year() >= EARLIEST_PLAUSIBLE_YEAR).then_some(parsed)
}

/// Parse an epoch-milliseconds timestamp under the same plausibility floor as
/// [`parse_rfc3339`] — a zeroed field would otherwise render as 1970-01-01.
/// Used by Maven Central's Solr API, the one registry that ships millis.
#[must_use]
pub(crate) fn parse_timestamp_millis(ms: i64) -> Option<DateTime<Utc>> {
    let parsed = DateTime::from_timestamp_millis(ms)?;
    (parsed.year() >= EARLIEST_PLAUSIBLE_YEAR).then_some(parsed)
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

    #[test]
    fn parse_rfc3339_should_reject_nuget_unlisted_sentinel() {
        // Exactly what api.nuget.org returns in `catalogEntry.published` for an
        // unlisted version, in both the offset and the Zulu spelling.
        assert!(parse_rfc3339("1900-01-01T00:00:00+00:00").is_none());
        assert!(parse_rfc3339("1900-01-01T00:00:00Z").is_none());
    }

    #[test]
    fn parse_rfc3339_should_reject_min_value_and_epoch_sentinels() {
        assert!(parse_rfc3339("0001-01-01T00:00:00Z").is_none());
        assert!(parse_rfc3339("1970-01-01T00:00:00Z").is_none());
    }

    #[test]
    fn parse_rfc3339_should_keep_the_oldest_real_registry_dates() {
        // CPAN's archive predates every other supported registry; nothing in
        // that range may be mistaken for a sentinel.
        assert!(parse_rfc3339("1995-08-15T00:00:00Z").is_some());
        assert!(parse_rfc3339("1980-01-01T00:00:00Z").is_some());
    }

    #[test]
    fn parse_timestamp_millis_should_reject_zero_and_keep_real_values() {
        assert!(parse_timestamp_millis(0).is_none());
        let at = parse_timestamp_millis(1_713_398_400_000).expect("real millis");
        assert_eq!(at.to_rfc3339(), "2024-04-18T00:00:00+00:00");
    }
}
