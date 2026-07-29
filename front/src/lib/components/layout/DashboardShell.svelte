<script lang="ts">
	import type { Permission } from '$lib/auth/types';
	import RefreshToolbar from '$lib/components/dashboard/RefreshToolbar.svelte';
	import StatusBanner from '$lib/components/kit/StatusBanner.svelte';
	import AuthGuard from '$lib/components/layout/AuthGuard.svelte';
	import PageHeader from '$lib/components/layout/PageHeader.svelte';
	import LoadingState from '$lib/components/kit/LoadingState.svelte';
	import { PAGE_STACK } from '$lib/components/kit/layout-styles';

	interface Props {
		title: string;
		description?: string;
		permission?: Permission;
		error?: string | null;
		isLoading?: boolean;
		hasContent?: boolean;
		isRefreshing?: boolean;
		lastUpdated?: Date | null;
		onRefresh?: () => void | Promise<void>;
		actions?: import('svelte').Snippet;
		children: import('svelte').Snippet;
	}

	let {
		title,
		description,
		permission,
		error = null,
		isLoading = false,
		hasContent = true,
		isRefreshing = false,
		lastUpdated = null,
		onRefresh,
		actions,
		children
	}: Props = $props();

	/** Press `r` to refresh when focus is not in a form control. */
	$effect(() => {
		if (!onRefresh) return;
		const refresh = onRefresh;

		function onKeydown(event: KeyboardEvent) {
			if (event.key !== 'r' && event.key !== 'R') return;
			if (event.metaKey || event.ctrlKey || event.altKey) return;
			const target = event.target;
			if (!(target instanceof HTMLElement)) return;
			if (target.closest('input, textarea, select, [contenteditable="true"]')) return;
			event.preventDefault();
			void refresh();
		}

		window.addEventListener('keydown', onKeydown);
		return () => window.removeEventListener('keydown', onKeydown);
	});
</script>

<AuthGuard {permission}>
	<div class={PAGE_STACK}>
		<div class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
			<PageHeader {title} {description} />
			{#if actions}
				<div class="flex shrink-0 flex-wrap items-center justify-end gap-2">
					{@render actions()}
				</div>
			{/if}
		</div>

		{#if onRefresh}
			<RefreshToolbar {lastUpdated} {isRefreshing} {onRefresh} />
		{/if}

		{#if error}
			<StatusBanner tone="danger">{error}</StatusBanner>
		{/if}

		{#if isLoading && !hasContent}
			<LoadingState />
		{:else}
			{@render children()}
		{/if}
	</div>
</AuthGuard>
