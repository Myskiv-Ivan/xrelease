import { describe, expect, it, beforeEach } from 'vitest';
import {
	emptySourceDraft,
	emptyTeamDraft,
	parseDesiredDocument,
	readDefaults,
	readNotifiers,
	readSources,
	readTeams,
	resetDraftKeySeq,
	writeNotifiers
} from './desired-document';
import {
	buildRoutingGraph,
	listNotifierChannels,
	validateDesiredDrafts
} from './desired-validation';

const SAMPLE = `defaults:
  interval_secs: 86400


notifiers:
  - type: apprise
    endpoint: http://apprise:8000
    urls: ["mailto://user:pass@example.com"]
    tags: [platform-team]
  - type: webhook
    url: https://hooks.example/sec
    tags: [security-team]

teams:
  - tag: platform-team
    name: Platform
  - tag: security-team
    name: Security

sources:
  - id: tokio
    type: github
    repo: tokio-rs/tokio
    routing_tag: platform-team
  - type: pypi
    name: requests
    routing_tag: security-team
  - type: docker
    image: library/nginx
`;

describe('desired-validation', () => {
	beforeEach(() => resetDraftKeySeq());

	it('lists notifier channels including apprise', () => {
		const parsed = parseDesiredDocument(SAMPLE);
		const channels = listNotifierChannels(parsed.data);
		expect(channels).toHaveLength(2);
		expect(channels[0]).toMatchObject({ type: 'apprise', wildcard: false, label: 'Apprise' });
		expect(channels[1]).toMatchObject({ type: 'webhook', tags: ['security-team'] });
	});

	it('builds source → team → channel routing edges', () => {
		const parsed = parseDesiredDocument(SAMPLE);
		const graph = buildRoutingGraph({
			sources: readSources(parsed.data),
			teams: readTeams(parsed.data),
			data: parsed.data
		});
		expect(graph.edges).toHaveLength(3);
		const tokio = graph.edges.find((e) => e.sourceLabel === 'tokio');
		expect(tokio?.teamTag).toBe('platform-team');
		expect(tokio?.channels.map((c) => c.type)).toContain('apprise');
		const untagged = graph.edges.find((e) => e.sourceLabel === 'library/nginx');
		expect(untagged?.teamTag).toBeNull();
		expect(untagged?.channels).toHaveLength(0);
		expect(graph.untaggedSources).toBe(1);
	});

	it('flags invalid repo and regex as errors', () => {
		const parsed = parseDesiredDocument(SAMPLE);
		const sources = readSources(parsed.data);
		sources[0]!.fields.repo = 'not-a-repo';
		sources[0]!.pattern = '(unclosed';
		const result = validateDesiredDrafts({
			defaults: readDefaults(parsed.data),
			teams: readTeams(parsed.data),
			sources,
			data: parsed.data
		});
		expect(result.ok).toBe(false);
		expect(result.errors.some((e) => e.path.includes('fields.repo'))).toBe(true);
		expect(result.errors.some((e) => e.path.includes('pattern'))).toBe(true);
	});

	it('errors when routing tag has no matching channel (matches server Apply)', () => {
		const parsed = parseDesiredDocument(SAMPLE);
		const teams = [...readTeams(parsed.data), { ...emptyTeamDraft(), tag: 'orphan-team' }];
		const sources = [
			...readSources(parsed.data),
			{ ...emptySourceDraft('npm'), fields: { name: 'left-pad' }, routing_tag: 'orphan-team' }
		];
		const result = validateDesiredDrafts({
			defaults: readDefaults(parsed.data),
			teams,
			sources,
			data: parsed.data,
			notifiers: readNotifiers(parsed.data)
		});
		expect(result.ok).toBe(false);
		expect(
			result.errors.some(
				(e) => e.path.includes('routing_tag') && e.message.includes('orphan-team')
			)
		).toBe(true);
	});

	it('keeps Apply valid when Express is tagged but Apprise urls are only redacted', () => {
		const doc = `notifiers:
  - type: apprise
    endpoint: http://apprise:8000
    urls: ["<redacted>"]
  - type: express
    base_url: https://cts.example.com
    group_chat_id: g-1
    access_token: <redacted>
    tags: [security-team]

teams:
  - tag: platform-team
  - tag: security-team

sources:
  - id: platform-app
    type: github
    repo: org/platform
    routing_tag: platform-team
  - id: sec-app
    type: github
    repo: org/sec
    routing_tag: security-team
`;
		const parsed = parseDesiredDocument(doc);
		const notifiers = readNotifiers(parsed.data);
		// Simulate UI write that drops redacted urls from the document body.
		const data = writeNotifiers(parsed.data, notifiers);
		expect(data.apprise).toBeUndefined();
		expect((data.notifiers as { urls?: unknown }[]).find((n) => (n as {type?:string}).type === 'apprise')?.urls).toBeUndefined();

		const result = validateDesiredDrafts({
			defaults: readDefaults(parsed.data),
			teams: readTeams(parsed.data),
			sources: readSources(parsed.data),
			notifiers,
			data
		});
		expect(result.ok).toBe(true);
		const apprise = listNotifierChannels(data, notifiers).find((c) => c.type === 'apprise');
		expect(apprise?.listens).toBe(true);
	});

	it('shows Apprise in the graph but blocks Apply when urls are gone (no redacted restore)', () => {
		// Empty urls without redacted flag: Apprise must appear (listens:false) so the
		// graph is honest, and validation must fail like the server (Express-only tags).
		const doc = `notifiers:
  - type: apprise
    endpoint: http://apprise:8000
    tags: [platform-team]
  - type: express
    base_url: https://cts.example.com
    group_chat_id: g-1
    access_token: token
    tags: [security-team]

teams:
  - tag: platform-team
  - tag: security-team

sources:
  - id: platform-app
    type: github
    repo: org/platform
    routing_tag: platform-team
  - id: sec-app
    type: github
    repo: org/sec
    routing_tag: security-team
`;
		const parsed = parseDesiredDocument(doc);
		const notifiers = readNotifiers(parsed.data);
		const channels = listNotifierChannels(parsed.data, notifiers);
		const apprise = channels.find((c) => c.type === 'apprise');
		expect(apprise).toBeTruthy();
		expect(apprise?.listens).toBe(false);

		const result = validateDesiredDrafts({
			defaults: readDefaults(parsed.data),
			teams: readTeams(parsed.data),
			sources: readSources(parsed.data),
			notifiers,
			data: parsed.data
		});
		expect(result.ok).toBe(false);
		expect(result.errors.some((e) => e.message.includes('platform-team'))).toBe(true);
		expect(result.errors.some((e) => e.message.includes('security-team'))).toBe(false);
	});

	it('live draft graph updates Express tags for both teams', () => {
		const doc = `notifiers:
  - type: apprise
    endpoint: http://apprise:8000
    urls: ["mailto://ops@example.com"]
  - type: express
    base_url: https://cts.example.com
    group_chat_id: g-1
    access_token: token
    tags: [platform-team]

teams:
  - tag: platform-team
  - tag: security-team

sources:
  - id: a
    type: github
    repo: org/a
    routing_tag: platform-team
  - id: b
    type: github
    repo: org/b
    routing_tag: security-team
`;
		const parsed = parseDesiredDocument(doc);
		const notifiers = readNotifiers(parsed.data);
		const express = notifiers.find((n) => n.type === 'express')!;
		express.tags = 'platform-team, security-team';
		const graph = buildRoutingGraph({
			sources: readSources(parsed.data),
			teams: readTeams(parsed.data),
			notifiers,
			data: writeNotifiers(parsed.data, notifiers)
		});
		expect(graph.edges.every((e) => !e.orphaned)).toBe(true);
		expect(
			listNotifierChannels(parsed.data, notifiers).find((c) => c.type === 'express')?.tags
		).toEqual(['platform-team', 'security-team']);
	});

	it('requires bitbucket server host', () => {
		const parsed = parseDesiredDocument(SAMPLE);
		const source = emptySourceDraft('bitbucket');
		source.fields = { repo: 'ws/repo', host: '', edition: 'server' };
		const result = validateDesiredDrafts({
			defaults: readDefaults(parsed.data),
			teams: readTeams(parsed.data),
			sources: [source],
			data: parsed.data
		});
		expect(result.errors.some((e) => e.path.endsWith('fields.host'))).toBe(true);
	});

	it('errors when ops_routing_tag has no matching channel', () => {
		const parsed = parseDesiredDocument(SAMPLE);
		const defaults = { ...readDefaults(parsed.data), ops_routing_tag: 'ops' };
		const result = validateDesiredDrafts({
			defaults,
			teams: readTeams(parsed.data),
			sources: readSources(parsed.data),
			data: parsed.data,
			notifiers: readNotifiers(parsed.data)
		});
		expect(result.ok).toBe(false);
		expect(
			result.errors.some(
				(e) => e.path === 'defaults.ops_routing_tag' && e.message.includes('ops')
			)
		).toBe(true);
	});

	it('warns when ops_routing_tag is missing from teams', () => {
		const parsed = parseDesiredDocument(SAMPLE);
		const notifiers = readNotifiers(parsed.data);
		const apprise = notifiers.find((n) => n.type === 'apprise')!;
		apprise.tags = 'platform-team, ops';
		const defaults = { ...readDefaults(parsed.data), ops_routing_tag: 'ops' };
		const result = validateDesiredDrafts({
			defaults,
			teams: readTeams(parsed.data),
			sources: readSources(parsed.data),
			data: writeNotifiers(parsed.data, notifiers),
			notifiers
		});
		expect(result.ok).toBe(true);
		expect(
			result.warnings.some(
				(w) => w.path === 'defaults.ops_routing_tag' && w.message.includes('Teams')
			)
		).toBe(true);
	});
});
