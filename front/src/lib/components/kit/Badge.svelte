<script lang="ts">
	import { Badge as ShadcnBadge } from '$lib/components/ui/badge/index.js';
	import type { BadgeVariant } from '$lib/components/ui/badge/badge.svelte';
	import { cn } from '$lib/utils';

	type Tone = 'default' | 'accent' | 'success' | 'warning' | 'danger' | 'muted';

	interface Props {
		tone?: Tone;
		/** Show a leading status dot (uses currentColor). */
		dot?: boolean;
		/** Animate the dot — for live/healthy indicators. */
		pulse?: boolean;
		class?: string;
		children: import('svelte').Snippet;
	}

	let { tone = 'default', dot = false, pulse = false, class: className = '', children }: Props =
		$props();

	const toneMap: Record<Tone, BadgeVariant> = {
		default: 'default',
		accent: 'accent',
		success: 'success',
		warning: 'warning',
		danger: 'destructive',
		muted: 'muted'
	};
</script>

<ShadcnBadge variant={toneMap[tone]} class={cn('rounded-md', className)}>
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
