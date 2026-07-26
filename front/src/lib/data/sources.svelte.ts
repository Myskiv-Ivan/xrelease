import { api } from '$lib/api/client';
import type { SourceDetail } from '$lib/api/types';
import { createResourceStore } from '$lib/data/resource.svelte';
import { t } from '$lib/i18n';
import { getOrganizationsState } from '$lib/stores/organizations.svelte';

function organizationParam(): string | null {
	const orgs = getOrganizationsState();
	return orgs.isMultiOrg ? orgs.selectedId : null;
}

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
