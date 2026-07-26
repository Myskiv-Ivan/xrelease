<script lang="ts">
	import type { SourceDetail } from '$lib/api/types';
	import SourceKindBadge from '$lib/components/sources/SourceKindBadge.svelte';
	import TeamBadge from '$lib/components/teams/TeamBadge.svelte';
	import SortHeader from '$lib/components/kit/SortHeader.svelte';
	import TableShell from '$lib/components/kit/TableShell.svelte';
	import {
		TABLE_BODY_CELL,
		TABLE_BODY_ROW,
		TABLE_DATE_CELL,
		TABLE_HEAD_ROW,
		TABLE_STICKY_END,
		TABLE_STICKY_END_HEAD
	} from '$lib/components/kit/table-styles';
	import { TYPE_CODE, TYPE_MUTED } from '$lib/components/kit/layout-styles';
	import type { SortState } from '$lib/core/sort';
	import { toggleSortKey } from '$lib/core/sort';
	import {
		latestRelease,
		pollErrorTone,
		sourceHealth,
		type SourceSortKey
	} from '$lib/config/source-presentation';
	import { toneMutedTextClass } from '$lib/components/kit/surface-styles';
	import { t } from '$lib/i18n';
	import { EMPTY_VALUE, formatDateTime, formatInterval, formatNumber } from '$lib/core/format';
	import { cn } from '$lib/utils';

	interface Props {
		sources: SourceDetail[];
		sort: SortState<SourceSortKey>;
		onSortChange: (next: SortState<SourceSortKey>) => void;
	}

	let { sources, sort, onSortChange }: Props = $props();

	function handleSort(key: SourceSortKey) {
		onSortChange(toggleSortKey(sort, key));
	}

	function healthLabel(health: ReturnType<typeof sourceHealth>): string {
		if (health === 'error') return t('sources.healthError');
		if (health === 'warning') return t('sources.healthStale');
		return t('sources.healthOk');
	}
</script>

<TableShell>
	<thead class={TABLE_HEAD_ROW}>
		<tr>
			<SortHeader
				label={t('sources.kind')}
				active={sort.key === 'kind'}
				direction={sort.direction}
				onclick={() => handleSort('kind')}
			/>
			<SortHeader
				label={t('sources.name')}
				active={sort.key === 'display_name'}
				direction={sort.direction}
				onclick={() => handleSort('display_name')}
			/>
				<SortHeader
					label={t('sources.team')}
					active={sort.key === 'routing_tag'}
					direction={sort.direction}
					onclick={() => handleSort('routing_tag')}
				/>
				<SortHeader label={t('sources.latestRelease')} />
				<SortHeader
					label={t('sources.lastRelease')}
					active={sort.key === 'latest_release_at'}
					direction={sort.direction}
					onclick={() => handleSort('latest_release_at')}
				/>
				<SortHeader label={t('sources.interval')} />
				<SortHeader
					label={t('sources.lastPoll')}
					active={sort.key === 'last_polled_at'}
					direction={sort.direction}
					onclick={() => handleSort('last_polled_at')}
				/>
				<SortHeader
					label={t('sources.seen')}
					active={sort.key === 'seen_count'}
					direction={sort.direction}
					onclick={() => handleSort('seen_count')}
				/>
				<SortHeader
					label={t('sources.errors')}
					active={sort.key === 'poll_errors'}
					direction={sort.direction}
					onclick={() => handleSort('poll_errors')}
				/>
				<SortHeader label={t('sources.details')} class="text-right {TABLE_STICKY_END_HEAD}" />
			</tr>
		</thead>
		<tbody>
			{#each sources as source (source.id)}
				{@const health = sourceHealth(source)}
				{@const release = latestRelease(source)}
				<tr class={TABLE_BODY_ROW}>
					<td class={cn(TABLE_BODY_CELL, 'whitespace-nowrap')}>
						<SourceKindBadge kind={source.kind} label={source.kind_label} />
					</td>
						<td class={TABLE_BODY_CELL}>
							<div class="flex items-start gap-2">
								<span
									class="mt-1.5 inline-flex size-2 shrink-0 rounded-full
										{health === 'ok'
										? 'bg-success'
										: health === 'warning'
											? 'bg-warning'
											: 'bg-destructive'}
										{health === 'ok' ? 'shadow-[0_0_0_3px] shadow-success/25' : ''}"
									title={healthLabel(health)}
									aria-label={healthLabel(health)}
								></span>
								<div class="min-w-0">
									<a
										href="/sources/{encodeURIComponent(source.id)}"
										class="block font-medium text-foreground no-underline hover:underline"
									>
										{source.display_name}
									</a>
									<div class={TYPE_MUTED}>{source.id}</div>
								</div>
							</div>
						</td>
					<td class={TABLE_BODY_CELL}>
						{#if source.routing_tag}
							<TeamBadge tag={source.routing_tag} />
						{:else}
							<span class={TYPE_MUTED}>{EMPTY_VALUE}</span>
						{/if}
					</td>
					<td class="{TABLE_BODY_CELL} {TYPE_CODE}">
						{source.latest_release_tag ?? EMPTY_VALUE}
					</td>
					<td class={TABLE_DATE_CELL}>
						{#if release.at}
							<span
								class={release.published ? '' : 'italic'}
								title={release.published
									? t('sources.lastReleasePublished')
									: t('sources.lastReleaseSynced')}
							>
								{formatDateTime(release.at)}
							</span>
						{:else}
							{EMPTY_VALUE}
						{/if}
					</td>
					<td class="{TABLE_BODY_CELL} whitespace-nowrap text-muted-foreground tabular-nums">
						{formatInterval(source.interval_secs)}
					</td>
					<td class={TABLE_DATE_CELL}>
						{formatDateTime(source.last_polled_at)}
					</td>
					<td class="{TABLE_BODY_CELL} tabular-nums">{formatNumber(source.seen_count)}</td>
					<td class="{TABLE_BODY_CELL} tabular-nums">
						<span class={toneMutedTextClass[pollErrorTone(source.poll_errors)]}>
							{formatNumber(source.poll_errors)}
						</span>
					</td>
					<td class={cn(TABLE_BODY_CELL, TABLE_STICKY_END, 'whitespace-nowrap text-right')}>
						<a
							href="/sources/{encodeURIComponent(source.id)}"
							class="inline-flex cursor-pointer items-center gap-1 text-sm font-medium whitespace-nowrap no-underline hover:underline"
						>
							{t('sources.details')}
							<span aria-hidden="true">→</span>
						</a>
					</td>
				</tr>
			{/each}
		</tbody>
	</TableShell>
