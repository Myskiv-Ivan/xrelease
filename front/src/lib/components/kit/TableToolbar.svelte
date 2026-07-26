<script lang="ts">
	import Input from '$lib/components/kit/Input.svelte';
	import { t } from '$lib/i18n';
	import { cn } from '$lib/utils';
	import { TYPE_MUTED } from '$lib/components/kit/layout-styles';

	interface Props {
		search?: string;
		searchPlaceholder?: string;
		shown: number;
		total: number;
		class?: string;
		filters?: import('svelte').Snippet;
		actions?: import('svelte').Snippet;
	}

	let {
		search = $bindable(''),
		searchPlaceholder,
		shown,
		total,
		class: className = '',
		filters,
		actions
	}: Props = $props();
</script>

<div class={cn('mb-4 flex flex-col gap-3 sm:flex-row sm:flex-wrap sm:items-center', className)}>
		<Input
			type="search"
			placeholder={searchPlaceholder ?? t('common.search')}
			aria-label={searchPlaceholder ?? t('common.search')}
			class="sm:max-w-xs"
			bind:value={search}
		/>
	{#if filters}
		<div class="flex flex-wrap items-center gap-2">
			{@render filters()}
		</div>
	{/if}
	{#if actions}
		<div class="flex flex-wrap items-center gap-2">
			{@render actions()}
		</div>
	{/if}
	<span class="{TYPE_MUTED} sm:ml-auto">
		{shown} {t('common.of')} {total}
	</span>
</div>
