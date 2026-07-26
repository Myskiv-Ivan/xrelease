<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, ApiClientError } from '$lib/api/client';
	import {
		getAuthMode,
		getOidcConfig,
		isApiKeyEnabled,
		isLocalEnabled,
		isOidcEnabled
	} from '$lib/auth/config';
	import { startOidcLogin } from '$lib/auth/oidc';
	import type { AppRole } from '$lib/auth/types';
	import AuthModeBadge from '$lib/components/auth/AuthModeBadge.svelte';
	import ErrorAlert from '$lib/components/dashboard/ErrorAlert.svelte';
	import AppBrand from '$lib/components/layout/AppBrand.svelte';
	import Button from '$lib/components/kit/Button.svelte';
	import Input from '$lib/components/kit/Input.svelte';
	import Panel from '$lib/components/kit/Panel.svelte';
	import StatusBanner from '$lib/components/kit/StatusBanner.svelte';
	import { FIELD_GROUP, FIELD_STACK, TYPE_PAGE_TITLE, TYPE_MUTED } from '$lib/components/kit/layout-styles';
	import { fieldLabelClass } from '$lib/components/kit/surface-styles';
	import { APP_VERSION } from '$lib/core/constants';
	import { resolveApiError } from '$lib/core/errors';
	import { t } from '$lib/i18n';
	import {
		getAuthState,
		initAuth,
		loginWithApiKey,
		loginWithLocalSession
	} from '$lib/stores/auth.svelte';
	import { initSettings } from '$lib/stores/settings.svelte';
	import { onMount } from 'svelte';

	const auth = getAuthState();
	const authMode = getAuthMode();
	const showOidc = isOidcEnabled();
	const showLocal = isLocalEnabled();
	const showApiKey = isApiKeyEnabled();
	const oidcMisconfigured =
		(authMode === 'oidc' || authMode === 'hybrid') && getOidcConfig() === null;

	let username = $state('admin');
	let password = $state('');
	let apiKeyInput = $state('');
	let error = $state<string | null>(null);
	let isSubmitting = $state(false);
	let isOidcStarting = $state(false);
	let displayVersion = $state(APP_VERSION);

	onMount(() => {
		initAuth();
		initSettings();
		if (auth.isAuthenticated) {
			goto('/');
		}

		void api.getStatus().then(
			(status) => {
				displayVersion = status.version;
			},
			() => {
				// Auth may be required — keep build-time version.
			}
		);
	});

	async function handleOidcLogin() {
		isOidcStarting = true;
		error = null;
		try {
			const returnTo = page.url.searchParams.get('returnTo') ?? '/';
			await startOidcLogin(returnTo);
		} catch (err) {
			error = resolveApiError(err, t('login.oidcFailed'));
			isOidcStarting = false;
		}
	}

	async function handleLocalSubmit(event: SubmitEvent) {
		event.preventDefault();
		const user = username.trim();
		if (!user || !password) {
			error = t('login.localRequired');
			return;
		}

		isSubmitting = true;
		error = null;
		try {
			const response = await api.login({ username: user, password });
			loginWithLocalSession({
				accessToken: response.access_token,
				expiresAt: Date.now() + response.expires_in * 1000,
				role: response.role as AppRole,
				username: response.user.username ?? user,
				displayName: response.user.display_name ?? undefined
			});
			goto(page.url.searchParams.get('returnTo') ?? '/');
		} catch (err) {
			error =
				err instanceof ApiClientError
					? err.message
					: resolveApiError(err, t('errors.verifyKey'));
		} finally {
			isSubmitting = false;
		}
	}

	async function handleApiKeySubmit(event: SubmitEvent) {
		event.preventDefault();
		const trimmed = apiKeyInput.trim();
		if (!trimmed) {
			error = t('login.required');
			return;
		}

		isSubmitting = true;
		error = null;
		try {
			await api.verifyApiKey(trimmed);
			loginWithApiKey(trimmed);
			goto(page.url.searchParams.get('returnTo') ?? '/');
		} catch (err) {
			error =
				err instanceof ApiClientError
					? err.message
					: resolveApiError(err, t('errors.verifyKey'));
		} finally {
			isSubmitting = false;
		}
	}
