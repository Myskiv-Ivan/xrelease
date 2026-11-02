/**
 * Client-side table window state shared by Sources / Outbox / Users / Seen / Advisories.
 *
 * One interface (`limit` / `offset` / `setWindow` / `reset` / `slice`) so pages
 * don't re-implement clamp + slice + filter-reset independently.
 */
import {
	TABLE_PAGE_DEFAULT,
	TABLE_PAGE_SIZES,
	clampTableOffset,
	paginateItems,
	resolveTablePageSize
} from './table-pagination.js';

export function createTablePager(pageSizes: readonly number[] = TABLE_PAGE_SIZES) {
	let limit = $state<number>(TABLE_PAGE_DEFAULT);
	let offset = $state(0);

	function reset() {
		offset = 0;
	}

	function setWindow(nextLimit: number, nextOffset: number) {
		limit = resolveTablePageSize(nextLimit, pageSizes);
		offset = Math.max(0, nextOffset);
	}

	function slice<T>(items: readonly T[]): { offset: number; items: T[] } {
		const safe = clampTableOffset(items.length, limit, offset);
		return { offset: safe, items: paginateItems(items, limit, safe) };
	}

	return {
		get limit() {
			return limit;
		},
		get offset() {
			return offset;
		},
		get pageSizes() {
			return pageSizes;
		},
		reset,
		setWindow,
		slice
	};
}

export type TablePager = ReturnType<typeof createTablePager>;
