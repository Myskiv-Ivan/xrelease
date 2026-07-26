import type { SourceDetail } from '$lib/api/types';
import type { KeyValueItem } from '$lib/types/ui';
import type { SurfaceTone } from '$lib/components/kit/surface-styles';
import { t } from '$lib/i18n';
import { EMPTY_VALUE, formatInterval, formatNumber, formatRelative } from '$lib/core/format';

export function sourceScheduleItems(source: SourceDetail, now: Date = new Date()): KeyValueItem[] {
	return [
		{ label: t('sources.latestRelease'), value: source.latest_release_tag ?? EMPTY_VALUE },
		{ label: t('sources.interval'), value: formatInterval(source.interval_secs) },
		{ label: t('sources.jitter'), value: formatInterval(source.jitter_secs) },
		{ label: t('sources.lastPolled'), value: formatRelative(source.last_polled_at, now) },
		{ label: t('sources.routingTag'), value: source.routing_tag ?? EMPTY_VALUE },
		{
			label: t('sources.notifySchedule'),
			value: source.notify_schedule ?? t('sources.notifyImmediately')
		}
	];
}

export function sourceMetricsItems(source: SourceDetail): KeyValueItem[] {
	return [
		{ label: t('overview.polls'), value: formatNumber(source.polls) },
		{ label: t('overview.notModified'), value: formatNumber(source.polls_not_modified) },
		{
			label: t('overview.pollErrors'),
			value: formatNumber(source.poll_errors),
			tone: source.poll_errors > 0 ? 'danger' : 'default'
		},
		{ label: t('overview.notifications'), value: formatNumber(source.notifications) },
		{ label: t('sources.providerKind'), value: source.kind }
	];
}

export function pollErrorTone(errors: number): SurfaceTone {
	return errors > 0 ? 'danger' : 'default';
}

/** Sortable columns: `latest_release_at` is derived, not a `SourceDetail` field. */
export type SourceSortKey = keyof SourceDetail | 'latest_release_at';

export interface LatestRelease {
	tag: string | null;
	/** RFC 3339 timestamp, or null when nothing has been seen yet. */
	at: string | null;
	/** False when `at` is our sync time because upstream published no date. */
	published: boolean;
}

/**
 * Date of the release shown in the “latest release” column.
 *
 * `seen_releases` is ordered by version, not by date, so the newest entry is not
 * necessarily the newest timestamp (a backported patch inverts it). Anchoring on
 * `latest_release_tag` keeps the tag and the date describing the same release.
 */
export function latestRelease(
	source: Pick<SourceDetail, 'latest_release_tag' | 'seen_releases'>
): LatestRelease {
	const releases = source.seen_releases ?? [];
	const tag = source.latest_release_tag ?? null;
	const match = (tag ? releases.find((entry) => entry.tag === tag) : undefined) ?? releases[0];

	if (!match) return { tag, at: null, published: false };
	return {
		tag: tag ?? match.tag,
		at: match.published_at ?? match.first_seen_at ?? null,
		published: Boolean(match.published_at)
	};
}

/** Sort key for the last-release column; ISO 8601 compares chronologically. */
export function latestReleaseSortKey(
	source: Pick<SourceDetail, 'latest_release_tag' | 'seen_releases'>
): string {
	return latestRelease(source).at ?? '';
}

export type SourceHealth = 'ok' | 'warning' | 'error';

/**
 * Compact row health for the sources table.
 * Errors win; stale polls (2× interval+jitter) and never-polled sources warn.
 */
export function sourceHealth(
	source: Pick<
		SourceDetail,
		'poll_errors' | 'last_polled_at' | 'interval_secs' | 'jitter_secs' | 'initialized'
	>,
	now: Date = new Date()
): SourceHealth {
	if (source.poll_errors > 0) return 'error';
	if (!source.last_polled_at) return source.initialized ? 'ok' : 'warning';

	const last = new Date(source.last_polled_at);
	if (Number.isNaN(last.getTime())) return 'warning';

	const maxAgeMs = (source.interval_secs + source.jitter_secs) * 2 * 1000;
	if (now.getTime() - last.getTime() > maxAgeMs) return 'warning';
	return 'ok';
}
