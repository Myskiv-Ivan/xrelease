<script lang="ts" module>
	export interface SegmentOption {
		value: string;
		label: string;
		/** Native tooltip — segments are terse. */
		title?: string;
	}
</script>

<script lang="ts">
	import { cn } from '$lib/utils';

	interface Props {
		value: string;
		options: SegmentOption[];
		/** Required: the group has no visible label. */
		'aria-label': string;
		/** Matches `Button` sizes so the two line up in a panel header row. */
		size?: 'sm' | 'md';
		disabled?: boolean;
		class?: string;
	}

	let {
		value = $bindable(''),
		options,
		'aria-label': ariaLabel,
		size = 'md',
		disabled = false,
		class: className = ''
	}: Props = $props();

	const trackSize = { sm: 'h-8', md: 'h-9' };
	const segmentSize = { sm: 'px-2 text-xs', md: 'px-2.5 text-sm' };

	/**
	 * Which segment holds the group's single tab stop. Falls back to the first
	 * one so an unmatched `value` cannot make the control keyboard-unreachable.
	 */
	const tabStop = $derived(Math.max(0, options.findIndex((option) => option.value === value)));

	/**
	 * A radiogroup, not a row of `aria-pressed` buttons: the choice is mutually
	 * exclusive, so screen readers should announce "1 of N" and arrow keys should
	 * move between segments. Roving tabindex keeps the group a single tab stop.
	 */
	function onKeydown(event: KeyboardEvent, index: number) {
		const delta =
			event.key === 'ArrowRight' || event.key === 'ArrowDown'
				? 1
				: event.key === 'ArrowLeft' || event.key === 'ArrowUp'
					? -1
					: 0;
		if (delta === 0) return;
		event.preventDefault();
		const next = options[(index + delta + options.length) % options.length];
		value = next.value;
		const target = event.currentTarget as HTMLElement;
		const sibling = target.parentElement?.querySelector<HTMLElement>(
			`[data-segment="${CSS.escape(next.value)}"]`
		);
		sibling?.focus();
	}
</script>

<!--
	Same pill chrome as the `default` Tabs variant (muted track, raised active
	segment) so in-panel switches read as one control family with page tabs.
-->
<div
	class={cn(
		'inline-flex w-fit items-center justify-center rounded-lg bg-muted p-[3px]',
		trackSize[size],
		disabled && 'pointer-events-none opacity-50',
		className
	)}
	role="radiogroup"
	aria-label={ariaLabel}
>
	{#each options as option, index (option.value)}
		{@const active = value === option.value}
		<button
			type="button"
			data-segment={option.value}
			role="radio"
			aria-checked={active}
			tabindex={index === tabStop ? 0 : -1}
			title={option.title}
			{disabled}
			class={cn(
				'inline-flex h-full cursor-pointer items-center justify-center rounded-md border border-transparent py-1 font-medium whitespace-nowrap transition-all',
				segmentSize[size],
				'focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:outline-none',
				active
					? 'bg-background text-foreground shadow-sm dark:border-input dark:bg-input/30'
					: 'text-foreground/60 hover:text-foreground dark:text-muted-foreground'
			)}
			onclick={() => (value = option.value)}
			onkeydown={(event) => onKeydown(event, index)}
		>
			{option.label}
		</button>
	{/each}
</div>
