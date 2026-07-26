/** Shared helpers for client-side table filtering. */

/** Sentinel for "no routing tag" in team filter dropdowns. */
export const UNTAGGED_FILTER = '__untagged__';

export function normalizeSearchQuery(query: string): string {
	return query.trim().toLowerCase();
}

export function matchesSearch(
	query: string,
	...fields: (string | number | null | undefined)[]
): boolean {
	const normalized = normalizeSearchQuery(query);
	if (!normalized) return true;
	return fields.some((field) => {
		if (field == null) return false;
		return String(field).toLowerCase().includes(normalized);
	});
}

export function passesSelectFilter<T extends string>(
	value: T,
	filter: string,
	allValue = 'all'
): boolean {
	return filter === allValue || value === filter;
}

/** Build `['all', …tags, UNTAGGED?]` from a list of routing tags (null = untagged). */
export function collectTeamFilterOptions(
	tags: Iterable<string | null | undefined>,
	untaggedSentinel = UNTAGGED_FILTER
): string[] {
	const set = new Set<string>();
	let hasUntagged = false;
	for (const tag of tags) {
		if (tag) set.add(tag);
		else hasUntagged = true;
	}
	return ['all', ...Array.from(set).sort(), ...(hasUntagged ? [untaggedSentinel] : [])];
}

export function passesTeamFilter(
	routingTag: string | null | undefined,
	filter: string,
	untaggedSentinel = UNTAGGED_FILTER
): boolean {
	if (filter === 'all') return true;
	return (routingTag ?? untaggedSentinel) === filter;
}
