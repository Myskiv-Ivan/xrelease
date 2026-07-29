import type { AdvisoryView, SeenReleaseView } from '$lib/api/types';
import type { BadgeTone } from '$lib/components/kit/Badge.svelte';
import { t } from '$lib/i18n';

/**
 * Severity tier, derived from the OpenAPI contract rather than redeclared.
 *
 * `AdvisoryView['severity']` is a union because the spec declares an `enum`, so
 * this stays provably in step with the backend's `advisory::Severity`
 * (`src/advisory/mod.rs`). Adding a tier server-side and regenerating
 * `schema.d.ts` then breaks the build here — the `SEVERITY_RANK` record below
 * would be missing a key — instead of silently ranking it as unknown.
 */
export type AdvisorySeverity = NonNullable<AdvisoryView['severity']>;

/**
 * Source kinds that map onto an OSV ecosystem (`src/advisory/mod.rs`).
 * Git forges, container registries, feeds, Artifact Hub, and CPAN are excluded.
 */
const ADVISORY_CAPABLE_KINDS = new Set([
	'pypi',
	'npm',
	'yarn',
	'cargo',
	'maven',
	'nuget',
	'hex',
	'rubygems',
	'packagist'
]);

/** Whether this source kind can ever carry advisory badges. */
export function sourceSupportsAdvisories(kind: string): boolean {
	return ADVISORY_CAPABLE_KINDS.has(kind);
}

/**
 * Rank per tier, weakest first. A `Record` over the derived union, so it is
 * exhaustive by construction.
 */
const SEVERITY_RANK: Record<AdvisorySeverity, number> = {
	low: 0,
	moderate: 1,
	high: 2,
	critical: 3
};

/**
 * Parse the wire value into a known tier.
 *
 * The backend only ever emits a label it explicitly received from the advisory
 * database, but the field is a plain `string` on the wire. An unrecognised
 * value resolves to `null` — treated the same as "no severity stated" — rather
 * than being rendered as an unstyled badge.
 */
export function parseSeverity(value: string | null | undefined): AdvisorySeverity | null {
	if (!value) return null;
	const normalized = value.trim().toLowerCase();
	return Object.hasOwn(SEVERITY_RANK, normalized) ? (normalized as AdvisorySeverity) : null;
}

/**
 * Badge tone for a severity tier — a descending urgency ramp.
 *
 * `low` is a themed `accent` tint, never `default`: the `default` badge variant
 * is a *filled primary* chip, which made a low-severity CVE the loudest thing
 * in the row — louder than the amber `moderate` beside it. Never `success`
 * either: a low-severity CVE is still a vulnerability, and green reads as
 * "resolved".
 */
export function severityTone(severity: AdvisorySeverity | null): BadgeTone {
	switch (severity) {
		case 'critical':
		case 'high':
			return 'danger';
		case 'moderate':
			return 'warning';
		case 'low':
			return 'accent';
		default:
			return 'muted';
	}
}

/**
 * Display bucket for the compact severity column: the four scored tiers plus
 * `unscored` for advisories whose database published no label.
 *
 * `unscored` is a *presentation* bucket, deliberately outside
 * [`AdvisorySeverity`] — that union stays exactly the backend's enum, so a tier
 * added server-side still breaks the build here rather than silently landing in
 * this catch-all.
 */
export type SeverityBucket = AdvisorySeverity | 'unscored';

/** Buckets worst-first — the order the compact column renders them in. */
export const SEVERITY_BUCKETS: readonly SeverityBucket[] = [
	'critical',
	'high',
	'moderate',
	'low',
	'unscored'
];

/**
 * One-letter code per bucket (C / H / M / L / I).
 *
 * Not a locale string: this is a fixed compact notation (the same one Trivy,
 * Grype, and GitHub's severity chips use), and it is always paired with a
 * spelled-out `title`/`aria-label` — so it never has to carry the meaning on
 * its own.
 */
const BUCKET_CODE: Record<SeverityBucket, string> = {
	critical: 'C',
	high: 'H',
	moderate: 'M',
	low: 'L',
	unscored: 'I'
};

/** Which bucket one advisory falls into. */
export function severityBucket(severity: string | null | undefined): SeverityBucket {
	return parseSeverity(severity) ?? 'unscored';
}

/**
 * Numeric rank for table sorting: worst highest, unscored below `low` — an
 * advisory with no stated severity carries the least actionable signal, not
 * zero signal.
 */
export function severityRank(severity: string | null | undefined): number {
	const parsed = parseSeverity(severity);
	return parsed == null ? -1 : SEVERITY_RANK[parsed];
}

/** Sortable columns of the advisories detail table (`order` = API order). */
export type AdvisorySortKey = 'order' | 'severity' | 'tag' | 'id';

