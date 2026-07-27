<script lang="ts">
	import { api, ApiClientError } from '$lib/api/client';
	import type { AppRoleName } from '$lib/api/types';
	import Button from '$lib/components/kit/Button.svelte';
	import Input from '$lib/components/kit/Input.svelte';
	import Panel from '$lib/components/kit/Panel.svelte';
	import Select from '$lib/components/kit/Select.svelte';
	import StatusBanner from '$lib/components/kit/StatusBanner.svelte';
	import { FIELD_GROUP, FIELD_STACK, TYPE_HINT } from '$lib/components/kit/layout-styles';
	import { fieldLabelClass } from '$lib/components/kit/surface-styles';
	import { resolveApiError } from '$lib/core/errors';
	import { t } from '$lib/i18n';
	import { getAuthState } from '$lib/stores/auth.svelte';
	import { getNetworkState } from '$lib/stores/network.svelte';

	interface Props {
		/** Called after a successful create so the list panel can reload. */
		onCreated?: () => void;
	}

	let { onCreated }: Props = $props();

	const auth = getAuthState();
	const network = getNetworkState();

	let username = $state('');
	let password = $state('');
	let displayName = $state('');
	let email = $state('');
	let role = $state<AppRoleName>('viewer');

	let isCreating = $state(false);
	let formError = $state<string | null>(null);
	let formSuccess = $state<string | null>(null);

	const canManage = $derived(auth.hasPermission('settings:write'));

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
			onCreated?.();
		} catch (err) {
			formError =
				err instanceof ApiClientError
					? err.message
					: resolveApiError(err, t('users.createFailed'));
		} finally {
			isCreating = false;
		}
	}
</script>

{#if canManage}
	<Panel title={t('users.createTitle')}>
		<form class={FIELD_STACK} onsubmit={handleCreate}>
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
					<label for="user-display-name" class={fieldLabelClass}>
						{t('users.displayName')}
					</label>
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
					<!-- Same address the IdP asserts → SSO adopts this account on first sign-in. -->
					<span class={TYPE_HINT}>{t('users.emailSsoHint')}</span>
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
