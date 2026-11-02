<script lang="ts">
	import type { MetricsSnapshot } from '#lib/api/types.js';
	import { getMetricsUrl } from '#lib/api/urls.js';
	import Button from '#lib/components/kit/Button.svelte';
	import KeyValueList from '#lib/components/kit/KeyValueList.svelte';
	import Panel from '#lib/components/kit/Panel.svelte';
	import { TYPE_SECTION } from '#lib/components/kit/layout-styles.js';
	import type { KeyValueItem } from '#lib/types/ui.js';
	import { t } from '#lib/i18n/index.js';

	interface Props {
		metrics: MetricsSnapshot;
	}

	let { metrics }: Props = $props();

	const cacheHitRate = $derived.by(() => {
		if (metrics.polls_total <= 0) return null;
		return Math.round((metrics.polls_not_modified / metrics.polls_total) * 100);
	});

	const metricItems = $derived.by((): KeyValueItem[] => {
		const items: KeyValueItem[] = [
			{ label: t('overview.polls'), value: metrics.polls_total },
			{ label: t('overview.notModified'), value: metrics.polls_not_modified },
			{
				label: t('overview.pollErrors'),
				value: metrics.poll_errors,
				tone: metrics.poll_errors > 0 ? 'danger' : 'default'
			},
			{ label: t('overview.notifications'), value: metrics.notifications_total }
		];
		if (cacheHitRate !== null) {
			items.push({ label: t('overview.cacheHitRate'), value: `${cacheHitRate}%` });
		}
		return items;
	});

	const webhookItems = $derived.by((): KeyValueItem[] => [
		{ label: t('overview.webhooksAccepted'), value: metrics.webhooks_accepted, tone: 'success' },
		{ label: t('overview.webhooksIgnored'), value: metrics.webhooks_ignored },
		{ label: t('overview.webhooksDuplicates'), value: metrics.webhooks_duplicates },
		{
			label: t('overview.webhookErrors'),
			value: metrics.webhooks_errors,
			tone: metrics.webhooks_errors > 0 ? 'danger' : 'default'
		}
	]);

	const deliveryItems = $derived.by((): KeyValueItem[] => [
		{ label: t('overview.outboxEnqueued'), value: metrics.outbox_enqueued_total },
		{
			label: t('overview.outboxFailures'),
			value: metrics.outbox_delivery_failures_total,
			tone: metrics.outbox_delivery_failures_total > 0 ? 'danger' : 'default'
		},
		{
			label: t('overview.outboxDeadLettered'),
			value: metrics.outbox_dead_lettered_total,
			tone: metrics.outbox_dead_lettered_total > 0 ? 'danger' : 'default'
		},
		{ label: t('overview.outboxRequeued'), value: metrics.outbox_requeued_total },
		{
			label: t('overview.breakerSkips'),
			value: metrics.notify_breaker_skips,
			tone: metrics.notify_breaker_skips > 0 ? 'danger' : 'default'
		},
		{
			label: t('overview.httpRateLimited'),
			value: metrics.http_rate_limited_total,
			tone: metrics.http_rate_limited_total > 0 ? 'danger' : 'default'
		},
		{ label: t('overview.configApplyOk'), value: metrics.config_apply_total },
		{
			label: t('overview.configApplyRejected'),
			value: metrics.config_apply_rejected_total,
			tone: metrics.config_apply_rejected_total > 0 ? 'danger' : 'default'
		},
		{ label: t('overview.pruneDeleted'), value: metrics.prune_deleted_total }
	]);
</script>

<!-- One panel, internal sections — avoids a 3-card equal grid. -->
<Panel title={t('overview.globalMetrics')}>
	{#snippet actions()}
		<Button href={getMetricsUrl()} variant="ghost" target="_blank" rel="noopener noreferrer">
			{t('overview.openPrometheus')}
		</Button>
	{/snippet}

	<div class="flex flex-col gap-5">
		<KeyValueList items={metricItems} layout="grid" columns={2} />

		<section class="border-t border-border/70 pt-4">
			<h3 class="mb-2.5 {TYPE_SECTION}">{t('overview.webhookActivity')}</h3>
			<KeyValueList items={webhookItems} layout="grid" columns={2} />
		</section>

		<section class="border-t border-border/70 pt-4">
			<h3 class="mb-2.5 {TYPE_SECTION}">{t('overview.deliveryHealth')}</h3>
			<KeyValueList items={deliveryItems} layout="grid" columns={3} />
		</section>
	</div>
</Panel>
