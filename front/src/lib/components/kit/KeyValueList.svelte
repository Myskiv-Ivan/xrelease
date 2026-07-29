<script lang="ts">
	import type { KeyValueItem } from '$lib/types/ui';
	import { statLabelClass, toneTextClass } from '$lib/components/kit/surface-styles';
	import { formatDisplayValue } from '$lib/core/format';

	interface Props {
		items: KeyValueItem[];
		layout?: 'list' | 'grid';
		columns?: 2 | 3 | 4;
	}

	let { items, layout = 'list', columns = 2 }: Props = $props();

	const gridCols: Record<2 | 3 | 4, string> = {
		2: 'grid-cols-1 sm:grid-cols-2',
		3: 'grid-cols-1 sm:grid-cols-2 lg:grid-cols-3',
		4: 'grid-cols-2 lg:grid-cols-4'
	};
</script>

{#if layout === 'grid'}
	<dl class="grid gap-x-4 gap-y-3 text-sm {gridCols[columns]}">
		{#each items as item (item.label)}
			<div class="min-w-0">
				<dt class={statLabelClass}>{item.label}</dt>
				<dd
					class="mt-0.5 truncate font-medium tabular-nums {toneTextClass[item.tone ?? 'default']}"
					title={formatDisplayValue(item.value)}
				>
					{formatDisplayValue(item.value)}
				</dd>
			</div>
		{/each}
	</dl>
{:else}
	<dl class="divide-y divide-border text-sm">
		{#each items as item (item.label)}
			<div class="flex items-baseline justify-between gap-4 py-2.5 first:pt-0 last:pb-0">
				<dt class="shrink-0 {statLabelClass}">{item.label}</dt>
				<dd
					class="min-w-0 truncate text-right font-medium tabular-nums {toneTextClass[item.tone ?? 'default']}"
					title={formatDisplayValue(item.value)}
				>
					{formatDisplayValue(item.value)}
				</dd>
			</div>
		{/each}
	</dl>
{/if}
