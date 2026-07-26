<script lang="ts">
	import type { SourceDetail } from '$lib/api/types';
	import SourceDetailActions from '$lib/components/sources/SourceDetailActions.svelte';
	import SourceDetailPanels from '$lib/components/sources/SourceDetailPanels.svelte';
	import SeenReleasesPanel from '$lib/components/sources/SeenReleasesPanel.svelte';
	import SourceKindBadge from '$lib/components/sources/SourceKindBadge.svelte';
	import SourceStatGrid from '$lib/components/sources/SourceStatGrid.svelte';
	import { t } from '$lib/i18n';

	interface Props {
		source: SourceDetail;
		isPolling?: boolean;
		onPoll: () => void | Promise<void>;
	}

	let { source, isPolling = false, onPoll }: Props = $props();
</script>

<SourceDetailActions {isPolling} {onPoll} />

<div class="mb-4 flex flex-wrap items-center gap-3">
	<SourceKindBadge kind={source.kind} label={source.kind_label} />
	{#if source.source_page_url}
		<a
			href={source.source_page_url}
			target="_blank"
			rel="noopener noreferrer"
			class="text-sm text-primary hover:underline"
		>
			{t('sources.sourceCatalog')} ↗
		</a>
	{/if}
</div>

<SourceStatGrid {source} />

<div class="mt-4">
	<SeenReleasesPanel releases={source.seen_releases} seenCount={source.seen_count} />
</div>

<div class="mt-4">
	<SourceDetailPanels {source} />
</div>
