export type SortDirection = 'asc' | 'desc';

export interface SortState<T extends string> {
	key: T;
	direction: SortDirection;
}

export function toggleSortKey<T extends string>(
	current: SortState<T>,
	nextKey: T
): SortState<T> {
	if (current.key === nextKey) {
		return {
			key: nextKey,
			direction: current.direction === 'asc' ? 'desc' : 'asc'
		};
	}
	return { key: nextKey, direction: 'asc' };
}

export function compareValues(
	left: string | number | boolean | null | undefined,
	right: string | number | boolean | null | undefined,
	direction: SortDirection
): number {
	const factor = direction === 'asc' ? 1 : -1;

	if (typeof left === 'number' && typeof right === 'number') {
		return (left - right) * factor;
	}

	const leftStr = left == null ? '' : String(left);
	const rightStr = right == null ? '' : String(right);
	return leftStr.localeCompare(rightStr) * factor;
}

/**
 * Natural compare for version-ish strings — "1.2.10" sorts after "1.2.9",
 * which plain lexicographic comparison gets wrong for every tag column.
 */
export function compareNaturalStrings(
	left: string,
	right: string,
	direction: SortDirection
): number {
	const factor = direction === 'asc' ? 1 : -1;
	return left.localeCompare(right, undefined, { numeric: true, sensitivity: 'base' }) * factor;
}

/**
 * Compare nullable ISO timestamps; rows without a date sort last in *either*
 * direction — "no known release date" is an absence, not the oldest instant.
 */
export function compareNullableDates(
	left: string | null | undefined,
	right: string | null | undefined,
	direction: SortDirection
): number {
	if (!left && !right) return 0;
	if (!left) return 1;
	if (!right) return -1;
	return compareValues(left, right, direction);
}

export function sortByKey<T, K extends keyof T>(
	items: T[],
	key: K,
	direction: SortDirection
): T[] {
	return [...items].sort((left, right) =>
		compareValues(
			left[key] as string | number | boolean | null | undefined,
			right[key] as string | number | boolean | null | undefined,
			direction
		)
	);
}
