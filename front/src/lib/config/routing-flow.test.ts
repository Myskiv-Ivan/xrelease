import { describe, expect, it, beforeEach } from 'vitest';
import {
	parseDesiredDocument,
	readSources,
	readTeams,
	resetDraftKeySeq
} from './desired-document';
import { buildRoutingGraph } from './desired-validation';
import { buildRoutingFlow, traceConnections, NODE_WIDTH, preferredRoutingViewportHeight } from './routing-flow';

const SAMPLE = `notifiers:
  - type: apprise
    endpoint: http://apprise:8000
    urls: ["mailto://ops@example.com"]
    tags: [platform]
  - type: smtp
    name: security mail
    host: smtp.example.com
    from: bot@example.com
    to: [sec@example.com]
    tags: [security]
  - type: webhook
    name: firehose
    url: https://hooks.example/all

teams:
  - tag: platform
    name: Platform
  - tag: security
    name: Security

sources:
  - id: tokio
    type: github
    repo: tokio-rs/tokio
    routing_tag: platform
  - id: openssl
    type: github
    repo: openssl/openssl
    routing_tag: security
  - id: nginx
    type: docker
    image: library/nginx
`;

function flowFrom(doc: string) {
	const parsed = parseDesiredDocument(doc);
	const graph = buildRoutingGraph({
		sources: readSources(parsed.data),
		teams: readTeams(parsed.data),
		data: parsed.data
	});
	return buildRoutingFlow(graph);
}

describe('buildRoutingFlow', () => {
	beforeEach(() => resetDraftKeySeq());

	it('lays out sources, teams and channels in three columns', () => {
		const flow = flowFrom(SAMPLE);
		const byKind = (kind: string) => flow.nodes.filter((node) => node.kind === kind);

		expect(byKind('source')).toHaveLength(3);
		expect(byKind('team')).toHaveLength(2);
		expect(byKind('channel')).toHaveLength(3);
		expect(byKind('channel').some((node) => node.badge === 'Apprise')).toBe(true);

		const columns = [0, 1, 2].map(
			(column) => new Set(flow.nodes.filter((n) => n.column === column).map((n) => n.x))
		);
		// One x per column, and columns advance left to right.
		expect(columns.every((xs) => xs.size === 1)).toBe(true);
		const [x0, x1, x2] = columns.map((xs) => [...xs][0]);
		expect(x1).toBeGreaterThan(x0 + NODE_WIDTH);
		expect(x2).toBeGreaterThan(x1 + NODE_WIDTH);
	});

	it('routes tagged sources through their team', () => {
		const flow = flowFrom(SAMPLE);
		const tokio = flow.nodes.find((node) => node.label === 'tokio');
		expect(tokio).toBeDefined();
		expect(flow.neighbors[tokio!.id]).toEqual(['team:platform']);
	});

	it('wires untagged sources straight to wildcard channels', () => {
		const flow = flowFrom(SAMPLE);
		const nginx = flow.nodes.find((node) => node.label === 'nginx')!;
		const targets = flow.neighbors[nginx.id] ?? [];

		expect(targets).toHaveLength(1);
		const channel = flow.nodes.find((node) => node.id === targets[0])!;
		expect(channel.kind).toBe('channel');
		expect(channel.label).toBe('firehose');
		expect(flow.edges.find((e) => e.from === nginx.id)?.wildcard).toBe(true);
	});

	it('flags a source that delivers nowhere', () => {
		const flow = flowFrom(`teams: []
notifiers:
  - type: webhook
    url: https://hooks.example/x
    tags: [platform]
sources:
  - id: orphan
    type: github
    repo: a/b
`);
		const orphan = flow.nodes.find((node) => node.label === 'orphan')!;
		expect(orphan.warning).toBe('no-target');
	});

	it('flags a team whose tag no channel carries', () => {
		const flow = flowFrom(`notifiers:
  - type: webhook
    url: https://hooks.example/x
    tags: [other]
teams:
  - tag: platform
sources:
  - id: a
    type: github
    repo: a/b
    routing_tag: platform
`);
		const team = flow.nodes.find((node) => node.kind === 'team')!;
		expect(team.warning).toBe('no-channel');
	});

	it('flags a channel no source reaches', () => {
		const flow = flowFrom(`notifiers:
  - type: webhook
    url: https://hooks.example/x
    tags: [nobody]
teams: []
sources: []
`);
		const channel = flow.nodes.find((node) => node.kind === 'channel')!;
		expect(channel.warning).toBe('unused');
	});

	it('produces bezier paths between node edges', () => {
		const flow = flowFrom(SAMPLE);
		expect(flow.edges.length).toBeGreaterThan(0);
		for (const edge of flow.edges) {
			expect(edge.path).toMatch(/^M [\d.]+ [\d.]+ C /);
		}
	});

	it('returns an empty canvas for an empty document', () => {
		const flow = flowFrom('sources: []\nteams: []\n');
		expect(flow.nodes).toHaveLength(0);
		expect(flow.edges).toHaveLength(0);
	});

	it('emits unique node and edge ids', () => {
		const flow = flowFrom(SAMPLE);
		expect(new Set(flow.nodes.map((n) => n.id)).size).toBe(flow.nodes.length);
		expect(new Set(flow.edges.map((e) => e.id)).size).toBe(flow.edges.length);
	});

	it('collapses duplicate team tags instead of emitting colliding keys', () => {
		// The editor lets a half-typed catalogue hold the same tag twice; duplicate
		// node ids would throw `each_key_duplicate` and blank the page.
		const flow = flowFrom(`notifiers:
  - type: webhook
    url: https://hooks.example/x
    tags: [platform]
teams:
  - tag: platform
    name: Platform
  - tag: platform
    name: Platform copy
sources:
  - id: a
    type: github
    repo: a/b
    routing_tag: platform
`);
		expect(flow.nodes.filter((node) => node.kind === 'team')).toHaveLength(1);
		expect(new Set(flow.nodes.map((n) => n.id)).size).toBe(flow.nodes.length);
		expect(new Set(flow.edges.map((e) => e.id)).size).toBe(flow.edges.length);
	});

	it('is deterministic', () => {
		resetDraftKeySeq();
		const first = flowFrom(SAMPLE);
		resetDraftKeySeq();
		const second = flowFrom(SAMPLE);
		expect(second.nodes.map((n) => [n.id, n.x, n.y])).toEqual(
			first.nodes.map((n) => [n.id, n.x, n.y])
		);
	});
});

