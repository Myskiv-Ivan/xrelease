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
/** Compact nodes when a column is dense — keeps labels readable under fit-zoom. */
export const NODE_HEIGHT_COMPACT = 44;
const COLUMN_GAP = 96;
const LANE_GAP = 20;
const ROW_GAP = 12;
const ROW_GAP_COMPACT = 8;
const PADDING = 24;
/** Stack past this count → extra lanes inside the column (sources especially). */
export const MAX_ROWS_PER_LANE = 10;

/**
 * Fixed canvas height for the routing map.
 *
 * Large topologies (20+ sources) used to grow the page to ~2 viewports — unreadable
 * and hard to scan. Cap the box; pan/zoom + multi-lane packing keep nodes usable.
 */
export function preferredRoutingViewportHeight(
	_flowHeight?: number,
	opts?: { min?: number; max?: number; chrome?: number }
): number {
	const min = opts?.min ?? 22 * 16; // 22rem
	const max = opts?.max ?? 36 * 16; // 36rem
	if (typeof window === 'undefined') {
		return Math.round(Math.min(max, Math.max(min, 28 * 16)));
	}
	const dvhCap = Math.floor(window.innerHeight * 0.62);
	return Math.round(Math.min(max, Math.max(min, dvhCap)));
}

function densityForCount(count: number): { height: number; rowGap: number } {
	if (count > MAX_ROWS_PER_LANE) {
		return { height: NODE_HEIGHT_COMPACT, rowGap: ROW_GAP_COMPACT };
	}
	return { height: NODE_HEIGHT, rowGap: ROW_GAP };
}

/**
 * Place nodes in one logical column, wrapping into side-by-side lanes when tall.
 * Returns the occupied width (for the next column's x) and the stack height.
 */
function placeColumn(
	nodes: FlowNode[],
	startX: number,
	centerInHeight: number
): { width: number; height: number } {
	if (nodes.length === 0) return { width: 0, height: 0 };

	const { height: nodeH, rowGap } = densityForCount(nodes.length);
	const lanes = Math.max(1, Math.ceil(nodes.length / MAX_ROWS_PER_LANE));
	const rowsPerLane = Math.ceil(nodes.length / lanes);
	const stackHeight =
		rowsPerLane * nodeH + Math.max(0, rowsPerLane - 1) * rowGap;
	const offsetY = PADDING + Math.max(0, (centerInHeight - stackHeight) / 2);

	nodes.forEach((node, index) => {
		const lane = Math.floor(index / rowsPerLane);
		const row = index % rowsPerLane;
		node.x = round(startX + lane * (NODE_WIDTH + LANE_GAP));
		node.y = round(offsetY + row * (nodeH + rowGap));
		node.height = nodeH;
	});

	const width = lanes * NODE_WIDTH + (lanes - 1) * LANE_GAP;
	return { width, height: stackHeight };
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
	/** Drawn height — compact when the column is dense. */
	height: number;
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

	// --- Nodes (x/y filled by placeColumn) ------------------------------------
	const sourceNodes: FlowNode[] = graph.sources.map((source) => ({
		id: sourceId(source.key),
		kind: 'source' as const,
		column: 0,
		label: source.label,
		sublabel: source.routingTag ?? '',
		badge: getSourceKindMeta(source.type).label,
		x: 0,
		y: 0,
		height: NODE_HEIGHT,
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
		x: 0,
		y: 0,
		height: NODE_HEIGHT,
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
		x: 0,
		y: 0,
		height: NODE_HEIGHT,
		warning: !channel.listens
			? 'not-listening'
			: (incoming.get(channelId(channel.key)) ?? []).length === 0
				? 'unused'
				: null,
		ref: channel.key
	}));

	// --- Placement: multi-lane wrap for tall columns, then centre shorter ones --
	const columns = [sourceNodes, teamNodes, channelNodes];
	const provisionalHeights = columns.map((nodes) => {
		if (nodes.length === 0) return 0;
		const { height: nodeH, rowGap } = densityForCount(nodes.length);
		const lanes = Math.max(1, Math.ceil(nodes.length / MAX_ROWS_PER_LANE));
		const rowsPerLane = Math.ceil(nodes.length / lanes);
		return rowsPerLane * nodeH + Math.max(0, rowsPerLane - 1) * rowGap;
	});
	const tallest = Math.max(0, ...provisionalHeights);

	let cursorX = PADDING;
	let maxRight = PADDING;
	for (const nodes of columns) {
		if (nodes.length === 0) continue;
		const placed = placeColumn(nodes, cursorX, tallest);
		maxRight = Math.max(maxRight, cursorX + placed.width);
		cursorX += placed.width + COLUMN_GAP;
	}

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
				from.y + from.height / 2,
				to.x,
				to.y + to.height / 2
			),
			wildcard: link.wildcard
		});

		track(neighbors, link.from, link.to);
		track(neighbors, link.to, link.from);
		track(incidentEdges, link.from, id);
		track(incidentEdges, link.to, id);
	}

	const width = nodes.length === 0 ? PADDING * 2 : maxRight + PADDING;

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
