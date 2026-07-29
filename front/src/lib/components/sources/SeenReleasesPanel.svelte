<script lang="ts">
	import type { SeenReleaseView } from '$lib/api/types';
	import Checkbox from '$lib/components/kit/Checkbox.svelte';
	import Panel from '$lib/components/kit/Panel.svelte';
	import EmptyState from '$lib/components/kit/EmptyState.svelte';
	import SortHeader from '$lib/components/kit/SortHeader.svelte';
	import TableShell from '$lib/components/kit/TableShell.svelte';
	import TableToolbar from '$lib/components/kit/TableToolbar.svelte';
	import AdvisorySeverityCounts from '$lib/components/sources/AdvisorySeverityCounts.svelte';
	import {
		TABLE_BODY_CELL,
		TABLE_BODY_ROW,
		TABLE_DATE_CELL,
		TABLE_HEAD_ROW
	} from '$lib/components/kit/table-styles';
	import { TYPE_CODE, TYPE_HINT, TYPE_MUTED } from '$lib/components/kit/layout-styles';
	import {
		advisorySearchText,
		allAdvisories,
		releaseAdvisories,
		severityRank
	} from '$lib/config/advisory-presentation';
	import { matchesSearch } from '$lib/core/table-filters';
	import {
		compareNaturalStrings,
		compareNullableDates,
		toggleSortKey,
		type SortState
	} from '$lib/core/sort';
	import { t } from '$lib/i18n';
	import { EMPTY_VALUE } from '$lib/core/format';
	import Timestamp from '$lib/components/kit/Timestamp.svelte';

	interface Props {
		/** Owning source id — links the severity column to the advisories page. */
		sourceId: string;
		releases: SeenReleaseView[];
		seenCount: number;
		/**
		 * When true, keep the Advisories column visible even before any CVE is
		 * cached (package source + `[advisories]` on). Otherwise the column only
		 * appears once at least one release carries advisories.
		 */
		expectAdvisories?: boolean;
	}

	let { sourceId, releases, seenCount, expectAdvisories = false }: Props = $props();

	/** `order` = the API's newest-version-first position (the `#` column). */
	type ReleaseSortKey = 'order' | 'tag' | 'published_at' | 'first_seen_at' | 'advisories';

	let search = $state('');
	let advisoriesOnly = $state(false);
	let sort = $state<SortState<ReleaseSortKey>>({ key: 'order', direction: 'asc' });

	/**
	 * Rows carry their server position: it is the row identity for keying (tags
	 * can legitimately repeat — a re-pushed container tag pointing at a new
	 * digest, so `tag` would throw `each_key_duplicate`), and it is what the `#`
	 * column shows, so a row keeps its recency rank however the table is sorted.
	 */
	const indexedRows = $derived(releases.map((release, order) => ({ release, order })));

	const filteredRows = $derived(
		indexedRows.filter(
			({ release }) =>
				(!advisoriesOnly || releaseAdvisories(release).length > 0) &&
				matchesSearch(search, release.tag, release.url, advisorySearchText(release))
		)
	);

	/** Worst bucket first, count as tiebreak — what the badge strip communicates. */
	function advisorySortRank(release: SeenReleaseView): [number, number] {
		const list = releaseAdvisories(release);
		if (list.length === 0) return [-2, 0];
		return [Math.max(...list.map((advisory) => severityRank(advisory.severity))), list.length];
	}

	const sortedRows = $derived.by(() => {
		const rows = [...filteredRows];
		const factor = sort.direction === 'asc' ? 1 : -1;
		switch (sort.key) {
			case 'order':
				rows.sort((a, b) => (a.order - b.order) * factor);
				break;
			case 'tag':
				rows.sort((a, b) => compareNaturalStrings(a.release.tag, b.release.tag, sort.direction));
				break;
			case 'published_at':
				rows.sort((a, b) =>
					compareNullableDates(a.release.published_at, b.release.published_at, sort.direction)
				);
				break;
			case 'first_seen_at':
				rows.sort((a, b) =>
					compareNullableDates(a.release.first_seen_at, b.release.first_seen_at, sort.direction)
				);
				break;
			case 'advisories':
				rows.sort((a, b) => {
					const [rankA, countA] = advisorySortRank(a.release);
					const [rankB, countB] = advisorySortRank(b.release);
					return (rankA - rankB || countA - countB) * factor || a.order - b.order;
				});
				break;
		}
		return rows;
	});

	function handleSort(key: ReleaseSortKey) {
		sort = toggleSortKey(sort, key);
	}

	/** Flattened across every synced release, for the header summary strip. */
	const summary = $derived(allAdvisories(releases));

	/**
	 * Show the column when we expect advisories for this source kind, or when
	 * anything already carries them. Reading `releases` (not `filteredRows`)
	 * so the column does not appear/disappear as the user types.
	 */
	const showAdvisories = $derived(expectAdvisories || summary.length > 0);
	const advisoriesHref = $derived(`/sources/${encodeURIComponent(sourceId)}/advisories`);
