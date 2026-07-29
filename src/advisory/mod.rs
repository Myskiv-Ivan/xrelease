//! Security-advisory enrichment for released versions.
//!
//! A release notification is far more actionable when it says *why* upgrading
//! matters. This module maps a released package version onto known advisories
//! (CVE / GHSA / RUSTSEC / …) via [`osv`].
//!
//! # Failure semantics
//!
//! Enrichment is **best-effort and must never block delivery**. Every entry
//! point returns advisories or an empty list; nothing here returns an error to
//! the delivery path. A timeout, an HTTP error, malformed JSON, or an open
//! breaker all mean "no advisories known", and the notification goes out
//! unenriched. That is the whole reason this is a separate module rather than an
//! inline call — the outbox's never-lose-a-notification guarantee must
//! survive an OSV outage.
//!
//! # "No advisories" vs. "never checked"
//!
//! [`AdvisoryLookup::lookup`] returns a [`LookupOutcome`], not a bare `Vec`,
//! because an empty result is ambiguous on its own: it means either "OSV was
//! asked and confirmed this version clean" or "OSV was not actually asked"
//! (disabled, no ecosystem, breaker open, network failure). Callers persist a
//! [`crate::store::Store::record_advisory_check`] row only when
//! `outcome.verified` — that distinction is what lets the source-detail
//! backfill (`src/api/observability.rs`) make forward progress through a
//! source's history instead of re-confirming the same already-clean releases
//! on every page load forever.

pub mod osv;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::breaker::Breaker;
use crate::config::AdvisoryConfig;
use crate::sources::PackageRegistry;

/// Severity label, as reported by the advisory database.
///
/// Deliberately *not* computed from a CVSS vector: scoring CVSS correctly is a
/// real algorithm, and a half-implementation that mis-ranks severity is worse
/// than reporting none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Low,
    Moderate,
    High,
    Critical,
}

impl Severity {
    /// Parse the labels advisory databases actually emit.
    ///
    /// GHSA uses `LOW|MODERATE|HIGH|CRITICAL`; RUSTSEC and some others use
    /// `MEDIUM` for the same tier as `MODERATE`.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_uppercase().as_str() {
            "LOW" => Some(Self::Low),
            "MODERATE" | "MEDIUM" => Some(Self::Moderate),
            "HIGH" => Some(Self::High),
            "CRITICAL" => Some(Self::Critical),
            _ => None,
        }
    }

    /// Stable lowercase label for payloads and templates.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Moderate => "moderate",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

/// One advisory affecting a released version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advisory {
    /// Primary database id (`GHSA-…`, `RUSTSEC-…`, `PYSEC-…`).
    pub id: String,
    /// Preferred human-facing id: a `CVE-…` alias when present, else [`Self::id`].
    ///
    /// Readers recognise CVE numbers; database-native ids they mostly do not.
    pub display_id: String,
    /// One-line summary, when the database provides one.
    pub summary: Option<String>,
    /// Severity label when explicitly stated by the database.
    pub severity: Option<Severity>,
    /// Raw CVSS vector string, carried verbatim when present.
    ///
    /// Populated even when [`Self::severity`] is `None`, so a reader can still
    /// judge impact without this code pretending to score it.
    pub cvss_vector: Option<String>,
    /// Canonical advisory URL.
    pub url: Option<String>,
}

impl Advisory {
    /// Highest severity across a set, when any is known.
    #[must_use]
    pub fn max_severity(advisories: &[Self]) -> Option<Severity> {
        advisories.iter().filter_map(|a| a.severity).max()
    }
}

/// An OSV ecosystem identifier.
///
/// Only registries OSV actually indexes are representable, so an unsupported
/// source cannot accidentally be queried with a wrong ecosystem string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ecosystem(&'static str);

