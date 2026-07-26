<script lang="ts">
	import type { Permission } from '$lib/auth/types';
	import EmptyState from '$lib/components/kit/EmptyState.svelte';
	import { getAuthState } from '$lib/stores/auth.svelte';
	import { t } from '$lib/i18n';

	interface Props {
		permission?: Permission;
		children: import('svelte').Snippet;
	}

	let { permission, children }: Props = $props();
	const auth = getAuthState();

	const isAllowed = $derived(
		!permission || (auth.isAuthenticated && auth.hasPermission(permission))
	);
</script>

{#if auth.isReady && isAllowed}
	{@render children()}
{:else if auth.isReady && auth.isAuthenticated}
	<EmptyState title={t('auth.forbiddenTitle')} description={t('auth.forbiddenDescription')} />
{/if}
