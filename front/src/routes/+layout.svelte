<script lang="ts">
	import '../app.css';
	import favicon from '$lib/assets/favicon.svg';
	import AppHeader from '$lib/components/layout/AppHeader.svelte';
	import AppFooter from '$lib/components/layout/AppFooter.svelte';
	import OfflineBanner from '$lib/components/layout/OfflineBanner.svelte';
	import ToastHost from '$lib/components/kit/ToastHost.svelte';
	import { LAYOUT_MAX } from '$lib/components/kit/layout-styles';
	import { startRefreshScheduler, stopRefreshScheduler } from '$lib/data/refresh-scheduler';
	import { getConfigSchemaStore } from '$lib/data/config-schema.svelte';
	import { getSourcesStore } from '$lib/data/sources.svelte';
	import { getStatusStore } from '$lib/data/status.svelte';
	import { getTeamsStore } from '$lib/data/teams.svelte';
	import { getAuthState, initAuth, refreshAuthMe } from '$lib/stores/auth.svelte';
	import { getOrganizationsState } from '$lib/stores/organizations.svelte';
	import { initNetworkMonitor } from '$lib/stores/network.svelte';
	import { startNowTicker, stopNowTicker } from '$lib/stores/now.svelte';
	import {
		initSettings,
		isAutoRefreshEnabled,
		getRefreshIntervalMs
	} from '$lib/stores/settings.svelte';
	import { t } from '$lib/i18n';
	import { page } from '$app/state';
	import { onMount } from 'svelte';

	let { children } = $props();

	const auth = getAuthState();
	const orgs = getOrganizationsState();
	const statusStore = getStatusStore();
	const sourcesStore = getSourcesStore();
	const teamsStore = getTeamsStore();
	const configSchemaStore = getConfigSchemaStore();

	/** Bare chrome for Scalar embed iframe — no header/footer, no shared body theme fights. */
	const isEmbedShell = $derived(page.url.pathname.startsWith('/docs/embed'));

	onMount(() => {
		initAuth();
		initSettings();
		return initNetworkMonitor();
	});

	$effect(() => {
		if (isEmbedShell) return;
		if (!auth.isReady) return;

		if (auth.isAuthenticated) {
			statusStore.start();
			sourcesStore.start();
			teamsStore.start();
			configSchemaStore.start();
			void refreshAuthMe();
			// Organization catalogue is bootstrap-only — load once per session.
			if (!orgs.isLoaded) void orgs.load();
			startNowTicker();
			return () => {
				stopNowTicker();
			};
		}

		statusStore.stop();
		sourcesStore.stop();
		teamsStore.stop();
		configSchemaStore.stop();
		stopNowTicker();
	});

	$effect(() => {
		if (isEmbedShell) return;
		void isAutoRefreshEnabled();
		void getRefreshIntervalMs();

		if (!auth.isAuthenticated || !isAutoRefreshEnabled()) {
			stopRefreshScheduler();
			return;
		}

		startRefreshScheduler(getRefreshIntervalMs());
		return () => stopRefreshScheduler();
	});
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
	<title>xrelease dashboard</title>
</svelte:head>

{#if isEmbedShell}
	{@render children()}
{:else}
	<div class="flex min-h-dvh flex-col">
		<a
			href="#main-content"
			class="sr-only focus:not-sr-only focus:absolute focus:top-3 focus:left-3 focus:z-50 focus:rounded-md focus:bg-primary focus:px-4 focus:py-2 focus:text-sm focus:font-medium focus:text-primary-foreground focus:shadow-md"
		>
			{t('common.skipToContent')}
		</a>
		<OfflineBanner />
		<AppHeader />
		<main id="main-content" class="mx-auto w-full {LAYOUT_MAX} flex-1 px-4 py-6 sm:px-6">
			{@render children()}
		</main>
		<AppFooter />
		<ToastHost />
	</div>
{/if}