impl Ecosystem {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }

    /// Map a package registry onto its OSV ecosystem, when one exists.
    ///
    /// `Cpan` has no OSV ecosystem, and Git forges / container registries /
    /// feeds are not package coordinates at all — those return `None` and are
    /// never queried.
    #[must_use]
    pub const fn for_registry(registry: PackageRegistry) -> Option<Self> {
        match registry {
            PackageRegistry::Pypi => Some(Self("PyPI")),
            // Yarn serves npm-compatible metadata from the same package
            // namespace, so advisories are indexed under `npm`.
            PackageRegistry::Npm | PackageRegistry::Yarn => Some(Self("npm")),
            PackageRegistry::Cargo => Some(Self("crates.io")),
            PackageRegistry::Maven => Some(Self("Maven")),
            PackageRegistry::Nuget => Some(Self("NuGet")),
            PackageRegistry::Hex => Some(Self("Hex")),
            PackageRegistry::Rubygems => Some(Self("RubyGems")),
            PackageRegistry::Packagist => Some(Self("Packagist")),
            PackageRegistry::Cpan => None,
        }
    }
}

/// Recover a package coordinate from a stored source id.
///
/// At delivery time only the `source_id` string survives — there is no live
/// [`crate::sources::PackageSource`] — so the registry and package name are
/// parsed back out of it. Returns `None` for any non-package source.
///
/// Splits on the *first* `:` after the optional `{org}::` prefix, because Maven
/// names are themselves `groupId:artifactId` and must stay intact (that is also
/// exactly the form OSV expects for the Maven ecosystem).
///
/// Borrows the name out of `source_id` rather than allocating: the caller
/// already owns the id for the duration of the lookup.
#[must_use]
pub fn coordinate_from_source_id(source_id: &str) -> Option<(PackageRegistry, &str)> {
    let bare = source_id
        .split_once(crate::config::ORGANIZATION_SEP)
        .map_or(source_id, |(_, rest)| rest);
    let (kind, name) = bare.split_once(':')?;
    let registry = PackageRegistry::from_kind(kind)?;
    let name = name.trim();
    (!name.is_empty()).then_some((registry, name))
}

/// Resolve a package coordinate for advisory lookup.
///
/// Prefers the registry parsed from [`coordinate_from_source_id`]. When that
/// fails (legacy multi-org ids like `platform::package:axios`, or a fully
/// custom id), falls back to the live source `kind` (machine id or kind
/// label). The package name prefers an explicit hint; otherwise the name
/// embedded in a well-formed source id, or a legacy `package:` id.
#[must_use]
pub fn coordinate_from_source<'a>(
    source_id: &'a str,
    kind: &str,
    package_name: &'a str,
) -> Option<(PackageRegistry, &'a str)> {
    let name_hint = package_name.trim();
    if let Some((registry, name_from_id)) = coordinate_from_source_id(source_id) {
        let name = if name_hint.is_empty() {
            name_from_id
        } else {
            name_hint
        };
        return (!name.is_empty()).then_some((registry, name));
    }
    let registry =
        PackageRegistry::from_kind(kind).or_else(|| PackageRegistry::from_kind_label(kind))?;
    let name = if name_hint.is_empty() {
        legacy_shared_package_name(source_id)?
    } else {
        name_hint
    };
    (!name.is_empty()).then_some((registry, name))
}

/// Name from a legacy multi-org id `…::package:{name}` / `package:{name}`.
fn legacy_shared_package_name(source_id: &str) -> Option<&str> {
    let bare = source_id
        .split_once(crate::config::ORGANIZATION_SEP)
        .map_or(source_id, |(_, rest)| rest);
    bare.strip_prefix("package:")
        .filter(|name| !name.is_empty())
}

/// Most advisories spelled out in one notification body.
///
/// Chat sinks reject oversized messages — Telegram's `sendMessage` caps a text
/// at 4 096 characters — and [`crate::pipeline::build_event`] already spends up
/// to ~3 000 of those on the changelog. An unbounded advisory block could push
/// a message past that limit and turn enrichment into a *delivery failure*, the
/// one thing this module promises it can never cause. A badly-vulnerable
/// version with dozens of CVEs therefore lists the first few and counts the
/// rest; the count and the highest severity in the header stay exact.
const MAX_RENDERED_ADVISORIES: usize = 8;

