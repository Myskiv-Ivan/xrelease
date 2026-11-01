import type { ConfigRevisionSummary } from '#lib/api/types.js';
import { createResourceStore } from '#lib/data/resource.svelte.js';
import { t } from '#lib/i18n/index.js';

interface RevisionsPage {
	revisions: ConfigRevisionSummary[];
	total: number;
}

/** API clamps history pages to 200 (`REVISIONS_MAX_LIMIT` on the server). */
export const REVISIONS_PAGE_ALL = 200;
export const REVISIONS_PAGE_DEFAULT = 20;

interface CreateConfigResourcesOptions<TView> {
	fetchView: () => Promise<{ view: TView; etag: string | null }>;
	fetchRevisions: (limit: number, offset: number) => Promise<RevisionsPage>;
}

/**
 * The config document plus its revision list, presented as one store.
 *
 * Global `/config` and per-organization config are the same shape over
 * different endpoints — two resources whose loading, error and freshness state
 * has to read as one panel. Keeping that merge in a factory is what stops the
 * global and org stores from disagreeing about, say, whether a revisions
 * failure counts as a page error.
 */
export function createConfigResources<TView>(options: CreateConfigResourcesOptions<TView>) {
	let revisionsLimit = REVISIONS_PAGE_DEFAULT;
	let revisionsOffset = 0;

	const viewResource = createResourceStore<{ view: TView; etag: string | null }>({
		fetcher: options.fetchView,
		fallbackError: t('errors.loadConfig')
	});

	const revisionsResource = createResourceStore<RevisionsPage>({
		fetcher: () => options.fetchRevisions(revisionsLimit, revisionsOffset),
		fallbackError: t('errors.loadConfigRevisions')
	});

	return {
		get view() {
			return viewResource.data?.view ?? null;
		},
		get etag() {
			return viewResource.data?.etag ?? null;
		},
		get revisions() {
			return revisionsResource.data?.revisions ?? [];
		},
		get revisionsTotal() {
			return revisionsResource.data?.total ?? 0;
		},
		get revisionsLimit() {
			return revisionsLimit;
		},
		get revisionsOffset() {
			return revisionsOffset;
		},
		/** Document errors win — a stale revision list is not worth blocking the page. */
		get error() {
			return viewResource.error ?? revisionsResource.error;
		},
		get isLoading() {
			return viewResource.isLoading || revisionsResource.isLoading;
		},
		get isRefreshing() {
			return viewResource.isRefreshing || revisionsResource.isRefreshing;
		},
		/** Newest of the two, so the toolbar never reports the staler half. */
		get lastUpdated() {
			const view = viewResource.lastUpdated;
			const revisions = revisionsResource.lastUpdated;
			if (!view) return revisions;
			if (!revisions) return view;
			return view > revisions ? view : revisions;
		},
		start() {
			viewResource.start();
			revisionsResource.start();
		},
		stop() {
			viewResource.stop();
			revisionsResource.stop();
		},
		async reload() {
			await Promise.all([viewResource.reload(), revisionsResource.reload()]);
		},
		async refresh() {
			await Promise.all([viewResource.refresh(), revisionsResource.refresh()]);
		},
		async setRevisionsWindow(limit: number, offset = 0) {
			revisionsLimit = Math.max(1, limit);
			revisionsOffset = Math.max(0, offset);
			await revisionsResource.reload();
		},
		reset() {
			revisionsLimit = REVISIONS_PAGE_DEFAULT;
			revisionsOffset = 0;
			viewResource.reset();
			revisionsResource.reset();
		}
	};
}

export type ConfigResources<TView> = ReturnType<typeof createConfigResources<TView>>;
