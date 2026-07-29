<script lang="ts" module>
	/**
	 * Semantic tones for the app's single alert surface.
	 *
	 * Exported so callers can derive a tone before rendering (config apply
	 * results, notifier tests) without re-declaring the union.
	 */
	export type BannerTone = 'info' | 'success' | 'warning' | 'danger';
</script>

<script lang="ts">
	import * as Alert from '$lib/components/ui/alert/index.js';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
	import InfoIcon from '@lucide/svelte/icons/info';
	import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
	import { cn } from '$lib/utils';

	interface Props {
		tone?: BannerTone;
		/** Optional heading above the message. */
		title?: string;
		/** Drop the leading glyph for dense inline banners. */
		icon?: boolean;
		children: import('svelte').Snippet;
		class?: string;
	}

	let { tone = 'info', title, icon = true, children, class: className = '' }: Props = $props();

	const tones: Record<BannerTone, string> = {
		info: 'border-primary/30 bg-primary/10 text-primary',
		success: 'border-success/40 bg-success/10 text-success',
		warning: 'border-warning/40 bg-warning/10 text-warning',
		danger: 'border-destructive/40 bg-destructive/10 text-destructive'
	};

	const icons = {
		info: InfoIcon,
		success: CircleCheckIcon,
		warning: TriangleAlertIcon,
		danger: CircleAlertIcon
	};

	const Icon = $derived(icons[tone]);
</script>

<!--
	The only alert surface in the app. Page-load failures, save results, offline
	state and validation warnings all render through here, so a danger banner
	never has two different looks depending on which component raised it.
-->
<Alert.Root
	variant={tone === 'danger' ? 'destructive' : 'default'}
	class={cn(tones[tone], className)}
	role={tone === 'danger' || tone === 'warning' ? 'alert' : 'status'}
>
	{#if icon}
		<Icon aria-hidden="true" />
	{/if}
	{#if title}
		<Alert.Title class="text-current">{title}</Alert.Title>
	{/if}
	<Alert.Description class="text-current">
		{@render children()}
	</Alert.Description>
</Alert.Root>
