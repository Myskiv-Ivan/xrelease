<script lang="ts" module>
	/**
	 * Semantic tones this badge accepts.
	 *
	 * Exported because callers derive a tone before they render — advisory
	 * severity, source health, outbox status. They previously typed that against
	 * `SurfaceTone` (stat-card accent bars), which is a *different*, narrower set:
	 * it has no `accent`/`muted`, so a caller that wanted either had to widen with
	 * a cast or settle for the wrong tone.
	 */
	export type BadgeTone = 'default' | 'accent' | 'success' | 'warning' | 'danger' | 'muted';
</script>

<script lang="ts">
	import { Badge as ShadcnBadge } from '$lib/components/ui/badge/index.js';
	import type { BadgeVariant } from '$lib/components/ui/badge/badge.svelte';
	import { cn } from '$lib/utils';

	interface Props {
		tone?: BadgeTone;
		/** Show a leading status dot (uses currentColor). */
		dot?: boolean;
		/** Animate the dot — for live/healthy indicators. */
		pulse?: boolean;
		/** Native tooltip; badges are terse, so the full meaning often lives here. */
		title?: string;
		class?: string;
		children: import('svelte').Snippet;
	}

	let {
		tone = 'default',
		dot = false,
		pulse = false,
		title,
		class: className = '',
		children
	}: Props = $props();

	const toneMap: Record<BadgeTone, BadgeVariant> = {
		default: 'default',
		accent: 'accent',
		success: 'success',
		warning: 'warning',
		danger: 'destructive',
		muted: 'muted'
	};
</script>

<ShadcnBadge variant={toneMap[tone]} {title} class={cn('rounded-md', className)}>
	{#if dot}
		<span class="relative flex h-1.5 w-1.5">
			{#if pulse}
				<span
					class="absolute inline-flex h-full w-full animate-ping rounded-full bg-current opacity-60"
				></span>
			{/if}
			<span class="relative inline-flex h-1.5 w-1.5 rounded-full bg-current"></span>
		</span>
	{/if}
	{@render children()}
</ShadcnBadge>
