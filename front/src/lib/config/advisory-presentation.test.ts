import { describe, expect, it } from 'vitest';
import type { AdvisoryView, SeenReleaseView } from '$lib/api/types';
import {
	advisoryRowSearchText,
	advisoryRows,
	advisorySearchText,
	allAdvisories,
	bucketCode,
	bucketLabel,
	bucketTone,
	parseSeverity,
	releaseAdvisories,
	SEVERITY_BUCKETS,
	severityBucket,
	severityCounts,
	severityCountsLabel,
	severityTone,
	sortBySeverity,
	sourceSupportsAdvisories
} from './advisory-presentation';

function advisory(id: string, severity?: AdvisoryView['severity']): AdvisoryView {
	return { id, display_id: id, severity: severity ?? null };
}

function release(tag: string, advisories?: AdvisoryView[]): SeenReleaseView {
	return { tag, first_seen_at: '2026-07-28T10:00:00.000Z', advisories };
}

describe('parseSeverity', () => {
	it('accepts the labels the backend emits', () => {
		expect(parseSeverity('critical')).toBe('critical');
		expect(parseSeverity('high')).toBe('high');
		expect(parseSeverity('moderate')).toBe('moderate');
		expect(parseSeverity('low')).toBe('low');
	});

	it('normalizes case and surrounding whitespace', () => {
		expect(parseSeverity(' HIGH ')).toBe('high');
	});

	it('returns null for an unknown tier rather than passing it through', () => {
		// `schema.d.ts` narrows `severity` to the enum, but that is a
		// compile-time guarantee only: the value arrives as parsed JSON, so a
		// backend/spec skew would reach this at runtime. An unstyled badge
		// would be worse than none.
		expect(parseSeverity('catastrophic')).toBeNull();
	});

	it('does not treat inherited Object properties as severities', () => {
		// Guards the `Object.hasOwn` lookup: a plain `in` check would resolve
		// 'toString' / 'constructor' off the prototype chain and admit them as
		// tiers, which then index the rank table as undefined.
		expect(parseSeverity('toString')).toBeNull();
		expect(parseSeverity('constructor')).toBeNull();
		expect(parseSeverity('__proto__')).toBeNull();
	});

	it('treats absent severity as unscored', () => {
		expect(parseSeverity(null)).toBeNull();
		expect(parseSeverity(undefined)).toBeNull();
		expect(parseSeverity('')).toBeNull();
	});
});

describe('severityTone', () => {
	it('maps critical and high to danger', () => {
		expect(severityTone('critical')).toBe('danger');
		expect(severityTone('high')).toBe('danger');
	});

	it('maps moderate to warning', () => {
		expect(severityTone('moderate')).toBe('warning');
	});

	it('keeps low quiet — never success, and never the filled primary chip', () => {
		// `default` renders as a solid primary badge, which made a low-severity
		// CVE louder than the amber `moderate` sitting next to it. `success`
		// would be worse still: green reads as "resolved".
		expect(severityTone('low')).toBe('accent');
		expect(severityTone('low')).not.toBe('success');
		expect(severityTone('low')).not.toBe('default');
	});

	it('descends in urgency across the whole ramp', () => {
		const ramp = (['critical', 'high', 'moderate', 'low'] as const).map(severityTone);
		expect(ramp).toEqual(['danger', 'danger', 'warning', 'accent']);
	});
});

describe('sortBySeverity', () => {
	it('orders most severe first', () => {
		const sorted = sortBySeverity([
			advisory('a', 'moderate'),
			advisory('b', 'critical'),
			advisory('c', 'low')
		]);
		expect(sorted.map((entry) => entry.severity)).toEqual(['critical', 'moderate', 'low']);
	});

	it('sorts unscored last but never drops them', () => {
		const sorted = sortBySeverity([advisory('unscored'), advisory('scored', 'low')]);
		expect(sorted.map((entry) => entry.id)).toEqual(['scored', 'unscored']);
	});

	it('breaks ties on the id readers recognise', () => {
		const sorted = sortBySeverity([
			advisory('CVE-2025-9999', 'high'),
			advisory('CVE-2025-1111', 'high')
		]);
		expect(sorted.map((entry) => entry.display_id)).toEqual([
			'CVE-2025-1111',
			'CVE-2025-9999'
		]);
	});

	it('does not mutate the input array', () => {
		const input = [advisory('a', 'low'), advisory('b', 'critical')];
		const snapshot = input.map((entry) => entry.id);
		sortBySeverity(input);
		expect(input.map((entry) => entry.id)).toEqual(snapshot);
	});
});

describe('releaseAdvisories', () => {
	it('defaults to empty when the field is absent', () => {
		// The list endpoint omits `advisories` entirely (skip_serializing_if),
		// so this must not be conflated with "looked up, found none".
		expect(releaseAdvisories(release('1.0.0'))).toEqual([]);
	});
});

describe('severityBucket', () => {
	it('keeps every scored tier as its own bucket', () => {
		expect(severityBucket('critical')).toBe('critical');
		expect(severityBucket('low')).toBe('low');
	});

	it('collects anything unlabelled into the unscored bucket', () => {
		expect(severityBucket(null)).toBe('unscored');
		expect(severityBucket('catastrophic')).toBe('unscored');
	});
});

