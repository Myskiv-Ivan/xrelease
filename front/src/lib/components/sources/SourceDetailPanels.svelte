<script lang="ts">
	import type { SourceDetail } from '$lib/api/types';
	import Panel from '$lib/components/kit/Panel.svelte';
	import KeyValueList from '$lib/components/kit/KeyValueList.svelte';
	import { PANEL_GRID } from '$lib/components/kit/layout-styles';
	import { sourceMetricsItems, sourceScheduleItems } from '$lib/config/source-presentation';
	import { t } from '$lib/i18n';

	interface Props {
		source: SourceDetail;
	}

	let { source }: Props = $props();
	const scheduleItems = $derived(sourceScheduleItems(source));
</script>

<div class={PANEL_GRID}>
	<Panel title={t('sources.schedule')}>
		<KeyValueList items={scheduleItems} />
	</Panel>
	<Panel title={t('sources.metrics')}>
		<KeyValueList items={sourceMetricsItems(source)} layout="grid" columns={2} />
	</Panel>
</div>
