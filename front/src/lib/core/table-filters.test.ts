import { describe, expect, it } from 'vitest';
import {
	UNTAGGED_FILTER,
	collectTeamFilterOptions,
	matchesSearch,
	normalizeSearchQuery,
	passesSelectFilter,
	passesTeamFilter
} from './table-filters';

describe('normalizeSearchQuery', () => {
	it('trims and lowercases', () => {
		expect(normalizeSearchQuery('  Tokio  ')).toBe('tokio');
	});
});

describe('matchesSearch', () => {
	it('returns true for an empty query', () => {
		expect(matchesSearch('', 'anything')).toBe(true);
		expect(matchesSearch('   ', null)).toBe(true);
	});

	it('matches case-insensitively across fields', () => {
		expect(matchesSearch('NGINX', 'nginx', 'docker')).toBe(true);
		expect(matchesSearch('1.27', null, 1.27)).toBe(true);
	});

	it('ignores null and undefined fields', () => {
		expect(matchesSearch('x', null, undefined)).toBe(false);
	});

	it('returns false when nothing matches', () => {
		expect(matchesSearch('zzz', 'nginx', 'docker')).toBe(false);
	});
});

describe('passesSelectFilter', () => {
	it('passes everything when filter is the all-sentinel', () => {
		expect(passesSelectFilter('github', 'all')).toBe(true);
	});

	it('matches exact values', () => {
		expect(passesSelectFilter('github', 'github')).toBe(true);
		expect(passesSelectFilter('docker', 'github')).toBe(false);
	});

	it('supports a custom all-sentinel', () => {
		expect(passesSelectFilter('pending', '*', '*')).toBe(true);
	});
});

describe('collectTeamFilterOptions / passesTeamFilter', () => {
	it('builds all + sorted tags + untagged sentinel', () => {
		expect(collectTeamFilterOptions(['b', null, 'a', 'b', undefined])).toEqual([
			'all',
			'a',
			'b',
			UNTAGGED_FILTER
		]);
	});

	it('omits untagged when every row is tagged', () => {
		expect(collectTeamFilterOptions(['ops'])).toEqual(['all', 'ops']);
	});

	it('matches team filter including untagged', () => {
		expect(passesTeamFilter('ops', 'all')).toBe(true);
		expect(passesTeamFilter('ops', 'ops')).toBe(true);
		expect(passesTeamFilter(null, UNTAGGED_FILTER)).toBe(true);
		expect(passesTeamFilter('ops', UNTAGGED_FILTER)).toBe(false);
	});
});
