import { api } from '#lib/api/client.js';
import { resolveApiError } from '#lib/core/errors.js';
import { t } from '#lib/i18n/index.js';
import { pushToast } from '#lib/stores/toast.svelte.js';

	export async function pollAllSources(reload: () => Promise<void>): Promise<void> {
		const result = await api.triggerCheck(false);
		const failed = result.sources_failed ?? 0;
		if (failed > 0) {
			pushToast({
				title: t('overview.pollPartial'),
				description: `${failed} / ${result.sources_checked}`,
				tone: 'error'
			});
		} else {
			pushToast({
				title: t('overview.pollSuccess'),
				description: `${result.sources_checked} / ${result.notifications_sent}`,
				tone: 'success'
			});
		}
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
