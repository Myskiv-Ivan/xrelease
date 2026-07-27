<script lang="ts">
	import type { SourceDetail } from '$lib/api/types';
	import Badge from '$lib/components/kit/Badge.svelte';
	import Panel from '$lib/components/kit/Panel.svelte';
	import StatCard from '$lib/components/dashboard/StatCard.svelte';
	import StatGrid from '$lib/components/kit/StatGrid.svelte';
	import ConfigRoutingFromDocument from '$lib/components/config/ConfigRoutingFromDocument.svelte';
	import { sourceHealth } from '$lib/config/source-presentation';
	import { formatNumber } from '$lib/core/format';
	import { getConfigStore } from '$lib/data/config.svelte';
	import { getOrgConfigStore } from '$lib/data/org-config.svelte';
	import { getSourcesStore } from '$lib/data/sources.svelte';
	import { t } from '$lib/i18n';
	import { getAuthState } from '$lib/stores/auth.svelte';
	import { getOrganizationsState } from '$lib/stores/organizations.svelte';
	import RelativeTime from '$lib/components/kit/RelativeTime.svelte';
	import { TYPE_MUTED, TYPE_HINT, TYPE_SECTION, TYPE_STATUS_ERROR } from '$lib/components/kit/layout-styles';

	const auth = getAuthState();
	const sourcesStore = getSourcesStore();
	const configStore = getConfigStore();
	const orgConfigStore = getOrgConfigStore();
	const orgs = getOrganizationsState();
	const canReadConfig = $derived(auth.hasPermission('config:read'));

	/** Org the org-config store is tracking (plain, for switch detection). */
	let activeOrg: string | null = null;

	$effect(() => {
		if (!auth.isReady || !auth.hasPermission('config:read')) return;
		if (!orgs.isLoaded) return;

		if (orgs.isMultiOrg) {
			configStore.stop();
			if (!orgs.selectedId) {
				orgConfigStore.stop();
				activeOrg = null;
				return;
			}
			if (activeOrg === null) {
				orgConfigStore.start();
			} else if (activeOrg !== orgs.selectedId) {
				void orgConfigStore.reload();
			}
			activeOrg = orgs.selectedId;
			return () => {
				orgConfigStore.stop();
				activeOrg = null;
			};
		}

		orgConfigStore.stop();
		activeOrg = null;
		configStore.start();
		return () => configStore.stop();
	});

	const healthCounts = $derived.by(() => {
		let healthy = 0;
		let stale = 0;
		let errors = 0;
		for (const source of sourcesStore.sources) {
			const health = sourceHealth(source);
			if (health === 'error') errors += 1;
			else if (health === 'warning') stale += 1;
			else healthy += 1;
		}
		return { healthy, stale, errors };
	});

	const attentionSources = $derived.by(() => {
		return sourcesStore.sources
			.filter((source) => sourceHealth(source) !== 'ok' || source.poll_errors > 0)
			.sort((a, b) => {
				const rank = (source: SourceDetail) => {
					if (source.poll_errors > 0) return 0;
					if (sourceHealth(source) === 'warning') return 1;
					return 2;
				};
				return rank(a) - rank(b) || b.poll_errors - a.poll_errors;
			})
			.slice(0, 6);
	});

	const configView = $derived(configStore.view);
	const orgDesired = $derived(orgConfigStore.view?.desired_content ?? null);
	const graphLoading = $derived(
		orgs.isMultiOrg ? orgConfigStore.isLoading : configStore.isLoading
	);
	const graphError = $derived(orgs.isMultiOrg ? orgConfigStore.error : configStore.error);
</script>

<div class="flex flex-col gap-4">
	{#if sourcesStore.sources.length > 0}
		<section class="flex flex-col gap-3">
			<h2 class={TYPE_SECTION}>{t('overview.sectionHealth')}</h2>
			<StatGrid columns={3}>
				<StatCard
					label={t('overview.healthySources')}
					value={formatNumber(healthCounts.healthy)}
					tone="success"
				/>
				<StatCard
					label={t('overview.staleSources')}
					value={formatNumber(healthCounts.stale)}
					tone={healthCounts.stale > 0 ? 'warning' : 'default'}
				/>
				<StatCard
					label={t('overview.errorSources')}
					value={formatNumber(healthCounts.errors)}
					tone={healthCounts.errors > 0 ? 'danger' : 'default'}
				/>
			</StatGrid>
		</section>
	{/if}

	{#if canReadConfig}
		<details class="group rounded-xl border border-border bg-card open:pb-0">
			<summary
				class="cursor-pointer list-none px-4 py-3 {TYPE_SECTION} marker:content-none [&::-webkit-details-marker]:hidden"
			>
				<span class="inline-flex items-center gap-2">
					{t('overview.routingSummary')}
					<span class="{TYPE_MUTED} font-normal group-open:hidden">
						· {t('overview.routingExpand')}
					</span>
				</span>
			</summary>
			<div class="border-t border-border px-4 py-3">
				{#if orgs.isMultiOrg}
					{#if orgDesired?.trim()}
						<ConfigRoutingFromDocument
							variant="page"
							desiredContent={orgDesired}
							effectiveContent={orgDesired}
							class="w-full"
						/>
					{:else if graphLoading}
						<p class={TYPE_HINT}>{t('common.loading')}</p>
					{:else if graphError}
						<p class={TYPE_STATUS_ERROR}>{graphError}</p>
					{:else}
						<p class={TYPE_HINT}>{t('overview.routingEmptyOrg')}</p>
					{/if}
				{:else if configView}
					<ConfigRoutingFromDocument
						variant="page"
						desiredContent={configView.desired_content}
						effectiveContent={configView.content}
						class="w-full"
					/>
				{:else if graphLoading}
					<p class={TYPE_HINT}>{t('common.loading')}</p>
				{:else if graphError}
					<p class={TYPE_STATUS_ERROR}>{graphError}</p>
				{:else}
					<p class={TYPE_HINT}>{t('config.editNoDesired')}</p>
				{/if}
			</div>
		</details>
	{/if}

	<Panel title={t('overview.needsAttention')}>
		{#if attentionSources.length === 0}
			<p class={TYPE_HINT}>{t('overview.needsAttentionEmpty')}</p>
		{:else}
			<ul class="grid gap-0 divide-y divide-border/60 sm:grid-cols-2 sm:gap-x-6 sm:divide-y-0">
				{#each attentionSources as source (source.id)}
					{@const health = sourceHealth(source)}
					<li class="flex items-center justify-between gap-3 border-b border-border/60 py-2.5 sm:border-b">
						<div class="min-w-0">
							<a
								href="/sources/{encodeURIComponent(source.id)}"
								class="block truncate text-sm font-medium text-foreground no-underline hover:underline"
							>
								{source.display_name}
							</a>
							<p class="truncate {TYPE_MUTED}">
								{source.kind}
								{#if source.last_polled_at}
									· <RelativeTime value={source.last_polled_at} class="inline" />
								{/if}
							</p>
						</div>
						<div class="flex shrink-0 items-center gap-2">
							{#if source.poll_errors > 0}
								<Badge tone="danger">{source.poll_errors}</Badge>
							{/if}
							<Badge
								tone={health === 'error'
									? 'danger'
									: health === 'warning'
										? 'warning'
										: 'muted'}
							>
								{health === 'error'
									? t('overview.healthError')
									: health === 'warning'
										? t('overview.healthStale')
										: t('overview.ok')}
							</Badge>
						</div>
					</li>
				{/each}
			</ul>
		{/if}
	</Panel>
</div>
