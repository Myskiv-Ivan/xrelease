<script lang="ts">
	import { api, ApiClientError } from '$lib/api/client';
	import type { AppRoleName, AuthUserView } from '$lib/api/types';
	import Badge from '$lib/components/kit/Badge.svelte';
	import Button from '$lib/components/kit/Button.svelte';
	import EmptyState from '$lib/components/kit/EmptyState.svelte';
	import Input from '$lib/components/kit/Input.svelte';
	import Panel from '$lib/components/kit/Panel.svelte';
	import RelativeTime from '$lib/components/kit/RelativeTime.svelte';
	import Select from '$lib/components/kit/Select.svelte';
	import StatusBanner from '$lib/components/kit/StatusBanner.svelte';
	import {
		FIELD_GROUP,
		FIELD_STACK,
		TYPE_CODE,
		TYPE_HINT,
		TYPE_MUTED
	} from '$lib/components/kit/layout-styles';
	import { fieldLabelClass } from '$lib/components/kit/surface-styles';
	import { roleLabel } from '$lib/auth/roles';
	import { resolveApiError } from '$lib/core/errors';
	import { t } from '$lib/i18n';
	import { getAuthState } from '$lib/stores/auth.svelte';
	import { getNetworkState } from '$lib/stores/network.svelte';
	import { onMount } from 'svelte';

	const auth = getAuthState();
	const network = getNetworkState();

	let users = $state<AuthUserView[]>([]);
	let loadError = $state<string | null>(null);
	let isLoading = $state(false);
	let isCreating = $state(false);
	let formError = $state<string | null>(null);
	let formSuccess = $state<string | null>(null);

	let username = $state('');
	let password = $state('');
	let displayName = $state('');
	let email = $state('');
	let role = $state<AppRoleName>('viewer');
	/** Draft SSO link emails keyed by user id. */
	let oidcEmailDraftById = $state<Record<number, string>>({});
	let linkingId = $state<number | null>(null);

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

	function identityLabel(user: AuthUserView): string {
		return (
			user.display_name ??
			user.username ??
			user.email ??
			(user.oidc_sub ? `oidc:${user.oidc_sub.slice(0, 12)}` : `#${user.id}`)
		);
	}

	function sourceTone(source: string): 'accent' | 'success' | 'muted' {
		if (source === 'oidc') return 'accent';
		if (source === 'local') return 'success';
		return 'muted';
	}

	function sourceLabel(source: string): string {
		if (source === 'oidc') return t('users.sourceOidc');
		if (source === 'local') return t('users.sourceLocal');
		return source;
	}

	async function handleCreate(event: SubmitEvent) {
		event.preventDefault();
		if (!canManage || !network.isOnline) return;

		const trimmedUser = username.trim();
		if (!trimmedUser || !password) {
			formError = t('users.createRequired');
			return;
		}
		if (password.length < 8) {
			formError = t('users.passwordTooShort');
			return;
		}

		isCreating = true;
		formError = null;
		formSuccess = null;
		try {
			await api.createUser({
				username: trimmedUser,
				password,
				role,
				email: email.trim() || null,
				display_name: displayName.trim() || null
			});
			formSuccess = t('users.created').replace('{username}', trimmedUser);
			username = '';
			password = '';
			displayName = '';
			email = '';
			role = 'viewer';
			await loadUsers();
		} catch (err) {
			formError =
				err instanceof ApiClientError
					? err.message
					: resolveApiError(err, t('users.createFailed'));
		} finally {
			isCreating = false;
		}
	}

	async function handleLinkOidc(user: AuthUserView) {
		if (!canManage || !network.isOnline || user.auth_source !== 'local') return;
		const draft = (oidcEmailDraftById[user.id] ?? user.email ?? '').trim();
		linkingId = user.id;
		formError = null;
		formSuccess = null;
		try {
			await api.linkUserOidcEmail(user.id, draft || null);
			formSuccess = draft
				? t('users.oidcLinkOk').replace('{user}', identityLabel(user))
				: t('users.oidcUnlinkOk').replace('{user}', identityLabel(user));
			await loadUsers();
		} catch (err) {
			formError =
				err instanceof ApiClientError
					? err.message
					: resolveApiError(err, t('users.oidcLinkFailed'));
		} finally {
			linkingId = null;
		}
	}
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
			<p class={TYPE_HINT}>{t('common.loading')}</p>
		{:else if users.length === 0}
			<EmptyState title={t('users.emptyTitle')} description={t('users.emptyDescription')} />
		{:else}
			<ul class="mb-4 flex flex-col gap-2">
				{#each users as user (user.id)}
					<li
						class="flex flex-col gap-2 rounded-md border border-border bg-card px-3 py-2 sm:flex-row sm:items-center sm:gap-3"
					>
						<div class="min-w-0 flex-1">
							<div class="flex flex-wrap items-center gap-2">
								<span class="truncate text-sm font-medium" title={identityLabel(user)}>
									{identityLabel(user)}
								</span>
								<Badge tone={sourceTone(user.auth_source)}>{sourceLabel(user.auth_source)}</Badge>
								<Badge tone="muted">{roleLabel(user.role)}</Badge>
								{#if user.oidc_sub}
									<span title={user.oidc_sub}>
										<Badge tone="accent">{t('users.oidcLinked')}</Badge>
									</span>
								{/if}
							</div>
							<div class="mt-1 flex flex-wrap gap-x-3 gap-y-0.5 {TYPE_MUTED}">
								{#if user.username}
									<span class={TYPE_CODE}>{user.username}</span>
								{/if}
								{#if user.email}
									<span>{user.email}</span>
								{/if}
								{#if user.oidc_sub}
									<span class="{TYPE_CODE} truncate" title={user.oidc_sub}>
										sub:{user.oidc_sub}
									</span>
								{/if}
							</div>
						</div>
						<div class="shrink-0 text-left sm:text-right {TYPE_MUTED}">
							<div>
								{t('users.createdAt')}
								<RelativeTime value={user.created_at} class="inline" />
							</div>
							{#if user.last_login_at}
								<div>
									{t('users.lastLogin')}
									<RelativeTime value={user.last_login_at} class="inline" />
								</div>
							{:else}
								<div>{t('users.neverLoggedIn')}</div>
							{/if}
						</div>
						{#if user.auth_source === 'local'}
							<div class="w-full border-t border-border/60 pt-2 sm:w-auto sm:border-0 sm:pt-0">
								<div class="{FIELD_GROUP} text-sm">
									<label for="oidc-email-{user.id}" class={fieldLabelClass}>
										{t('users.oidcEmail')}
									</label>
									<div class="flex flex-wrap gap-2">
										<Input
											id="oidc-email-{user.id}"
											type="email"
											autocomplete="off"
											class="min-w-[14rem] flex-1"
											placeholder={t('users.oidcEmailPlaceholder')}
											disabled={linkingId === user.id || !network.isOnline}
											value={oidcEmailDraftById[user.id] ?? user.email ?? ''}
											oninput={(e) => {
												oidcEmailDraftById = {
													...oidcEmailDraftById,
													[user.id]: e.currentTarget.value
												};
											}}
										/>
										<Button
											variant="outline"
											size="sm"
											disabled={linkingId === user.id || !network.isOnline}
											onclick={() => void handleLinkOidc(user)}
										>
											{linkingId === user.id
												? t('users.oidcLinking')
												: user.email
													? t('users.oidcSave')
													: t('users.oidcLink')}
										</Button>
									</div>
									<span class={TYPE_MUTED}>
										{user.oidc_sub ? t('users.oidcBoundHint') : t('users.oidcPendingHint')}
									</span>
								</div>
							</div>
						{/if}
					</li>
				{/each}
			</ul>
		{/if}

		<form class="{FIELD_STACK} border-t border-border pt-4" onsubmit={handleCreate}>
			<p class="text-sm font-medium">{t('users.createTitle')}</p>
			<p class={TYPE_HINT}>{t('users.createHint')}</p>

			<div class="grid gap-3 sm:grid-cols-2">
				<div class={FIELD_GROUP}>
					<label for="user-username" class={fieldLabelClass}>{t('users.username')}</label>
					<Input
						id="user-username"
						autocomplete="off"
						bind:value={username}
						disabled={isCreating}
						required
					/>
				</div>
				<div class={FIELD_GROUP}>
					<label for="user-password" class={fieldLabelClass}>{t('users.password')}</label>
					<Input
						id="user-password"
						type="password"
						autocomplete="new-password"
						bind:value={password}
						disabled={isCreating}
						required
					/>
				</div>
				<div class={FIELD_GROUP}>
					<label for="user-display-name" class={fieldLabelClass}>{t('users.displayName')}</label>
					<Input
						id="user-display-name"
						autocomplete="off"
						bind:value={displayName}
						disabled={isCreating}
					/>
				</div>
				<div class={FIELD_GROUP}>
					<label for="user-email" class={fieldLabelClass}>{t('users.email')}</label>
					<Input
						id="user-email"
						type="email"
						autocomplete="off"
						bind:value={email}
						disabled={isCreating}
					/>
				</div>
				<div class={FIELD_GROUP}>
					<label for="user-role" class={fieldLabelClass}>{t('users.role')}</label>
					<Select id="user-role" bind:value={role} disabled={isCreating}>
						<option value="viewer">{t('users.roleViewer')}</option>
						<option value="operator">{t('users.roleOperator')}</option>
						<option value="admin">{t('users.roleAdmin')}</option>
					</Select>
				</div>
			</div>

			{#if formError}
				<StatusBanner tone="danger">{formError}</StatusBanner>
			{/if}
			{#if formSuccess}
				<StatusBanner tone="success">{formSuccess}</StatusBanner>
			{/if}

			<Button type="submit" size="sm" disabled={isCreating || !network.isOnline}>
				{isCreating ? t('users.creating') : t('users.create')}
			</Button>
		</form>
	</Panel>
{/if}
