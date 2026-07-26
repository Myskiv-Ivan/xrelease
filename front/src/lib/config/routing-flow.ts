/**
 * Layered layout for the routing canvas: Sources → Teams → Channels.
 *
 * Pure geometry, so the canvas component stays presentational and the layout
 * stays unit-testable. Coordinates are in unscaled graph space; the viewer
 * applies pan/zoom on top.
 */

import type { RoutingGraph } from './desired-validation';
import { getSinkKindLabel } from './sink-kinds';
import { getSourceKindMeta } from './source-kinds';

export type FlowNodeKind = 'source' | 'team' | 'channel';

/** Why a node is flagged; the component maps these to localised copy. */
export type FlowWarning = 'no-target' | 'no-channel' | 'unused' | 'not-listening';

export const NODE_WIDTH = 200;
export const NODE_HEIGHT = 56;
const COLUMN_GAP = 96;
const ROW_GAP = 12;
const PADDING = 24;

/**
 * Viewport height so large graphs (20+ sources) stay near 1:1 scale —
 * grow with content and let the page scroll instead of shrinking nodes.
 */
export function preferredRoutingViewportHeight(
	flowHeight: number,
	opts?: { min?: number; max?: number; chrome?: number }
): number {
	const min = opts?.min ?? 36 * 16; // 36rem
	const chrome = opts?.chrome ?? 48;
	const needed = Math.max(0, flowHeight) + chrome;
	if (opts?.max != null) {
		return Math.round(Math.min(opts.max, Math.max(min, needed)));
	}
	if (typeof window === 'undefined') {
		return Math.round(Math.max(min, needed));
	}
	// Soft cap ≈ one viewport; hard cap allows scroll for tall topologies.
	const softMax = Math.floor(window.innerHeight * 0.92);
	const hardMax = Math.floor(window.innerHeight * 1.85);
	if (needed <= softMax) return Math.round(Math.max(min, needed));
	return Math.round(Math.min(hardMax, Math.max(min, needed)));
}

export interface FlowNode {
	id: string;
	kind: FlowNodeKind;
	/** 0 = sources, 1 = teams, 2 = channels. */
	column: number;
	label: string;
	sublabel: string;
	badge: string;
	x: number;
	y: number;
	warning: FlowWarning | null;
	/** Key of the underlying draft/channel, for selection hand-off to the editor. */
	ref: string;
}

export interface FlowEdge {
	id: string;
	from: string;
	to: string;
	/** SVG cubic-bezier `d` attribute in graph space. */
	path: string;
	/** Wildcard sinks fan out to everything — drawn dashed to damp the noise. */
	wildcard: boolean;
}

export interface RoutingFlow {
	nodes: FlowNode[];
	edges: FlowEdge[];
	width: number;
	height: number;
	/** Node id → directly connected node ids, both directions. */
	neighbors: Record<string, string[]>;
	/** Node id → incident edge ids. */
	incidentEdges: Record<string, string[]>;
}

const sourceId = (key: string) => `source:${key}`;
const teamId = (tag: string) => `team:${tag}`;
const channelId = (key: string) => `channel:${key}`;

function round(value: number): number {
	return Math.round(value * 10) / 10;
}

function bezier(x1: number, y1: number, x2: number, y2: number): string {
	const curve = Math.max(48, (x2 - x1) * 0.45);
	return `M ${round(x1)} ${round(y1)} C ${round(x1 + curve)} ${round(y1)}, ${round(
		x2 - curve
	)} ${round(y2)}, ${round(x2)} ${round(y2)}`;
}

/** Mean index of connected nodes in the previous column; used to damp crossings. */
function barycenter(ids: string[], order: Map<string, number>, fallback: number): number {
	const known = ids.map((id) => order.get(id)).filter((v): v is number => v != null);
	if (known.length === 0) return fallback;
	return known.reduce((sum, v) => sum + v, 0) / known.length;
}

function columnX(column: number): number {
	return PADDING + column * (NODE_WIDTH + COLUMN_GAP);
}

/**
 * Project a routing graph onto a three-column canvas.
 *
 * Untagged sources bypass the team column and wire straight to wildcard sinks,
 * which is what the runtime actually does with them.
 */
