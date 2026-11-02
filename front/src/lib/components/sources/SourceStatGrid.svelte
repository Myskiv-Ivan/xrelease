<script lang="ts">
	import type { SourceDetail } from '#lib/api/types.js';
	import StatCard from '#lib/components/kit/StatCard.svelte';
	import StatGrid from '#lib/components/kit/StatGrid.svelte';
	import { pollErrorTone } from '#lib/config/source-presentation.js';
	import { t } from '#lib/i18n/index.js';
	import { formatNumber, formatDateTimeFull, EMPTY_VALUE } from '#lib/core/format.js';

	interface Props {
		source: SourceDetail;
	}

	let { source }: Props = $props();
	const lastPolledLabel = $derived(formatDateTimeFull(source.last_polled_at));
</script>

<StatGrid columns={5} layout="lead">
	<StatCard
		lead
		label={t('sources.latestRelease')}
		value={source.latest_release_tag ?? EMPTY_VALUE}
		mono={Boolean(source.latest_release_tag)}
		tone={source.initialized ? 'success' : 'default'}
	/>
	<StatCard label={t('sources.kind')} value={source.kind_label} />
	<StatCard label={t('sources.seenReleases')} value={formatNumber(source.seen_count)} />
	<StatCard label={t('sources.lastPolled')} value={lastPolledLabel} />
	<StatCard
		label={t('sources.errors')}
		value={formatNumber(source.poll_errors)}
		tone={pollErrorTone(source.consecutive_errors)}
	/>
</StatGrid>
