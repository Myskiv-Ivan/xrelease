import { api } from '#lib/api/client.js';
import type { SourceDetail } from '#lib/api/types.js';
import { organizationParam } from '#lib/data/organization-scope.js';
import { createResourceStore } from '#lib/data/resource.svelte.js';
import { t } from '#lib/i18n/index.js';

const resource = createResourceStore<SourceDetail[]>({
	fetcher: () => api.listSources(organizationParam()),
	fallbackError: t('errors.loadSources')
});

const sourcesStore = {
	get sources() {
		return resource.data ?? [];
	},
	get error() {
		return resource.error;
	},
	get isLoading() {
		return resource.isLoading;
	},
	get isRefreshing() {
		return resource.isRefreshing;
	},
	get lastUpdated() {
		return resource.lastUpdated;
	},
	findById: (id: string) => resource.data?.find((source) => source.id === id) ?? null,
	fetchById: (id: string) => api.getSource(id),
	start: () => resource.start(),
	stop: () => resource.stop(),
	reload: () => resource.reload(),
	refresh: () => resource.refresh()
};

export function getSourcesStore() {
	return sourcesStore;
}

export function resetSourcesStore() {
	resource.reset();
}
