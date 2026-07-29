import { api } from '$lib/api/client';
import type { ConfigView } from '$lib/api/types';
import { createConfigResources } from '$lib/data/config-resources.svelte';

/** The instance-wide config document — authoritative on single-document deployments. */
const resources = createConfigResources<ConfigView>({
	fetchView: () => api.getConfig(),
	fetchRevisions: () => api.listConfigRevisions(50, 0)
});

export function getConfigStore() {
	return resources;
}

export function resetConfigStore() {
	resources.reset();
}
