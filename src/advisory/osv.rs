//! OSV.dev query client (`POST /v1/query`).
//!
//! `/v1/query` is used rather than `/v1/querybatch` because batch responses
//! carry only `{id, modified}` per match, so full details would need one
//! `GET /v1/vulns/{id}` per advisory — more round-trips than the single-package
//! endpoint, which returns complete vulnerability objects in one call.

use serde::Deserialize;

use super::{Advisory, Ecosystem, Severity};

/// Query advisories for one exact `(ecosystem, package, version)`.
///
/// # Errors
/// Returns the transport / decode error so the caller can trip its breaker.
/// Callers must translate this into "no advisories" — enrichment never fails a
/// delivery.
pub async fn query(
    http: &reqwest::Client,
    endpoint_base: &str,
    timeout: std::time::Duration,
    ecosystem: Ecosystem,
    name: &str,
    version: &str,
) -> Result<Vec<Advisory>, reqwest::Error> {
    let url = format!("{endpoint_base}/v1/query");
    let body = serde_json::json!({
        "package": { "name": name, "ecosystem": ecosystem.as_str() },
        "version": version,
    });

    // Per-request, so the shared engine client (its User-Agent, its connection
    // pool) is reused while enrichment still gets a deadline far shorter than a
    // provider fetch would want.
    let response = http
        .post(&url)
        .timeout(timeout)
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json::<OsvQueryResponse>()
        .await?;

    Ok(response
        .vulns
        .into_iter()
        .filter(OsvVuln::is_active)
        .map(OsvVuln::into_advisory)
        .collect())
}

/// `{"vulns": [...]}`. A version with no known advisories omits `vulns`
/// entirely, hence the `default`.
#[derive(Debug, Deserialize)]
struct OsvQueryResponse {
    #[serde(default)]
    vulns: Vec<OsvVuln>,
}

#[derive(Debug, Deserialize)]
struct OsvVuln {
    id: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    details: Option<String>,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    severity: Vec<OsvSeverity>,
    #[serde(default)]
    database_specific: Option<serde_json::Value>,
    /// RFC 3339 timestamp, present only on an entry retracted upstream.
    #[serde(default)]
    withdrawn: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OsvSeverity {
    #[serde(default)]
    score: Option<String>,
}

impl OsvVuln {
    /// Whether the entry still stands.
    ///
    /// A withdrawn entry was retracted upstream (false positive, or a corrected
    /// affected-range). osv.dev drops these from query results, but
    /// `[advisories].endpoint` exists so an operator can point at a self-hosted
    /// mirror, and a mirror need not filter — reporting a retracted advisory as
    /// a live one is worse than reporting nothing.
    fn is_active(&self) -> bool {
        self.withdrawn
            .as_deref()
            .map(str::trim)
            .is_none_or(str::is_empty)
    }

