<script lang="ts">
	import Badge from '$lib/components/kit/Badge.svelte';
	import { getDisplayName } from '$lib/auth/session';
	import { getAuthState } from '$lib/stores/auth.svelte';
	import { getOrganizationsState } from '$lib/stores/organizations.svelte';
	import { TYPE_MUTED } from '$lib/components/kit/layout-styles';

	const auth = getAuthState();
	const orgs = getOrganizationsState();
	const displayName = $derived(getDisplayName(auth.profile));
	const badgeRole = $derived(
		orgs.isMultiOrg && orgs.selectedId
			? auth.roleLabel(orgs.selectedId)
			: auth.roleLabel()
	);
</script>

{#if auth.isAuthenticated && auth.profile}
	<div class="flex max-w-[12rem] items-center gap-1.5 sm:max-w-none sm:gap-2">
		<Badge tone="muted" class="shrink-0">{badgeRole}</Badge>
		{#if displayName}
			<span
				class="hidden truncate {TYPE_MUTED} md:inline"
				title={displayName}
			>
				{displayName}
			</span>
		{/if}
	</div>
{/if}
