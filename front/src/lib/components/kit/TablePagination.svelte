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
	<div class="mt-3 flex flex-wrap items-center gap-3">
		<span class="{TYPE_MUTED} tabular-nums text-sm">
			{t('common.tableRange')
				.replace('{start}', String(rangeStart))
				.replace('{end}', String(rangeEnd))
				.replace('{total}', String(total))}
		</span>
		<div class="ml-auto flex flex-wrap items-center gap-2">
			<label class="flex items-center gap-2 text-sm">
				<span class="{TYPE_MUTED} text-xs whitespace-nowrap">{t('common.tablePageSize')}</span>
				<Select
					class="h-9 w-auto min-w-[4.5rem]"
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
				<Button variant="ghost" size="sm" disabled={!canPrev} onclick={goPrev}>
					{t('common.tablePrev')}
				</Button>
				<Button variant="ghost" size="sm" disabled={!canNext} onclick={goNext}>
					{t('common.tableNext')}
				</Button>
			{/if}
		</div>
	</div>
{/if}
