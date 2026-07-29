import type { ConfigRevisionSummary } from '$lib/api/types';
import { createResourceStore } from '$lib/data/resource.svelte';
import { t } from '$lib/i18n';

interface RevisionsPage {
	revisions: ConfigRevisionSummary[];
	total: number;
}

interface CreateConfigResourcesOptions<TView> {
	fetchView: () => Promise<{ view: TView; etag: string | null }>;
	fetchRevisions: () => Promise<RevisionsPage>;
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
	const viewResource = createResourceStore<{ view: TView; etag: string | null }>({
		fetcher: options.fetchView,
		fallbackError: t('errors.loadConfig')
	});

	const revisionsResource = createResourceStore<RevisionsPage>({
		fetcher: options.fetchRevisions,
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
		reset() {
			viewResource.reset();
			revisionsResource.reset();
		}
	};
}

export type ConfigResources<TView> = ReturnType<typeof createConfigResources<TView>>;
