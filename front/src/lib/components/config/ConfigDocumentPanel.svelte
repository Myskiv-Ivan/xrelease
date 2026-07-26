<script lang="ts">
	import type { ConfigView } from '$lib/api/types';
	import Button from '$lib/components/kit/Button.svelte';
	import Panel from '$lib/components/kit/Panel.svelte';
	import StatusBanner from '$lib/components/kit/StatusBanner.svelte';
	import { TYPE_CODE, TYPE_BODY, TYPE_HINT } from '$lib/components/kit/layout-styles';
	import {
		appriseTargetsMissing,
		extractAppDocument,
		extractBootstrapDocument,
		parseAppLayerData
	} from '$lib/config/config-layers';
	import { cn } from '$lib/utils';
	import { t } from '$lib/i18n';

	export type ConfigDocView = 'bootstrap' | 'app';

	interface Props {
		view: ConfigView;
	}

	let { view }: Props = $props();

	let layer = $state<ConfigDocView>('bootstrap');
	let copied = $state(false);

	const bootstrapContent = $derived(
		extractBootstrapDocument(view.content, view.bootstrap_sections)
	);
	const appContent = $derived(extractAppDocument(view.desired_content, view.content));
	const appData = $derived(parseAppLayerData(view.desired_content, view.content));
	const appriseMissing = $derived(appData ? appriseTargetsMissing(appData) : false);
	const content = $derived(layer === 'bootstrap' ? bootstrapContent : appContent);
	const title = $derived(
		layer === 'bootstrap' ? t('config.docViewBootstrap') : t('config.docViewApp')
	);
	const hint = $derived(
		layer === 'bootstrap' ? t('config.docViewBootstrapHint') : t('config.docViewAppHint')
	);
	const empty = $derived(
		layer === 'bootstrap' ? t('config.bootstrapEmpty') : t('config.desiredEmpty')
	);

	async function copy() {
		if (!content) return;
		try {
			await navigator.clipboard.writeText(content);
			copied = true;
			setTimeout(() => {
				copied = false;
			}, 1500);
		} catch {
			// clipboard may be unavailable
		}
	}
</script>

<Panel {title}>
	{#snippet actions()}
		<div
			class="inline-flex rounded-md border border-border p-0.5"
			role="group"
			aria-label={t('config.tabTechnical')}
		>
			<button
				type="button"
				class={cn(
					'cursor-pointer rounded px-2.5 py-1 text-xs font-medium transition-colors',
					layer === 'bootstrap'
						? 'bg-muted text-foreground'
						: 'text-muted-foreground hover:text-foreground'
				)}
				aria-pressed={layer === 'bootstrap'}
				onclick={() => (layer = 'bootstrap')}
			>
				{t('config.docViewBootstrap')}
			</button>
			<button
				type="button"
				class={cn(
					'cursor-pointer rounded px-2.5 py-1 text-xs font-medium transition-colors',
					layer === 'app'
						? 'bg-muted text-foreground'
						: 'text-muted-foreground hover:text-foreground'
				)}
				aria-pressed={layer === 'app'}
				onclick={() => (layer = 'app')}
			>
				{t('config.docViewApp')}
			</button>
		</div>
		{#if content}
			<Button variant="ghost" size="sm" onclick={copy}>
				{copied ? t('config.copied') : t('config.copy')}
			</Button>
		{/if}
	{/snippet}

	<p class="mb-3 {TYPE_HINT}">{hint}</p>

	{#if layer === 'app' && appriseMissing}
		<div class="mb-3">
			<StatusBanner tone="warning">{t('config.docViewAppAppriseUrlsMissing')}</StatusBanner>
		</div>
	{/if}

	{#if content}
		<pre
			class={cn(
				'max-h-[min(70dvh,36rem)] overflow-auto rounded-md border border-border bg-muted/40 p-3 leading-relaxed whitespace-pre-wrap',
				TYPE_CODE
			)}
		>{content}</pre>
	{:else}
		<p class={cn(TYPE_BODY, 'text-muted-foreground')}>{empty}</p>
	{/if}
</Panel>
