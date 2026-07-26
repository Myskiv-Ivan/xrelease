import { api } from '$lib/api/client';
import type { OutboxEntry } from '$lib/api/types';
import { createResourceStore } from '$lib/data/resource.svelte';
import { t } from '$lib/i18n';
import { getOrganizationsState } from '$lib/stores/organizations.svelte';

function organizationParam(): string | null {
	const orgs = getOrganizationsState();
	return orgs.isMultiOrg ? orgs.selectedId : null;
}

const resource = createResourceStore<OutboxEntry[]>({
	fetcher: async () => {
		const response = await api.listOutbox(100, organizationParam());
		return response.entries;
	},
	fallbackError: t('errors.loadOutbox')
});

const outboxStore = {
	get entries() {
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
	start: () => resource.start(),
	stop: () => resource.stop(),
	reload: () => resource.reload(),
	refresh: () => resource.refresh()
};

export function getOutboxStore() {
	return outboxStore;
}

export function resetOutboxStore() {
	resource.reset();
}
