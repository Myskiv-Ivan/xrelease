<script lang="ts">
	import { page } from '$app/state';
	import SourceDetailView from '$lib/components/sources/SourceDetailView.svelte';
	import DashboardShell from '$lib/components/layout/DashboardShell.svelte';
	import EmptyState from '$lib/components/kit/EmptyState.svelte';
	import { pollErrorToast, pollOneSource } from '$lib/core/poll';
	import { createSourceDetailQuery } from '$lib/data/source-detail.svelte';
	import { getSourcesStore } from '$lib/data/sources.svelte';
	import { t } from '$lib/i18n';
	import { getAuthState } from '$lib/stores/auth.svelte';

	const auth = getAuthState();
	const sourcesStore = getSourcesStore();
	const sourceId = $derived(decodeURIComponent(page.params.id ?? ''));
	const listedSource = $derived(sourceId ? sourcesStore.findById(sourceId) : null);

	const detailQuery = createSourceDetailQuery(() => sourceId);
	const source = $derived(detailQuery.data);
	const authReady = $derived(auth.isReady && auth.isAuthenticated);
	const isLoading = $derived(
		Boolean(sourceId) && !source && !detailQuery.error && (!authReady || detailQuery.isLoading)
	);
	const isNotFound = $derived(
		!isLoading && !detailQuery.error && sourceId !== '' && !source
	);
	let isPolling = $state(false);

	async function handleRefresh() {
		await Promise.all([detailQuery.reload(), sourcesStore.refresh()]);
	}

	async function handlePollOne() {
		if (!sourceId) return;
		isPolling = true;
		try {
			await pollOneSource(sourceId, handleRefresh);
		} catch (err) {
			pollErrorToast(err);
		} finally {
			isPolling = false;
		}
	}
</script>

<DashboardShell
	title={source?.display_name ?? listedSource?.display_name ?? sourceId}
	description={source?.id ?? listedSource?.id ?? ''}
	permission="sources:read"
	error={detailQuery.error ?? sourcesStore.error}
	isLoading={isLoading}
	hasContent={Boolean(source) || isNotFound || Boolean(detailQuery.error)}
	isRefreshing={detailQuery.isRefreshing || sourcesStore.isRefreshing}
	lastUpdated={detailQuery.lastUpdated ?? sourcesStore.lastUpdated}
	onRefresh={handleRefresh}
>
	{#if source}
		<SourceDetailView {source} {isPolling} onPoll={handlePollOne} />
	{:else if isNotFound}
		<EmptyState
			title={t('sources.notFoundTitle')}
			description={`${t('sources.notFoundDescription')} (${sourceId})`}
		/>
	{/if}
</DashboardShell>
