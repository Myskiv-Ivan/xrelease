	<script lang="ts">
		import * as Card from '$lib/components/ui/card/index.js';
		import { TYPE_SECTION } from '$lib/components/kit/layout-styles';
		import { cn } from '$lib/utils';

		interface Props {
			title?: string;
			/** Optional controls rendered on the right of the panel header. */
			actions?: import('svelte').Snippet;
			children: import('svelte').Snippet;
			class?: string;
		}

		let { title, actions, children, class: className = '' }: Props = $props();
	</script>

	<Card.Root size="sm" class={cn('min-w-0 overflow-hidden', className)}>
		{#if title || actions}
			<Card.Header class="flex flex-row items-center justify-between gap-3 space-y-0 px-4 pb-0 pt-0">
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
		<Card.Content class="min-w-0 px-4 pt-3 pb-4">
			{@render children()}
		</Card.Content>
	</Card.Root>
