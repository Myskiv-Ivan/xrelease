<script lang="ts">
	import DashboardShell from '$lib/components/layout/DashboardShell.svelte';
	import ConfigDocumentPanel from '$lib/components/config/ConfigDocumentPanel.svelte';
	import ConfigEditPanel from '$lib/components/config/ConfigEditPanel.svelte';
	import ConfigProvenancePanel from '$lib/components/config/ConfigProvenancePanel.svelte';
	import ConfigRevisionsPanel from '$lib/components/config/ConfigRevisionsPanel.svelte';
	import OrgConfigView from '$lib/components/config/OrgConfigView.svelte';
	import EmptyState from '$lib/components/kit/EmptyState.svelte';
	import { PAGE_STACK } from '$lib/components/kit/layout-styles';
	import * as Tabs from '$lib/components/kit/tabs';
	import { getConfigStore } from '$lib/data/config.svelte';
	import { getOrgConfigStore } from '$lib/data/org-config.svelte';
	import { getOrganizationsState } from '$lib/stores/organizations.svelte';
	import { t } from '$lib/i18n';
	import { onMount } from 'svelte';

	const configStore = getConfigStore();
	const orgConfigStore = getOrgConfigStore();
	const orgs = getOrganizationsState();
	let tab = $state('overview');

	/** Org-scoped once the catalogue confirms multiple orgs and one is selected. */
	const orgScoped = $derived(orgs.isMultiOrg && orgs.selectedId != null);
	/** Hold rendering until the catalogue decides global vs org (or fails). */
	const catalogueReady = $derived(orgs.isLoaded || orgs.error != null);

	/** The org the org-config store is currently tracking (plain, non-reactive). */
	let activeOrg: string | null = null;

	onMount(() => {
		return () => {
			configStore.stop();
			orgConfigStore.stop();
		};
	});

	$effect(() => {
		// Wait for the catalogue so a multi-org instance never fetches the global
		// document just to abandon it; the layout loads the catalogue on auth.
		if (!catalogueReady) return;

		if (orgScoped) {
			configStore.stop();
			if (activeOrg === null) {
				orgConfigStore.start();
			} else if (activeOrg !== orgs.selectedId) {
				void orgConfigStore.reload();
			}
			activeOrg = orgs.selectedId;
		} else {
			if (activeOrg !== null) {
				orgConfigStore.stop();
				activeOrg = null;
			}
			configStore.start();
		}
	});

	const error = $derived(orgScoped ? orgConfigStore.error : configStore.error);
	const isLoading = $derived(
		!catalogueReady || (orgScoped ? orgConfigStore.isLoading : configStore.isLoading)
	);
	const isRefreshing = $derived(
		orgScoped ? orgConfigStore.isRefreshing : configStore.isRefreshing
	);
	const lastUpdated = $derived(orgScoped ? orgConfigStore.lastUpdated : configStore.lastUpdated);
	const hasContent = $derived(
		orgScoped ? orgConfigStore.view != null : configStore.view != null
	);
	const pageDescription = $derived(
		orgScoped ? t('org.configDescription') : t('config.description')
	);
</script>

<DashboardShell
	title={t('config.title')}
	description={pageDescription}
	permission="config:read"
	{error}
	{isLoading}
	{hasContent}
	{isRefreshing}
	{lastUpdated}
	onRefresh={() => (orgScoped ? orgConfigStore.refresh() : configStore.refresh())}
>
	{#if orgScoped}
		{#if !orgConfigStore.view}
			{#if !orgConfigStore.isLoading}
				<EmptyState title={t('errors.loadConfig')} />
			{/if}
		{:else}
			<OrgConfigView
				view={orgConfigStore.view}
				etag={orgConfigStore.etag}
				revisions={orgConfigStore.revisions}
				revisionsTotal={orgConfigStore.revisionsTotal}
				onApplied={() => orgConfigStore.refresh()}
			/>
		{/if}
	{:else if !configStore.view}
		{#if !configStore.isLoading}
			<EmptyState title={t('errors.loadConfig')} />
		{/if}
	{:else}
		{@const view = configStore.view}
		<div class={PAGE_STACK}>
			<Tabs.Root bind:value={tab} class="gap-4">
				<Tabs.List variant="line" class="w-full justify-start">
					<Tabs.Trigger value="overview">{t('config.tabOverview')}</Tabs.Trigger>
					<Tabs.Trigger value="edit">{t('config.tabEdit')}</Tabs.Trigger>
					<Tabs.Trigger value="technical">{t('config.tabTechnical')}</Tabs.Trigger>
				</Tabs.List>

				<Tabs.Content value="overview" class="flex flex-col gap-4">
					<ConfigProvenancePanel {view} onApplied={() => configStore.refresh()} />
					<ConfigRevisionsPanel
						revisions={configStore.revisions}
						total={configStore.revisionsTotal}
					/>
				</Tabs.Content>

				<Tabs.Content value="edit">
					<ConfigEditPanel
						{view}
						etag={configStore.etag}
						onApplied={() => configStore.refresh()}
					/>
				</Tabs.Content>

				<Tabs.Content value="technical" class="flex flex-col gap-4">
					<ConfigDocumentPanel {view} />
				</Tabs.Content>
			</Tabs.Root>
		</div>
	{/if}
</DashboardShell>
