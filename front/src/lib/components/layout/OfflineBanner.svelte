<script lang="ts">
	import StatusBanner from '$lib/components/kit/StatusBanner.svelte';
	import { t } from '$lib/i18n';
	import { getNetworkState } from '$lib/stores/network.svelte';

	const network = getNetworkState();
</script>

{#if !network.isOnline}
	<div class="border-b border-warning/40">
		<StatusBanner tone="warning" class="rounded-none border-0 text-center">
			{t('offline.message')}
		</StatusBanner>
	</div>
{/if}
