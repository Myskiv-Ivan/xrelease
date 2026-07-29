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
	import Timestamp from '$lib/components/kit/Timestamp.svelte';
	import { TYPE_MUTED, TYPE_HINT } from '$lib/components/kit/layout-styles';

	interface Props {
		lastUpdated?: Date | null;
		isRefreshing?: boolean;
		onRefresh?: () => void | Promise<void>;
	}

	let { lastUpdated = null, isRefreshing = false, onRefresh }: Props = $props();
	const settings = getSettingsState();
	const network = getNetworkState();
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

	{#if lastUpdated}
		<!--
			Freshness chrome, not a data field: relative reads better here, and
			`Timestamp` keeps the exact instant in the tooltip using the same
			pinned-locale format as every table (`toLocaleString()` would follow
			the operator's OS and disagree with them).
		-->
		<span class="{TYPE_MUTED} sm:ml-auto">
			{t('common.updated')} <Timestamp value={lastUpdated} format="relative" class="inline" />
		</span>
	{/if}
</div>
