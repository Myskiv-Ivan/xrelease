<script lang="ts">
	import { page } from '$app/state';
	import AdvisoryDetailTable from '$lib/components/sources/AdvisoryDetailTable.svelte';
	import AdvisorySeverityCounts from '$lib/components/sources/AdvisorySeverityCounts.svelte';
	import Button from '$lib/components/kit/Button.svelte';
	import DashboardShell from '$lib/components/layout/DashboardShell.svelte';
	import EmptyState from '$lib/components/kit/EmptyState.svelte';
	import Panel from '$lib/components/kit/Panel.svelte';
	import Select from '$lib/components/kit/Select.svelte';
	import TableToolbar from '$lib/components/kit/TableToolbar.svelte';
	import { TYPE_HINT } from '$lib/components/kit/layout-styles';
	import {
		advisoryRowSearchText,
		advisoryRows,
		allAdvisories,
		bucketLabel,
		SEVERITY_BUCKETS,
		severityBucket,
		severityRank,
		sourceSupportsAdvisories,
		type AdvisorySortKey
	} from '$lib/config/advisory-presentation';
	import { matchesSearch, passesSelectFilter } from '$lib/core/table-filters';
	import {
		compareNaturalStrings,
		compareValues,
		type SortState
	} from '$lib/core/sort';
	import { createSourceDetailQuery } from '$lib/data/source-detail.svelte';
	import { getStatusStore } from '$lib/data/status.svelte';
	import { t } from '$lib/i18n';

	const statusStore = getStatusStore();
	const sourceId = $derived(decodeURIComponent(page.params.id ?? ''));
	const detailQuery = createSourceDetailQuery(() => sourceId);
	const source = $derived(detailQuery.data);

	let search = $state('');
	let bucketFilter = $state('all');
	let tagFilter = $state('all');
	/** `order` = the API's order: newest release first, worst finding first. */
	let sort = $state<SortState<AdvisorySortKey>>({ key: 'order', direction: 'asc' });

	const rows = $derived(advisoryRows(source?.seen_releases ?? []));
	const summary = $derived(allAdvisories(source?.seen_releases ?? []));

	/** Versions that actually carry advisories, in the table's release order. */
	const tagOptions = $derived.by(() => {
		const seen = new Set<string>();
		for (const row of rows) seen.add(row.tag);
		return [...seen];
	});

	const filtered = $derived(
		rows.filter(
			(row) =>
				(bucketFilter === 'all' || severityBucket(row.advisory.severity) === bucketFilter) &&
				passesSelectFilter(row.tag, tagFilter) &&
				matchesSearch(search, advisoryRowSearchText(row))
		)
	);

	const sortedRows = $derived.by(() => {
		// Default is the API's meaning-bearing order; no column maps to it, so
		// it simply holds until the first header click replaces it.
		if (sort.key === 'order') return filtered;
		const result = [...filtered];
		switch (sort.key) {
			case 'severity':
				result.sort((a, b) => {
					const diff =
						(severityRank(a.advisory.severity) - severityRank(b.advisory.severity)) *
						(sort.direction === 'asc' ? 1 : -1);
					return diff !== 0 ? diff : a.advisory.display_id.localeCompare(b.advisory.display_id);
				});
				break;
			case 'tag':
				result.sort((a, b) => compareNaturalStrings(a.tag, b.tag, sort.direction));
				break;
			case 'id':
				result.sort((a, b) =>
					compareValues(a.advisory.display_id, b.advisory.display_id, sort.direction)
				);
				break;
		}
		return result;
	});

	/**
	 * Why the table is empty, in the operator's terms. "No CVEs" and "enrichment
	 * is switched off" look identical otherwise, and only one of them is good
	 * news.
	 */
	const emptyDescription = $derived(
		!statusStore.status?.advisories.enabled
			? t('advisories.emptyDisabled')
			: source && !sourceSupportsAdvisories(source.kind)
				? t('advisories.emptyUnsupported')
				: t('advisories.emptyClean')
	);

	const sourceHref = $derived(`/sources/${encodeURIComponent(sourceId)}`);
</script>

<DashboardShell
	title={source?.display_name ?? sourceId}
	description={t('advisories.pageTitle')}
	permission="sources:read"
	error={detailQuery.error}
	isLoading={Boolean(sourceId) && !source && !detailQuery.error && detailQuery.isLoading}
	hasContent={Boolean(source) || Boolean(detailQuery.error)}
	isRefreshing={detailQuery.isRefreshing}
	lastUpdated={detailQuery.lastUpdated}
	onRefresh={() => detailQuery.reload()}
>
	{#snippet actions()}
		<Button variant="outline" href={sourceHref}>{t('advisories.backToSource')}</Button>
	{/snippet}

	{#if source}
		<Panel title={t('sources.advisories')}>
			{#snippet actions()}
				<AdvisorySeverityCounts advisories={summary} title={t('advisories.panelSummary')} />
			{/snippet}

			{#if rows.length === 0}
				<EmptyState
					variant="embedded"
					title={t('advisories.emptyTitle')}
					description={emptyDescription}
				/>
			{:else}
				<TableToolbar
					bind:search
					searchPlaceholder={t('advisories.search')}
					shown={sortedRows.length}
					total={rows.length}
				>
					{#snippet filters()}
						<Select
							class="w-auto"
							bind:value={bucketFilter}
							aria-label={t('advisories.severityCol')}
						>
							<option value="all">{t('advisories.allSeverities')}</option>
							{#each SEVERITY_BUCKETS as bucket (bucket)}
								<option value={bucket}>{bucketLabel(bucket)}</option>
							{/each}
						</Select>
						<Select class="w-auto" bind:value={tagFilter} aria-label={t('sources.releaseTag')}>
							<option value="all">{t('advisories.allTags')}</option>
							{#each tagOptions as tag (tag)}
								<option value={tag}>{tag}</option>
							{/each}
						</Select>
					{/snippet}
				</TableToolbar>

				{#if sortedRows.length === 0}
					<EmptyState
						variant="embedded"
						title={t('sources.noMatches')}
						description={t('sources.noMatchesDescription')}
					/>
				{:else}
					<AdvisoryDetailTable
						rows={sortedRows}
						{sort}
						onSortChange={(next) => (sort = next)}
					/>
				{/if}

				<p class="{TYPE_HINT} mt-3">{t('advisories.sourceNote')}</p>
			{/if}
		</Panel>
	{/if}
</DashboardShell>
