import { api } from '$lib/api/client';
import type { ConfigRevisionsResponse, ConfigView } from '$lib/api/types';
import { createResourceStore } from '$lib/data/resource.svelte';
import { t } from '$lib/i18n';

const viewResource = createResourceStore<{ view: ConfigView; etag: string | null }>({
	fetcher: () => api.getConfig(),
	fallbackError: t('errors.loadConfig')
});

const revisionsResource = createResourceStore<ConfigRevisionsResponse>({
	fetcher: () => api.listConfigRevisions(50, 0),
	fallbackError: t('errors.loadConfigRevisions')
});

const configStore = {
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
	get error() {
		return viewResource.error ?? revisionsResource.error;
	},
	get isLoading() {
		return viewResource.isLoading || revisionsResource.isLoading;
	},
	get isRefreshing() {
		return viewResource.isRefreshing || revisionsResource.isRefreshing;
	},
	get lastUpdated() {
		const a = viewResource.lastUpdated;
		const b = revisionsResource.lastUpdated;
		if (!a) return b;
		if (!b) return a;
		return a > b ? a : b;
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
	}
};

export function getConfigStore() {
	return configStore;
}

export function resetConfigStore() {
	viewResource.reset();
	revisionsResource.reset();
}
