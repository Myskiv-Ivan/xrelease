import { api } from '$lib/api/client';
import type { OrganizationConfigView, OrganizationRevisionsResponse } from '$lib/api/types';
import { createResourceStore } from '$lib/data/resource.svelte';
import { t } from '$lib/i18n';
import { getOrganizationsState } from '$lib/stores/organizations.svelte';

/**
 * Config + revisions for the organization currently selected in the switcher.
 *
 * The fetchers read `selectedId` at call time, so a `reload()` after the
 * selection changes re-targets the org routes without recreating the store.
 * Callers must drive that reload — the config page watches `selectedId`.
 */
const orgs = getOrganizationsState();

function requireSelected(): string {
	const id = orgs.selectedId;
	if (!id) throw new Error('no organization selected');
	return id;
}

const viewResource = createResourceStore<{ view: OrganizationConfigView; etag: string | null }>({
	fetcher: () => api.getOrgConfig(requireSelected()),
	fallbackError: t('errors.loadConfig')
});

const revisionsResource = createResourceStore<OrganizationRevisionsResponse>({
	fetcher: () => api.listOrgConfigRevisions(requireSelected(), 50, 0),
	fallbackError: t('errors.loadConfigRevisions')
});

const orgConfigStore = {
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

export function getOrgConfigStore() {
	return orgConfigStore;
}

export function resetOrgConfigStore() {
	viewResource.reset();
	revisionsResource.reset();
}
