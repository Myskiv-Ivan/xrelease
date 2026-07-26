<script lang="ts">
	import { getToastState } from '$lib/stores/toast.svelte';
	import { cn } from '$lib/utils';

	const toast = getToastState();

	const toneClasses = {
		default: 'border-border bg-card text-card-foreground',
		success: 'border-success/40 bg-success/10 text-success',
		error: 'border-destructive/40 bg-destructive/10 text-destructive'
	};
</script>

{#if toast.items.length > 0}
	<div
		class="pointer-events-none fixed bottom-4 right-4 z-50 flex w-full max-w-sm flex-col gap-2 px-4 sm:px-0"
	>
		{#each toast.items as item (item.id)}
			<div
				class={cn(
					'pointer-events-auto rounded-lg border px-4 py-3 text-sm shadow-lg',
					toneClasses[item.tone]
				)}
				role="status"
			>
				<p class="font-medium text-current">{item.title}</p>
				{#if item.description}
					<p class="mt-1 text-xs leading-relaxed text-current/80">{item.description}</p>
				{/if}
			</div>
		{/each}
	</div>
{/if}
