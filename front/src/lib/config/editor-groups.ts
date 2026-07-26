/**
 * Group editor rows by kind while preserving each item's original index
 * (validation paths are `sources.N` / `notifiers.N`).
 */

export interface KindGroupItem<T> {
	item: T;
	/** Index in the flat draft array — keep for `sources.${index}` paths. */
	index: number;
}

export interface KindGroup<T> {
	kind: string;
	items: KindGroupItem<T>[];
}

/**
 * Bucket items by kind. `kindOrder` (schema / picker order) decides group
 * sequence; unknown kinds append alphabetically after the known list.
 */
export function groupIndexedByKind<T>(
	items: T[],
	kindOf: (item: T) => string,
	kindOrder: string[] = []
): KindGroup<T>[] {
	const buckets = new Map<string, KindGroupItem<T>[]>();
	items.forEach((item, index) => {
		const kind = (kindOf(item) || 'unknown').trim() || 'unknown';
		const list = buckets.get(kind) ?? [];
		list.push({ item, index });
		buckets.set(kind, list);
	});

	const groups: KindGroup<T>[] = [];
	const seen = new Set<string>();

	for (const kind of kindOrder) {
		const bucket = buckets.get(kind);
		if (!bucket?.length) continue;
		seen.add(kind);
		groups.push({ kind, items: bucket });
	}

	const extras = [...buckets.keys()]
		.filter((kind) => !seen.has(kind))
		.sort((a, b) => a.localeCompare(b));
	for (const kind of extras) {
		groups.push({ kind, items: buckets.get(kind)! });
	}

	return groups;
}
