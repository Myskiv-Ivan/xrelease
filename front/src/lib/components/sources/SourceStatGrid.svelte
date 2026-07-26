<script lang="ts">
	import type { SourceDetail } from '$lib/api/types';
	import StatCard from '$lib/components/dashboard/StatCard.svelte';
	import StatGrid from '$lib/components/kit/StatGrid.svelte';
	import { pollErrorTone } from '$lib/config/source-presentation';
	import { t } from '$lib/i18n';
	import { formatNumber, formatDateTime, EMPTY_VALUE } from '$lib/core/format';

	interface Props {
		source: SourceDetail;
	}

	let { source }: Props = $props();
	const lastPolledLabel = $derived(formatDateTime(source.last_polled_at));
</script>

<StatGrid columns={5}>
	<StatCard label={t('sources.kind')} value={source.kind_label} />
	<StatCard
		label={t('sources.latestRelease')}
		value={source.latest_release_tag ?? EMPTY_VALUE}
		mono={Boolean(source.latest_release_tag)}
		tone={source.initialized ? 'success' : 'default'}
	/>
	<StatCard label={t('sources.seenReleases')} value={formatNumber(source.seen_count)} />
	<StatCard label={t('sources.lastPolled')} value={lastPolledLabel} />
	<StatCard
		label={t('sources.errors')}
		value={formatNumber(source.poll_errors)}
		tone={pollErrorTone(source.poll_errors)}
	/>
</StatGrid>