/** Compact letter for a bucket. */
export function bucketCode(bucket: SeverityBucket): string {
	return BUCKET_CODE[bucket];
}

/** Badge tone for a bucket. */
export function bucketTone(bucket: SeverityBucket): BadgeTone {
	return bucket === 'unscored' ? 'muted' : severityTone(bucket);
}

/**
 * Spelled-out bucket name, for tooltips and the details page.
 *
 * `unscored` has no entry under `advisories.severity.*` on purpose — that
 * subtree mirrors the backend enum and nothing else.
 */
export function bucketLabel(bucket: SeverityBucket): string {
	return bucket === 'unscored' ? t('advisories.unscored') : t(`advisories.severity.${bucket}`);
}

/** One populated bucket of a set. */
export interface SeverityCount {
	bucket: SeverityBucket;
	count: number;
}

/**
 * Non-empty buckets of a set, worst first.
 *
 * Empty buckets are omitted rather than rendered as `C0 H0 …`: a row of zeros
 * is noise in a table cell, and the absence of a letter already says "none".
 */
export function severityCounts(advisories: readonly AdvisoryView[]): SeverityCount[] {
	const totals = new Map<SeverityBucket, number>();
	for (const advisory of advisories) {
		const bucket = severityBucket(advisory.severity);
		totals.set(bucket, (totals.get(bucket) ?? 0) + 1);
	}
	return SEVERITY_BUCKETS.filter((bucket) => totals.has(bucket)).map((bucket) => ({
		bucket,
		count: totals.get(bucket) ?? 0
	}));
}

/**
 * Spelled-out summary of a count strip — `"2 critical, 1 unscored"`.
 *
 * The strip encodes severity as colour *and* letter; this is the third channel,
 * carried on `title`/`aria-label` so the meaning survives for a screen reader
 * and for anyone who cannot separate the tints.
 */
export function severityCountsLabel(counts: readonly SeverityCount[]): string {
	return counts
		.map(({ bucket, count }) => `${count} ${bucketLabel(bucket).toLowerCase()}`)
		.join(', ');
}

/**
 * Sort order for display: most severe first, then by the id readers recognise.
 *
 * Unscored advisories sort last — they carry the least actionable signal — but
 * are never dropped. Returns a new array; the input is left untouched so the
 * caller's reactive state is not mutated in place.
 */
export function sortBySeverity(advisories: readonly AdvisoryView[]): AdvisoryView[] {
	return [...advisories].sort((left, right) => {
		const leftSeverity = parseSeverity(left.severity);
		const rightSeverity = parseSeverity(right.severity);
		const leftRank = leftSeverity ? SEVERITY_RANK[leftSeverity] : -1;
		const rightRank = rightSeverity ? SEVERITY_RANK[rightSeverity] : -1;
		if (leftRank !== rightRank) return rightRank - leftRank;
		return left.display_id.localeCompare(right.display_id);
	});
}

/**
 * Advisories attached to one seen release.
 *
 * Absent (not empty) on the sources-list endpoint, which omits the field
 * entirely — so this must never be conflated with "looked up, found none".
 */
export function releaseAdvisories(release: Pick<SeenReleaseView, 'advisories'>): AdvisoryView[] {
	return release.advisories ?? [];
}

/** Every advisory across a source's synced releases, for a summary strip. */
export function allAdvisories(releases: readonly SeenReleaseView[]): AdvisoryView[] {
	return releases.flatMap(releaseAdvisories);
}

/** One row of the advisories detail table: an advisory plus the version it hits. */
export interface AdvisoryRow {
	/** Release tag the advisory applies to. */
	tag: string;
	advisory: AdvisoryView;
}

/**
 * Flatten a source's releases into detail rows.
 *
 * Release order is preserved as the API returned it (newest first), and within
 * one release advisories are ordered worst-first — so the top of the table is
 * always "newest version, most urgent finding", which is the thing an operator
 * came to the page for.
 */
export function advisoryRows(releases: readonly SeenReleaseView[]): AdvisoryRow[] {
	return releases.flatMap((release) =>
		sortBySeverity(releaseAdvisories(release)).map((advisory) => ({
			tag: release.tag,
			advisory
		}))
	);
}

/** Searchable text for one detail row (version, both ids, summary). */
export function advisoryRowSearchText(row: AdvisoryRow): string {
	return [row.tag, row.advisory.display_id, row.advisory.id, row.advisory.summary ?? ''].join(' ');
}

/** Searchable text for one release's advisories (ids and summaries). */
export function advisorySearchText(release: Pick<SeenReleaseView, 'advisories'>): string {
	return releaseAdvisories(release)
		.flatMap((advisory) => [advisory.display_id, advisory.id, advisory.summary ?? ''])
		.join(' ');
}