/// Render advisories as a Markdown block to append to a notification body.
///
/// Empty input yields an empty string, so callers can append unconditionally.
#[must_use]
pub fn render_markdown(advisories: &[Advisory]) -> String {
    if advisories.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n\n---\n**Security advisories** (");
    out.push_str(&advisories.len().to_string());
    if let Some(max) = Advisory::max_severity(advisories) {
        out.push_str(", highest: ");
        out.push_str(max.as_str());
    }
    out.push_str(")\n");

    for advisory in advisories.iter().take(MAX_RENDERED_ADVISORIES) {
        out.push_str("\n- **");
        out.push_str(&advisory.display_id);
        out.push_str("**");
        if let Some(severity) = advisory.severity {
            out.push_str(" (");
            out.push_str(severity.as_str());
            out.push(')');
        }
        if let Some(summary) = &advisory.summary {
            out.push_str(" — ");
            out.push_str(summary);
        }
        // Only shown when no label was published — the vector is the fallback
        // signal, not a second copy of the severity.
        if advisory.severity.is_none() {
            if let Some(vector) = &advisory.cvss_vector {
                out.push_str(" — `");
                out.push_str(vector);
                out.push('`');
            }
        }
        if let Some(url) = &advisory.url {
            out.push_str("\n  ");
            out.push_str(url);
        }
    }
    let omitted = advisories.len().saturating_sub(MAX_RENDERED_ADVISORIES);
    if omitted > 0 {
        out.push_str("\n- …and ");
        out.push_str(&omitted.to_string());
        out.push_str(" more");
    }
    out
}

/// Result of one [`AdvisoryLookup::lookup`] call.
///
/// Bundles `advisories` with whether that answer is authoritative. Read
/// [`Self::advisories`] alone to render or attach findings — an unverified
/// empty result behaves exactly like a verified one there, by design: the
/// delivery path must render "nothing found" regardless of why. Only code
/// deciding *whether to remember* the check (persisting
/// [`crate::store::Store::record_advisory_check`]) needs [`Self::verified`].
#[derive(Debug, Clone, Default)]
pub struct LookupOutcome {
    /// Known advisories. Empty both when OSV confirmed none and when the
    /// lookup did not actually run — see [`Self::verified`].
    pub advisories: Vec<Advisory>,
    /// Whether `advisories` is a real OSV answer — a fresh response or a
    /// cached one — rather than a skip (disabled, no ecosystem, blank input,
    /// open breaker) or a transport/decode failure.
    pub verified: bool,
}

impl LookupOutcome {
    /// A real OSV answer — fresh or replayed from cache — whether or not it
    /// found anything.
    fn from_verified_answer(advisories: Vec<Advisory>) -> Self {
        Self {
            advisories,
            verified: true,
        }
    }
}

/// Cache + breaker state shared across lookups.
///
/// One instance lives on the engine; `lookup` takes `&self` so concurrent
/// deliveries share both the cache and the breaker.
#[derive(Debug)]
pub struct AdvisoryLookup {
    config: AdvisoryConfig,
    http: reqwest::Client,
    state: Mutex<LookupState>,
}

#[derive(Debug)]
struct LookupState {
    cache: HashMap<CacheKey, CacheEntry>,
    breaker: Breaker,
}

type CacheKey = (&'static str, String, String);

/// Cache ceiling. Entries are only *ignored* once stale, never dropped by
/// reading, so without a bound the map would grow for the process lifetime —
/// one entry per `(ecosystem, package, version)` ever delivered. At the cap,
/// stale entries are swept and, failing that, the map is cleared: degrading to
/// re-querying is acceptable, leaking is not.
const CACHE_MAX_ENTRIES: usize = 4_096;

#[derive(Debug, Clone)]
struct CacheEntry {
    advisories: Vec<Advisory>,
    stored_at: Instant,
}

impl AdvisoryLookup {
    /// Build a lookup for this configuration, sharing the engine's HTTP client.
    ///
    /// The shared client brings the configured User-Agent and connection pool;
    /// the per-lookup deadline is applied per *request* rather than by building a
    /// second client. A separate client would have needed a fallible build, and
    /// falling back to `Client::default()` there would have silently produced a
    /// client with **no timeout at all** — a hung OSV could then stall a delivery
    /// indefinitely, the exact failure this design forbids.
    #[must_use]
    pub fn new(config: AdvisoryConfig, http: reqwest::Client) -> Self {
        Self {
            config,
            http,
            state: Mutex::new(LookupState {
                cache: HashMap::new(),
                breaker: Breaker::new(),
            }),
        }
    }

    /// Per-request deadline for one OSV call.
    ///
    /// Floored at one second: `timeout_secs = 0` would hand reqwest a zero
    /// deadline, so every lookup fails instantly and then trips the breaker —
    /// enrichment silently dead with no error an operator could act on. The
    /// documented way to switch this off is `enabled = false` (or a blank
    /// endpoint), not a zero timeout.
    fn timeout(&self) -> Duration {
        Duration::from_secs(u64::from(self.config.timeout_secs).max(1))
    }

    /// Consecutive failures tolerated before OSV is skipped.
    ///
    /// Floored at one for the same reason: `Breaker::record` compares
    /// `failures >= threshold`, so a zero threshold is not "never open" but
    /// "open on the first hiccup".
    fn breaker_threshold(&self) -> u32 {
        self.config.breaker_threshold.max(1)
    }

    /// Whether lookups are configured to run at all.
    #[must_use]
    pub fn active(&self) -> bool {
        self.config.active()
    }

    /// Interval between background sweep rounds, or `None` when the sweep is
    /// switched off (see [`AdvisoryConfig::sweep_active`]).
    #[must_use]
    pub fn sweep_interval(&self) -> Option<Duration> {
        self.config
            .sweep_active()
            .then(|| Duration::from_secs(u64::from(self.config.sweep_interval_secs)))
    }

    /// Versions looked up per source per sweep round.
    #[must_use]
    pub fn sweep_batch(&self) -> usize {
        self.config.sweep_batch as usize
    }

    /// Advisories for one released package version.
    ///
    /// Returns an empty, unverified [`LookupOutcome`] — never an error — when
    /// enrichment is disabled, the registry has no OSV ecosystem, the breaker
    /// is open, or the query fails.
    pub async fn lookup(
        &self,
        registry: PackageRegistry,
        name: &str,
        version: &str,
    ) -> LookupOutcome {
        if !self.config.active() {
            return LookupOutcome::default();
        }
        let Some(ecosystem) = Ecosystem::for_registry(registry) else {
            return LookupOutcome::default();
        };
        if name.trim().is_empty() || version.trim().is_empty() {
            return LookupOutcome::default();
        }

        let key = (
            ecosystem.as_str(),
            name.trim().to_owned(),
            version.trim().to_owned(),
        );
        if let Some(hit) = self.cached(&key) {
            // A cache hit is a real, previously-verified OSV answer replayed
            // without a fresh round-trip — not a lesser or provisional one.
            return LookupOutcome::from_verified_answer(hit);
        }
        if !self.breaker_allows() {
            tracing::debug!(
                ecosystem = ecosystem.as_str(),
                package = name,
                "advisory lookup skipped — breaker open"
            );
            return LookupOutcome::default();
        }

        let outcome = osv::query(
            &self.http,
            self.config.endpoint_base(),
            self.timeout(),
            ecosystem,
            &key.1,
            &key.2,
        )
        .await;

        match outcome {
            Ok(advisories) => {
                self.record_success(key, advisories.clone());
                LookupOutcome::from_verified_answer(advisories)
            }
            Err(err) => {
                // Warn, never propagate: the notification still goes out.
                tracing::warn!(
                    ecosystem = ecosystem.as_str(),
                    package = %key.1,
                    version = %key.2,
                    error = %err,
                    "advisory lookup failed — delivering without enrichment"
                );
                self.record_failure();
                LookupOutcome::default()
            }
        }
    }

    fn cached(&self, key: &CacheKey) -> Option<Vec<Advisory>> {
        if self.config.cache_ttl_secs == 0 {
            return None;
        }
        let ttl = Duration::from_secs(u64::from(self.config.cache_ttl_secs));
        let state = self.lock();
        let entry = state.cache.get(key)?;
        (entry.stored_at.elapsed() < ttl).then(|| entry.advisories.clone())
    }

    fn breaker_allows(&self) -> bool {
        self.lock().breaker.allows(Instant::now())
    }

    fn record_success(&self, key: CacheKey, advisories: Vec<Advisory>) {
        let ttl = self.config.cache_ttl_secs;
        let mut state = self.lock();
        state
            .breaker
            .record(Instant::now(), true, 0, Duration::ZERO);
        if ttl == 0 {
            return;
        }
        if state.cache.len() >= CACHE_MAX_ENTRIES {
            let ttl = Duration::from_secs(u64::from(ttl));
            state
                .cache
                .retain(|_, entry| entry.stored_at.elapsed() < ttl);
            if state.cache.len() >= CACHE_MAX_ENTRIES {
                // Every entry is still fresh — the working set genuinely exceeds
                // the cap. Drop it all rather than grow past the bound.
                state.cache.clear();
            }
        }
        state.cache.insert(
            key,
            CacheEntry {
                advisories,
                stored_at: Instant::now(),
            },
        );
    }

    fn record_failure(&self) {
        let threshold = self.breaker_threshold();
        let cooldown = Duration::from_secs(u64::from(self.config.breaker_cooldown_secs));
        let mut state = self.lock();
        if state
            .breaker
            .record(Instant::now(), false, threshold, cooldown)
        {
            tracing::warn!(
                cooldown_secs = self.config.breaker_cooldown_secs,
                "advisory lookups suspended — OSV failed {threshold} times in a row"
            );
        }
    }

    /// A panicking caller must not wedge enrichment for the process lifetime;
    /// the state is a cache plus a breaker, so recovering is safe.
    fn lock(&self) -> std::sync::MutexGuard<'_, LookupState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn advisory(id: &str, severity: Option<Severity>) -> Advisory {
        Advisory {
            id: id.to_owned(),
            display_id: id.to_owned(),
            summary: None,
            severity,
            cvss_vector: None,
            url: None,
        }
    }

    #[test]
    fn severity_should_parse_ghsa_and_rustsec_labels() {
        assert_eq!(Severity::parse("CRITICAL"), Some(Severity::Critical));
        assert_eq!(Severity::parse("moderate"), Some(Severity::Moderate));
        // RUSTSEC says MEDIUM where GHSA says MODERATE — same tier.
        assert_eq!(Severity::parse("medium"), Some(Severity::Moderate));
        assert_eq!(Severity::parse(" High "), Some(Severity::High));
        assert_eq!(Severity::parse("catastrophic"), None);
    }

    #[test]
    fn severity_should_order_low_to_critical() {
        assert!(Severity::Critical > Severity::High);
        assert!(Severity::High > Severity::Moderate);
        assert!(Severity::Moderate > Severity::Low);
    }

    #[test]
    fn max_severity_should_ignore_unscored_advisories() {
        let set = [
            advisory("GHSA-a", None),
            advisory("GHSA-b", Some(Severity::Moderate)),
            advisory("GHSA-c", Some(Severity::Critical)),
        ];
        assert_eq!(Advisory::max_severity(&set), Some(Severity::Critical));
    }

    #[test]
    fn max_severity_should_be_none_when_nothing_is_scored() {
        let set = [advisory("GHSA-a", None)];
        assert_eq!(Advisory::max_severity(&set), None);
    }

    #[test]
    fn ecosystem_should_map_yarn_onto_npm() {
        assert_eq!(
            Ecosystem::for_registry(PackageRegistry::Yarn).map(Ecosystem::as_str),
            Some("npm"),
            "Yarn serves npm metadata, so advisories are indexed under npm"
        );
        assert_eq!(
            Ecosystem::for_registry(PackageRegistry::Npm).map(Ecosystem::as_str),
            Some("npm")
        );
    }

    #[test]
    fn ecosystem_should_use_osv_canonical_names() {
        assert_eq!(
            Ecosystem::for_registry(PackageRegistry::Cargo).map(Ecosystem::as_str),
            Some("crates.io")
        );
        assert_eq!(
            Ecosystem::for_registry(PackageRegistry::Pypi).map(Ecosystem::as_str),
            Some("PyPI")
        );
        assert_eq!(
            Ecosystem::for_registry(PackageRegistry::Rubygems).map(Ecosystem::as_str),
            Some("RubyGems")
        );
    }

    #[test]
    fn ecosystem_should_reject_cpan() {
        assert!(
            Ecosystem::for_registry(PackageRegistry::Cpan).is_none(),
            "OSV has no CPAN ecosystem — must not be queried with a guess"
        );
    }

    #[test]
    fn coordinate_should_parse_plain_package_ids() {
        assert_eq!(
            coordinate_from_source_id("cargo:serde"),
            Some((PackageRegistry::Cargo, "serde"))
        );
        assert_eq!(
            coordinate_from_source_id("pypi:requests"),
            Some((PackageRegistry::Pypi, "requests"))
        );
    }

    #[test]
    fn coordinate_should_keep_maven_group_and_artifact_together() {
        assert_eq!(
            coordinate_from_source_id("maven:com.google.guava:guava"),
            Some((PackageRegistry::Maven, "com.google.guava:guava")),
            "OSV expects groupId:artifactId as the Maven package name"
        );
    }

    #[test]
    fn coordinate_should_strip_the_organization_prefix() {
        assert_eq!(
            coordinate_from_source_id("platform::cargo:serde"),
            Some((PackageRegistry::Cargo, "serde"))
        );
        assert_eq!(
            coordinate_from_source_id("security::maven:org.acme:lib"),
            Some((PackageRegistry::Maven, "org.acme:lib"))
        );
    }

    #[test]
    fn coordinate_should_reject_non_package_sources() {
        assert_eq!(coordinate_from_source_id("github:tokio-rs/tokio"), None);
        assert_eq!(coordinate_from_source_id("docker:library/nginx"), None);
        assert_eq!(coordinate_from_source_id("feed:https://x/y"), None);
        assert_eq!(coordinate_from_source_id("no-colon"), None);
        assert_eq!(coordinate_from_source_id("cargo:"), None);
    }

    #[test]
    fn coordinate_should_reject_legacy_shared_package_prefix() {
        assert_eq!(
            coordinate_from_source_id("platform::package:axios"),
            None,
            "legacy shared package: prefix is not a registry kind"
        );
    }

    #[test]
    fn coordinate_from_source_should_fall_back_to_kind_and_name() {
        assert_eq!(
            coordinate_from_source("platform::package:axios", "npm", "axios"),
            Some((PackageRegistry::Npm, "axios"))
        );
        assert_eq!(
            coordinate_from_source("platform::package:axios", "npm", ""),
            Some((PackageRegistry::Npm, "axios")),
            "legacy package: id still carries the package name"
        );
    }

    #[test]
    fn coordinate_from_source_should_accept_kind_label_from_outbox() {
        assert_eq!(
            coordinate_from_source("platform::package:serde", "crates.io", "serde"),
            Some((PackageRegistry::Cargo, "serde"))
        );
    }

    #[test]
    fn coordinate_from_source_should_use_id_name_when_hint_empty() {
        assert_eq!(
            coordinate_from_source("platform::npm:axios", "npm", ""),
            Some((PackageRegistry::Npm, "axios"))
        );
    }

    #[test]
    fn render_markdown_should_be_empty_for_no_advisories() {
        assert!(render_markdown(&[]).is_empty());
    }

    #[test]
    fn render_markdown_should_lead_with_count_and_highest_severity() {
        let rendered = render_markdown(&[
            advisory("GHSA-a", Some(Severity::Low)),
            advisory("GHSA-b", Some(Severity::Critical)),
        ]);
        assert!(rendered.contains("**Security advisories** (2, highest: critical)"));
    }

    #[test]
    fn render_markdown_should_omit_severity_when_unscored() {
        let rendered = render_markdown(&[advisory("GHSA-a", None)]);
        assert!(
            rendered.contains("**Security advisories** (1)"),
            "no severity known — must not claim one: {rendered}"
        );
    }

    #[test]
    fn render_markdown_should_show_cvss_vector_only_without_a_label() {
        let with_label = Advisory {
            severity: Some(Severity::High),
            cvss_vector: Some("CVSS:3.1/AV:N".into()),
            ..advisory("GHSA-a", Some(Severity::High))
        };
        let rendered = render_markdown(&[with_label]);
        assert!(rendered.contains("(high)"));
        assert!(
            !rendered.contains("CVSS:3.1"),
            "the vector is a fallback, not a duplicate of the label: {rendered}"
        );

        let unlabelled = Advisory {
            cvss_vector: Some("CVSS:3.1/AV:N".into()),
            ..advisory("GHSA-b", None)
        };
        let rendered = render_markdown(&[unlabelled]);
        assert!(rendered.contains("CVSS:3.1/AV:N"));
    }

    /// Enrichment must never be able to fail a delivery, and an unbounded block
    /// appended to an already-long body is exactly how it could: Telegram
    /// rejects a `sendMessage` text over 4 096 characters.
    #[test]
    fn render_markdown_should_cap_how_many_advisories_it_spells_out() {
        let many: Vec<Advisory> = (0..MAX_RENDERED_ADVISORIES + 5)
            .map(|n| advisory(&format!("GHSA-{n}"), Some(Severity::High)))
            .collect();
        let rendered = render_markdown(&many);

        assert!(
            rendered.contains(&format!("**Security advisories** ({}", many.len())),
            "the header count stays exact: {rendered}"
        );
        assert_eq!(
            rendered.matches("GHSA-").count(),
            MAX_RENDERED_ADVISORIES,
            "only the first few are spelled out: {rendered}"
        );
        assert!(rendered.contains("…and 5 more"), "{rendered}");
    }

    #[test]
    fn timeout_should_never_be_zero() {
        // A zero deadline would fail every request instantly and then trip the
        // breaker — enrichment silently dead with nothing to act on.
        let lookup = AdvisoryLookup::new(
            AdvisoryConfig {
                enabled: true,
                timeout_secs: 0,
                ..AdvisoryConfig::default()
            },
            reqwest::Client::new(),
        );
        assert_eq!(lookup.timeout(), Duration::from_secs(1));
    }

    #[test]
    fn breaker_threshold_should_never_be_zero() {
        // `failures >= 0` is always true, so a zero threshold opens on the very
        // first hiccup rather than disabling the breaker.
        let lookup = AdvisoryLookup::new(
            AdvisoryConfig {
                enabled: true,
                breaker_threshold: 0,
                ..AdvisoryConfig::default()
            },
            reqwest::Client::new(),
        );
        assert_eq!(lookup.breaker_threshold(), 1);
    }

    #[tokio::test]
    async fn lookup_should_be_inert_when_disabled() {
        let lookup = AdvisoryLookup::new(AdvisoryConfig::default(), reqwest::Client::new());
        assert!(!lookup.active());
        // Endpoint is unreachable in tests; a disabled lookup must not call it.
        let found = lookup
            .lookup(PackageRegistry::Cargo, "serde", "1.0.0")
            .await;
        assert!(found.advisories.is_empty());
        assert!(
            !found.verified,
            "a skip must not be mistaken for a confirmed-clean answer"
        );
    }

    #[tokio::test]
    async fn lookup_should_skip_registries_without_an_ecosystem() {
        let lookup = AdvisoryLookup::new(
            AdvisoryConfig {
                enabled: true,
                endpoint: "http://127.0.0.1:1".into(),
                ..AdvisoryConfig::default()
            },
            reqwest::Client::new(),
        );
        // Would fail loudly if it attempted the unreachable endpoint.
        let found = lookup
            .lookup(PackageRegistry::Cpan, "Some::Module", "1.0")
            .await;
        assert!(found.advisories.is_empty());
        assert!(!found.verified);
    }

    #[tokio::test]
    async fn lookup_should_open_breaker_after_threshold_failures() {
        let lookup = AdvisoryLookup::new(
            AdvisoryConfig {
                enabled: true,
                // Reserved-discard port: connection fails fast and deterministically.
                endpoint: "http://127.0.0.1:1".into(),
                timeout_secs: 1,
                cache_ttl_secs: 0,
                breaker_threshold: 2,
                breaker_cooldown_secs: 300,
                ..AdvisoryConfig::default()
            },
            reqwest::Client::new(),
        );

        for _ in 0..2 {
            let found = lookup
                .lookup(PackageRegistry::Cargo, "serde", "1.0.0")
                .await;
            assert!(found.advisories.is_empty());
            assert!(
                !found.verified,
                "a transport failure is not a verified answer"
            );
        }
        assert!(
            !lookup.breaker_allows(),
            "breaker must open so a broken OSV stops being dialled per delivery"
        );
    }

    #[tokio::test]
    async fn breaker_skip_should_also_be_unverified() {
        // Same failure mode as above, but exercising the *open-breaker skip*
        // branch specifically (no network attempt at all on the 3rd call),
        // not just the two live failures that open it.
        let lookup = AdvisoryLookup::new(
            AdvisoryConfig {
                enabled: true,
                endpoint: "http://127.0.0.1:1".into(),
                timeout_secs: 1,
                cache_ttl_secs: 0,
                breaker_threshold: 1,
                breaker_cooldown_secs: 300,
                ..AdvisoryConfig::default()
            },
            reqwest::Client::new(),
        );
        lookup
            .lookup(PackageRegistry::Cargo, "serde", "1.0.0")
            .await;
        assert!(!lookup.breaker_allows());

        let skipped = lookup
            .lookup(PackageRegistry::Cargo, "serde", "1.0.0")
            .await;
        assert!(skipped.advisories.is_empty());
        assert!(
            !skipped.verified,
            "a breaker-skipped lookup never asked OSV — must not read as confirmed clean"
        );
    }

    #[tokio::test]
    async fn cache_should_serve_repeat_lookups_without_network() {
        let lookup = AdvisoryLookup::new(
            AdvisoryConfig {
                enabled: true,
                endpoint: "http://127.0.0.1:1".into(),
                ..AdvisoryConfig::default()
            },
            reqwest::Client::new(),
        );
        let key = ("crates.io", "serde".to_owned(), "1.0.0".to_owned());
        lookup.record_success(key, vec![advisory("RUSTSEC-0001", Some(Severity::High))]);

        // Endpoint is unreachable, so a non-empty result proves the cache served it.
        let found = lookup
            .lookup(PackageRegistry::Cargo, "serde", "1.0.0")
            .await;
        assert_eq!(found.advisories.len(), 1);
        assert_eq!(found.advisories[0].id, "RUSTSEC-0001");
        assert!(
            found.verified,
            "a cache hit replays a real prior OSV answer"
        );
    }

    /// A verified query that genuinely found nothing must be distinguishable
    /// from a skip — this is the exact distinction the backfill's forward
    /// progress depends on (see the module doc's "No advisories" vs. "never
    /// checked" section).
    #[tokio::test]
    async fn a_clean_live_response_should_be_verified() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/v1/query"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "vulns": []
                })),
            )
            .mount(&server)
            .await;

        let lookup = AdvisoryLookup::new(
            AdvisoryConfig {
                enabled: true,
                endpoint: server.uri(),
                cache_ttl_secs: 0,
                ..AdvisoryConfig::default()
            },
            reqwest::Client::new(),
        );

        let found = lookup
            .lookup(PackageRegistry::Cargo, "serde", "1.0.0")
            .await;
        assert!(found.advisories.is_empty());
        assert!(
            found.verified,
            "OSV genuinely answered 'nothing known' — that is a real, checked result"
        );
    }

    #[test]
    fn cache_should_stay_within_its_bound() {
        // Entries are only ignored once stale, never dropped by reading, so
        // without eviction this map would grow for the process lifetime.
        let lookup = AdvisoryLookup::new(
            AdvisoryConfig {
                enabled: true,
                ..AdvisoryConfig::default()
            },
            reqwest::Client::new(),
        );
        for n in 0..(CACHE_MAX_ENTRIES + 50) {
            lookup.record_success(
                ("crates.io", format!("crate-{n}"), "1.0.0".to_owned()),
                Vec::new(),
            );
        }
        assert!(
            lookup.lock().cache.len() <= CACHE_MAX_ENTRIES,
            "cache must not exceed its bound"
        );
    }

    #[tokio::test]
    async fn cache_should_be_bypassed_when_ttl_is_zero() {
        let lookup = AdvisoryLookup::new(
            AdvisoryConfig {
                enabled: true,
                endpoint: "http://127.0.0.1:1".into(),
                timeout_secs: 1,
                cache_ttl_secs: 0,
                ..AdvisoryConfig::default()
            },
            reqwest::Client::new(),
        );
        let key = ("crates.io", "serde".to_owned(), "1.0.0".to_owned());
        lookup.record_success(key, vec![advisory("RUSTSEC-0001", None)]);

        let found = lookup
            .lookup(PackageRegistry::Cargo, "serde", "1.0.0")
            .await;
        assert!(
            found.advisories.is_empty(),
            "ttl=0 must not consult the cache"
        );
        assert!(
            !found.verified,
            "the live query behind it hits an unreachable endpoint and fails"
        );
    }
}
