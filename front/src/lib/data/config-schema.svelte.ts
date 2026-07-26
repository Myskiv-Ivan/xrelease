import { api } from '$lib/api/client';
import type { ConfigSchema } from '$lib/api/types';
import { createResourceStore } from '$lib/data/resource.svelte';
import { t } from '$lib/i18n';

	const resource = createResourceStore<ConfigSchema>({
		fetcher: () => api.getConfigSchema(),
		fallbackError: t('errors.loadConfigSchema'),
		// Schema is build/deployment-stable; avoid the 30s observability poll.
		autoRefresh: false
	});

const configSchemaStore = {
	get schema() {
		return resource.data;
	},
	get sourceKinds() {
		return resource.data?.source_kinds ?? [];
	},
	get sinkKinds() {
		return resource.data?.sink_kinds ?? [];
	},
	get teamTags() {
		return resource.data?.team_tags ?? [];
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
	labelForKind(kind: string): string | null {
		const match = resource.data?.source_kinds.find((entry) => entry.value === kind);
		return match?.label ?? null;
	},
	start: () => resource.start(),
	stop: () => resource.stop(),
	reload: () => resource.reload(),
	refresh: () => resource.refresh()
};

export function getConfigSchemaStore() {
	return configSchemaStore;
}

export function resetConfigSchemaStore() {
	resource.reset();
}
