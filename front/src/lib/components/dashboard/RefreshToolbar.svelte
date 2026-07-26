<script lang="ts">
	import Button from '$lib/components/kit/Button.svelte';
	import Checkbox from '$lib/components/kit/Checkbox.svelte';
	import Select from '$lib/components/kit/Select.svelte';
	import { REFRESH_INTERVALS } from '$lib/core/constants';
	import { t } from '$lib/i18n';
	import { getNetworkState } from '$lib/stores/network.svelte';
	import {
		getSettingsState,
		setAutoRefresh,
		setRefreshIntervalMs
	} from '$lib/stores/settings.svelte';
	import { getNowStore } from '$lib/stores/now.svelte';
	import { formatRelative } from '$lib/core/format';
	import { TYPE_MUTED, TYPE_HINT } from '$lib/components/kit/layout-styles';

	interface Props {
		lastUpdated?: Date | null;
		isRefreshing?: boolean;
		onRefresh?: () => void | Promise<void>;
	}

	let { lastUpdated = null, isRefreshing = false, onRefresh }: Props = $props();
	const settings = getSettingsState();
	const network = getNetworkState();
	const now = getNowStore();
	const updatedLabel = $derived(
		lastUpdated ? formatRelative(lastUpdated, now.current) : null
	);
</script>

<div
	class="flex flex-wrap items-center gap-x-3 gap-y-2 rounded-lg border border-border/70 bg-muted/20 px-3 py-2"
>
	{#if onRefresh}
		<Button
			variant="outline"
			size="sm"
			disabled={isRefreshing || !network.isOnline}
			title={t('common.refreshHint')}
			onclick={() => onRefresh?.()}
		>
			{isRefreshing ? t('common.refreshing') : t('common.refresh')}
		</Button>
	{/if}

	<label class="flex min-h-9 cursor-pointer items-center gap-2 {TYPE_HINT}">
		<Checkbox
			checked={settings.autoRefresh}
			onchange={(event) => setAutoRefresh(event.currentTarget.checked)}
		/>
		{t('common.autoRefresh')}
	</label>

	<Select
		class="w-auto"
		value={settings.refreshIntervalMs}
		disabled={!settings.autoRefresh}
		aria-label={t('settings.interval')}
		onchange={(event) => setRefreshIntervalMs(Number(event.currentTarget.value))}
	>
		{#each REFRESH_INTERVALS as option}
			<option value={option.value}>{option.label}</option>
		{/each}
	</Select>

	{#if lastUpdated && updatedLabel}
		<span class="{TYPE_MUTED} sm:ml-auto" title={lastUpdated.toLocaleString()}>
			{t('common.updated')} {updatedLabel}
		</span>
	{/if}
</div>
