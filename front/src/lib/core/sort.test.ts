import { describe, expect, it } from 'vitest';
import {
	compareNaturalStrings,
	compareNullableDates,
	compareValues,
	sortByKey,
	toggleSortKey
} from './sort';

describe('toggleSortKey', () => {
	it('flips direction when the key is unchanged', () => {
		expect(toggleSortKey({ key: 'name', direction: 'asc' }, 'name')).toEqual({
			key: 'name',
			direction: 'desc'
		});
	});

	it('resets to ascending when switching keys', () => {
		expect(toggleSortKey({ key: 'name', direction: 'desc' }, 'created')).toEqual({
			key: 'created',
			direction: 'asc'
		});
	});
});

describe('compareValues', () => {
	it('orders numbers numerically', () => {
		expect(compareValues(2, 10, 'asc')).toBeLessThan(0);
		expect(compareValues(2, 10, 'desc')).toBeGreaterThan(0);
	});

	it('orders strings lexicographically', () => {
		expect(compareValues('a', 'b', 'asc')).toBeLessThan(0);
	});

	it('treats null as an empty string', () => {
		expect(compareValues(null, 'a', 'asc')).toBeLessThan(0);
		expect(compareValues(null, null, 'asc')).toBe(0);
	});
});

describe('compareNaturalStrings', () => {
	it('orders version segments numerically', () => {
		expect(compareNaturalStrings('1.2.9', '1.2.10', 'asc')).toBeLessThan(0);
		expect(compareNaturalStrings('1.2.9', '1.2.10', 'desc')).toBeGreaterThan(0);
	});

	it('still orders plain words', () => {
		expect(compareNaturalStrings('alpine', 'bookworm', 'asc')).toBeLessThan(0);
	});
});

describe('compareNullableDates', () => {
	it('keeps missing dates last in both directions', () => {
		expect(compareNullableDates(null, '2026-01-01T00:00:00Z', 'asc')).toBeGreaterThan(0);
		expect(compareNullableDates(null, '2026-01-01T00:00:00Z', 'desc')).toBeGreaterThan(0);
		expect(compareNullableDates('', '2026-01-01T00:00:00Z', 'asc')).toBeGreaterThan(0);
	});

	it('orders present dates chronologically', () => {
		expect(
			compareNullableDates('2025-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 'asc')
		).toBeLessThan(0);
		expect(compareNullableDates(null, null, 'asc')).toBe(0);
	});
});

describe('sortByKey', () => {
	const rows = [
		{ id: 'b', n: 3 },
		{ id: 'a', n: 1 },
		{ id: 'c', n: 2 }
	];

	it('sorts ascending without mutating input', () => {
		const sorted = sortByKey(rows, 'n', 'asc');
		expect(sorted.map((r) => r.n)).toEqual([1, 2, 3]);
		expect(rows[0].id).toBe('b'); // original untouched
	});

	it('sorts descending by string key', () => {
		const sorted = sortByKey(rows, 'id', 'desc');
		expect(sorted.map((r) => r.id)).toEqual(['c', 'b', 'a']);
	});
});
