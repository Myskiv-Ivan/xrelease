<script lang="ts">
	import Button from '#lib/components/kit/Button.svelte';
	import Select from '#lib/components/kit/Select.svelte';
	import { TYPE_MUTED } from '#lib/components/kit/layout-styles.js';
	import {
		TABLE_PAGE_DEFAULT,
		TABLE_PAGE_SIZES,
		clampTableOffset,
		resolveTablePageSize
	} from '#lib/core/table-pagination.js';
	import { t } from '#lib/i18n/index.js';

	interface Props {
		total: number;
		limit?: number;
		offset?: number;
		/** Allowed page sizes — History uses a 200-capped list. */
		pageSizes?: readonly number[];
		/** When false, only the page-size control is shown (e.g. short lists). */
		showNav?: boolean;
		onWindowChange: (limit: number, offset: number) => void;
	}

	let {
		total,
		limit = TABLE_PAGE_DEFAULT,
		offset = 0,
		pageSizes = TABLE_PAGE_SIZES,
		showNav = true,
		onWindowChange
	}: Props = $props();

	/** Prefer the live window size; only coerce when parsing the Select. */
	const safeLimit = $derived(limit > 0 ? limit : TABLE_PAGE_DEFAULT);
	const safeOffset = $derived(clampTableOffset(total, safeLimit, offset));
	const rangeStart = $derived(total === 0 ? 0 : safeOffset + 1);
	const rangeEnd = $derived(Math.min(safeOffset + safeLimit, total));
	const canPrev = $derived(safeOffset > 0);
	const canNext = $derived(safeOffset + safeLimit < total);
	const pageSizeKey = $derived(
		pageSizes.includes(safeLimit)
			? String(safeLimit)
			: String(resolveTablePageSize(safeLimit, pageSizes))
	);

	function changePageSize(key: string) {
		onWindowChange(resolveTablePageSize(key, pageSizes), 0);
	}

	function goPrev() {
		onWindowChange(safeLimit, Math.max(0, safeOffset - safeLimit));
	}

	function goNext() {
		onWindowChange(safeLimit, safeOffset + safeLimit);
	}
</script>

{#if total > 0}
	<div
		class="mt-3 flex flex-col gap-3 border-t border-border/60 pt-3 sm:flex-row sm:items-center sm:justify-between"
	>
		<span class="{TYPE_MUTED} order-2 tabular-nums text-sm sm:order-1">
			{t('common.tableRange')
				.replace('{start}', String(rangeStart))
				.replace('{end}', String(rangeEnd))
				.replace('{total}', String(total))}
		</span>

		<div class="order-1 flex flex-wrap items-center gap-3 sm:order-2 sm:justify-end">
			<label class="inline-flex items-center gap-2 text-sm text-foreground">
				<span class="whitespace-nowrap {TYPE_MUTED}">{t('common.tablePageSize')}</span>
				<!-- Compact select: override kit default h-11 / min-w so label + control share one baseline. -->
				<Select
					class="w-[4.75rem] shrink-0 [&_select]:h-9 [&_select]:min-h-9 [&_select]:min-w-0 [&_select]:py-0 [&_select]:pr-8 [&_select]:text-sm"
					aria-label={t('common.tablePageSize')}
					value={pageSizeKey}
					onchange={(e) => changePageSize(e.currentTarget.value)}
				>
					{#each pageSizes as size (size)}
						<option value={String(size)}>{size}</option>
					{/each}
				</Select>
			</label>

			{#if showNav && total > safeLimit}
				<div class="inline-flex items-center gap-1">
					<Button variant="ghost" size="sm" disabled={!canPrev} onclick={goPrev}>
						{t('common.tablePrev')}
					</Button>
					<Button variant="ghost" size="sm" disabled={!canNext} onclick={goNext}>
						{t('common.tableNext')}
					</Button>
				</div>
			{/if}
		</div>
	</div>
{/if}
