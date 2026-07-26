<script lang="ts">
	import { TABLE_HEAD_CELL, TABLE_SORT_BUTTON } from '$lib/components/kit/table-styles';
	import { cn } from '$lib/utils';

	interface Props {
		label: string;
		active?: boolean;
		direction?: 'asc' | 'desc';
		/** When set, renders a sortable button; otherwise a static label (same chrome). */
		onclick?: () => void;
		class?: string;
	}

	let {
		label,
		active = false,
		direction = 'asc',
		onclick,
		class: className = ''
	}: Props = $props();

	const indicator = $derived(onclick && active ? (direction === 'asc' ? '↑' : '↓') : '');
	const ariaSort = $derived(
		onclick ? (active ? (direction === 'asc' ? 'ascending' : 'descending') : 'none') : undefined
	);
</script>

<th class={cn(TABLE_HEAD_CELL, className)} aria-sort={ariaSort}>
	{#if onclick}
		<button type="button" class={TABLE_SORT_BUTTON} {onclick}>
			<span class="whitespace-nowrap">{label}</span>
			{#if indicator}
				<span class="tabular-nums text-foreground" aria-hidden="true">{indicator}</span>
			{/if}
		</button>
	{:else}
		<span
			class={cn(
				'inline-flex w-full items-center whitespace-nowrap',
				className.includes('text-right')
					? 'justify-end'
					: className.includes('text-center')
						? 'justify-center'
						: 'justify-start'
			)}
		>
			{label}
		</span>
	{/if}
</th>
