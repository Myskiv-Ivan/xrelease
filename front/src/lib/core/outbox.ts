import { api } from '$lib/api/client';
import type { OutboxEntry } from '$lib/api/types';
import { resolveApiError } from '$lib/core/errors';
import { t } from '$lib/i18n';
import { pushToast } from '$lib/stores/toast.svelte';

/** Revive dead-letter rows → pending (same as CLI `xrelease outbox requeue`). */
export async function requeueDeadOutbox(reload: () => Promise<void>): Promise<number> {
	const result = await api.requeueOutbox();
	pushToast({
		title: t('outbox.requeueSuccess'),
		description: t('outbox.requeueCount').replace('{count}', String(result.requeued)),
		tone: 'success'
	});
	await reload();
	return result.requeued;
}

export function requeueErrorToast(err: unknown): void {
	pushToast({
		title: t('outbox.requeueFailed'),
		description: resolveApiError(err, t('errors.requeueOutbox')),
		tone: 'error'
	});
}

/** True when delivery is held until a future `deliver_after`. */
export function isDeferredDelivery(
	entry: Pick<OutboxEntry, 'deliver_after'>,
	now: Date = new Date()
): boolean {
	if (!entry.deliver_after) return false;
	const at = new Date(entry.deliver_after);
	if (Number.isNaN(at.getTime())) return false;
	return at.getTime() > now.getTime();
}
