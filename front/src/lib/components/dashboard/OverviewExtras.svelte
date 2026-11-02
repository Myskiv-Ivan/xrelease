<script lang="ts">
	import type { SourceDetail } from '#lib/api/types.js';
	import Badge from '#lib/components/kit/Badge.svelte';
	import Button from '#lib/components/kit/Button.svelte';
	import Panel from '#lib/components/kit/Panel.svelte';
	import Section from '#lib/components/kit/Section.svelte';
	import StatCard from '#lib/components/kit/StatCard.svelte';
	import StatGrid from '#lib/components/kit/StatGrid.svelte';
	import ConfigRoutingFromDocument from '#lib/components/config/ConfigRoutingFromDocument.svelte';
	import { sourceHealth } from '#lib/config/source-presentation.js';
	import { formatNumber } from '#lib/core/format.js';
	import { createScopedConfig } from '#lib/data/config-scope.svelte.js';
	import { getSourcesStore } from '#lib/data/sources.svelte.js';
	import { t } from '#lib/i18n/index.js';
	import { getAuthState } from '#lib/stores/auth.svelte.js';
	import Timestamp from '#lib/components/kit/Timestamp.svelte';
	import {
		PANEL_STACK,
		TYPE_HINT,
		TYPE_MUTED,
		TYPE_SECTION,
		TYPE_STATUS_ERROR
	} from '#lib/components/kit/layout-styles.js';

	const auth = getAuthState();
	const sourcesStore = getSourcesStore();
	const canReadConfig = $derived(auth.hasPermission('config:read'));

	/** Same controller the config page uses — the routing graph reads the document
	    the instance is actually authoritative on, global or per-organization. */
	const config = createScopedConfig({
		isEnabled: () => auth.isReady && auth.hasPermission('config:read')
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
			.filter((source) => sourceHealth(source) !== 'ok')
			.sort((a, b) => {
				const rank = (source: SourceDetail) => {
					if (source.consecutive_errors > 0) return 0;
					if (sourceHealth(source) === 'warning') return 1;
					return 2;
				};
				return rank(a) - rank(b) || b.consecutive_errors - a.consecutive_errors;
			})
			.slice(0, 6);
	});

	const healthLead = $derived.by(() => {
		if (healthCounts.errors > 0) return 'errors' as const;
		if (healthCounts.stale > 0) return 'stale' as const;
		return 'healthy' as const;
	});

	const configView = $derived(config.global.view);
	const orgDesired = $derived(config.org.view?.desired_content ?? null);
	const graphLoading = $derived(config.isLoading);
	const graphError = $derived(config.error);
</script>

<div class={PANEL_STACK}>
	{#if sourcesStore.sources.length > 0}
		<Section title={t('overview.sectionHealth')}>
			<StatGrid columns={3} layout="lead">
				{#if healthLead === 'errors'}
					<StatCard
						lead
						label={t('overview.errorSources')}
						value={formatNumber(healthCounts.errors)}
						tone="danger"
						hint={t('overview.errorSourcesHint')}
						href="/sources?health=error"
					/>
					<StatCard
						label={t('overview.staleSources')}
						value={formatNumber(healthCounts.stale)}
						tone={healthCounts.stale > 0 ? 'warning' : 'default'}
						href={healthCounts.stale > 0 ? '/sources?health=stale' : undefined}
					/>
					<StatCard
						label={t('overview.healthySources')}
						value={formatNumber(healthCounts.healthy)}
						tone="success"
					/>
				{:else if healthLead === 'stale'}
					<StatCard
						lead
						label={t('overview.staleSources')}
						value={formatNumber(healthCounts.stale)}
						tone="warning"
						hint={t('overview.staleSourcesHint')}
						href="/sources?health=stale"
					/>
					<StatCard
						label={t('overview.errorSources')}
						value={formatNumber(healthCounts.errors)}
						tone={healthCounts.errors > 0 ? 'danger' : 'default'}
						href={healthCounts.errors > 0 ? '/sources?health=error' : undefined}
					/>
					<StatCard
						label={t('overview.healthySources')}
						value={formatNumber(healthCounts.healthy)}
						tone="success"
					/>
				{:else}
					<StatCard
						lead
						label={t('overview.healthySources')}
						value={formatNumber(healthCounts.healthy)}
						tone="success"
					/>
					<StatCard
						label={t('overview.staleSources')}
						value={formatNumber(healthCounts.stale)}
						tone="default"
					/>
					<StatCard
						label={t('overview.errorSources')}
						value={formatNumber(healthCounts.errors)}
						tone="default"
					/>
				{/if}
			</StatGrid>
		</Section>
	{/if}

	{#if canReadConfig}
		<details class="group rounded-lg border border-border bg-card open:pb-0">
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
				{#if config.isOrgScoped}
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

	{#if attentionSources.length === 0}
		<!-- Healthy: a status line, not an empty card. -->
		<p class="border-t border-border/70 pt-3 text-sm text-muted-foreground" role="status">
			<span class="font-medium text-foreground">{t('overview.needsAttention')}</span>
			<span class="text-border" aria-hidden="true"> · </span>
			{t('overview.needsAttentionEmpty')}
		</p>
	{:else}
		<Panel title={t('overview.needsAttention')}>
			{#snippet actions()}
				<Button href="/sources?health=attention" variant="ghost" size="sm">
					{t('overview.viewFailingSources')}
				</Button>
			{/snippet}
			<p class="mb-3 {TYPE_HINT}">{t('overview.needsAttentionHint')}</p>
			<ul class="flex flex-col gap-0 divide-y divide-border/60">
				{#each attentionSources as source (source.id)}
					{@const health = sourceHealth(source)}
					<li class="flex flex-col gap-2 py-3 first:pt-0 last:pb-0 sm:flex-row sm:items-start sm:justify-between sm:gap-4">
						<div class="min-w-0 flex-1">
							<a
								href="/sources/{encodeURIComponent(source.id)}"
								class="block truncate text-sm font-medium text-foreground no-underline hover:underline"
							>
								{source.display_name}
							</a>
							<p class="truncate {TYPE_MUTED}">
								{source.kind}
								{#if source.last_polled_at}
									· <Timestamp value={source.last_polled_at} class="inline" />
								{/if}
							</p>
							{#if source.last_error}
								<p
									class="mt-1 line-clamp-2 text-sm text-destructive"
									title={source.last_error}
								>
									{source.last_error}
								</p>
							{:else if health === 'warning'}
								<p class="mt-1 text-sm text-warning">
									{t('overview.staleSourcesHint')}
								</p>
							{/if}
						</div>
						<div class="flex shrink-0 flex-wrap items-center gap-2">
							{#if source.consecutive_errors > 0}
								<Badge tone="danger" title={source.last_error ?? undefined}>
									{source.consecutive_errors}
								</Badge>
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
							<Button
								href="/sources/{encodeURIComponent(source.id)}"
								variant="outline"
								size="sm"
							>
								{t('overview.openSource')}
							</Button>
						</div>
					</li>
				{/each}
			</ul>
		</Panel>
	{/if}
</div>
