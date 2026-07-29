<script lang="ts">
	import type { OutboxEntry } from '$lib/api/types';
	import TeamBadge from '$lib/components/teams/TeamBadge.svelte';
	import Badge from '$lib/components/kit/Badge.svelte';
	import Timestamp from '$lib/components/kit/Timestamp.svelte';
	import SortHeader from '$lib/components/kit/SortHeader.svelte';
	import TableShell from '$lib/components/kit/TableShell.svelte';
	import {
		TABLE_BODY_CELL,
		TABLE_BODY_ROW,
		TABLE_DATE_CELL,
		TABLE_HEAD_ROW
	} from '$lib/components/kit/table-styles';
	import { TYPE_CODE, TYPE_FIELD_ERROR, TYPE_MUTED } from '$lib/components/kit/layout-styles';
	import type { SortState } from '$lib/core/sort';
	import { toggleSortKey } from '$lib/core/sort';
	import { t } from '$lib/i18n';
	import { EMPTY_VALUE } from '$lib/core/format';

	interface Props {
		entries: OutboxEntry[];
		sort: SortState<keyof OutboxEntry>;
		onSortChange: (next: SortState<keyof OutboxEntry>) => void;
		sourceLabel: (sourceId: string) => string;
	}

	let { entries, sort, onSortChange, sourceLabel }: Props = $props();

	const statusTone: Record<OutboxEntry['status'], 'warning' | 'danger' | 'muted'> = {
		pending: 'warning',
		failed: 'danger',
		dead: 'muted'
	};

	function handleSort(key: keyof OutboxEntry) {
		onSortChange(toggleSortKey(sort, key));
	}
</script>

<TableShell>
	<thead class={TABLE_HEAD_ROW}>
		<tr>
			<SortHeader
				label={t('outbox.status')}
				active={sort.key === 'status'}
				direction={sort.direction}
				onclick={() => handleSort('status')}
			/>
			<SortHeader
				label={t('outbox.source')}
				active={sort.key === 'source_id'}
				direction={sort.direction}
				onclick={() => handleSort('source_id')}
			/>
			<SortHeader
				label={t('outbox.team')}
				active={sort.key === 'routing_tag'}
				direction={sort.direction}
				onclick={() => handleSort('routing_tag')}
			/>
			<SortHeader
				label={t('outbox.identity')}
				active={sort.key === 'identity'}
				direction={sort.direction}
				onclick={() => handleSort('identity')}
			/>
			<SortHeader label={t('outbox.titleCol')} />
			<SortHeader
				label={t('outbox.attempts')}
				active={sort.key === 'attempts'}
				direction={sort.direction}
				onclick={() => handleSort('attempts')}
			/>
			<SortHeader
				label={t('outbox.created')}
				active={sort.key === 'created_at'}
				direction={sort.direction}
				onclick={() => handleSort('created_at')}
			/>
			<SortHeader
				label={t('outbox.deliverAfter')}
				active={sort.key === 'deliver_after'}
				direction={sort.direction}
				onclick={() => handleSort('deliver_after')}
			/>
			<SortHeader label={t('outbox.error')} />
		</tr>
	</thead>
	<tbody>
		{#each entries as entry (entry.id)}
			{@const label = sourceLabel(entry.source_id)}
			<tr class="{TABLE_BODY_ROW} align-top">
				<td class={TABLE_BODY_CELL}>
					<Badge tone={statusTone[entry.status]}>{t(`outboxStatus.${entry.status}`)}</Badge>
				</td>
				<td class={TABLE_BODY_CELL}>
					<a
						href="/sources/{encodeURIComponent(entry.source_id)}"
						class="block cursor-pointer no-underline hover:underline"
					>
						<div class="font-medium">{label}</div>
						{#if label !== entry.source_id}
							<div class={TYPE_MUTED}>{entry.source_id}</div>
						{/if}
					</a>
				</td>
				<td class={TABLE_BODY_CELL}>
					{#if entry.routing_tag}
						<TeamBadge tag={entry.routing_tag} />
					{:else}
						<span class={TYPE_MUTED}>{EMPTY_VALUE}</span>
					{/if}
				</td>
				<td class="{TABLE_BODY_CELL} max-w-[14rem] truncate {TYPE_CODE}" title={entry.identity}>
					{entry.identity}
				</td>
				<td class="{TABLE_BODY_CELL} max-w-xs">
					{#if entry.url}
						<a
							href={entry.url}
							target="_blank"
							rel="noopener noreferrer"
							class="block cursor-pointer truncate whitespace-nowrap no-underline hover:underline"
							title={entry.title}
						>
							{entry.title}
						</a>
					{:else}
						<span class="block truncate whitespace-nowrap" title={entry.title}>{entry.title}</span>
					{/if}
				</td>
				<td class="{TABLE_BODY_CELL} tabular-nums">{entry.attempts}</td>
				<td class={TABLE_DATE_CELL}>
					<Timestamp value={entry.created_at} />
				</td>
				<td class={TABLE_DATE_CELL}>
					{#if entry.deliver_after}
						<Timestamp value={entry.deliver_after} />
					{:else}
						<span class={TYPE_MUTED}>{t('outbox.deliverSoon')}</span>
					{/if}
				</td>
				<td
					class="max-w-xs {TABLE_BODY_CELL} truncate {TYPE_FIELD_ERROR}"
					title={entry.last_error ?? undefined}
				>
					{entry.last_error ?? EMPTY_VALUE}
				</td>
			</tr>
		{/each}
	</tbody>
</TableShell>
