import { api } from '$lib/api/client';
import type { ReadyResponse, StatusResponse } from '$lib/api/types';
import { createResourceStore } from '$lib/data/resource.svelte';
import { t } from '$lib/i18n';

interface StatusSnapshot {
	status: StatusResponse;
	/** Null when the readiness probe itself failed (shown as unknown). */
	ready: ReadyResponse | null;
}

const resource = createResourceStore<StatusSnapshot>({
	fetcher: async () => {
		const [status, ready] = await Promise.all([
			api.getStatus(),
			api.getReady().catch(() => null)
		]);
		return { status, ready };
	},
	fallbackError: t('errors.loadStatus')
});

const statusStore = {
	get status() {
		return resource.data?.status ?? null;
	},
	get ready() {
		return resource.data?.ready ?? null;
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
	get outboxPending() {
		return resource.data?.status.outbox_pending ?? 0;
	},
	get outboxDead() {
		return resource.data?.status.outbox_dead ?? 0;
	},
	start: () => resource.start(),
	stop: () => resource.stop(),
	reload: () => resource.reload(),
	refresh: () => resource.refresh()
};

export function getStatusStore() {
	return statusStore;
}

export function resetStatusStore() {
	resource.reset();
}