export function buildRoutingFlow(graph: RoutingGraph): RoutingFlow {
	const listening = graph.channels.filter((channel) => channel.listens);
	const wildcards = listening.filter((channel) => channel.wildcard);

	// --- Logical edges (positions come later) ---------------------------------
	const links: Array<{ from: string; to: string; wildcard: boolean }> = [];

	for (const source of graph.sources) {
		if (source.routingTag) {
			links.push({ from: sourceId(source.key), to: teamId(source.routingTag), wildcard: false });
		} else {
			for (const channel of wildcards) {
				links.push({ from: sourceId(source.key), to: channelId(channel.key), wildcard: true });
			}
		}
	}

	for (const team of graph.teams) {
		for (const channel of listening) {
			if (!channel.wildcard && !channel.tags.includes(team.tag)) continue;
			links.push({ from: teamId(team.tag), to: channelId(channel.key), wildcard: channel.wildcard });
		}
	}

	const outgoing = new Map<string, string[]>();
	const incoming = new Map<string, string[]>();
	for (const link of links) {
		(outgoing.get(link.from) ?? outgoing.set(link.from, []).get(link.from)!).push(link.to);
		(incoming.get(link.to) ?? incoming.set(link.to, []).get(link.to)!).push(link.from);
	}

	// --- Column ordering (barycentre sweep, sources keep authored order) -------
	const sourceOrder = new Map(graph.sources.map((s, i) => [sourceId(s.key), i] as const));

	const teams = [...graph.teams].sort((a, b) => {
		const ax = barycenter(incoming.get(teamId(a.tag)) ?? [], sourceOrder, Number.MAX_SAFE_INTEGER);
		const bx = barycenter(incoming.get(teamId(b.tag)) ?? [], sourceOrder, Number.MAX_SAFE_INTEGER);
		return ax === bx ? a.tag.localeCompare(b.tag) : ax - bx;
	});

	const teamOrder = new Map(teams.map((t, i) => [teamId(t.tag), i] as const));
	const upstreamOrder = new Map([...sourceOrder, ...teamOrder]);

	const channels = [...graph.channels].sort((a, b) => {
		const ax = barycenter(
			incoming.get(channelId(a.key)) ?? [],
			upstreamOrder,
			Number.MAX_SAFE_INTEGER
		);
		const bx = barycenter(
			incoming.get(channelId(b.key)) ?? [],
			upstreamOrder,
			Number.MAX_SAFE_INTEGER
		);
		return ax === bx ? a.label.localeCompare(b.label) : ax - bx;
	});

	// --- Nodes ----------------------------------------------------------------
	const sourceNodes: FlowNode[] = graph.sources.map((source) => ({
		id: sourceId(source.key),
		kind: 'source' as const,
		column: 0,
		label: source.label,
		sublabel: source.routingTag ?? '',
		badge: getSourceKindMeta(source.type).label,
		x: columnX(0),
		y: 0,
		warning: (outgoing.get(sourceId(source.key)) ?? []).length === 0 ? 'no-target' : null,
		ref: source.key
	}));

	const teamNodes: FlowNode[] = teams.map((team) => ({
		id: teamId(team.tag),
		kind: 'team' as const,
		column: 1,
		label: team.name || team.tag,
		sublabel: team.name ? team.tag : '',
		badge: `${team.sourceCount} src · ${team.channelCount} ch`,
		x: columnX(1),
		y: 0,
		warning: team.channelCount === 0 ? 'no-channel' : null,
		ref: team.tag
	}));

	const channelNodes: FlowNode[] = channels.map((channel) => ({
		id: channelId(channel.key),
		kind: 'channel' as const,
		column: 2,
		label: channel.label,
		sublabel: !channel.listens
			? 'no targets'
			: channel.wildcard
				? ''
				: channel.tags.join(', '),
		badge: getSinkKindLabel(channel.type),
		x: columnX(2),
		y: 0,
		warning: !channel.listens
			? 'not-listening'
			: (incoming.get(channelId(channel.key)) ?? []).length === 0
				? 'unused'
				: null,
		ref: channel.key
	}));

	// --- Vertical placement: stack each column, then centre the short ones -----
	const columns = [sourceNodes, teamNodes, channelNodes];
	const columnHeights = columns.map((nodes) =>
		nodes.length === 0 ? 0 : nodes.length * NODE_HEIGHT + (nodes.length - 1) * ROW_GAP
	);
	const tallest = Math.max(0, ...columnHeights);

	columns.forEach((nodes, index) => {
		const offset = PADDING + (tallest - columnHeights[index]) / 2;
		nodes.forEach((node, row) => {
			node.y = round(offset + row * (NODE_HEIGHT + ROW_GAP));
		});
	});

	const nodes = [...sourceNodes, ...teamNodes, ...channelNodes];
	const byId = new Map(nodes.map((node) => [node.id, node] as const));

	// --- Edge geometry --------------------------------------------------------
	const edges: FlowEdge[] = [];
	const neighbors: Record<string, string[]> = {};
	const incidentEdges: Record<string, string[]> = {};

	const track = (map: Record<string, string[]>, key: string, value: string) => {
		(map[key] ??= []).push(value);
	};

	for (const link of links) {
		const from = byId.get(link.from);
		const to = byId.get(link.to);
		if (!from || !to) continue;

		const id = `${link.from}→${link.to}`;
		edges.push({
			id,
			from: link.from,
			to: link.to,
			path: bezier(
				from.x + NODE_WIDTH,
				from.y + NODE_HEIGHT / 2,
				to.x,
				to.y + NODE_HEIGHT / 2
			),
			wildcard: link.wildcard
		});

		track(neighbors, link.from, link.to);
		track(neighbors, link.to, link.from);
		track(incidentEdges, link.from, id);
		track(incidentEdges, link.to, id);
	}

	const usedColumns = columns.filter((nodes) => nodes.length > 0).length;
	const width = usedColumns === 0 ? PADDING * 2 : columnX(columns.length - 1) + NODE_WIDTH + PADDING;

	return {
		nodes,
		edges,
		width,
		height: tallest + PADDING * 2,
		neighbors,
		incidentEdges
	};
}

/**
 * Everything a node reaches, in both directions: pick a channel to see every
 * source feeding it, or a source to see where its releases land.
 */
export function traceConnections(
	flow: RoutingFlow,
	id: string
): { nodes: Set<string>; edges: Set<string> } {
	const nodes = new Set<string>([id]);
	const edges = new Set<string>();

	const walk = (direction: 'forward' | 'backward') => {
		const queue = [id];
		while (queue.length > 0) {
			const current = queue.shift()!;
			for (const edge of flow.edges) {
				const [from, to] =
					direction === 'forward' ? [edge.from, edge.to] : [edge.to, edge.from];
				if (from !== current || nodes.has(to)) {
					if (from === current) edges.add(edge.id);
					continue;
				}
				edges.add(edge.id);
				nodes.add(to);
				queue.push(to);
			}
		}
	};

	walk('forward');
	walk('backward');
	return { nodes, edges };
}
