import { describe, expect, it } from 'vitest';
import {
	REVISIONS_API_MAX,
	REVISIONS_PAGE_SIZES,
	TABLE_PAGE_DEFAULT,
	clampTableOffset,
	paginateItems,
	resolveTablePageSize,
	revisionsApiLimit
} from './table-pagination.js';

describe('table-pagination', () => {
	it('defaults unknown sizes to 10', () => {
		expect(resolveTablePageSize('all')).toBe(TABLE_PAGE_DEFAULT);
		expect(resolveTablePageSize(15)).toBe(TABLE_PAGE_DEFAULT);
		expect(resolveTablePageSize(50)).toBe(50);
	});

	it('resolves against a custom size list (history)', () => {
		expect(resolveTablePageSize(500, REVISIONS_PAGE_SIZES)).toBe(TABLE_PAGE_DEFAULT);
		expect(resolveTablePageSize(200, REVISIONS_PAGE_SIZES)).toBe(REVISIONS_API_MAX);
		expect(REVISIONS_PAGE_SIZES).not.toContain(500);
		expect(REVISIONS_PAGE_SIZES).toContain(REVISIONS_API_MAX);
	});

	it('clamps offset onto a valid page', () => {
		expect(clampTableOffset(0, 10, 20)).toBe(0);
		expect(clampTableOffset(25, 10, 40)).toBe(20);
		expect(clampTableOffset(25, 10, 10)).toBe(10);
	});

	it('slices the current page', () => {
		const items = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
		expect(paginateItems(items, 10, 0)).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
		expect(paginateItems(items, 10, 10)).toEqual([11]);
	});

	it('caps revision API limits at 200', () => {
		expect(revisionsApiLimit(500)).toBe(REVISIONS_API_MAX);
		expect(revisionsApiLimit(50)).toBe(50);
	});
});
