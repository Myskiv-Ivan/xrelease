import { describe, expect, it } from 'vitest';
import { latestRelease, latestReleaseSortKey, sourceHealth } from './source-presentation';

describe('sourceHealth', () => {
	const now = new Date('2026-07-19T12:00:00.000Z');

	it('flags poll errors', () => {
		expect(
			sourceHealth(
				{
					poll_errors: 2,
					last_polled_at: '2026-07-19T11:59:00.000Z',
					interval_secs: 3600,
					jitter_secs: 60,
					initialized: true
				},
				now
			)
		).toBe('error');
	});

	it('warns when never polled and not initialized', () => {
		expect(
			sourceHealth(
				{
					poll_errors: 0,
					last_polled_at: null,
					interval_secs: 3600,
					jitter_secs: 60,
					initialized: false
				},
				now
			)
		).toBe('warning');
	});

	it('warns when last poll is stale', () => {
		expect(
			sourceHealth(
				{
					poll_errors: 0,
					last_polled_at: '2026-07-19T08:00:00.000Z',
					interval_secs: 3600,
					jitter_secs: 0,
					initialized: true
				},
				now
			)
		).toBe('warning');
	});

	it('returns ok for a fresh poll', () => {
		expect(
			sourceHealth(
				{
					poll_errors: 0,
					last_polled_at: '2026-07-19T11:30:00.000Z',
					interval_secs: 3600,
					jitter_secs: 60,
					initialized: true
				},
				now
			)
		).toBe('ok');
	});
});

describe('latestRelease', () => {
	it('reports the date of the tag shown in the latest-release column', () => {
		// `seen_releases` is version-ordered: 1.9.1 is a backport published later.
		const result = latestRelease({
			latest_release_tag: 'v2.0.0',
			seen_releases: [
				{
					tag: 'v2.0.0',
					published_at: '2026-07-01T10:00:00.000Z',
					first_seen_at: '2026-07-01T11:00:00.000Z'
				},
				{
					tag: 'v1.9.1',
					published_at: '2026-07-15T10:00:00.000Z',
					first_seen_at: '2026-07-15T11:00:00.000Z'
				}
			]
		});
		expect(result).toEqual({
			tag: 'v2.0.0',
			at: '2026-07-01T10:00:00.000Z',
			published: true
		});
	});

	it('falls back to first_seen_at when upstream exposes no publish date', () => {
		const result = latestRelease({
			latest_release_tag: 'v1.0.0',
			seen_releases: [{ tag: 'v1.0.0', first_seen_at: '2026-07-02T09:00:00.000Z' }]
		});
		expect(result).toMatchObject({ at: '2026-07-02T09:00:00.000Z', published: false });
	});

	it('falls back to the newest entry when no tag matches', () => {
		const result = latestRelease({
			latest_release_tag: 'v3.0.0',
			seen_releases: [
				{ tag: 'v2.0.0', published_at: '2026-07-01T10:00:00.000Z', first_seen_at: 'x' }
			]
		});
		expect(result).toMatchObject({ tag: 'v3.0.0', at: '2026-07-01T10:00:00.000Z' });
	});

	it('handles a source that has seen nothing', () => {
		expect(latestRelease({ latest_release_tag: null, seen_releases: [] })).toEqual({
			tag: null,
			at: null,
			published: false
		});
	});

	it('sorts undated sources last in ascending order', () => {
		const dated = { latest_release_tag: 'v1', seen_releases: [{ tag: 'v1', first_seen_at: 'z' }] };
		const undated = { latest_release_tag: null, seen_releases: [] };
		expect(latestReleaseSortKey(undated)).toBe('');
		expect(latestReleaseSortKey(dated) > latestReleaseSortKey(undated)).toBe(true);
	});
});
