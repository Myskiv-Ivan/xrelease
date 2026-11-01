import { api } from '#lib/api/client.js';
import type { ConfigView } from '#lib/api/types.js';
import { createConfigResources } from '#lib/data/config-resources.svelte.js';

/** The instance-wide config document — authoritative on single-document deployments. */
const resources = createConfigResources<ConfigView>({
	fetchView: () => api.getConfig(),
	fetchRevisions: (limit, offset) => api.listConfigRevisions(limit, offset)
});

export function getConfigStore() {
	return resources;
}

export function resetConfigStore() {
	resources.reset();
}
