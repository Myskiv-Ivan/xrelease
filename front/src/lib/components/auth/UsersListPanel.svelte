<script lang="ts">
	import { api } from '$lib/api/client';
	import type { AuthUserView } from '$lib/api/types';
	import Button from '$lib/components/kit/Button.svelte';
	import EmptyState from '$lib/components/kit/EmptyState.svelte';
	import LoadingState from '$lib/components/kit/LoadingState.svelte';
	import Panel from '$lib/components/kit/Panel.svelte';
	import StatusBanner from '$lib/components/kit/StatusBanner.svelte';
	import UsersTable from '$lib/components/auth/UsersTable.svelte';
	import { TYPE_HINT } from '$lib/components/kit/layout-styles';
	import { resolveApiError } from '$lib/core/errors';
	import { t } from '$lib/i18n';
	import { getAuthState } from '$lib/stores/auth.svelte';
	import { getNetworkState } from '$lib/stores/network.svelte';
	import { onMount } from 'svelte';

	interface Props {
		/** Bumped by the create panel so a new account shows without a manual refresh. */
		reloadToken?: number;
	}

	let { reloadToken = 0 }: Props = $props();

	const auth = getAuthState();
	const network = getNetworkState();

	let users = $state<AuthUserView[]>([]);
	let loadError = $state<string | null>(null);
	let isLoading = $state(false);

	const canManage = $derived(auth.hasPermission('settings:write'));

	async function loadUsers() {
		if (!canManage) return;
		isLoading = true;
		loadError = null;
		try {
			const response = await api.listUsers();
			users = response.users;
		} catch (err) {
			loadError = resolveApiError(err, t('users.loadFailed'));
			users = [];
		} finally {
			isLoading = false;
		}
	}

	onMount(() => {
		void loadUsers();
	});

	// Reacts to the create panel's token; the initial mount load already ran.
	let lastToken = 0;
	$effect(() => {
		if (reloadToken === lastToken) return;
		lastToken = reloadToken;
		void loadUsers();
	});
</script>

{#if canManage}
	<Panel title={t('users.title')}>
		{#snippet actions()}
			<Button
				variant="outline"
				size="sm"
				disabled={isLoading || !network.isOnline}
				onclick={() => void loadUsers()}
			>
				{isLoading ? t('common.refreshing') : t('common.refresh')}
			</Button>
		{/snippet}

		<p class="mb-3 {TYPE_HINT}">{t('users.hint')}</p>

		{#if loadError}
			<div class="mb-3">
				<StatusBanner tone="danger">{loadError}</StatusBanner>
			</div>
		{/if}

		{#if isLoading && users.length === 0}
			<LoadingState />
		{:else if users.length === 0}
			<EmptyState title={t('users.emptyTitle')} description={t('users.emptyDescription')} />
		{:else}
			<UsersTable {users} />
		{/if}
	</Panel>
{/if}
