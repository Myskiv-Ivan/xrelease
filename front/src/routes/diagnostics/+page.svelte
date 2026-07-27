<script lang="ts">
	import { api } from '$lib/api/client';
	import type { HealthResponse, ReadyResponse } from '$lib/api/types';
	import DashboardShell from '$lib/components/layout/DashboardShell.svelte';
	import NotifierTestPanel from '$lib/components/config/NotifierTestPanel.svelte';
	import Button from '$lib/components/kit/Button.svelte';
	import KeyValueList from '$lib/components/kit/KeyValueList.svelte';
	import MetaList from '$lib/components/kit/MetaList.svelte';
	import Panel from '$lib/components/kit/Panel.svelte';
	import { PANEL_GRID, PAGE_STACK, TYPE_HINT } from '$lib/components/kit/layout-styles';
	import type { KeyValueItem } from '$lib/types/ui';
	import { WEBHOOK_ENDPOINTS } from '$lib/core/constants';
	import { resolveApiError } from '$lib/core/errors';
	import { t } from '$lib/i18n';
	import { getNetworkState } from '$lib/stores/network.svelte';

	const network = getNetworkState();

	let health = $state<HealthResponse | null>(null);
	let ready = $state<ReadyResponse | null>(null);
	let error = $state<string | null>(null);
	let isRunning = $state(false);

	async function runChecks() {
		isRunning = true;
		error = null;
		try {
			const [healthResult, readyResult] = await Promise.all([
				api.getHealth(),
				api.getReady()
			]);
			health = healthResult;
			ready = readyResult;
		} catch (err) {
			error = resolveApiError(err, t('diagnostics.failed'));
		} finally {
			isRunning = false;
		}
	}

	const livenessItems = $derived.by((): KeyValueItem[] => {
		if (!health) return [];
		return [
			{
				label: t('diagnostics.status'),
				value: health.status,
				tone: health.status === 'ok' ? 'success' : 'danger'
			},
			{ label: t('diagnostics.sources'), value: health.sources },
			{ label: t('diagnostics.outboxPending'), value: health.outbox_pending ?? 0 },
			{
				label: t('diagnostics.outboxDead'),
				value: health.outbox_dead ?? 0,
				tone: (health.outbox_dead ?? 0) > 0 ? 'danger' : 'default'
			}
		];
	});

	const readinessItems = $derived.by((): KeyValueItem[] => {
		if (!ready) return [];
		return [
			{ label: t('diagnostics.status'), value: ready.status },
			{
				label: t('diagnostics.databaseOk'),
				value: ready.database_ok ? t('overview.ok') : t('overview.error'),
				tone: ready.database_ok ? 'success' : 'danger'
			},
			{
				label: t('diagnostics.notifiersOk'),
				value: ready.notifiers_ok ? t('overview.ok') : t('overview.unreachable'),
				tone: ready.notifiers_ok ? 'success' : 'warning'
			},
			{
				label: t('diagnostics.outboxDead'),
				value: ready.outbox_dead ?? 0,
				tone: (ready.outbox_dead ?? 0) > 0 ? 'danger' : 'default'
			}
		];
	});

	const webhookItems = WEBHOOK_ENDPOINTS.map((endpoint) => ({
		title: endpoint.forge,
		code: endpoint.path,
		detail: endpoint.auth
	}));
</script>

<DashboardShell
	title={t('diagnostics.title')}
	permission="diagnostics:read"
	error={error}
	hasContent={true}
>
	{#snippet actions()}
		<Button disabled={isRunning || !network.isOnline} onclick={runChecks}>
			{isRunning ? t('diagnostics.running') : t('diagnostics.runCheck')}
		</Button>
	{/snippet}

	<div class={PAGE_STACK}>
		<div class={PANEL_GRID}>
			<Panel title={t('diagnostics.liveness')}>
				{#if health}
					<KeyValueList items={livenessItems} />
				{:else}
					<p class={TYPE_HINT}>{t('diagnostics.notRun')}</p>
				{/if}
			</Panel>

			<Panel title={t('diagnostics.readiness')}>
				{#if ready}
					<KeyValueList items={readinessItems} />
				{:else}
					<p class={TYPE_HINT}>{t('diagnostics.notRun')}</p>
				{/if}
			</Panel>
		</div>

		<NotifierTestPanel />

		<Panel title={t('diagnostics.webhookEndpoints')}>
			<MetaList items={webhookItems} />
		</Panel>
	</div>
</DashboardShell>
