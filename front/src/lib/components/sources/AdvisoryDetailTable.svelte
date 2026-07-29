<script lang="ts">
	import Badge from '$lib/components/kit/Badge.svelte';
	import SortHeader from '$lib/components/kit/SortHeader.svelte';
	import TableShell from '$lib/components/kit/TableShell.svelte';
	import {
		TABLE_BODY_CELL,
		TABLE_BODY_ROW,
		TABLE_HEAD_ROW
	} from '$lib/components/kit/table-styles';
	import { TYPE_CODE, TYPE_MUTED } from '$lib/components/kit/layout-styles';
	import {
		bucketCode,
		bucketLabel,
		bucketTone,
		severityBucket,
		type AdvisoryRow,
		type AdvisorySortKey
	} from '$lib/config/advisory-presentation';
	import { toggleSortKey, type SortState } from '$lib/core/sort';
	import { EMPTY_VALUE } from '$lib/core/format';
	import { t } from '$lib/i18n';

	interface Props {
		/** Already filtered and sorted — the page owns both, like SourcesTable. */
		rows: AdvisoryRow[];
		sort: SortState<AdvisorySortKey>;
		onSortChange: (next: SortState<AdvisorySortKey>) => void;
	}

	let { rows, sort, onSortChange }: Props = $props();

	function handleSort(key: AdvisorySortKey) {
		onSortChange(toggleSortKey(sort, key));
	}
</script>

<TableShell>
	<thead class={TABLE_HEAD_ROW}>
		<tr>
			<SortHeader
				label={t('advisories.severityCol')}
				class="w-24"
				active={sort.key === 'severity'}
				direction={sort.direction}
				onclick={() => handleSort('severity')}
			/>
			<SortHeader
				label={t('sources.releaseTag')}
				active={sort.key === 'tag'}
				direction={sort.direction}
				onclick={() => handleSort('tag')}
			/>
			<SortHeader
				label={t('advisories.idCol')}
				active={sort.key === 'id'}
				direction={sort.direction}
				onclick={() => handleSort('id')}
			/>
			<SortHeader label={t('advisories.summaryCol')} />
			<SortHeader label={t('advisories.cvssCol')} />
		</tr>
	</thead>
	<tbody>
		<!--
			Keyed on `tag` + the database-native id: `display_id` collides whenever
			two advisories alias one CVE, and the same advisory legitimately repeats
			across several affected versions.
		-->
		{#each rows as row (`${row.tag}:${row.advisory.id}`)}
			{@const bucket = severityBucket(row.advisory.severity)}
			<tr class="{TABLE_BODY_ROW} align-top">
				<td class="{TABLE_BODY_CELL} whitespace-nowrap">
					<!--
						Letter for continuity with the compact column, word for meaning.
						The letter is `aria-hidden` so the cell is not announced twice.
					-->
					<Badge tone={bucketTone(bucket)} class="px-1.5 font-mono">
						<span aria-hidden="true">{bucketCode(bucket)}</span>
					</Badge>
					<span class="{TYPE_MUTED} ml-1.5">{bucketLabel(bucket)}</span>
				</td>
				<td class="{TABLE_BODY_CELL} max-w-[12rem] truncate {TYPE_CODE}" title={row.tag}>
					{row.tag}
				</td>
				<td class={TABLE_BODY_CELL}>
					{#if row.advisory.url}
						<a
							href={row.advisory.url}
							target="_blank"
							rel="noopener noreferrer"
							class="cursor-pointer font-medium text-primary no-underline hover:underline {TYPE_CODE}"
						>
							{row.advisory.display_id}
						</a>
					{:else}
						<span class={TYPE_CODE}>{row.advisory.display_id}</span>
					{/if}
					{#if row.advisory.id !== row.advisory.display_id}
						<div class="{TYPE_MUTED} {TYPE_CODE}">{row.advisory.id}</div>
					{/if}
				</td>
				<td class="{TABLE_BODY_CELL} max-w-md">
					{#if row.advisory.summary}
						<span class="block">{row.advisory.summary}</span>
					{:else}
						<span class={TYPE_MUTED}>{EMPTY_VALUE}</span>
					{/if}
				</td>
				<td class="{TABLE_BODY_CELL} max-w-[18rem]">
					{#if row.advisory.cvss_vector}
						<span class="block truncate {TYPE_CODE}" title={row.advisory.cvss_vector}>
							{row.advisory.cvss_vector}
						</span>
					{:else}
						<span class={TYPE_MUTED}>{EMPTY_VALUE}</span>
					{/if}
				</td>
			</tr>
		{/each}
	</tbody>
</TableShell>
