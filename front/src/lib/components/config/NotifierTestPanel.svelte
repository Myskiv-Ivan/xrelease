<script lang="ts">
	import { api, ApiClientError } from '$lib/api/client';
	import type { NotifierTestResult, NotifierView } from '$lib/api/types';
	import Badge from '$lib/components/kit/Badge.svelte';
	import Button from '$lib/components/kit/Button.svelte';
	import Panel from '$lib/components/kit/Panel.svelte';
	import StatusBanner from '$lib/components/kit/StatusBanner.svelte';
	import { TYPE_CODE, TYPE_HINT, TYPE_MUTED } from '$lib/components/kit/layout-styles';
	import { resolveApiError } from '$lib/core/errors';
	import { t } from '$lib/i18n';
	import { getAuthState } from '$lib/stores/auth.svelte';
	import { getNetworkState } from '$lib/stores/network.svelte';
	import { onMount } from 'svelte';

	const auth = getAuthState();
	const network = getNetworkState();

	let notifiers = $state<NotifierView[]>([]);
	let loadError = $state<string | null>(null);
	let isLoading = $state(false);
	let testingIndex = $state<number | 'all' | null>(null);
	let results = $state<Record<number, NotifierTestResult>>({});
	let banner = $state<{ tone: 'success' | 'danger'; text: string } | null>(null);

	const canTest = $derived(auth.hasPermission('poll:execute'));

	async function loadNotifiers() {
		isLoading = true;
		loadError = null;
		try {
			const response = await api.listNotifiers();
			notifiers = response.notifiers;
		} catch (err) {
			loadError = resolveApiError(err, t('notifiers.loadFailed'));
			notifiers = [];
		} finally {
			isLoading = false;
		}
	}

	onMount(() => {
		void loadNotifiers();
	});

	function applyResults(next: NotifierTestResult[]) {
		const map = { ...results };
		for (const row of next) {
			map[row.index] = row;
		}
		results = map;
		const failed = next.filter((row) => !row.ok);
		if (failed.length === 0) {
			banner = {
				tone: 'success',
				text:
					next.length === 1
						? t('notifiers.testOkOne')
						: t('notifiers.testOkAll').replace('{count}', String(next.length))
			};
		} else {
			banner = {
				tone: 'danger',
				text: t('notifiers.testFailed').replace('{count}', String(failed.length))
			};
		}
	}

	async function runTest(index?: number) {
		if (!canTest || !network.isOnline) return;
		testingIndex = index ?? 'all';
		banner = null;
		try {
			const response = await api.testNotifiers(index === undefined ? {} : { index });
			applyResults(response.results);
		} catch (err) {
			const message =
				err instanceof ApiClientError
					? err.message
					: resolveApiError(err, t('notifiers.testError'));
			banner = { tone: 'danger', text: message };
		} finally {
			testingIndex = null;
		}
	}

	function tagsLabel(tags: string[]): string {
		return tags.length === 0 ? t('notifiers.wildcard') : tags.join(', ');
	}
</script>

<Panel title={t('notifiers.title')}>
	{#snippet actions()}
		<div class="flex flex-wrap items-center gap-2">
			<Button
				variant="outline"
				size="sm"
				disabled={isLoading || !network.isOnline}
				onclick={() => void loadNotifiers()}
			>
				{t('notifiers.refresh')}
			</Button>
			{#if canTest}
				<Button
					size="sm"
					disabled={
						isLoading ||
						!network.isOnline ||
						notifiers.length === 0 ||
						testingIndex !== null
					}
					onclick={() => void runTest()}
				>
					{testingIndex === 'all' ? t('notifiers.testing') : t('notifiers.testAll')}
				</Button>
			{/if}
		</div>
	{/snippet}

		<p class="mb-3 {TYPE_HINT}">{t('notifiers.hint')}</p>

		{#if banner}
		<div class="mb-3">
			<StatusBanner tone={banner.tone}>{banner.text}</StatusBanner>
		</div>
	{/if}

	{#if loadError}
		<StatusBanner tone="danger">{loadError}</StatusBanner>
	{:else if isLoading && notifiers.length === 0}
		<p class={TYPE_HINT}>{t('notifiers.loading')}</p>
	{:else if notifiers.length === 0}
		<p class={TYPE_HINT}>{t('notifiers.empty')}</p>
	{:else}
		<ul class="flex flex-col gap-2">
			{#each notifiers as sink (sink.index)}
				{@const result = results[sink.index]}
				<li
					class="flex flex-wrap items-center gap-2 rounded-md border border-border bg-card px-3 py-2"
				>
					<Badge tone="muted">{sink.kind}</Badge>
					<span class="min-w-0 truncate text-sm font-medium" title={sink.name}>{sink.name}</span>
					<span class="{TYPE_CODE} {TYPE_MUTED}" title={tagsLabel(sink.tags)}>
						{tagsLabel(sink.tags)}
					</span>
					{#if result}
						<Badge tone={result.ok ? 'success' : 'danger'} dot>
							{result.ok ? t('notifiers.resultOk') : t('notifiers.resultFail')}
						</Badge>
					{/if}
					{#if canTest}
						<div class="ml-auto shrink-0">
							<Button
								variant="outline"
								size="sm"
								disabled={!network.isOnline || testingIndex !== null}
								onclick={() => void runTest(sink.index)}
							>
								{testingIndex === sink.index
									? t('notifiers.testing')
									: t('notifiers.testOne')}
							</Button>
						</div>
					{/if}
					{#if result?.error}
						<p class="basis-full text-xs text-destructive">{result.error}</p>
					{/if}
				</li>
			{/each}
		</ul>
	{/if}

	{#if !canTest}
		<p class="mt-3 {TYPE_MUTED}">{t('notifiers.needOperator')}</p>
	{/if}
</Panel>
