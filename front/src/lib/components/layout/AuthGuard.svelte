<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { loginPath } from '$lib/auth/return-to';
	import RoleGuard from '$lib/components/auth/RoleGuard.svelte';
	import LoadingState from '$lib/components/kit/LoadingState.svelte';
	import type { Permission } from '$lib/auth/types';
	import { t } from '$lib/i18n';
	import { getAuthState } from '$lib/stores/auth.svelte';

	interface Props {
		permission?: Permission;
		children: import('svelte').Snippet;
	}

	let { permission, children }: Props = $props();
	const auth = getAuthState();

	// Redirect only once auth has initialised. Gating on `isReady` prevents a
	// hard refresh / deep-link from bouncing an already-authenticated user to
	// /login: this child mounts before the layout's `initAuth()` resolves, so
	// `isAuthenticated` is still false at first paint.
	$effect(() => {
		if (auth.isReady && !auth.isAuthenticated) {
			goto(loginPath(page.url));
		}
	});
</script>

{#if auth.isReady && auth.isAuthenticated}
	<RoleGuard {permission}>
		{@render children()}
	</RoleGuard>
{:else if auth.isReady}
	<div class="mx-auto max-w-md px-4 py-16 text-center text-muted-foreground">
		{t('common.redirecting')}
	</div>
{:else}
	<div class="mx-auto max-w-md px-4 py-16">
		<LoadingState />
	</div>
{/if}