describe('bucketCode', () => {
	it('gives each bucket a distinct letter', () => {
		const codes = SEVERITY_BUCKETS.map(bucketCode);
		expect(codes).toEqual(['C', 'H', 'M', 'L', 'I']);
		expect(new Set(codes).size).toBe(codes.length);
	});
});

describe('bucketLabel', () => {
	it('resolves every bucket to a real locale string, not a raw key', () => {
		// A missing key makes `t()` echo the key back — that would render
		// "advisories.severity.high" in the tooltip.
		for (const bucket of SEVERITY_BUCKETS) {
			const label = bucketLabel(bucket);
			expect(label).toBeTruthy();
			expect(label).not.toContain('advisories.');
		}
	});

	it('names the unscored bucket without inventing a tier for it', () => {
		expect(bucketLabel('unscored')).toBe('Unscored');
	});
});

describe('bucketTone', () => {
	it('mutes the unscored bucket rather than styling it as a tier', () => {
		expect(bucketTone('unscored')).toBe('muted');
	});

	it('matches the severity ramp for scored buckets', () => {
		expect(bucketTone('critical')).toBe('danger');
		expect(bucketTone('moderate')).toBe('warning');
	});
});

describe('severityCounts', () => {
	it('groups a set worst-first', () => {
		const counts = severityCounts([
			advisory('a', 'low'),
			advisory('b', 'critical'),
			advisory('c', 'low'),
			advisory('d', 'high')
		]);
		expect(counts).toEqual([
			{ bucket: 'critical', count: 1 },
			{ bucket: 'high', count: 1 },
			{ bucket: 'low', count: 2 }
		]);
	});

	it('omits empty buckets instead of padding the strip with zeros', () => {
		const counts = severityCounts([advisory('a', 'high')]);
		expect(counts).toEqual([{ bucket: 'high', count: 1 }]);
	});

	it('counts unlabelled advisories rather than dropping them', () => {
		expect(severityCounts([advisory('a')])).toEqual([{ bucket: 'unscored', count: 1 }]);
	});

	it('is empty for an empty set', () => {
		expect(severityCounts([])).toEqual([]);
	});
});

describe('severityCountsLabel', () => {
	it('spells the strip out for a tooltip and a screen reader', () => {
		// The strip encodes severity as colour and a letter; this is the third
		// channel, so the meaning survives without either.
		const counts = severityCounts([advisory('a', 'critical'), advisory('b')]);
		expect(severityCountsLabel(counts)).toBe('1 critical, 1 unscored');
	});
});

describe('advisoryRows', () => {
	it('flattens releases, newest release first, worst advisory first within one', () => {
		const rows = advisoryRows([
			release('2.0.0', [advisory('a', 'low'), advisory('b', 'critical')]),
			release('1.0.0', [advisory('c', 'high')])
		]);
		expect(rows.map((row) => `${row.tag}/${row.advisory.id}`)).toEqual([
			'2.0.0/b',
			'2.0.0/a',
			'1.0.0/c'
		]);
	});

	it('skips releases with no advisories', () => {
		expect(advisoryRows([release('1.0.0'), release('2.0.0', [advisory('a')])])).toHaveLength(1);
	});
});

describe('advisoryRowSearchText', () => {
	it('matches on the version as well as both ids and the summary', () => {
		const [row] = advisoryRows([
			release('1.2.3', [
				{
					id: 'GHSA-xxxx',
					display_id: 'CVE-2025-12345',
					severity: 'high',
					summary: 'Heap overflow'
				}
			])
		]);
		const text = advisoryRowSearchText(row);
		expect(text).toContain('1.2.3');
		expect(text).toContain('CVE-2025-12345');
		expect(text).toContain('GHSA-xxxx');
		expect(text).toContain('Heap overflow');
	});
});

describe('allAdvisories', () => {
	it('flattens across releases and tolerates the list payload omitting the field', () => {
		expect(
			allAdvisories([release('1.0.0'), release('2.0.0', [advisory('a'), advisory('b')])])
		).toHaveLength(2);
	});
});

describe('advisorySearchText', () => {
	it('includes the CVE alias, the native id, and the summary', () => {
		const entry = release('1.0.0', [
			{
				id: 'GHSA-xxxx',
				display_id: 'CVE-2025-12345',
				severity: 'high',
				summary: 'Heap overflow'
			}
		]);
		const text = advisorySearchText(entry);
		expect(text).toContain('CVE-2025-12345');
		expect(text).toContain('GHSA-xxxx');
		expect(text).toContain('Heap overflow');
	});

	it('is empty when the release has no advisories', () => {
		expect(advisorySearchText(release('1.0.0'))).toBe('');
	});
});

describe('sourceSupportsAdvisories', () => {
	it('accepts package registries that map onto an OSV ecosystem', () => {
		expect(sourceSupportsAdvisories('pypi')).toBe(true);
		expect(sourceSupportsAdvisories('npm')).toBe(true);
		expect(sourceSupportsAdvisories('yarn')).toBe(true);
		expect(sourceSupportsAdvisories('cargo')).toBe(true);
	});

	it('rejects forges, containers, feeds, and CPAN', () => {
		expect(sourceSupportsAdvisories('github')).toBe(false);
		expect(sourceSupportsAdvisories('docker')).toBe(false);
		expect(sourceSupportsAdvisories('feed')).toBe(false);
		expect(sourceSupportsAdvisories('cpan')).toBe(false);
	});
});
