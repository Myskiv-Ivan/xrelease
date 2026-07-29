<script lang="ts">
	import { TYPE_SECTION } from '$lib/components/kit/layout-styles';
	import { cn } from '$lib/utils';

	interface Props {
		title: string;
		/** Optional controls on the right of the heading row. */
		actions?: import('svelte').Snippet;
		children: import('svelte').Snippet;
		class?: string;
	}

	let { title, actions, children, class: className = '' }: Props = $props();
</script>

<!--
	A titled band of content on a page — the level above `Panel`, used to group
	stat grids. Owning the heading here keeps every section at `h2` with the same
	gap; the overview page had three hand-written copies of exactly this markup.
-->
<section class={cn('flex flex-col gap-3', className)}>
	<div class="flex flex-wrap items-center justify-between gap-3">
		<h2 class={TYPE_SECTION}>{title}</h2>
		{#if actions}
			<div class="flex items-center gap-2">
				{@render actions()}
			</div>
		{/if}
	</div>
	{@render children()}
</section>
