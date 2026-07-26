import { handleUnauthorized, resolveApiError } from '$lib/core/errors';
import { ApiClientError } from '$lib/api/client';
import { pauseRefresh, registerRefreshTask } from '$lib/data/refresh-scheduler';
import { getAuthState } from '$lib/stores/auth.svelte';

export interface QueryState<T> {
	readonly data: T | null;
	readonly error: string | null;
	readonly isLoading: boolean;
	readonly isRefreshing: boolean;
	readonly lastUpdated: Date | null;
	reload: () => Promise<void>;
	refresh: () => Promise<void>;
	start: () => void;
	stop: () => void;
	/** Discard data and any in-flight result (e.g. when the query key changes). */
	reset: () => void;
}

interface CreateQueryOptions<T> {
	fetcher: () => Promise<T>;
	fallbackError: string;
	/** When false, skips fetch (e.g. until auth is ready). */
	isEnabled?: () => boolean;
	/**
	 * When false, load once on `start` / explicit `reload` and do not join the
	 * global auto-refresh scheduler (default true).
	 */
	autoRefresh?: boolean;
}

/**
 * Unified async query with silent refresh, auth guard, and shared auto-refresh
 * driven by the global refresh scheduler (one interval for all queries).
 */
export function createQuery<T>(options: CreateQueryOptions<T>): QueryState<T> {
	const auth = getAuthState();

	let data = $state<T | null>(null);
	let error = $state<string | null>(null);
	let isLoading = $state(false);
	let isRefreshing = $state(false);
	let lastUpdated = $state<Date | null>(null);

	let started = false;
	let inFlight = false;
	let hasLoaded = false;
	/**
	 * Monotonic run id. A completion only applies its result when its run is
	 * still the newest — a stale fetch (superseded by an explicit reload, a
	 * key change via [`reset`], or [`stop`]) is discarded instead of
	 * overwriting fresher state.
	 */
	let generation = 0;
	let unregisterRefresh: (() => void) | null = null;

	async function execute(silent: boolean) {
		if (options.isEnabled && !options.isEnabled()) return;
		if (!auth.isAuthenticated) {
			if (!silent) started = false;
			return;
		}
		// Silent (scheduler) refreshes dedupe against any in-flight run;
		// explicit loads supersede it instead, so a reload after a key change
		// is never swallowed by a slow earlier fetch.
		if (inFlight && silent) return;

		generation += 1;
		const run = generation;
		inFlight = true;

		if (silent) {
			isRefreshing = true;
		} else if (!hasLoaded) {
			isLoading = true;
		}
		error = null;

		try {
			const result = await options.fetcher();
			if (run !== generation) return;
			data = result;
			hasLoaded = true;
			lastUpdated = new Date();
		} catch (err) {
			if (run !== generation) return;
			if (!handleUnauthorized(err)) {
				if (err instanceof ApiClientError && err.status === 429) {
					pauseRefresh();
					error = resolveApiError(err, options.fallbackError);
				} else {
					error = resolveApiError(err, options.fallbackError);
				}
			}
		} finally {
			if (run === generation) {
				isLoading = false;
				isRefreshing = false;
				inFlight = false;
			}
		}
	}

	function start() {
		if (started) {
			// A prior start may have bailed before the first fetch (e.g. auth race).
			if (!hasLoaded && !inFlight) {
				void execute(false);
			}
			return;
		}
		started = true;
		if (options.autoRefresh !== false) {
			unregisterRefresh = registerRefreshTask(() => execute(true));
		}
		void execute(false);
	}

	function stop() {
		started = false;
		hasLoaded = false;
		isLoading = false;
		isRefreshing = false;
		inFlight = false;
		generation += 1;
		unregisterRefresh?.();
		unregisterRefresh = null;
	}

	function reset() {
		generation += 1;
		started = false;
		inFlight = false;
		hasLoaded = false;
		data = null;
		error = null;
		isLoading = false;
		isRefreshing = false;
		lastUpdated = null;
		unregisterRefresh?.();
		unregisterRefresh = null;
	}

	return {
		get data() {
			return data;
		},
		get error() {
			return error;
		},
		get isLoading() {
			return isLoading;
		},
		get isRefreshing() {
			return isRefreshing;
		},
		get lastUpdated() {
			return lastUpdated;
		},
		reload: () => execute(false),
		refresh: () => execute(true),
		start,
		stop,
		reset
	};
}
