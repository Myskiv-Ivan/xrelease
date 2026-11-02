<script lang="ts">
	import { TYPE_EMPTY_TITLE, TYPE_PAGE_DESC } from '#lib/components/kit/layout-styles.js';
	import { cn } from '#lib/utils.js';

	interface Props {
		title: string;
		description?: string;
		variant?: 'default' | 'embedded';
		/** Optional CTA row under the description. */
		actions?: import('svelte').Snippet;
	}

	let { title, description, variant = 'default', actions }: Props = $props();
</script>

<!-- Ops empty: left-aligned, not a centered marketing void. -->
<div
	class={cn(
		'text-left',
		variant === 'embedded'
			? 'rounded-md bg-muted/25 px-3 py-6'
			: 'border-t border-dashed border-border/80 px-1 py-8'
	)}
>
	<p class={TYPE_EMPTY_TITLE}>{title}</p>
	{#if description}
		<p class={cn('mt-2 max-w-md', TYPE_PAGE_DESC, 'leading-relaxed')}>
			{description}
		</p>
	{/if}
	{#if actions}
		<div class="mt-4 flex flex-wrap items-center gap-2">
			{@render actions()}
		</div>
	{/if}
</div>
