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
