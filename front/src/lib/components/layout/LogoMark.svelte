<script lang="ts">
	import { cn } from '$lib/utils';

	interface Props {
		size?: 'sm' | 'md' | 'lg';
		class?: string;
	}

	let { size = 'sm', class: className = '' }: Props = $props();

	/** Wide terminal wordmark — viewBox 120×44 (~2.73:1). */
	const sizeClass = $derived(
		size === 'lg' ? 'h-12 w-[8.25rem]' : size === 'md' ? 'h-9 w-24' : 'h-7 w-[4.75rem]'
	);
</script>

<!--
	Inlined rather than <img src=logo.svg>: an <img> SVG is an isolated document
	that cannot read app tokens, so the chip would stay locked to one palette and
	show as a dark box in light mode. Inline lets the mark track the theme —
	`card` chip, `primary` glyphs, `accent` caret/cursor.
	`$lib/assets/logo.svg` is the standalone twin (docs / og-image), kept in sync.
-->
<svg
	viewBox="0 0 120 44"
	fill="none"
	aria-hidden="true"
	focusable="false"
	class={cn(
		'shrink-0 drop-shadow-[0_0_6px_color-mix(in_oklch,var(--primary)_28%,transparent)]',
		sizeClass,
		className
	)}
>
	<rect width="120" height="44" rx="7" class="fill-card" />
	<rect
		x="1"
		y="1"
		width="118"
		height="42"
		rx="6"
		stroke-width="1.5"
		class="stroke-primary/40"
	/>
	<path
		d="M8 11h104M8 17h104M8 23h104M8 29h104M8 35h104"
		stroke-width="1"
		class="stroke-primary/[0.07]"
	/>
	<!-- scale 0.78 keeps the wordmark off the chip edge; translate re-centres it -->
	<g transform="translate(28.02 16.54) scale(0.78)">
		<!-- prompt caret + cursor carry the accent (operator action) token -->
		<g class="fill-accent">
			<path
				opacity="0.8"
				d="M2 0h2v2h-2zM4 2h2v2h-2zM6 4h2v2h-2zM4 6h2v2h-2zM2 8h2v2h-2z"
			/>
			<path
				transform="translate(70 0)"
				d="M0 12h2v2h-2zM2 12h2v2h-2zM4 12h2v2h-2zM6 12h2v2h-2zM8 12h2v2h-2z"
			/>
		</g>
		<!-- x r l s -->
		<g class="fill-primary">
			<path
				transform="translate(14 0)"
				d="M0 0h2v2h-2zM8 0h2v2h-2zM0 2h2v2h-2zM8 2h2v2h-2zM2 4h2v2h-2zM6 4h2v2h-2zM4 6h2v2h-2zM2 8h2v2h-2zM6 8h2v2h-2zM0 10h2v2h-2zM8 10h2v2h-2zM0 12h2v2h-2zM8 12h2v2h-2z"
			/>
			<path
				transform="translate(28 0)"
				d="M0 0h2v2h-2zM2 0h2v2h-2zM4 0h2v2h-2zM6 0h2v2h-2zM0 2h2v2h-2zM8 2h2v2h-2zM0 4h2v2h-2zM8 4h2v2h-2zM0 6h2v2h-2zM2 6h2v2h-2zM4 6h2v2h-2zM6 6h2v2h-2zM0 8h2v2h-2zM4 8h2v2h-2zM0 10h2v2h-2zM6 10h2v2h-2zM0 12h2v2h-2zM8 12h2v2h-2z"
			/>
			<path
				transform="translate(42 0)"
				d="M0 0h2v2h-2zM0 2h2v2h-2zM0 4h2v2h-2zM0 6h2v2h-2zM0 8h2v2h-2zM0 10h2v2h-2zM0 12h2v2h-2zM2 12h2v2h-2zM4 12h2v2h-2zM6 12h2v2h-2zM8 12h2v2h-2z"
			/>
			<path
				transform="translate(56 0)"
				d="M2 0h2v2h-2zM4 0h2v2h-2zM6 0h2v2h-2zM8 0h2v2h-2zM0 2h2v2h-2zM0 4h2v2h-2zM2 6h2v2h-2zM4 6h2v2h-2zM6 6h2v2h-2zM8 8h2v2h-2zM8 10h2v2h-2zM0 12h2v2h-2zM2 12h2v2h-2zM4 12h2v2h-2zM6 12h2v2h-2z"
			/>
		</g>
	</g>
</svg>
