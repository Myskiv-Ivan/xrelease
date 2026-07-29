import { api } from '$lib/api/client';
import type { OrganizationConfigView } from '$lib/api/types';
import { createConfigResources } from '$lib/data/config-resources.svelte';
import { getOrganizationsState } from '$lib/stores/organizations.svelte';

/**
 * Config + revisions for the organization currently selected in the switcher.
 *
 * The fetchers read `selectedId` at call time, so a `reload()` after the
 * selection changes re-targets the org routes without recreating the store.
 * Callers must drive that reload — `createScopedConfig` does it for every
 * surface that reads config.
 */
const orgs = getOrganizationsState();

function requireSelected(): string {
	const id = orgs.selectedId;
	if (!id) throw new Error('no organization selected');
	return id;
}

const resources = createConfigResources<OrganizationConfigView>({
	fetchView: () => api.getOrgConfig(requireSelected()),
	fetchRevisions: () => api.listOrgConfigRevisions(requireSelected(), 50, 0)
});

export function getOrgConfigStore() {
	return resources;
}

export function resetOrgConfigStore() {
	resources.reset();
}
