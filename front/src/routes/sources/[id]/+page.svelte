<script lang="ts">
	import { page } from '$app/state';
	import SourceDetailView from '$lib/components/sources/SourceDetailView.svelte';
	import DashboardShell from '$lib/components/layout/DashboardShell.svelte';
	import EmptyState from '$lib/components/kit/EmptyState.svelte';
	import { pollErrorToast, pollOneSource } from '$lib/core/poll';
	import { createQuery } from '$lib/data/query.svelte';
	import { getSourcesStore } from '$lib/data/sources.svelte';
	import { t } from '$lib/i18n';
	import type { SourceDetail } from '$lib/api/types';
	import { untrack } from 'svelte';

	const sourcesStore = getSourcesStore();
	const sourceId = $derived(decodeURIComponent(page.params.id ?? ''));
	const listedSource = $derived(sourceId ? sourcesStore.findById(sourceId) : null);

	// Always fetch the detail endpoint so seen_releases / last_polled_at stay
	// fresh after poll, auto-refresh, or client-side navigation from the list.
	const detailQuery = createQuery<SourceDetail>({
		fetcher: () => {
			const id = untrack(() => sourceId);
			return sourcesStore.fetchById(id);
		},
		fallbackError: t('errors.loadSource'),
		isEnabled: () => Boolean(untrack(() => sourceId))
	});

	const source = $derived(detailQuery.data ?? listedSource);
	const isLoading = $derived(
		(detailQuery.isLoading && !source) || (sourcesStore.isLoading && !source)
	);
	const isNotFound = $derived(
		!isLoading && !detailQuery.error && sourceId !== '' && !source
	);
	let isPolling = $state(false);

	/**
	 * Bind the detail query to `sourceId` only. Side effects are untracked so
	 * auth/profile updates during fetch cannot re-enter this effect (that loop
	 * froze the tab on Details navigation).
	 */
	$effect(() => {
		const id = sourceId;
		if (!id) {
			untrack(() => detailQuery.stop());
			return;
		}

		untrack(() => {
			detailQuery.reset();
			detailQuery.start();
		});

		return () => {
			detailQuery.stop();
		};
	});

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
	title={source?.display_name ?? sourceId}
	description={source?.id ?? ''}
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
