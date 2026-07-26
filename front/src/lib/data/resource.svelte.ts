import { createQuery, type QueryState } from '$lib/data/query.svelte';

interface CreateResourceStoreOptions<T> {
	fetcher: () => Promise<T>;
	fallbackError: string;
	/** Forwarded to {@link createQuery}; default true. */
	autoRefresh?: boolean;
}

/**
 * Lazy singleton wrapper around {@link createQuery}: one shared query instance
 * per module, recreated after `reset()`, never closed over a stale query.
 *
 * Getters must NOT call `ensure()` — creating `$state` during a template read
 * throws `state_unsafe_mutation`. Only `start` / `reload` / `refresh` create
 * the query; unread stores stay null until started.
 */
export function createResourceStore<T>(options: CreateResourceStoreOptions<T>) {
	let query = $state<QueryState<T> | null>(null);

	function ensure(): QueryState<T> {
		if (!query) {
			query = createQuery<T>({
				fetcher: options.fetcher,
				fallbackError: options.fallbackError,
				autoRefresh: options.autoRefresh
			});
		}
		return query;
	}

	return {
		ensure,
		get data() {
			return query?.data ?? null;
		},
		get error() {
			return query?.error ?? null;
		},
		get isLoading() {
			return query?.isLoading ?? false;
		},
		get isRefreshing() {
			return query?.isRefreshing ?? false;
		},
		get lastUpdated() {
			return query?.lastUpdated ?? null;
		},
		start: () => ensure().start(),
		stop: () => query?.stop(),
		reload: () => ensure().reload(),
		refresh: () => ensure().refresh(),
		reset() {
			query?.stop();
			query = null;
		}
	};
}

export type ResourceStore<T> = ReturnType<typeof createResourceStore<T>>;