    fn into_advisory(self) -> Advisory {
        let display_id = self
            .aliases
            .iter()
            .find(|alias| alias.starts_with("CVE-"))
            .cloned()
            .unwrap_or_else(|| self.id.clone());

        // OSV puts the label under `database_specific.severity` when the source
        // database states one. We never derive it from the CVSS vector.
        let severity = self
            .database_specific
            .as_ref()
            .and_then(|extra| extra.get("severity"))
            .and_then(serde_json::Value::as_str)
            .and_then(Severity::parse);

        // Keep the vector verbatim so a reader can judge impact even when no
        // label was published.
        let cvss_vector = best_cvss_vector(self.severity);

        let summary = self
            .summary
            .or(self.details)
            .map(|text| first_line(&text))
            .filter(|text| !text.is_empty());

        Advisory {
            url: Some(format!("https://osv.dev/vulnerability/{}", self.id)),
            id: self.id,
            display_id,
            summary,
            severity,
            cvss_vector,
        }
    }
}

/// Richest CVSS vector across an entry's `severity` list, when it has one.
///
/// Scanning the whole list matters: OSV entries routinely carry several scores
/// (a `CVSS_V2` beside a `CVSS_V3`, plus non-CVSS database scores like a bare
/// `7.5`) in no guaranteed order. Taking the first score and *then* checking it
/// looks like a vector would drop a perfectly good v3/v4 vector whenever a
/// non-vector score happened to be listed first — CVSS v2 scores are bare
/// `AV:N/AC:L/…` strings with no `CVSS:` prefix, so that ordering is common.
///
/// Among real vectors the highest CVSS version wins: it is the scoring the
/// database maintains, and the older one is left in place for compatibility.
fn best_cvss_vector(severity: Vec<OsvSeverity>) -> Option<String> {
    severity
        .into_iter()
        .filter_map(|entry| entry.score)
        .filter(|score| score.starts_with("CVSS:"))
        .max_by_key(|score| cvss_major_version(score))
}

/// Major CVSS version of a `CVSS:3.1/AV:N/…` vector; `0` when unparseable.
fn cvss_major_version(vector: &str) -> u8 {
    vector
        .strip_prefix("CVSS:")
        .and_then(|rest| rest.split(['.', '/']).next())
        .and_then(|major| major.parse().ok())
        .unwrap_or(0)
}

/// Longest summary carried into a notification, in characters.
///
/// OSV `summary` is usually a single sentence, but nothing in the schema caps
/// it and the `details` fallback is free-form Markdown.
const MAX_SUMMARY_CHARS: usize = 240;

/// `details` is Markdown and can be pages long; a notification wants one line.
///
/// Section headings (`## Impact`) and horizontal rules are document structure,
/// not summary — skipping them is what makes the fallback readable instead of
/// putting the literal text `## Impact` in front of an operator.
fn first_line(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !is_markdown_furniture(line))
        .unwrap_or_default();
    match line.char_indices().nth(MAX_SUMMARY_CHARS) {
        Some((idx, _)) => format!("{}…", &line[..idx]),
        None => line.to_owned(),
    }
}

