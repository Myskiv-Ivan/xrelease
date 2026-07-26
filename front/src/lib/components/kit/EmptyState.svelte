	<script lang="ts">
		import { TYPE_EMPTY_TITLE, TYPE_PAGE_DESC } from '$lib/components/kit/layout-styles';
		import { cn } from '$lib/utils';

		interface Props {
			title: string;
			description?: string;
			variant?: 'default' | 'embedded';
			/** Optional CTA row under the description. */
			actions?: import('svelte').Snippet;
		}

		let { title, description, variant = 'default', actions }: Props = $props();
	</script>

	<div
		class={cn(
			'text-center',
			variant === 'embedded'
				? 'rounded-lg bg-muted/30 px-4 py-8'
				: 'rounded-xl border border-dashed border-border/80 bg-muted/20 px-6 py-10'
		)}
	>
		<p class={TYPE_EMPTY_TITLE}>{title}</p>
		{#if description}
			<p class={cn('mx-auto mt-2 max-w-sm', TYPE_PAGE_DESC, 'leading-relaxed')}>
				{description}
			</p>
		{/if}
		{#if actions}
			<div class="mt-4 flex flex-wrap items-center justify-center gap-2">
				{@render actions()}
			</div>
		{/if}
	</div>
