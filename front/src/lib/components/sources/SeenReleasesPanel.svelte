<script lang="ts">
	import type { SeenReleaseView } from '$lib/api/types';
	import Panel from '$lib/components/kit/Panel.svelte';
	import EmptyState from '$lib/components/kit/EmptyState.svelte';
	import Input from '$lib/components/kit/Input.svelte';
	import TableShell from '$lib/components/kit/TableShell.svelte';
	import {
		TABLE_BODY_CELL,
		TABLE_BODY_ROW,
		TABLE_DATE_CELL,
		TABLE_HEAD_CELL,
		TABLE_HEAD_ROW
	} from '$lib/components/kit/table-styles';
	import { TYPE_CODE } from '$lib/components/kit/layout-styles';
	import { matchesSearch } from '$lib/core/table-filters';
	import { t } from '$lib/i18n';
	import { EMPTY_VALUE, formatDateTime } from '$lib/core/format';
	import RelativeTime from '$lib/components/kit/RelativeTime.svelte';

	interface Props {
		releases: SeenReleaseView[];
		seenCount: number;
	}

	let { releases, seenCount }: Props = $props();
	let search = $state('');

	const filteredReleases = $derived.by(() =>
		releases.filter((release) => matchesSearch(search, release.tag, release.url))
	);
</script>

<Panel title="{t('sources.seenReleases')} ({seenCount})">
	{#if releases.length === 0}
		<EmptyState
			variant="embedded"
			title={t('sources.noSyncedReleases')}
			description={t('sources.awaitingBaseline')}
		/>
	{:else}
		<div class="mb-3">
			<Input
				type="search"
				placeholder={t('sources.releaseSearch')}
				class="w-full sm:max-w-xs"
				bind:value={search}
			/>
		</div>

		{#if filteredReleases.length === 0}
			<EmptyState
				variant="embedded"
				title={t('sources.noMatches')}
				description={t('sources.noMatchesDescription')}
			/>
		{:else}
			<TableShell bordered={false} class="-mx-1">
				<thead class={TABLE_HEAD_ROW}>
					<tr>
						<th class="{TABLE_HEAD_CELL} w-12">#</th>
						<th class={TABLE_HEAD_CELL}>{t('sources.releaseTag')}</th>
						<th class={TABLE_HEAD_CELL}>{t('sources.releaseLink')}</th>
						<th class={TABLE_HEAD_CELL}>{t('sources.releasedAt')}</th>
						<th class={TABLE_HEAD_CELL}>{t('sources.syncedAt')}</th>
					</tr>
				</thead>
				<tbody>
					<!--
						Keyed by position, not by tag: `seen_release` is keyed on
						(source_id, identity) server-side and `tag` is the display label, so
						two rows can legitimately share one tag (a re-pushed container tag
						pointing at a new digest). Keying on it throws `each_key_duplicate`
						and blanks the page.
					-->
					{#each filteredReleases as release, index (index)}
						<tr class={TABLE_BODY_ROW}>
							<td class="{TABLE_BODY_CELL} text-muted-foreground tabular-nums">{index + 1}</td>
							<td class="{TABLE_BODY_CELL} max-w-[14rem] truncate {TYPE_CODE}" title={release.tag}>
								{release.tag}
							</td>
							<td class={TABLE_BODY_CELL}>
								{#if release.url}
									<a
										href={release.url}
										target="_blank"
										rel="noopener noreferrer"
										class="text-xs font-medium text-primary no-underline hover:underline"
									>
										{t('sources.releaseLink')}
									</a>
								{:else}
									<span class="text-muted-foreground">{EMPTY_VALUE}</span>
								{/if}
							</td>
							<td
								class={TABLE_DATE_CELL}
								title={release.published_at
									? new Date(release.published_at).toLocaleString()
									: t('sources.releaseDateUnknown')}
							>
								{formatDateTime(release.published_at)}
							</td>
							<td class={TABLE_DATE_CELL}>
								<RelativeTime value={release.first_seen_at} />
							</td>
						</tr>
					{/each}
				</tbody>
			</TableShell>
		{/if}
	{/if}
</Panel>
