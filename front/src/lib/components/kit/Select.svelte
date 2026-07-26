<script lang="ts">
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import { cn } from '$lib/utils';

	interface Props {
		id?: string;
		class?: string;
		value?: string | number;
		disabled?: boolean;
		'aria-label'?: string;
		children: import('svelte').Snippet;
		onchange?: (event: Event & { currentTarget: HTMLSelectElement }) => void;
	}

	let {
		id,
		class: className = '',
		value = $bindable<string | number>(''),
		disabled = false,
		'aria-label': ariaLabel,
		children,
		onchange
	}: Props = $props();

	/**
	 * Opaque field chrome + appearance-none so the custom chevron is the only
	 * arrow. Transparent backgrounds made light/dark OS popups unreadable.
	 */
	const controlClass =
		'border-input bg-background text-foreground focus-visible:border-ring focus-visible:ring-ring/50 h-9 min-w-[7.5rem] w-full cursor-pointer appearance-none rounded-md border py-1 pr-9 pl-2.5 text-sm shadow-xs outline-none transition-[color,box-shadow] focus-visible:ring-3 disabled:cursor-not-allowed disabled:opacity-50 [-webkit-appearance:none]';
</script>

<div class={cn('relative inline-flex max-w-full', className)}>
	<select
		{id}
		class={controlClass}
		bind:value
		{disabled}
		{onchange}
		aria-label={ariaLabel}
	>
		{@render children()}
	</select>
	<ChevronDownIcon
		class="pointer-events-none absolute top-1/2 right-2.5 size-3.5 -translate-y-1/2 text-muted-foreground"
		aria-hidden="true"
	/>
</div>
