<script lang="ts">
	import type { ConfigRevisionSummary } from '$lib/api/types';
	import Badge from '$lib/components/kit/Badge.svelte';
	import EmptyState from '$lib/components/kit/EmptyState.svelte';
	import Panel from '$lib/components/kit/Panel.svelte';
	import RelativeTime from '$lib/components/kit/RelativeTime.svelte';
	import TableShell from '$lib/components/kit/TableShell.svelte';
	import {
		TABLE_BODY_CELL,
		TABLE_BODY_ROW,
		TABLE_DATE_CELL,
		TABLE_HEAD_CELL,
		TABLE_HEAD_ROW
	} from '$lib/components/kit/table-styles';
	import { TYPE_CODE, TYPE_FIELD_ERROR, TYPE_MUTED } from '$lib/components/kit/layout-styles';
	import { EMPTY_VALUE } from '$lib/core/format';
	import { t } from '$lib/i18n';

	interface Props {
		revisions: ConfigRevisionSummary[];
		total: number;
	}

	let { revisions, total }: Props = $props();
</script>

<Panel title={t('config.history')}>
	{#snippet actions()}
		{#if total > 0}
			<span class="{TYPE_MUTED} tabular-nums">
				{total} {t('config.historyTotal')}
			</span>
		{/if}
	{/snippet}

	{#if revisions.length === 0}
		<EmptyState title={t('config.historyEmpty')} />
	{:else}
		<TableShell>
			<thead class={TABLE_HEAD_ROW}>
				<tr>
					<th class={TABLE_HEAD_CELL}>{t('config.revision')}</th>
					<th class={TABLE_HEAD_CELL}>{t('outbox.status')}</th>
					<th class={TABLE_HEAD_CELL}>{t('config.revisionLabel')}</th>
					<th class={TABLE_HEAD_CELL}>{t('config.appliedBy')}</th>
					<th class={TABLE_HEAD_CELL}>{t('config.appliedAt')}</th>
					<th class={TABLE_HEAD_CELL}>{t('config.contentSha')}</th>
					<th class={TABLE_HEAD_CELL}>{t('config.error')}</th>
				</tr>
			</thead>
			<tbody>
				{#each revisions as rev (rev.id)}
					<tr class="{TABLE_BODY_ROW} align-middle">
						<td class="{TABLE_BODY_CELL} {TYPE_CODE}">{rev.id}</td>
						<td class={TABLE_BODY_CELL}>
							<Badge tone={rev.status === 'applied' ? 'success' : 'danger'}>
								{rev.status === 'applied'
									? t('config.statusApplied')
									: t('config.statusRejected')}
							</Badge>
						</td>
						<td class="{TABLE_BODY_CELL} max-w-[10rem] truncate" title={rev.revision_label ?? undefined}>
							{rev.revision_label ?? EMPTY_VALUE}
						</td>
						<td class="{TABLE_BODY_CELL} max-w-[9rem] truncate" title={rev.applied_by ?? undefined}>
							{rev.applied_by ?? EMPTY_VALUE}
						</td>
						<td class={TABLE_DATE_CELL}>
							<RelativeTime value={rev.applied_at} />
						</td>
						<td class="{TABLE_BODY_CELL} {TYPE_CODE}" title={rev.content_sha256}>
							{rev.content_sha256.slice(0, 12)}…
						</td>
						<td
							class="max-w-xs {TABLE_BODY_CELL} truncate {TYPE_FIELD_ERROR}"
							title={rev.error ?? undefined}
						>
							{rev.error ?? EMPTY_VALUE}
						</td>
					</tr>
				{/each}
			</tbody>
		</TableShell>
	{/if}
</Panel>
