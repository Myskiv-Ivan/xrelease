<script lang="ts">
	import StatCard from '$lib/components/dashboard/StatCard.svelte';
	import OverviewExtras from '$lib/components/dashboard/OverviewExtras.svelte';
	import ConfigApplyPanel from '$lib/components/dashboard/ConfigApplyPanel.svelte';
	import MetricsSummary from '$lib/components/dashboard/MetricsSummary.svelte';
	import MetricsEmbed from '$lib/components/dashboard/MetricsEmbed.svelte';
	import DashboardShell from '$lib/components/layout/DashboardShell.svelte';
	import Button from '$lib/components/kit/Button.svelte';
	import Panel from '$lib/components/kit/Panel.svelte';
	import StatGrid from '$lib/components/kit/StatGrid.svelte';
	import KeyValueList from '$lib/components/kit/KeyValueList.svelte';
	import type { KeyValueItem } from '$lib/types/ui';
	import { PANEL_GRID, PAGE_STACK, TYPE_SECTION } from '$lib/components/kit/layout-styles';
	import { pollAllSources, pollErrorToast } from '$lib/core/poll';
	import { getStatusStore } from '$lib/data/status.svelte';
	import { t } from '$lib/i18n';
	import { getNetworkState } from '$lib/stores/network.svelte';
	import { getAuthState } from '$lib/stores/auth.svelte';
	import { formatNumber, formatUptime } from '$lib/core/format';

	const statusStore = getStatusStore();
	const network = getNetworkState();
	const auth = getAuthState();
	let isPolling = $state(false);

	async function handlePollAll() {
		isPolling = true;
		try {
			await pollAllSources(() => statusStore.reload());
		} catch (err) {
			pollErrorToast(err);
		} finally {
			isPolling = false;
		}
	}

	const dependencyItems = $derived.by((): KeyValueItem[] => {
		if (!statusStore.status) return [];
		const items: KeyValueItem[] = [
			{
				label: t('overview.database'),
				value: statusStore.status.database_ok ? t('overview.ok') : t('overview.error'),
				tone: statusStore.status.database_ok ? 'success' : 'danger'
			}
		];
		if (statusStore.ready) {
			items.push(
				{
					label: t('overview.notifiers'),
					value: statusStore.ready.notifiers_ok ? t('overview.ok') : t('overview.unreachable'),
					tone: statusStore.ready.notifiers_ok ? 'success' : 'warning'
				},
				{ label: t('overview.readiness'), value: statusStore.ready.status }
			);
		}
		return items;
	});

	const openBreakers = $derived(
		statusStore.status?.breakers.filter((breaker) => breaker.open).length ?? 0
	);
</script>

<DashboardShell
	title={t('overview.title')}
	permission="status:read"
	error={statusStore.error}
	isLoading={statusStore.isLoading}
	hasContent={Boolean(statusStore.status)}
	isRefreshing={statusStore.isRefreshing}
	lastUpdated={statusStore.lastUpdated}
	onRefresh={() => statusStore.refresh()}
>
	{#snippet actions()}
		{#if auth.hasPermission('poll:execute')}
			<Button
				variant="accent"
				disabled={isPolling || statusStore.isLoading || !network.isOnline}
				onclick={handlePollAll}
			>
				{isPolling ? t('overview.polling') : t('overview.pollAll')}
			</Button>
		{/if}
	{/snippet}

	{#if statusStore.status}
		<div class={PAGE_STACK}>
			<section class="flex flex-col gap-3">
				<h2 class={TYPE_SECTION}>{t('overview.sectionStatus')}</h2>
				<StatGrid columns={4}>
					<StatCard
						label={t('overview.sources')}
						value={formatNumber(statusStore.status.sources_configured)}
					/>
					<StatCard
						label={t('overview.outboxPending')}
						value={formatNumber(statusStore.status.outbox_pending)}
						tone={statusStore.status.outbox_pending > 0 ? 'warning' : 'default'}
					/>
					<StatCard
						label={t('overview.outboxDead')}
						value={formatNumber(statusStore.status.outbox_dead)}
						tone={statusStore.status.outbox_dead > 0 ? 'danger' : 'default'}
					/>
					<StatCard
						label={t('overview.breakersOpen')}
						value={formatNumber(openBreakers)}
						tone={openBreakers > 0 ? 'danger' : 'default'}
					/>
				</StatGrid>
			</section>

			<section class="flex flex-col gap-3">
				<h2 class={TYPE_SECTION}>{t('overview.sectionRuntime')}</h2>
				<StatGrid columns={4}>
					<StatCard label={t('overview.version')} value={statusStore.status.version} mono />
					<StatCard
						label={t('overview.uptime')}
						value={formatUptime(statusStore.status.uptime_secs)}
					/>
					<StatCard
						label={t('overview.notificationsSent')}
						value={formatNumber(statusStore.status.metrics.notifications_total)}
					/>
					<StatCard
						label={t('overview.outboxDeferred')}
						value={formatNumber(statusStore.status.outbox_deferred)}
						tone={statusStore.status.outbox_deferred > 0 ? 'warning' : 'default'}
					/>
				</StatGrid>
			</section>

			{#if statusStore.status.breakers.length > 0}
				<Panel title={t('overview.breakers')}>
					<ul class="flex flex-col gap-2 text-sm">
						{#each statusStore.status.breakers as breaker (breaker.label + breaker.kind)}
							<li class="flex items-center justify-between gap-3">
								<span class="font-mono text-xs">
									{breaker.label}
									<span class="text-muted-foreground">({breaker.kind})</span>
								</span>
								<span
									class={breaker.open ? 'text-destructive' : 'text-muted-foreground'}
								>
									{breaker.open ? t('overview.breakerOpen') : t('overview.breakerClosed')}
								</span>
							</li>
						{/each}
					</ul>
				</Panel>
			{/if}

			<OverviewExtras />

			<div class={PANEL_GRID}>
				<Panel title={t('overview.dependencies')}>
					<KeyValueList items={dependencyItems} />
				</Panel>
				{#if auth.hasPermission('config:read')}
					<ConfigApplyPanel status={statusStore.status.config_apply} />
				{/if}
			</div>

			<MetricsSummary metrics={statusStore.status.metrics} />
			<MetricsEmbed />
		</div>
	{/if}
</DashboardShell>
