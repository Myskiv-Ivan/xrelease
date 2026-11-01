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
	}

	let { label, value, hint, tone = 'default', mono = false }: Props = $props();
</script>

<!--
	Metric cell inside StatGrid — not a nested card. Tone is color on the value
	(+ faint tint), never a left accent bar (AI-slop tell).
-->
<div
	class={cn(
		'flex min-w-0 flex-col justify-center px-4 py-3',
		tone !== 'default' && toneTintClass[tone]
	)}
>
	<p class={statLabelClass}>{label}</p>
	<p
		class={cn(statValueClass, toneTextClass[tone], mono && 'font-mono', 'truncate')}
		title={String(value)}
	>
		{value}
	</p>
	{#if hint}
		<p class="mt-1 leading-snug {TYPE_MUTED}">{hint}</p>
	{/if}
</div>
