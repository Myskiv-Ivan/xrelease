<script lang="ts">
	import DashboardShell from '$lib/components/layout/DashboardShell.svelte';
	import ConfigDocumentPanel from '$lib/components/config/ConfigDocumentPanel.svelte';
	import ConfigEditPanel from '$lib/components/config/ConfigEditPanel.svelte';
	import ConfigProvenancePanel from '$lib/components/config/ConfigProvenancePanel.svelte';
	import ConfigRevisionsPanel from '$lib/components/config/ConfigRevisionsPanel.svelte';
	import OrgConfigView from '$lib/components/config/OrgConfigView.svelte';
	import EmptyState from '$lib/components/kit/EmptyState.svelte';
	import { PAGE_STACK, PANEL_STACK } from '$lib/components/kit/layout-styles';
	import * as Tabs from '$lib/components/kit/tabs';
	import { createScopedConfig } from '$lib/data/config-scope.svelte';
	import { t } from '$lib/i18n';

	const config = createScopedConfig();
	let tab = $state('overview');

	const pageDescription = $derived(
		config.isOrgScoped ? t('org.configDescription') : t('config.description')
	);
</script>

<DashboardShell
	title={t('config.title')}
	description={pageDescription}
	permission="config:read"
	error={config.error}
	isLoading={config.isLoading}
	hasContent={config.hasContent}
	isRefreshing={config.isRefreshing}
	lastUpdated={config.lastUpdated}
	onRefresh={() => config.refresh()}
>
	{#if config.isOrgScoped}
		{#if config.org.view}
			<OrgConfigView
				view={config.org.view}
				etag={config.org.etag}
				revisions={config.org.revisions}
				revisionsTotal={config.org.revisionsTotal}
				onApplied={() => config.org.refresh()}
			/>
		{:else if !config.org.isLoading}
			<EmptyState title={t('errors.loadConfig')} />
		{/if}
	{:else if config.global.view}
		{@const view = config.global.view}
		<div class={PAGE_STACK}>
			<Tabs.Root bind:value={tab} class="gap-4">
				<Tabs.List variant="line" class="w-full justify-start">
					<Tabs.Trigger value="overview">{t('config.tabOverview')}</Tabs.Trigger>
					<Tabs.Trigger value="edit">{t('config.tabEdit')}</Tabs.Trigger>
					<Tabs.Trigger value="technical">{t('config.tabTechnical')}</Tabs.Trigger>
				</Tabs.List>

				<Tabs.Content value="overview" class={PANEL_STACK}>
					<ConfigProvenancePanel {view} onApplied={() => config.global.refresh()} />
					<ConfigRevisionsPanel
						revisions={config.global.revisions}
						total={config.global.revisionsTotal}
					/>
				</Tabs.Content>

				<Tabs.Content value="edit">
					<ConfigEditPanel
						{view}
						etag={config.global.etag}
						onApplied={() => config.global.refresh()}
					/>
				</Tabs.Content>

				<Tabs.Content value="technical" class={PANEL_STACK}>
					<ConfigDocumentPanel {view} />
				</Tabs.Content>
			</Tabs.Root>
		</div>
	{:else if !config.global.isLoading}
		<EmptyState title={t('errors.loadConfig')} />
	{/if}
</DashboardShell>
