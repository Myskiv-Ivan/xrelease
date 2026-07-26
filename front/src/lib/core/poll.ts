import { api } from '$lib/api/client';
import { resolveApiError } from '$lib/core/errors';
import { t } from '$lib/i18n';
import { pushToast } from '$lib/stores/toast.svelte';

export async function pollAllSources(reload: () => Promise<void>): Promise<void> {
	const result = await api.triggerCheck(false);
	pushToast({
		title: t('overview.pollSuccess'),
		description: `${result.sources_checked} / ${result.notifications_sent}`,
		tone: 'success'
	});
	await reload();
}

export async function pollOneSource(
	sourceId: string,
	reload: () => Promise<void>
): Promise<void> {
	const result = await api.triggerCheckSource(sourceId, false);
	pushToast({
		title: t('overview.pollSuccess'),
		description: String(result.notifications_sent),
		tone: 'success'
	});
	await reload();
}

export function pollErrorToast(err: unknown): void {
	pushToast({
		title: t('overview.pollFailed'),
		description: resolveApiError(err, t('errors.poll')),
		tone: 'error'
	});
}
