<script lang="ts">
	import DashboardShell from '#lib/components/layout/DashboardShell.svelte';
	import { getSettingsState } from '#lib/stores/settings.svelte.js';
	import { t } from '#lib/i18n/index.js';
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
	<!-- Stay inside LAYOUT_MAX — full-bleed w-screen ignores lg:pl-56 and overflows the rail. -->
	<div class="min-w-0 w-full">
		<iframe
			bind:this={frame}
			title={t('docs.title')}
			src={embedSrc}
			class="min-h-[calc(100dvh-12rem)] w-full max-w-full rounded-lg border border-border bg-card"
			onload={postTheme}
		></iframe>
	</div>
</DashboardShell>
