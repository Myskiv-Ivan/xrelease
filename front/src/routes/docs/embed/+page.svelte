<script lang="ts">
	/**
	 * Bare Scalar host for the /docs iframe.
	 * Runs without app chrome so Scalar can own `body.dark-mode` without
	 * corrupting the parent dashboard theme, and so expand/collapse clicks
	 * are not intercepted by SPA link rewriting.
	 */
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { getOpenApiSpecUrl } from '$lib/api/urls';
	import { getBearerToken } from '$lib/auth/credentials';
	import { APP_NAME } from '$lib/core/constants';
	import { initAuth } from '$lib/stores/auth.svelte';
	import { initSettings } from '$lib/stores/settings.svelte';

	let container = $state<HTMLDivElement | null>(null);

	function themeFromQuery(): 'dark' | 'light' {
		const q = page.url.searchParams.get('theme');
		if (q === 'light' || q === 'dark') return q;
		if (typeof document !== 'undefined' && document.documentElement.classList.contains('dark')) {
			return 'dark';
		}
		return 'dark';
	}

	function applyBodyMode(mode: 'dark' | 'light') {
		document.documentElement.classList.toggle('dark', mode === 'dark');
		document.documentElement.dataset.theme = mode;
		document.body.classList.toggle('dark-mode', mode === 'dark');
		document.body.classList.toggle('light-mode', mode === 'light');
		document.documentElement.style.colorScheme = mode;
	}

	onMount(() => {
		initAuth();
		initSettings();

		let cancelled = false;
		let instance: { destroy?: () => void; updateConfiguration?: (c: Record<string, unknown>) => void } | null =
			null;

		const mode = themeFromQuery();
		applyBodyMode(mode);

		function onMessage(event: MessageEvent) {
			if (event.origin !== window.location.origin) return;
			const data = event.data as { type?: string; theme?: string } | null;
			if (!data || data.type !== 'xrelease-theme') return;
			if (data.theme !== 'dark' && data.theme !== 'light') return;
			applyBodyMode(data.theme);
			instance?.updateConfiguration?.({
				darkMode: data.theme === 'dark',
				forceDarkModeState: data.theme,
				hideDarkModeToggle: true
			});
		}

		window.addEventListener('message', onMessage);

		void (async () => {
			const [{ createApiReference }, _styles] = await Promise.all([
				import('@scalar/api-reference'),
				import('@scalar/api-reference/style.css')
			]);
			if (cancelled || !container) return;

			const token = getBearerToken();
			instance = createApiReference(container, {
				url: getOpenApiSpecUrl(),
				darkMode: mode === 'dark',
				forceDarkModeState: mode,
				hideDarkModeToggle: true,
				withDefaultFonts: false,
				layout: 'modern',
				showSidebar: true,
				hideModels: false,
				metaData: { title: `${APP_NAME} API` },
				...(token
					? {
							authentication: {
								preferredSecurityScheme: 'bearerAuth',
								securitySchemes: {
									bearerAuth: { token }
								}
							}
						}
					: {})
			});
		})();

		return () => {
			cancelled = true;
			window.removeEventListener('message', onMessage);
			instance?.destroy?.();
			if (container) container.replaceChildren();
		};
	});
</script>

<svelte:head>
	<title>{APP_NAME} API</title>
</svelte:head>

<div class="scalar-embed min-h-dvh w-full bg-background text-foreground">
	<div bind:this={container} class="scalar-api min-h-dvh w-full"></div>
</div>

<style>
	:global(html),
	:global(body) {
		margin: 0;
		min-height: 100%;
	}
	:global(.scalar-api) {
		--scalar-font: var(--font-sans);
		--scalar-font-code: var(--font-mono);
	}
</style>
