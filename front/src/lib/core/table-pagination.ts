/**
 * Shared table paging — Sources, Outbox, Users, Seen, Advisories, History.
 *
 * Deep module: callers only need page sizes + window helpers; clamp / slice /
 * API-cap live here so every table shares one behaviour.
 */

export const TABLE_PAGE_SIZES = [10, 20, 50, 100, 500] as const;
export type TablePageSize = (typeof TABLE_PAGE_SIZES)[number];

export const TABLE_PAGE_DEFAULT: TablePageSize = 10;

/** Config revision ledger API clamps at 200 (`REVISIONS_MAX_LIMIT`). */
export const REVISIONS_API_MAX = 200;

/** History page sizes — no 500 (would silently clamp and desync the Select). */
export const REVISIONS_PAGE_SIZES = [10, 20, 50, 100, REVISIONS_API_MAX] as const;

export function resolveTablePageSize(
	raw: string | number,
	sizes: readonly number[] = TABLE_PAGE_SIZES
): number {
	const n = typeof raw === 'number' ? raw : Number(raw);
	if (Number.isFinite(n) && sizes.includes(n)) return n;
	return sizes.includes(TABLE_PAGE_DEFAULT) ? TABLE_PAGE_DEFAULT : (sizes[0] ?? TABLE_PAGE_DEFAULT);
}

/** Keep offset on a page boundary that still has rows after filter/size changes. */
export function clampTableOffset(total: number, limit: number, offset: number): number {
	if (total <= 0 || limit <= 0) return 0;
	const maxOffset = Math.floor((total - 1) / limit) * limit;
	return Math.min(Math.max(0, offset), maxOffset);
}

export function paginateItems<T>(items: readonly T[], limit: number, offset: number): T[] {
	const start = clampTableOffset(items.length, limit, offset);
	const size = Math.max(1, limit);
	return items.slice(start, start + size);
}

/** Limit sent to the revisions API (defensive; UI should already use REVISIONS_PAGE_SIZES). */
export function revisionsApiLimit(uiLimit: number): number {
	return Math.min(Math.max(1, uiLimit), REVISIONS_API_MAX);
}
