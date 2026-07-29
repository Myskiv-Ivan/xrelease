<script lang="ts">
	import { onMount } from 'svelte';
	import type { OutboxEntry } from '$lib/api/types';
	import { getOutboxStore } from '$lib/data/outbox.svelte';
	import { getSourcesStore } from '$lib/data/sources.svelte';
	import { getTeamsStore } from '$lib/data/teams.svelte';
	import { getAuthState } from '$lib/stores/auth.svelte';
	import { getNetworkState } from '$lib/stores/network.svelte';
	import { getNowStore } from '$lib/stores/now.svelte';
	import DashboardShell from '$lib/components/layout/DashboardShell.svelte';
	import OrgScopeNotice from '$lib/components/layout/OrgScopeNotice.svelte';
	import OutboxTable from '$lib/components/outbox/OutboxTable.svelte';
	import EmptyState from '$lib/components/kit/EmptyState.svelte';
	import Button from '$lib/components/kit/Button.svelte';
	import Checkbox from '$lib/components/kit/Checkbox.svelte';
	import Select from '$lib/components/kit/Select.svelte';
	import TableToolbar from '$lib/components/kit/TableToolbar.svelte';
	import * as Tabs from '$lib/components/kit/tabs';
	import { exportRowsAsCsv } from '$lib/core/csv';
	import { isDeferredDelivery, requeueDeadOutbox, requeueErrorToast } from '$lib/core/outbox';
	import { scopeToOrganization } from '$lib/data/organization-scope';
	import {
		UNTAGGED_FILTER,
		collectTeamFilterOptions,
		matchesSearch,
		passesSelectFilter,
		passesTeamFilter
	} from '$lib/core/table-filters';
	import { sortByKey } from '$lib/core/sort';
	import type { SortState } from '$lib/core/sort';
	import { t } from '$lib/i18n';
	import { TYPE_HINT } from '$lib/components/kit/layout-styles';

	const auth = getAuthState();
	const network = getNetworkState();
	const now = getNowStore();
	const outboxStore = getOutboxStore();
	const sourcesStore = getSourcesStore();
	const teamsStore = getTeamsStore();

	let sourceFilter = $state('all');
	let statusFilter = $state('all');
	let teamFilter = $state('all');
	let search = $state('');
	let deferredOnly = $state(false);
	let isRequeuing = $state(false);

	const STATUS_TABS = ['all', 'pending', 'failed', 'dead'] as const;

	let sort = $state<SortState<keyof OutboxEntry>>({
		key: 'created_at',
		direction: 'desc'
	});

	onMount(() => {
		if (!auth.isAuthenticated) return;
		outboxStore.start();
		return () => outboxStore.stop();
	});

	$effect(() => {
		if (auth.isAuthenticated) return;
		outboxStore.stop();
	});

	function sourceLabel(sourceId: string): string {
		return sourcesStore.findById(sourceId)?.display_name ?? sourceId;
	}

	function statusLabel(status: string): string {
		if (status === 'all') return t('outboxStatus.all');
		const key = `outboxStatus.${status}`;
		const translated = t(key);
		return translated === key ? status : translated;
	}

	const orgScopedEntries = $derived(
		scopeToOrganization(outboxStore.entries, (entry) => entry.source_id)
	);

	const statusCounts = $derived.by(() => {
		const counts: Record<string, number> = { all: orgScopedEntries.length };
		for (const entry of orgScopedEntries) {
			counts[entry.status] = (counts[entry.status] ?? 0) + 1;
		}
		return counts;
	});

	const deadCount = $derived(statusCounts.dead ?? 0);
	const deferredCount = $derived(
		orgScopedEntries.filter((entry) => isDeferredDelivery(entry, now.current)).length
	);

	const sourceOptions = $derived.by(() => {
		const ids = new Set(orgScopedEntries.map((entry) => entry.source_id));
		return [
			'all',
			...Array.from(ids).sort((left, right) => sourceLabel(left).localeCompare(sourceLabel(right)))
		];
	});

	const teamOptions = $derived(
		collectTeamFilterOptions(orgScopedEntries.map((entry) => entry.routing_tag))
	);

	const filteredEntries = $derived.by(() => {
		return orgScopedEntries.filter((entry) => {
			if (!passesSelectFilter(entry.source_id, sourceFilter)) return false;
			if (!passesSelectFilter(entry.status, statusFilter)) return false;
			if (!passesTeamFilter(entry.routing_tag, teamFilter)) return false;
			if (deferredOnly && !isDeferredDelivery(entry, now.current)) return false;
			return matchesSearch(
				search,
				entry.title,
				entry.identity,
				entry.source_id,
				sourceLabel(entry.source_id),
				entry.routing_tag,
				entry.last_error
			);
		});
	});

	const sortedEntries = $derived(sortByKey(filteredEntries, sort.key, sort.direction));

	function exportCsv() {
		exportRowsAsCsv(
			'xrelease-outbox',
			[
				'id',
				'status',
				'source_id',
				'routing_tag',
				'identity',
				'title',
				'attempts',
				'created_at',
				'deliver_after',
				'last_error',
				'url'
			],
			sortedEntries.map((entry) => [
				entry.id,
				entry.status,
				entry.source_id,
				entry.routing_tag,
				entry.identity,
				entry.title,
				entry.attempts,
				entry.created_at,
				entry.deliver_after,
				entry.last_error,
				entry.url
			])
		);
	}

	async function handleRequeue() {
		if (!auth.hasPermission('outbox:requeue') || deadCount === 0) return;
		isRequeuing = true;
		try {
			await requeueDeadOutbox(() => outboxStore.reload());
		} catch (err) {
			requeueErrorToast(err);
		} finally {
			isRequeuing = false;
		}
	}
