<script lang="ts">
	import { browser } from '$app/env';
	import Panel from '#lib/components/kit/Panel.svelte';
	import { readUiSetting } from '#lib/config/runtime.js';
	import { t } from '#lib/i18n/index.js';

	/**
	 * Optional Grafana dashboard iframe.
	 * Set `VITE_GRAFANA_EMBED_URL` (runtime env or bake-time); leave unset to hide.
	 */
	const embedUrl = readUiSetting('VITE_GRAFANA_EMBED_URL');
	const hasEmbed = browser && typeof embedUrl === 'string' && embedUrl.length > 0;
</script>

{#if hasEmbed}
	<Panel title={t('overview.metricsEmbed')}>
		<iframe
			title={t('overview.metricsEmbed')}
			src={embedUrl}
			class="h-[28rem] w-full rounded-md border border-border bg-muted/20"
			loading="lazy"
			referrerpolicy="no-referrer"
		></iframe>
	</Panel>
{/if}
