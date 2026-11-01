<script lang="ts">
	import { cn } from '#lib/utils.js';

	interface Props {
		columns?: 2 | 3 | 4 | 5;
		class?: string;
		children: import('svelte').Snippet;
	}

	let { columns = 4, class: className = '', children }: Props = $props();

	const columnClasses: Record<2 | 3 | 4 | 5, string> = {
		2: 'sm:grid-cols-2',
		3: 'sm:grid-cols-2 lg:grid-cols-3',
		4: 'sm:grid-cols-2 lg:grid-cols-4',
		5: 'sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-5'
	};
</script>

<!--
	One bordered strip with hairline dividers — replaces a mosaic of identical
	cards (Operate hard-fail: stacked card grids as layout).
-->
<div
	class={cn(
		'grid overflow-hidden rounded-lg border border-border bg-card',
		'divide-y divide-border sm:divide-y-0 sm:divide-x',
		columnClasses[columns],
		className
	)}
>
	{@render children()}
</div>
