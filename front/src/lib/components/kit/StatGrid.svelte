<script lang="ts">
	import { cn } from '#lib/utils.js';

	interface Props {
		/** Number of columns at the `lg` breakpoint. Below that, always 2. */
		columns?: 2 | 3 | 4 | 5;
		/**
		 * `equal` — every cell same width (avoid for primary status strips).
		 * `lead` — first cell spans 2 tracks on `lg` so the hero metric reads larger.
		 */
		layout?: 'equal' | 'lead';
		children: import('svelte').Snippet;
		class?: string;
	}

	let { columns = 4, layout = 'equal', children, class: className = '' }: Props = $props();

	const gridCols = $derived.by(() => {
		if (layout === 'lead') {
			// First cell spans 2 → track count is columns + 1 on lg.
			if (columns === 2) return 'grid-cols-2 lg:grid-cols-3';
			if (columns === 3) return 'grid-cols-2 lg:grid-cols-4';
			if (columns === 4) return 'grid-cols-2 lg:grid-cols-5';
			return 'grid-cols-2 lg:grid-cols-6';
		}
		if (columns === 2) return 'grid-cols-2';
		if (columns === 3) return 'grid-cols-2 lg:grid-cols-3';
		if (columns === 4) return 'grid-cols-2 lg:grid-cols-4';
		return 'grid-cols-2 sm:grid-cols-3 lg:grid-cols-5';
	});
</script>

<div
	class={cn(
		'grid overflow-hidden rounded-lg border border-border bg-card',
		'divide-x divide-y divide-border',
		gridCols,
		layout === 'lead' && '[&>*:first-child]:lg:col-span-2',
		className
	)}
>
	{@render children()}
</div>
