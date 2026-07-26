<script lang="ts">
	import DashboardShell from '$lib/components/layout/DashboardShell.svelte';
	import { getSettingsState } from '$lib/stores/settings.svelte';
	import { t } from '$lib/i18n';
	import { onMount } from 'svelte';

	const settings = getSettingsState();
	let frame = $state<HTMLIFrameElement | null>(null);
	/** Stable iframe URL — theme updates go via postMessage so expand state survives. */
	let embedSrc = $state('/docs/embed?theme=dark');

	function readTheme(): 'dark' | 'light' {
		if (typeof document === 'undefined') return 'dark';
		return document.documentElement.classList.contains('dark') ? 'dark' : 'light';
	}

	function postTheme() {
		frame?.contentWindow?.postMessage(
			{ type: 'xrelease-theme', theme: readTheme() },
			window.location.origin
		);
	}

	onMount(() => {
		embedSrc = `/docs/embed?theme=${readTheme()}`;
		const obs = new MutationObserver(() => postTheme());
		obs.observe(document.documentElement, {
			attributes: true,
			attributeFilter: ['class', 'data-theme']
		});
		return () => obs.disconnect();
	});

	$effect(() => {
		void settings.theme;
		postTheme();
	});
</script>

<DashboardShell title={t('docs.title')} description={t('docs.description')} permission="about:read">
	<div class="relative left-1/2 w-screen max-w-[100vw] -translate-x-1/2 px-4 sm:px-6">
		<iframe
			bind:this={frame}
			title={t('docs.title')}
			src={embedSrc}
			class="min-h-[calc(100dvh-12rem)] w-full rounded-xl border border-border bg-card"
			onload={postTheme}
		></iframe>
	</div>
</DashboardShell>
