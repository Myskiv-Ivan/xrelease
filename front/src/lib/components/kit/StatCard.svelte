<script lang="ts">
	import {
		statLabelClass,
		statValueClass,
		toneTextClass,
		toneTintClass,
		type SurfaceTone
	} from '#lib/components/kit/surface-styles.js';
	import { TYPE_MUTED } from '#lib/components/kit/layout-styles.js';
	import { cn } from '#lib/utils.js';

	interface Props {
		label: string;
		value: string | number;
		hint?: string;
		tone?: SurfaceTone;
		/** Render the value in a monospace face (versions, tags, hashes). */
		mono?: boolean;
		/** Hero metric inside a `StatGrid layout="lead"` first cell. */
		lead?: boolean;
		/** Optional deep-link — whole cell becomes a focusable destination. */
		href?: string;
	}

	let {
		label,
		value,
		hint,
		tone = 'default',
		mono = false,
		lead = false,
		href
	}: Props = $props();

	const cellClass = $derived(
		cn(
			'flex min-w-0 flex-col justify-center px-4 py-3',
			lead && 'sm:px-5 sm:py-4',
			tone !== 'default' && toneTintClass[tone],
			href &&
				'cursor-pointer no-underline transition-colors duration-150 hover:bg-muted/40 focus-visible:bg-muted/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset'
		)
	);
</script>

<!--
	Metric cell inside StatGrid — not a nested card. Tone is color on the value
	(+ faint tint), never a left accent bar (AI-slop tell).
-->
{#if href}
	<a {href} class={cellClass}>
		<p class={statLabelClass}>{label}</p>
		<p
			class={cn(
				statValueClass,
				lead && 'sm:text-3xl',
				toneTextClass[tone],
				mono && 'font-mono',
				'truncate'
			)}
			title={String(value)}
		>
			{value}
		</p>
		{#if hint}
			<p class="mt-1 leading-snug {TYPE_MUTED}">{hint}</p>
		{/if}
	</a>
{:else}
	<div class={cellClass}>
		<p class={statLabelClass}>{label}</p>
		<p
			class={cn(
				statValueClass,
				lead && 'sm:text-3xl',
				toneTextClass[tone],
				mono && 'font-mono',
				'truncate'
			)}
			title={String(value)}
		>
			{value}
		</p>
		{#if hint}
			<p class="mt-1 leading-snug {TYPE_MUTED}">{hint}</p>
		{/if}
	</div>
{/if}
