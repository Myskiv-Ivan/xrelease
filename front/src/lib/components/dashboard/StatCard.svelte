<script lang="ts">
	import * as Card from '$lib/components/ui/card/index.js';
	import {
		statLabelClass,
		statValueClass,
		toneBarClass,
		toneTextClass,
		toneTintClass,
		type SurfaceTone
	} from '$lib/components/kit/surface-styles';
	import { TYPE_MUTED } from '$lib/components/kit/layout-styles';
	import { cn } from '$lib/utils';

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

<Card.Root
	size="sm"
	class={cn(
		'relative overflow-hidden motion-safe:transition-shadow motion-safe:duration-200',
		tone !== 'default' && toneTintClass[tone],
		'motion-safe:hover:shadow-md'
	)}
>
	{#if tone !== 'default'}
		<span class={cn('absolute inset-y-0 left-0 w-1', toneBarClass[tone])} aria-hidden="true"></span>
	{/if}
	<Card.Content class={cn('px-4 py-3', tone !== 'default' && 'pl-5')}>
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
	</Card.Content>
</Card.Root>
