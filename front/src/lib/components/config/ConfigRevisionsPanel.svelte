<script lang="ts">
	import type { ConfigRevisionSummary } from '#lib/api/types.js';
	import Badge from '#lib/components/kit/Badge.svelte';
	import Button from '#lib/components/kit/Button.svelte';
	import EmptyState from '#lib/components/kit/EmptyState.svelte';
	import Panel from '#lib/components/kit/Panel.svelte';
	import Select from '#lib/components/kit/Select.svelte';
	import Timestamp from '#lib/components/kit/Timestamp.svelte';
	import TableShell from '#lib/components/kit/TableShell.svelte';
	import {
		TABLE_BODY_CELL,
		TABLE_BODY_ROW,
		TABLE_DATE_CELL,
		TABLE_HEAD_CELL,
		TABLE_HEAD_ROW
	} from '#lib/components/kit/table-styles.js';
	import { TYPE_CODE, TYPE_FIELD_ERROR, TYPE_MUTED } from '#lib/components/kit/layout-styles.js';
	import { EMPTY_VALUE } from '#lib/core/format.js';
	import { parseAppliedBy, type AppliedBy } from '#lib/config/applied-by.js';
	import {
		REVISIONS_PAGE_ALL,
		REVISIONS_PAGE_DEFAULT
	} from '#lib/data/config-resources.svelte.js';
	import { t } from '#lib/i18n/index.js';

	interface Props {
		revisions: ConfigRevisionSummary[];
		total: number;
		limit?: number;
		offset?: number;
		/** Server window change — parent reloads the ledger page. */
		onWindowChange?: (limit: number, offset: number) => void | Promise<void>;
	}

	let {
		revisions,
		total,
		limit = REVISIONS_PAGE_DEFAULT,
		offset = 0,
		onWindowChange
	}: Props = $props();

	const pageSizeKey = $derived(limit >= REVISIONS_PAGE_ALL ? 'all' : String(limit));
	const rangeStart = $derived(total === 0 ? 0 : offset + 1);
	const rangeEnd = $derived(Math.min(offset + revisions.length, total));
	const canPrev = $derived(offset > 0);
	const canNext = $derived(offset + revisions.length < total);

	function originLabel(origin: AppliedBy['origin']): string {
		if (origin === 'local') return t('config.originLocal');
		if (origin === 'oidc') return t('config.originOidc');
		if (origin === 'api') return t('config.originApi');
		if (origin === 'anonymous') return t('config.originAnonymous');
		return t('config.originUnknown');
	}

	function resolveLimit(key: string): number {
		if (key === 'all') return REVISIONS_PAGE_ALL;
		const n = Number(key);
		return Number.isFinite(n) && n > 0 ? n : REVISIONS_PAGE_DEFAULT;
	}

	function changePageSize(key: string) {
		void onWindowChange?.(resolveLimit(key), 0);
	}

	function goPrev() {
		const nextOffset = Math.max(0, offset - limit);
		void onWindowChange?.(limit, nextOffset);
	}

	function goNext() {
		void onWindowChange?.(limit, offset + limit);
	}
</script>

<Panel title={t('config.history')}>
	{#snippet actions()}
		{#if total > 0}
			<span class="{TYPE_MUTED} tabular-nums">
				{total} {t('config.historyTotal')}
			</span>
		{/if}
	{/snippet}

	{#if revisions.length === 0 && total === 0}
		<EmptyState title={t('config.historyEmpty')} />
	{:else}
		<div class="mb-3 flex flex-wrap items-center gap-3">
			<span class="{TYPE_MUTED} tabular-nums text-sm">
				{t('config.historyRange')
					.replace('{start}', String(rangeStart))
					.replace('{end}', String(rangeEnd))
					.replace('{total}', String(total))}
			</span>
			{#if onWindowChange}
				<div class="ml-auto flex flex-wrap items-center gap-2">
					<label class="flex items-center gap-2 text-sm">
						<span class="{TYPE_MUTED} text-xs whitespace-nowrap">
							{t('config.historyPageSize')}
						</span>
						<Select
							class="h-9 w-auto min-w-[4.5rem]"
							aria-label={t('config.historyPageSize')}
							value={pageSizeKey}
							onchange={(e) => changePageSize(e.currentTarget.value)}
						>
							<option value="10">10</option>
							<option value="20">20</option>
							<option value="50">50</option>
							<option value="all">{t('config.historyPageAll')}</option>
						</Select>
					</label>
					<Button variant="ghost" size="sm" disabled={!canPrev} onclick={goPrev}>
						{t('config.historyPrev')}
					</Button>
					<Button variant="ghost" size="sm" disabled={!canNext} onclick={goNext}>
						{t('config.historyNext')}
					</Button>
				</div>
			{/if}
		</div>

		{#if revisions.length === 0}
			<p class={TYPE_MUTED}>{t('config.historyPageEmpty')}</p>
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
						{@const by = parseAppliedBy(rev.applied_by)}
						<tr class="{TABLE_BODY_ROW} align-middle">
							<td class="{TABLE_BODY_CELL} {TYPE_CODE}">{rev.id}</td>
							<td class={TABLE_BODY_CELL}>
								<Badge tone={rev.status === 'applied' ? 'success' : 'danger'}>
									{rev.status === 'applied'
										? t('config.statusApplied')
										: t('config.statusRejected')}
								</Badge>
							</td>
							<td
								class="{TABLE_BODY_CELL} max-w-[10rem] truncate"
								title={rev.revision_label ?? undefined}
							>
								{rev.revision_label ?? EMPTY_VALUE}
							</td>
							<!-- title keeps the raw server label for support / debugging -->
							<td class="{TABLE_BODY_CELL} max-w-[14rem]" title={rev.applied_by ?? undefined}>
								{#if by}
									<div class="flex min-w-0 flex-col gap-0.5">
										<span class="truncate text-sm">{by.identity}</span>
										<span class={TYPE_MUTED}>
											{originLabel(by.origin)}{by.claimed ? ` · ${by.claimed}` : ''}
										</span>
									</div>
								{:else}
									{EMPTY_VALUE}
								{/if}
							</td>
							<td class={TABLE_DATE_CELL}>
								<Timestamp value={rev.applied_at} />
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
	{/if}
</Panel>
