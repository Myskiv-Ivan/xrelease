<script lang="ts">
	import { api, ApiClientError } from '$lib/api/client';
	import {
		isApiKeyEnabled,
		isOidcEnabled,
		requiresApiKeyForApi
	} from '$lib/auth/config';
	import { getDisplayName } from '$lib/auth/session';
	import AuthModeBadge from '$lib/components/auth/AuthModeBadge.svelte';
	import ErrorAlert from '$lib/components/dashboard/ErrorAlert.svelte';
	import Button from '$lib/components/kit/Button.svelte';
	import Input from '$lib/components/kit/Input.svelte';
	import KeyValueList from '$lib/components/kit/KeyValueList.svelte';
	import Panel from '$lib/components/kit/Panel.svelte';
	import Badge from '$lib/components/kit/Badge.svelte';
	import { FIELD_GROUP, FIELD_STACK, TYPE_STATUS_SUCCESS } from '$lib/components/kit/layout-styles';
	import { fieldLabelClass } from '$lib/components/kit/surface-styles';
	import { resolveApiError } from '$lib/core/errors';
	import { t } from '$lib/i18n';
	import { roleLabel } from '$lib/auth/roles';
	import { getAuthState, loginWithApiKey } from '$lib/stores/auth.svelte';
	import type { KeyValueItem } from '$lib/types/ui';
	import { onMount } from 'svelte';

	const auth = getAuthState();

	let apiKeyInput = $state(auth.apiKey ?? '');
	let error = $state<string | null>(null);
	let isSaving = $state(false);
	let saved = $state(false);

	/**
	 * OIDC provisioning policy is process config (`[api.oidc] auto_create_users`
	 * / XRELEASE_OIDC_AUTO_CREATE_USERS), so it is reported by the server and
	 * shown read-only — changing it needs a backend restart, not a UI toggle.
	 */
	let autoCreateUsers = $state<boolean | null>(null);

	onMount(() => {
		if (!isOidcEnabled()) return;
		void api
			.getAuthMethods()
			.then((methods) => {
				autoCreateUsers = methods.oidc_auto_create_users;
			})
			// Non-critical: the panel just omits the row if the probe fails.
			.catch(() => {
				autoCreateUsers = null;
			});
	});

	const sessionItems = $derived.by((): KeyValueItem[] => {
		if (!auth.profile) return [];
		const items: KeyValueItem[] = [
			{ label: t('settings.authRole'), value: roleLabel(auth.profile.appRole) },
			{
				label: t('settings.apiKeyStatus'),
				value: auth.apiKey ? t('settings.apiKeyConfigured') : t('settings.apiKeyMissing'),
				tone: auth.apiKey ? 'success' : 'warning'
			}
		];
		const name = getDisplayName(auth.profile);
		if (name) {
			items.unshift({ label: t('settings.signedInAs'), value: name });
		}
		if (isOidcEnabled() && auth.profile.oidcRoles.length > 0) {
			items.push({
				label: t('settings.idpRoles'),
				value: auth.profile.oidcRoles.slice(0, 4).join(', ')
			});
		}
		if (isOidcEnabled() && autoCreateUsers !== null) {
			items.push({
				label: t('settings.oidcAutoCreate'),
				value: autoCreateUsers
					? t('settings.oidcAutoCreateOn')
					: t('settings.oidcAutoCreateOff'),
				tone: autoCreateUsers ? 'default' : 'success'
			});
		}
		return items;
	});

	async function handleSaveKey(event: SubmitEvent) {
		event.preventDefault();
		const trimmed = apiKeyInput.trim();
		if (requiresApiKeyForApi() && !trimmed) {
			error = t('login.required');
			return;
		}

		isSaving = true;
		error = null;
		saved = false;
		try {
			if (trimmed) {
				await api.verifyApiKey(trimmed);
				loginWithApiKey(trimmed);
			}
			saved = true;
		} catch (err) {
			error =
				err instanceof ApiClientError
					? err.message
					: resolveApiError(err, t('errors.verifyKey'));
		} finally {
			isSaving = false;
		}
	}
</script>

<Panel title={t('settings.security')}>
	<div class={FIELD_STACK}>
		<div class="flex flex-wrap items-center gap-2">
			<AuthModeBadge />
			{#if auth.profile}
				<Badge tone="muted">{roleLabel(auth.profile.appRole)}</Badge>
			{/if}
		</div>

		{#if auth.profile}
			<KeyValueList items={sessionItems} />
		{/if}

		{#if isApiKeyEnabled()}
			<form class="{FIELD_STACK} border-t border-border pt-4" onsubmit={handleSaveKey}>
				<div class={FIELD_GROUP}>
					<label for="settings-api-key" class={fieldLabelClass}>{t('login.apiKey')}</label>
					<Input
						id="settings-api-key"
						type="password"
						autocomplete="off"
						placeholder={t('login.placeholder')}
						bind:value={apiKeyInput}
						disabled={isSaving}
					/>
				</div>
				<Button type="submit" size="sm" disabled={isSaving}>
					{isSaving ? t('login.verifying') : t('common.save')}
				</Button>
			</form>
		{/if}

		{#if error}
			<ErrorAlert message={error} />
		{/if}
		{#if saved}
			<p class={TYPE_STATUS_SUCCESS}>{t('settings.apiKeySaved')}</p>
		{/if}
	</div>
</Panel>