</script>

<DashboardShell
	title={t('outbox.title')}
	permission="outbox:read"
	error={outboxStore.error}
	isLoading={outboxStore.isLoading}
	hasContent={orgScopedEntries.length > 0 || outboxStore.entries.length > 0}
	isRefreshing={outboxStore.isRefreshing}
	lastUpdated={outboxStore.lastUpdated}
	onRefresh={() => outboxStore.refresh()}
>
	{#snippet actions()}
		{#if auth.hasPermission('outbox:requeue')}
			<Button
				variant="accent"
				disabled={isRequeuing || deadCount === 0 || !network.isOnline}
				title={t('outbox.requeueHint')}
				onclick={handleRequeue}
			>
				{isRequeuing ? t('outbox.requeuing') : t('outbox.requeueDead')}
				{#if deadCount > 0}
					<span class="tabular-nums opacity-80">({deadCount})</span>
				{/if}
			</Button>
		{/if}
	{/snippet}

	<OrgScopeNotice />

	{#if orgScopedEntries.length === 0}
		<EmptyState title={t('outbox.emptyTitle')} description={t('outbox.emptyDescription')} />
	{:else}
		<Tabs.Root bind:value={statusFilter} class="gap-4">
			<Tabs.List variant="line" class="w-full justify-start">
				{#each STATUS_TABS as status}
					<Tabs.Trigger value={status} class="gap-2">
						{statusLabel(status)}
						<span class="tabular-nums text-muted-foreground">{statusCounts[status] ?? 0}</span>
					</Tabs.Trigger>
				{/each}
			</Tabs.List>

			<TableToolbar
				bind:search
				searchPlaceholder={t('outbox.search')}
				shown={sortedEntries.length}
				total={outboxStore.entries.length}
			>
				{#snippet filters()}
					<Select class="w-auto" bind:value={sourceFilter} aria-label={t('outbox.source')}>
						{#each sourceOptions as sourceId}
							<option value={sourceId}>
								{sourceId === 'all' ? t('common.allSources') : sourceLabel(sourceId)}
							</option>
						{/each}
					</Select>
					<Select class="w-auto" bind:value={teamFilter} aria-label={t('outbox.team')}>
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
					<label class="inline-flex cursor-pointer items-center gap-2 {TYPE_HINT}">
						<Checkbox bind:checked={deferredOnly} />
						<span>
							{t('outbox.deferredOnly')}
							{#if deferredCount > 0}
								<span class="tabular-nums">({deferredCount})</span>
							{/if}
						</span>
					</label>
				{/snippet}
				{#snippet actions()}
					<Button
						variant="outline"
						size="sm"
						disabled={sortedEntries.length === 0}
						onclick={exportCsv}
					>
						{t('common.exportCsv')}
					</Button>
				{/snippet}
			</TableToolbar>

			{#if sortedEntries.length === 0}
				<EmptyState
					title={t('outbox.noMatches')}
					description={t('outbox.noMatchesDescription')}
				/>
			{:else}
				<OutboxTable
					entries={sortedEntries}
					{sort}
					onSortChange={(next) => (sort = next)}
					{sourceLabel}
				/>
			{/if}
		</Tabs.Root>
	{/if}
</DashboardShell>