/// A line carrying no prose: an ATX heading (`## Impact`) or a rule (`---`).
fn is_markdown_furniture(line: &str) -> bool {
    line.starts_with('#')
        || (line.len() >= 3 && line.chars().all(|ch| matches!(ch, '-' | '*' | '_' | ' ')))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Vec<Advisory> {
        let response: OsvQueryResponse = serde_json::from_str(json).expect("parse");
        response
            .vulns
            .into_iter()
            .filter(OsvVuln::is_active)
            .map(OsvVuln::into_advisory)
            .collect()
    }

    #[test]
    fn empty_response_should_yield_no_advisories() {
        // OSV omits `vulns` entirely for a clean version.
        assert!(parse("{}").is_empty());
        assert!(parse(r#"{"vulns": []}"#).is_empty());
    }

    #[test]
    fn should_prefer_cve_alias_as_display_id() {
        let found = parse(
            r#"{"vulns":[{
                "id":"GHSA-xxxx-yyyy-zzzz",
                "aliases":["CVE-2025-12345","OSV-2025-1"],
                "summary":"Heap overflow"
            }]}"#,
        );
        assert_eq!(found[0].id, "GHSA-xxxx-yyyy-zzzz");
        assert_eq!(
            found[0].display_id, "CVE-2025-12345",
            "readers recognise CVE numbers, not database-native ids"
        );
    }

    #[test]
    fn should_fall_back_to_native_id_without_a_cve_alias() {
        let found = parse(r#"{"vulns":[{"id":"RUSTSEC-2025-0001","aliases":[]}]}"#);
        assert_eq!(found[0].display_id, "RUSTSEC-2025-0001");
    }

    #[test]
    fn should_read_severity_label_from_database_specific() {
        let found = parse(
            r#"{"vulns":[{
                "id":"GHSA-a",
                "database_specific":{"severity":"CRITICAL"}
            }]}"#,
        );
        assert_eq!(found[0].severity, Some(Severity::Critical));
    }

    #[test]
    fn should_carry_cvss_vector_without_inventing_a_severity() {
        let found = parse(
            r#"{"vulns":[{
                "id":"GHSA-b",
                "severity":[{"type":"CVSS_V3","score":"CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H"}]
            }]}"#,
        );
        assert_eq!(
            found[0].severity, None,
            "must not derive a label from a CVSS vector"
        );
        assert_eq!(
            found[0].cvss_vector.as_deref(),
            Some("CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H")
        );
    }

    #[test]
    fn should_ignore_non_cvss_score_strings() {
        let found = parse(r#"{"vulns":[{"id":"GHSA-c","severity":[{"score":"7.5"}]}]}"#);
        assert_eq!(
            found[0].cvss_vector, None,
            "a bare number is not a vector — do not present it as one"
        );
    }

    /// A CVSS v2 score is a bare `AV:N/…` with no `CVSS:` prefix, and OSV lists
    /// it before the v3 vector. Checking only the first entry lost the v3 one.
    #[test]
    fn should_find_a_cvss_vector_behind_a_non_vector_score() {
        let found = parse(
            r#"{"vulns":[{
                "id":"GHSA-i",
                "severity":[
                    {"type":"CVSS_V2","score":"AV:N/AC:L/Au:N/C:P/I:P/A:P"},
                    {"type":"CVSS_V3","score":"CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H"}
                ]
            }]}"#,
        );
        assert_eq!(
            found[0].cvss_vector.as_deref(),
            Some("CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H")
        );
    }

    #[test]
    fn should_prefer_the_highest_cvss_version() {
        let found = parse(
            r#"{"vulns":[{
                "id":"GHSA-j",
                "severity":[
                    {"type":"CVSS_V3","score":"CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H"},
                    {"type":"CVSS_V4","score":"CVSS:4.0/AV:N/AC:L/AT:N/PR:N/UI:N/VC:H/VI:H/VA:H"}
                ]
            }]}"#,
        );
        assert_eq!(
            found[0].cvss_vector.as_deref(),
            Some("CVSS:4.0/AV:N/AC:L/AT:N/PR:N/UI:N/VC:H/VI:H/VA:H"),
            "v4 is the scoring the database maintains; v3 stays for compatibility"
        );
    }

    /// A withdrawn entry was retracted upstream. osv.dev filters these, but a
    /// self-hosted mirror need not, and a retracted advisory must never be
    /// presented as a live one.
    #[test]
    fn should_drop_withdrawn_advisories() {
        let found = parse(
            r#"{"vulns":[
                {"id":"GHSA-withdrawn","withdrawn":"2025-01-01T00:00:00Z"},
                {"id":"GHSA-live"}
            ]}"#,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "GHSA-live");
    }

    #[test]
    fn should_fall_back_from_summary_to_first_line_of_details() {
        let found = parse(
            r#"{"vulns":[{
                "id":"GHSA-d",
                "details":"\n\n## Impact\n---\nRemote code execution via crafted input.\nMore text."
            }]}"#,
        );
        assert_eq!(
            found[0].summary.as_deref(),
            Some("Remote code execution via crafted input."),
            "a heading is structure, not a summary a reader can act on"
        );
    }

    #[test]
    fn should_truncate_an_overlong_summary() {
        let long = "x".repeat(MAX_SUMMARY_CHARS + 50);
        let found = parse(&format!(
            r#"{{"vulns":[{{"id":"GHSA-k","summary":"{long}"}}]}}"#
        ));
        let summary = found[0].summary.as_deref().expect("summary");
        assert_eq!(summary.chars().count(), MAX_SUMMARY_CHARS + 1);
        assert!(summary.ends_with('…'));
    }

    #[test]
    fn should_prefer_summary_over_details() {
        let found =
            parse(r#"{"vulns":[{"id":"GHSA-e","summary":"Short","details":"Long details"}]}"#);
        assert_eq!(found[0].summary.as_deref(), Some("Short"));
    }

    #[test]
    fn should_build_canonical_osv_url() {
        let found = parse(r#"{"vulns":[{"id":"GHSA-f"}]}"#);
        assert_eq!(
            found[0].url.as_deref(),
            Some("https://osv.dev/vulnerability/GHSA-f")
        );
    }

    #[test]
    fn should_tolerate_unknown_fields() {
        // OSV adds fields over time; a new one must not break enrichment.
        let found = parse(
            r#"{"vulns":[{"id":"GHSA-g","published":"2025-01-01T00:00:00Z","affected":[]}],
                "next_page_token":"abc"}"#,
        );
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn should_drop_blank_summaries() {
        let found = parse(r#"{"vulns":[{"id":"GHSA-h","summary":"   "}]}"#);
        assert_eq!(found[0].summary, None);
    }
}
