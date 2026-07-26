<script lang="ts">
	import LogoMark from '$lib/components/layout/LogoMark.svelte';
	import { APP_NAME } from '$lib/core/constants';
	import { TYPE_CODE, TYPE_MUTED } from '$lib/components/kit/layout-styles';
	import { cn } from '$lib/utils';

	interface Props {
		version?: string | null;
		size?: 'sm' | 'lg';
		/** stack = icon above name (login); inline = icon left of name (header). */
		layout?: 'stack' | 'inline';
		class?: string;
	}

	let { version, size = 'sm', layout = 'stack', class: className = '' }: Props = $props();

	const logoSize = $derived(size === 'lg' ? 'lg' : 'sm');

	const nameClass = $derived(
		cn(
			'font-heading font-semibold tracking-tight text-foreground',
			size === 'lg' ? 'text-2xl' : 'text-base',
			layout === 'inline' && size === 'sm' && 'hidden sm:inline'
		)
	);

	const versionClass = cn(TYPE_MUTED, TYPE_CODE);
</script>

{#if layout === 'inline'}
	<div class={cn('flex items-center gap-2.5', className)}>
		<LogoMark size={logoSize === 'lg' ? 'md' : 'sm'} />
		<div class="flex min-w-0 flex-col gap-0.5">
			<span class={nameClass}>{APP_NAME}</span>
			{#if version}
				<p class={versionClass}>v{version}</p>
			{/if}
		</div>
	</div>
{:else}
	<div class={cn('flex flex-col items-center gap-3 text-center', className)}>
		<LogoMark size={logoSize} />
		<div class="flex flex-col items-center gap-1">
			<span class={nameClass}>{APP_NAME}</span>
			{#if version}
				<p class={versionClass}>v{version}</p>
			{/if}
		</div>
	</div>
{/if}
