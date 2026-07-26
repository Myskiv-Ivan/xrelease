<script lang="ts">
	import { TYPE_CODE, TYPE_MUTED, TYPE_HINT } from '$lib/components/kit/layout-styles';
	import { cn } from '$lib/utils';

	export interface MetaListItem {
		title: string;
		code?: string;
		detail?: string;
	}

	interface Props {
		items: MetaListItem[];
		empty?: string;
		class?: string;
	}

	let { items, empty, class: className = '' }: Props = $props();
</script>

<div class={cn('min-w-0', className)}>
	{#if items.length === 0}
		{#if empty}
			<p class={TYPE_HINT}>{empty}</p>
		{/if}
	{:else}
		<ul class="divide-y divide-border/60">
			{#each items as item (item.code ?? item.title)}
				<li
					class="grid min-w-0 grid-cols-1 gap-1 py-2.5 first:pt-0 last:pb-0 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center sm:gap-x-4"
				>
					<div class="min-w-0 space-y-0.5">
						<div class="font-medium">{item.title}</div>
						{#if item.code}
							<code class={cn('block truncate', TYPE_CODE, TYPE_MUTED)} title={item.code}>
								{item.code}
							</code>
						{/if}
					</div>
					{#if item.detail}
						<span
							class={cn(
								'shrink-0 max-w-[14rem] truncate whitespace-nowrap sm:max-w-[18rem] sm:text-right',
								TYPE_MUTED,
								'tabular-nums'
							)}
							title={item.detail}
						>
							{item.detail}
						</span>
					{/if}
				</li>
			{/each}
		</ul>
	{/if}
</div>