</script>

<Panel title="{t('sources.seenReleases')} ({seenCount})">
	{#snippet actions()}
		{#if showAdvisories}
			<AdvisorySeverityCounts advisories={summary} title={t('advisories.panelSummary')} />
			<a
				href={advisoriesHref}
				class="cursor-pointer text-xs font-medium text-primary no-underline hover:underline"
			>
				{t('advisories.viewDetails')}
			</a>
		{/if}
	{/snippet}

	{#if releases.length === 0}
		<EmptyState
			variant="embedded"
			title={t('sources.noSyncedReleases')}
			description={t('sources.awaitingBaseline')}
		/>
	{:else}
		<TableToolbar
			bind:search
			searchPlaceholder={t('sources.releaseSearch')}
			shown={sortedRows.length}
			total={releases.length}
		>
			{#snippet filters()}
				{#if showAdvisories}
					<label class="inline-flex cursor-pointer items-center gap-2 {TYPE_HINT}">
						<Checkbox bind:checked={advisoriesOnly} />
						{t('sources.advisoriesOnly')}
					</label>
				{/if}
			{/snippet}
		</TableToolbar>

		{#if sortedRows.length === 0}
			<EmptyState
				variant="embedded"
				title={t('sources.noMatches')}
				description={t('sources.noMatchesDescription')}
			/>
		{:else}
			<TableShell bordered={false} class="-mx-1">
				<thead class={TABLE_HEAD_ROW}>
					<tr>
						<SortHeader
							label="#"
							class="w-12"
							active={sort.key === 'order'}
							direction={sort.direction}
							onclick={() => handleSort('order')}
						/>
						<SortHeader
							label={t('sources.releaseTag')}
							active={sort.key === 'tag'}
							direction={sort.direction}
							onclick={() => handleSort('tag')}
						/>
						<SortHeader label={t('sources.releaseLink')} />
						{#if showAdvisories}
							<SortHeader
								label={t('sources.advisories')}
								class="w-40"
								active={sort.key === 'advisories'}
								direction={sort.direction}
								onclick={() => handleSort('advisories')}
							/>
						{/if}
						<SortHeader
							label={t('sources.releasedAt')}
							active={sort.key === 'published_at'}
							direction={sort.direction}
							onclick={() => handleSort('published_at')}
						/>
						<SortHeader
							label={t('sources.syncedAt')}
							active={sort.key === 'first_seen_at'}
							direction={sort.direction}
							onclick={() => handleSort('first_seen_at')}
						/>
					</tr>
				</thead>
				<tbody>
					{#each sortedRows as { release, order } (order)}
						{@const advisories = releaseAdvisories(release)}
						<tr class={TABLE_BODY_ROW}>
							<td class="{TABLE_BODY_CELL} text-muted-foreground tabular-nums">{order + 1}</td>
							<td class="{TABLE_BODY_CELL} max-w-[14rem] truncate {TYPE_CODE}" title={release.tag}>
								{release.tag}
							</td>
							<td class={TABLE_BODY_CELL}>
								{#if release.url}
									<a
										href={release.url}
										target="_blank"
										rel="noopener noreferrer"
										class="cursor-pointer text-xs font-medium text-primary no-underline hover:underline"
									>
										{t('sources.releaseLink')}
									</a>
								{:else}
									<span class="text-muted-foreground">{EMPTY_VALUE}</span>
								{/if}
							</td>
							{#if showAdvisories}
								<!--
									Counts only — the ids, summaries, and CVSS vectors live on the
									advisories page. Rendering one badge per CVE here made a row
									with a handful of findings several lines tall and pushed the
									date columns off screen, which is what the release table is
									actually for.
								-->
								<td class={TABLE_BODY_CELL}>
									{#if advisories.length > 0}
										<AdvisorySeverityCounts {advisories} href={advisoriesHref} />
									{:else}
										<span class="text-muted-foreground">{EMPTY_VALUE}</span>
									{/if}
								</td>
							{/if}
							<td class={TABLE_DATE_CELL}>
								{#if release.published_at}
									<Timestamp value={release.published_at} />
								{:else}
									<span class="text-muted-foreground" title={t('sources.releaseDateUnknown')}>
										{EMPTY_VALUE}
									</span>
								{/if}
							</td>
							<td class={TABLE_DATE_CELL}>
								<Timestamp value={release.first_seen_at} />
							</td>
						</tr>
					{/each}
				</tbody>
			</TableShell>
		{/if}

		{#if showAdvisories && summary.length === 0}
			<p class="{TYPE_MUTED} mt-3">{t('advisories.noneKnownHint')}</p>
		{/if}
	{/if}
</Panel>