</script>

<div
	class="mx-auto flex w-full max-w-sm flex-col justify-center gap-6 py-8 sm:min-h-[calc(100dvh-10rem)] sm:max-w-md sm:gap-7 sm:py-10"
>
	<header class="flex flex-col items-center gap-4 text-center">
		<AppBrand size="lg" layout="stack" version={displayVersion} />
		<div class="flex flex-col items-center gap-2">
			<h1 class={TYPE_PAGE_TITLE}>{t('login.title')}</h1>
			<AuthModeBadge />
		</div>
	</header>

	{#if page.url.searchParams.get('oidc') === '1'}
		<StatusBanner tone="success">{t('login.oidcSuccess')}</StatusBanner>
	{/if}

	{#if oidcMisconfigured}
		<StatusBanner tone="warning">{t('login.oidcNotConfigured')}</StatusBanner>
	{/if}

	{#if error}
		<ErrorAlert message={error} />
	{/if}

	<div class={FIELD_STACK}>
		{#if showLocal}
			<Panel>
				<form class={FIELD_STACK} onsubmit={handleLocalSubmit}>
					<div class={FIELD_GROUP}>
						<label for="username" class={fieldLabelClass}>{t('login.username')}</label>
						<Input
							id="username"
							type="text"
							autocomplete="username"
							placeholder={t('login.usernamePlaceholder')}
							bind:value={username}
							disabled={isSubmitting}
						/>
					</div>
					<div class={FIELD_GROUP}>
						<label for="password" class={fieldLabelClass}>{t('login.password')}</label>
						<Input
							id="password"
							type="password"
							autocomplete="current-password"
							placeholder={t('login.passwordPlaceholder')}
							bind:value={password}
							disabled={isSubmitting}
						/>
					</div>
					<Button type="submit" class="w-full" disabled={isSubmitting}>
						{isSubmitting ? t('login.verifying') : t('login.continueLocal')}
					</Button>
				</form>
			</Panel>
		{/if}

		{#if showOidc}
			{#if showLocal}
				<div class="relative flex items-center justify-center py-0.5" role="separator">
					<div class="absolute inset-x-0 top-1/2 border-t border-border"></div>
					<span class="relative bg-background px-3 {TYPE_MUTED}">
						{t('login.orSso')}
					</span>
				</div>
			{/if}
			<Panel>
				<Button class="w-full" disabled={isOidcStarting} onclick={() => void handleOidcLogin()}>
					{isOidcStarting ? t('login.oidcRedirecting') : t('login.oidcContinue')}
				</Button>
			</Panel>
		{/if}

		{#if showApiKey}
			{#if showLocal || showOidc}
				<div class="relative flex items-center justify-center py-0.5" role="separator">
					<div class="absolute inset-x-0 top-1/2 border-t border-border"></div>
					<span class="relative bg-background px-3 {TYPE_MUTED}">
						{t('login.orApiKey')}
					</span>
				</div>
			{/if}
			<Panel>
				<form class={FIELD_STACK} onsubmit={handleApiKeySubmit}>
					<div class={FIELD_GROUP}>
						<label for="api-key" class={fieldLabelClass}>{t('login.apiKey')}</label>
						<Input
							id="api-key"
							type="password"
							autocomplete="off"
							placeholder={t('login.placeholder')}
							bind:value={apiKeyInput}
							disabled={isSubmitting}
						/>
					</div>
					<Button type="submit" class="w-full" disabled={isSubmitting}>
						{isSubmitting ? t('login.verifying') : t('login.continue')}
					</Button>
				</form>
			</Panel>
		{/if}

		{#if !showOidc && !showApiKey && !showLocal}
			<StatusBanner tone="danger">{t('login.noMethods')}</StatusBanner>
		{/if}
	</div>
</div>
