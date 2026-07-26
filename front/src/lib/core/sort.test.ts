import { describe, expect, it } from 'vitest';
import { compareValues, sortByKey, toggleSortKey } from './sort';

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
