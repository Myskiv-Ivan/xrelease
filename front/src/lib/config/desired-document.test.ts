import { describe, expect, it, beforeEach } from 'vitest';
import {
	contentTypeForFormat,
	detectDesiredFormat,
	emptySourceDraft,
	parseDesiredDocument,
	primaryFieldsForKind,
	readDefaults,
	readSources,
	readTeams,
	resetDraftKeySeq,
	stringifyDesiredDocument,
	writeDefaults,
	writeSources,
	writeTeams
} from './desired-document';

const SAMPLE_YAML = `defaults:
  interval_secs: 86400
  jitter_secs: 3600
  poll_on_startup: true


notifiers:
  - type: apprise
    endpoint: http://apprise:8000
    urls: []
  - type: webhook
    url: https://hooks.example/x

teams:
  - tag: platform-team
    name: Platform

sources:
  - id: tokio
    type: github
    repo: tokio-rs/tokio
    pattern: '^tokio-\\\\d'
    routing_tag: platform-team
  - type: pypi
    name: requests
    routing_tag: platform-team
`;

describe('desired-document', () => {
	beforeEach(() => {
		resetDraftKeySeq();
	});

	it('detects yaml vs toml', () => {
		expect(detectDesiredFormat(SAMPLE_YAML)).toBe('yaml');
		expect(detectDesiredFormat('[defaults]\ninterval_secs = 1\n')).toBe('toml');
		expect(detectDesiredFormat('# leading comment\n[defaults]\ninterval_secs = 1\n')).toBe(
			'toml'
		);
		expect(contentTypeForFormat('yaml')).toBe('application/yaml');
		expect(contentTypeForFormat('toml')).toBe('application/toml');
	});

	it('parses comment-prefixed toml desired documents', () => {
		const raw = `# Application config
[defaults]
interval_secs = 42

[[sources]]
type = "npm"
name = "left-pad"
`;
		const parsed = parseDesiredDocument(raw);
		expect(parsed.format).toBe('toml');
		expect(readDefaults(parsed.data).interval_secs).toBe(42);
		expect(readSources(parsed.data)[0]?.fields.name).toBe('left-pad');
	});

	it('preserves source tokens across round-trip', () => {
		const raw = `sources:
  - type: github
    repo: org/private
    token: ghp_secret
    routing_tag: ops
`;
		const parsed = parseDesiredDocument(raw);
		const sources = readSources(parsed.data);
		expect(sources[0]?.token).toBe('ghp_secret');
		const out = stringifyDesiredDocument({
			format: 'yaml',
			data: writeSources(parsed.data, sources)
		});
		const again = parseDesiredDocument(out);
		expect(readSources(again.data)[0]?.token).toBe('ghp_secret');
	});

	it('drops redacted source tokens but keeps token_env', () => {
		const raw = `sources:
  - type: github
    repo: org/private
    token: <redacted>
    token_env: XRELEASE_UI_SRC_0_TOKEN
`;
		const parsed = parseDesiredDocument(raw);
		const sources = readSources(parsed.data);
		expect(sources[0]?.token).toBe('');
		expect(sources[0]?.redacted).toContain('token');
		expect(sources[0]?.token_env).toBe('XRELEASE_UI_SRC_0_TOKEN');
		const written = writeSources(parsed.data, sources).sources as Record<string, unknown>[];
		expect(written[0]?.token).toBeUndefined();
		expect(written[0]?.token_env).toBe('XRELEASE_UI_SRC_0_TOKEN');
		expect(JSON.stringify(written)).not.toContain('<redacted>');
	});

	it('skips non-finite defaults instead of writing NaN', () => {
		const parsed = parseDesiredDocument('defaults:\n  interval_secs: 10\n');
		const next = writeDefaults(parsed.data, {
			interval_secs: Number.NaN as unknown as number,
			jitter_secs: 1,
			upstream_requests_per_minute: 0,
			poll_on_startup: true,
			notify_schedule: '',
			ops_routing_tag: ''
		});
		expect((next.defaults as Record<string, unknown>).interval_secs).toBe(10);
		expect((next.defaults as Record<string, unknown>).jitter_secs).toBe(1);
	});

	it('round-trips yaml while preserving unknown top-level keys', () => {
		const parsed = parseDesiredDocument(SAMPLE_YAML);
		expect(parsed.format).toBe('yaml');
		expect(Array.isArray(parsed.data.notifiers)).toBe(true);
		expect(parsed.data.apprise).toBeUndefined();

		const defaults = readDefaults(parsed.data);
		expect(defaults.interval_secs).toBe(86400);

		const teams = readTeams(parsed.data);
		expect(teams).toHaveLength(1);
		expect(teams[0]).toMatchObject({ tag: 'platform-team', name: 'Platform' });
		expect(teams[0]?.key).toBeTruthy();

		const sources = readSources(parsed.data);
		expect(sources).toHaveLength(2);
		expect(sources[0]?.fields.repo).toBe('tokio-rs/tokio');
		expect(sources[1]?.fields.name).toBe('requests');

		const nextDefaults = writeDefaults(parsed.data, {
			...defaults,
			interval_secs: 3600
		});
		const nextTeams = writeTeams(nextDefaults, [
			...teams,
			{ key: 't-extra', tag: 'security-team', name: 'SecOps' }
		]);
		const nextSources = writeSources(nextTeams, [
			...sources,
			{ ...emptySourceDraft('docker'), fields: { image: 'library/nginx', registry: '' } }
		]);

		const out = stringifyDesiredDocument({ format: 'yaml', data: nextSources });
		const again = parseDesiredDocument(out);
		expect(again.data.defaults).toMatchObject({ interval_secs: 3600 });
		expect(readTeams(again.data)).toHaveLength(2);
		expect(readSources(again.data)).toHaveLength(3);
		expect(again.data.notifiers).toEqual(parsed.data.notifiers);
		expect(again.data.apprise).toEqual(parsed.data.apprise);
	});

	it('round-trips toml desired documents', () => {
		const toml = `
[defaults]
interval_secs = 100

[[teams]]
tag = "ops"
name = "Ops"

[[sources]]
type = "cargo"
name = "serde"
routing_tag = "ops"
`;
		const parsed = parseDesiredDocument(toml, 'toml');
		expect(parsed.format).toBe('toml');
		const sources = readSources(parsed.data);
		expect(sources[0]?.type).toBe('cargo');
		const out = stringifyDesiredDocument({
			format: 'toml',
			data: writeSources(parsed.data, sources)
		});
		expect(out).toContain('interval_secs');
		expect(out).toContain('serde');
	});

	it('lists primary fields per kind', () => {
		expect(primaryFieldsForKind('github').map((f) => f.key)).toEqual(['repo']);
		expect(primaryFieldsForKind('gitea').map((f) => f.key)).toEqual(['host', 'repo']);
		expect(primaryFieldsForKind('pypi').map((f) => f.key)).toEqual(['name']);
	});

	it('round-trips source preset with filter fields', () => {
		const parsed = parseDesiredDocument(SAMPLE_YAML);
		const [source] = readSources(parsed.data);
		source.preset = 'semver';
		source.pattern = String.raw`^v?\d+\.\d+\.\d+$`;
		const out = writeSources(parsed.data, [source, ...readSources(parsed.data).slice(1)]);
		const again = readSources(out);
		expect(again[0]?.preset).toBe('semver');
		expect(again[0]?.pattern).toBe(String.raw`^v?\d+\.\d+\.\d+$`);
	});
});
