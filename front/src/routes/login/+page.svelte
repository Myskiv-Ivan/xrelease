<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, ApiClientError } from '#lib/api/client.js';
	import {
		getAuthMode,
		getOidcConfig,
		isApiKeyEnabled,
		isLocalEnabled,
		isOidcEnabled
	} from '#lib/auth/config.js';
	import { startOidcLogin } from '#lib/auth/oidc.js';
	import { safeReturnTo } from '#lib/auth/return-to.js';
	import type { AppRole } from '#lib/auth/types.js';
	import AppBrand from '#lib/components/layout/AppBrand.svelte';
	import Button from '#lib/components/kit/Button.svelte';
	import Input from '#lib/components/kit/Input.svelte';
	import StatusBanner from '#lib/components/kit/StatusBanner.svelte';
	import { FIELD_GROUP, FIELD_STACK, TYPE_PAGE_TITLE, TYPE_MUTED } from '#lib/components/kit/layout-styles.js';
	import { cn } from '#lib/utils.js';
	import { fieldLabelClass } from '#lib/components/kit/surface-styles.js';
	import { resolveApiError } from '#lib/core/errors.js';
	import { t } from '#lib/i18n/index.js';
	import {
		getAuthState,
		initAuth,
		loginWithApiKey,
		loginWithLocalSession
	} from '#lib/stores/auth.svelte.js';
	import { initSettings } from '#lib/stores/settings.svelte.js';
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

	const isBusy = $derived(isSubmitting || isOidcStarting);
	const formSurface = 'rounded-lg border border-border bg-card p-4 sm:p-5';

	onMount(() => {
		initAuth();
		initSettings();
		// Already signed in (e.g. hit /login?returnTo=/config from a bookmark) —
		// resume the deep link, do not dump to Overview.
		if (auth.isAuthenticated) {
			void goto(safeReturnTo(page.url.searchParams), { replaceState: true });
		}
	});

	async function handleOidcLogin() {
		isOidcStarting = true;
		error = null;
		try {
			await startOidcLogin(safeReturnTo(page.url.searchParams));
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
			await goto(safeReturnTo(page.url.searchParams), { replaceState: true });
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
			await goto(safeReturnTo(page.url.searchParams), { replaceState: true });
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

<!-- Full-bleed wash (layout drops max-width on /login); form stays measured. -->
<div class="login-shell flex flex-col px-4 py-10 sm:px-6 sm:py-12">
	<div
		class="mx-auto flex w-full max-w-sm flex-1 flex-col justify-center gap-6 sm:max-w-md sm:gap-8"
	>
	<header class="flex flex-col gap-3">
		<AppBrand size="lg" layout="inline" />
		<h1 class={TYPE_PAGE_TITLE}>{t('login.title')}</h1>
	</header>

	{#if page.url.searchParams.get('oidc') === '1'}
		<StatusBanner tone="success">{t('login.oidcSuccess')}</StatusBanner>
	{/if}

	{#if oidcMisconfigured}
		<StatusBanner tone="warning">{t('login.oidcNotConfigured')}</StatusBanner>
	{/if}

	{#if error}
		<StatusBanner tone="danger">{error}</StatusBanner>
	{/if}

	<div class={FIELD_STACK}>
		{#if showLocal}
			<form class={cn(FIELD_STACK, formSurface)} onsubmit={handleLocalSubmit}>
				<div class={FIELD_GROUP}>
					<label for="username" class={fieldLabelClass}>{t('login.username')}</label>
					<Input
						id="username"
						type="text"
						autocomplete="username"
						placeholder={t('login.usernamePlaceholder')}
						bind:value={username}
						disabled={isBusy}
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
						disabled={isBusy}
					/>
				</div>
				<Button type="submit" class="w-full" disabled={isBusy}>
					{isSubmitting ? t('login.verifying') : t('login.continueLocal')}
				</Button>
				{#if showOidc}
					<div class="relative flex items-center justify-center py-0.5" role="separator">
						<div class="absolute inset-x-0 top-1/2 border-t border-border"></div>
						<span class="relative bg-card px-3 {TYPE_MUTED}">
							{t('login.orSso')}
						</span>
					</div>
					<Button
						type="button"
						variant="outline"
						class="w-full"
						disabled={isBusy}
						onclick={() => void handleOidcLogin()}
					>
						{isOidcStarting ? t('login.oidcRedirecting') : t('login.oidcContinue')}
					</Button>
				{/if}
			</form>
		{:else if showOidc}
			<div class={formSurface}>
				<Button class="w-full" disabled={isBusy} onclick={() => void handleOidcLogin()}>
					{isOidcStarting ? t('login.oidcRedirecting') : t('login.oidcContinue')}
				</Button>
			</div>
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
			<form class={cn(FIELD_STACK, formSurface)} onsubmit={handleApiKeySubmit}>
				<div class={FIELD_GROUP}>
					<label for="api-key" class={fieldLabelClass}>{t('login.apiKey')}</label>
					<Input
						id="api-key"
						type="password"
						autocomplete="off"
						placeholder={t('login.placeholder')}
						bind:value={apiKeyInput}
						disabled={isBusy}
					/>
				</div>
				<Button type="submit" class="w-full" disabled={isBusy}>
					{isSubmitting ? t('login.verifying') : t('login.continue')}
				</Button>
			</form>
		{/if}

		{#if !showOidc && !showApiKey && !showLocal}
			<StatusBanner tone="danger">{t('login.noMethods')}</StatusBanner>
		{/if}
	</div>
	</div>
</div>