describe('traceConnections', () => {
	beforeEach(() => resetDraftKeySeq());

	it('traces a source forward to its channels', () => {
		const flow = flowFrom(SAMPLE);
		const tokio = flow.nodes.find((node) => node.label === 'tokio')!;
		const { nodes } = traceConnections(flow, tokio.id);

		expect(nodes.has('team:platform')).toBe(true);
		// platform is carried by the apprise sink, and the wildcard webhook takes all.
		expect([...nodes].filter((id) => id.startsWith('channel:'))).toHaveLength(2);
		expect(nodes.has('team:security')).toBe(false);
	});

	it('traces a channel backward to every source feeding it', () => {
		const flow = flowFrom(SAMPLE);
		const mail = flow.nodes.find((node) => node.label === 'security mail')!;
		const { nodes } = traceConnections(flow, mail.id);

		const sources = flow.nodes.filter((n) => n.kind === 'source' && nodes.has(n.id));
		expect(sources.map((n) => n.label)).toEqual(['openssl']);
	});

	it('pulls every source into a wildcard channel trace', () => {
		const flow = flowFrom(SAMPLE);
		const firehose = flow.nodes.find((node) => node.label === 'firehose')!;
		const { nodes } = traceConnections(flow, firehose.id);

		const sources = flow.nodes.filter((n) => n.kind === 'source' && nodes.has(n.id));
		expect(sources.map((n) => n.label).sort()).toEqual(['nginx', 'openssl', 'tokio']);
	});

	it('includes the node itself and terminates on cycles-free graphs', () => {
		const flow = flowFrom(SAMPLE);
		const team = flow.nodes.find((node) => node.id === 'team:platform')!;
		const { nodes, edges } = traceConnections(flow, team.id);
		expect(nodes.has(team.id)).toBe(true);
		expect(edges.size).toBeGreaterThan(0);
	});
});

describe('preferredRoutingViewportHeight', () => {
	it('grows with tall graphs instead of clamping to a short viewport', () => {
		// 20 sources ≈ 20*56 + 19*12 + 48 content
		const tall = 20 * 56 + 19 * 12 + 48;
		const height = preferredRoutingViewportHeight(tall, { min: 400, max: 2000 });
		expect(height).toBeGreaterThanOrEqual(tall);
		expect(height).toBeLessThanOrEqual(2000);
	});

	it('respects min when the graph is small', () => {
		expect(preferredRoutingViewportHeight(100, { min: 500, max: 900 })).toBe(500);
	});
});
