<script lang="ts">
	import type { ConfigApplyStatus } from '$lib/api/types';
	import Badge from '$lib/components/kit/Badge.svelte';
	import Button from '$lib/components/kit/Button.svelte';
	import KeyValueList from '$lib/components/kit/KeyValueList.svelte';
	import Panel from '$lib/components/kit/Panel.svelte';
	import {
		configApiEnabled,
		configProvenanceItems
	} from '$lib/config/config-presentation';
	import { t } from '$lib/i18n';
	import { getNowStore } from '$lib/stores/now.svelte';

	interface Props {
		status: ConfigApplyStatus;
	}

	let { status }: Props = $props();

	const now = getNowStore();
	const enabled = $derived(configApiEnabled(status));
	const items = $derived(
		configProvenanceItems(
			{
				desired_source: status.desired_source,
				revision: status.revision,
				revision_label: status.revision_label,
				applied_at: status.applied_at,
				last_rejected_revision: status.last_rejected_revision
			},
			now.current
		)
	);
</script>

<Panel title={t('overview.configStatus')}>
	{#snippet actions()}
		<div class="flex items-center gap-2">
			<Badge tone={enabled ? 'success' : 'muted'} dot>
				{enabled ? t('config.configApiOn') : t('config.configApiOff')}
			</Badge>
			<Button href="/config" variant="outline" size="sm">{t('nav.config')}</Button>
		</div>
	{/snippet}

	<KeyValueList {items} layout="grid" columns={2} />

	{#if status.last_rejected_error}
		<p class="mt-3 truncate text-xs text-destructive" title={status.last_rejected_error}>
			{status.last_rejected_error}
		</p>
	{/if}
</Panel>
