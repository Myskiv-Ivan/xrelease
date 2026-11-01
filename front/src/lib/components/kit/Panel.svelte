<script lang="ts">
	import * as Card from '#lib/components/ui/card/index.js';
	import { TYPE_SECTION } from '#lib/components/kit/layout-styles.js';
	import { cn } from '#lib/utils.js';

	interface Props {
		title?: string;
		/** Optional controls rendered on the right of the panel header. */
		actions?: import('svelte').Snippet;
		children: import('svelte').Snippet;
		class?: string;
	}

	let { title, actions, children, class: className = '' }: Props = $props();
</script>

<Card.Root size="sm" class={cn('min-w-0 overflow-hidden gap-0 py-0', className)}>
	{#if title || actions}
		<Card.Header
			class="flex flex-row items-center justify-between gap-3 space-y-0 border-b border-border/70 px-4 py-2.5"
		>
			{#if title}
				<h2 class={TYPE_SECTION}>{title}</h2>
			{/if}
			{#if actions}
				<div class="flex items-center gap-2">
					{@render actions()}
				</div>
			{/if}
		</Card.Header>
	{/if}
	<Card.Content class="min-w-0 px-4 py-3">
		{@render children()}
	</Card.Content>
</Card.Root>
