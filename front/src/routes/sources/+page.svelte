<script lang="ts">
	import type { SourceDetail } from '$lib/api/types';
	import DashboardShell from '$lib/components/layout/DashboardShell.svelte';
	import SourcesTable from '$lib/components/sources/SourcesTable.svelte';
	import EmptyState from '$lib/components/kit/EmptyState.svelte';
	import Button from '$lib/components/kit/Button.svelte';
	import Select from '$lib/components/kit/Select.svelte';
	import TableToolbar from '$lib/components/kit/TableToolbar.svelte';
	import { getSourceKindMeta } from '$lib/config/source-kinds';
	import {
		latestRelease,
		latestReleaseSortKey,
		type SourceSortKey
	} from '$lib/config/source-presentation';
	import { exportRowsAsCsv } from '$lib/core/csv';
	import type { SortState } from '$lib/core/sort';
	import { compareValues, sortByKey } from '$lib/core/sort';
	import {
		UNTAGGED_FILTER,
		collectTeamFilterOptions,
		matchesSearch,
		passesSelectFilter,
		passesTeamFilter
	} from '$lib/core/table-filters';
	import { belongsToOrganization } from '$lib/core/organization';
	import { getSourcesStore } from '$lib/data/sources.svelte';
	import { getTeamsStore } from '$lib/data/teams.svelte';
	import { getOrganizationsState } from '$lib/stores/organizations.svelte';
	import { getAuthState } from '$lib/stores/auth.svelte';
	import { t } from '$lib/i18n';
	import { TYPE_HINT } from '$lib/components/kit/layout-styles';

	const sourcesStore = getSourcesStore();
	const teamsStore = getTeamsStore();
	const orgs = getOrganizationsState();
	const auth = getAuthState();

	let sort = $state<SortState<SourceSortKey>>({
		key: 'display_name',
		direction: 'asc'
	});
	let kindFilter = $state<string>('all');
	let teamFilter = $state<string>('all');
	let search = $state('');

	/** Multi-org runtime is composed; switcher scopes this view to `{org}::…` ids. */
	const orgScopedSources = $derived.by(() => {
		if (!orgs.isMultiOrg || !orgs.selectedId) return sourcesStore.sources;
		return sourcesStore.sources.filter((source) =>
			belongsToOrganization(source.id, orgs.selectedId)
		);
	});

	const kindOptions = $derived.by(() => {
		const kinds = new Set(orgScopedSources.map((source) => source.kind));
		return ['all', ...Array.from(kinds).sort()];
	});

	const teamOptions = $derived(
		collectTeamFilterOptions(orgScopedSources.map((source) => source.routing_tag))
	);

	const filteredSources = $derived.by(() => {
		return orgScopedSources.filter((source) => {
			if (!passesSelectFilter(source.kind, kindFilter)) return false;
			if (!passesTeamFilter(source.routing_tag, teamFilter)) return false;
			return matchesSearch(
				search,
				source.display_name,
				source.id,
				source.kind_label,
				source.latest_release_tag,
				source.routing_tag
			);
		});
	});

	const sortedSources = $derived.by(() => {
		// The last-release column is derived from `seen_releases`, so it needs its
		// own comparator; every other column is a plain field lookup.
		if (sort.key === 'latest_release_at') {
			return [...filteredSources].sort((left, right) =>
				compareValues(latestReleaseSortKey(left), latestReleaseSortKey(right), sort.direction)
			);
		}
		return sortByKey(filteredSources, sort.key, sort.direction);
	});

	function exportCsv() {
		exportRowsAsCsv(
			'xrelease-sources',
			[
				'id',
				'kind',
				'display_name',
				'routing_tag',
				'latest_release_tag',
				'latest_release_at',
				'last_polled_at',
				'poll_errors',
				'seen_count'
			],
			sortedSources.map((source) => [
				source.id,
				source.kind,
				source.display_name,
				source.routing_tag,
				source.latest_release_tag,
				latestRelease(source).at,
				source.last_polled_at,
				source.poll_errors,
				source.seen_count
			])
		);
	}
</script>

<DashboardShell
	title={t('sources.title')}
	permission="sources:read"
	error={sourcesStore.error}
	isLoading={sourcesStore.isLoading}
	hasContent={orgScopedSources.length > 0 || sourcesStore.sources.length > 0}
	isRefreshing={sourcesStore.isRefreshing}
	lastUpdated={sourcesStore.lastUpdated}
	onRefresh={() => sourcesStore.refresh()}
>
	{#if orgs.isMultiOrg && orgs.selected}
		<p class="mb-3 {TYPE_HINT}">
			{t('org.filterActive').replace('{org}', orgs.selected.name)}
			<span class="text-muted-foreground"> — {t('org.filterAllHint')}</span>
		</p>
	{/if}
	{#if orgScopedSources.length === 0}
		<EmptyState title={t('sources.emptyTitle')} description={t('sources.emptyDescription')}>
			{#snippet actions()}
				{#if auth.hasPermission('config:read')}
					<Button href="/config" variant="outline" size="sm">{t('common.openConfig')}</Button>
				{/if}
			{/snippet}
		</EmptyState>
	{:else}
		<TableToolbar
			bind:search
			searchPlaceholder={t('sources.search')}
			shown={sortedSources.length}
			total={orgScopedSources.length}
		>
			{#snippet filters()}
				<Select class="w-auto" bind:value={kindFilter} aria-label={t('sources.kind')}>
					{#each kindOptions as kind}
						<option value={kind}>
							{kind === 'all' ? t('common.allKinds') : getSourceKindMeta(kind).label}
						</option>
					{/each}
				</Select>
				<Select class="w-auto" bind:value={teamFilter} aria-label={t('sources.team')}>
					{#each teamOptions as team}
						<option value={team}>
							{team === 'all'
								? t('common.allTeams')
								: team === UNTAGGED_FILTER
									? t('common.untagged')
									: (teamsStore.nameFor(team) ?? team)}
						</option>
					{/each}
				</Select>
			{/snippet}
			{#snippet actions()}
				<Button
					variant="outline"
					size="sm"
					disabled={sortedSources.length === 0}
					onclick={exportCsv}
				>
					{t('common.exportCsv')}
				</Button>
			{/snippet}
		</TableToolbar>

		{#if sortedSources.length === 0}
			<EmptyState title={t('sources.noMatches')} description={t('sources.noMatchesDescription')} />
		{:else}
			<SourcesTable sources={sortedSources} {sort} onSortChange={(next) => (sort = next)} />
		{/if}
	{/if}
</DashboardShell>
