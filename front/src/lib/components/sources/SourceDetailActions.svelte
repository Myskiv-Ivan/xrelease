<script lang="ts">
	import Button from '$lib/components/kit/Button.svelte';
	import { t } from '$lib/i18n';
	import { getAuthState } from '$lib/stores/auth.svelte';
	import { getNetworkState } from '$lib/stores/network.svelte';

	interface Props {
		isPolling?: boolean;
		onPoll: () => void | Promise<void>;
	}

	let { isPolling = false, onPoll }: Props = $props();
	const network = getNetworkState();
	const auth = getAuthState();
</script>

<div class="mb-4 flex flex-wrap gap-2">
	<Button variant="outline" href="/sources">{t('sources.back')}</Button>
	{#if auth.hasPermission('poll:execute')}
		<Button variant="accent" disabled={isPolling || !network.isOnline} onclick={() => onPoll()}>
			{isPolling ? t('overview.polling') : t('sources.pollNow')}
		</Button>
	{/if}
</div>
