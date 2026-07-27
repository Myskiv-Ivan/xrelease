<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api } from '$lib/api/client';
	import { requiresApiKeyForApi } from '$lib/auth/config';
	import { getStoredApiKey } from '$lib/auth/credentials';
	import {
		completeOidcCallback,
		writeOidcAppRole,
		writeOidcOrganizationRoles
	} from '$lib/auth/oidc';
	import { sanitizeReturnTo } from '$lib/auth/return-to';
	import type { AppRole } from '$lib/auth/types';
	import ErrorAlert from '$lib/components/dashboard/ErrorAlert.svelte';
	import AppBrand from '$lib/components/layout/AppBrand.svelte';
	import Button from '$lib/components/kit/Button.svelte';
	import LoadingState from '$lib/components/kit/LoadingState.svelte';
	import Panel from '$lib/components/kit/Panel.svelte';
	import { TYPE_PAGE_DESC } from '$lib/components/kit/layout-styles';
	import { APP_VERSION } from '$lib/core/constants';
	import { resolveApiError } from '$lib/core/errors';
	import { t } from '$lib/i18n';
	import { initAuth, loginWithOidcSession } from '$lib/stores/auth.svelte';
	import { initSettings } from '$lib/stores/settings.svelte';
	import { onMount } from 'svelte';

	let error = $state<string | null>(null);

	onMount(() => {
		initAuth();
		initSettings();

		void (async () => {
			try {
				// Re-validate: this value round-tripped through session storage.
				const returnTo = sanitizeReturnTo(
					(await completeOidcCallback(page.url.searchParams)).returnTo
				);
				loginWithOidcSession();

				try {
					const synced = await api.syncOidcUser();
					writeOidcAppRole(synced.role as AppRole);
					writeOidcOrganizationRoles(
						(synced.organization_roles ?? {}) as Record<string, AppRole>
					);
					loginWithOidcSession();
				} catch {
					// Soft-fail: client-side claim mapping still works for this session.
				}

				if (requiresApiKeyForApi() && !getStoredApiKey()) {
					const loginUrl = `/login?returnTo=${encodeURIComponent(returnTo)}&oidc=1`;
					await goto(loginUrl);
					return;
				}

				await goto(returnTo);
			} catch (err) {
				error = resolveApiError(err, t('login.oidcFailed'));
			}
		})();
	});
</script>

<div
	class="mx-auto flex w-full max-w-sm flex-col justify-center gap-6 py-8 sm:min-h-[calc(100dvh-10rem)] sm:max-w-md sm:gap-7 sm:py-10"
>
	<header class="flex flex-col items-center gap-4 text-center">
		<AppBrand size="lg" layout="stack" version={APP_VERSION} />
		<p class={TYPE_PAGE_DESC}>{t('login.oidcCompleting')}</p>
	</header>

	<Panel>
		{#if error}
			<div class="flex flex-col gap-4">
				<ErrorAlert message={error} />
				<Button href="/login" variant="outline" class="w-full">{t('login.backToLogin')}</Button>
			</div>
		{:else}
			<LoadingState label={t('login.oidcCompleting')} />
		{/if}
	</Panel>
</div>
