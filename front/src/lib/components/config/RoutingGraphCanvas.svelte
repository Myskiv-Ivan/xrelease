<script lang="ts">
	import Badge from '$lib/components/kit/Badge.svelte';
	import Button from '$lib/components/kit/Button.svelte';
	import type { RoutingGraph } from '$lib/config/desired-validation';
	import {
		buildRoutingFlow,
		traceConnections,
		NODE_HEIGHT,
		NODE_WIDTH,
		type FlowNode,
		type FlowWarning
	} from '$lib/config/routing-flow';
	import { TYPE_CODE, TYPE_MUTED, TYPE_HINT } from '$lib/components/kit/layout-styles';
	import { t } from '$lib/i18n';

	interface Props {
		graph: RoutingGraph;
		/** Canvas height; the graph is scaled to fit on load. */
		height?: string;
		/** Grow with the parent (parent must give a real height via flex/calc). */
		fill?: boolean;
	}

	let { graph, height = 'min(70dvh, 42rem)', fill = false }: Props = $props();

	const MIN_SCALE = 0.2;
	const MAX_SCALE = 2.5;
	const FIT_PADDING = 32;
	/** Fit may zoom in a little so a two-node graph does not float in empty space. */
	const MAX_FIT_SCALE = 1.15;

	let viewport = $state<HTMLDivElement | null>(null);
	/** Measured, not read from clientWidth: the tab is display:none until opened. */
	let boxWidth = $state(0);
	let boxHeight = $state(0);
	let tx = $state(0);
	let ty = $state(0);
	let scale = $state(1);
	let selected = $state<string | null>(null);
	let hovered = $state<string | null>(null);
	let panning = $state(false);

	const flow = $derived(buildRoutingFlow(graph));

	/** Selection wins over hover so a chosen trace survives moving the pointer. */
	const focus = $derived(selected ?? hovered);
	const trace = $derived(focus ? traceConnections(flow, focus) : null);

	const clamp = (value: number, min: number, max: number) =>
		Math.min(max, Math.max(min, value));

	function fit() {
		if (boxWidth < 1 || boxHeight < 1) return;
		if (flow.width === 0 || flow.height === 0) return;
		const next = clamp(
			Math.min((boxWidth - FIT_PADDING) / flow.width, (boxHeight - FIT_PADDING) / flow.height),
			MIN_SCALE,
			MAX_FIT_SCALE
		);
		scale = next;
		tx = (boxWidth - flow.width * next) / 2;
		ty = (boxHeight - flow.height * next) / 2;
	}

	// The canvas lives in a tab panel that is display:none until selected, so at
	// mount its box is 0×0 — fitting then would clamp to the minimum scale and
	// park the graph in the corner. Measure instead, and fit once a real box exists.
	$effect(() => {
		const el = viewport;
		if (!el) return;
		const observer = new ResizeObserver((entries) => {
			const box = entries[0]?.contentRect;
			if (!box) return;
			boxWidth = box.width;
			boxHeight = box.height;
		});
		observer.observe(el);
		return () => observer.disconnect();
	});

	// Re-fit when the graph's extent changes (nodes added or removed) or when the
	// panel first gets a real box. Comparing the extent rather than the flow
	// object matters: the config store re-polls and hands us a fresh graph every
	// tick, which would otherwise yank the canvas back while the operator pans.
	let fittedExtent = '';
	$effect(() => {
		const extent = `${flow.width}x${flow.height}`;
		if (boxWidth < 1 || boxHeight < 1 || extent === fittedExtent) return;
		fittedExtent = extent;
		fit();
	});

	function zoomBy(factor: number) {
		if (boxWidth < 1) return;
		zoomAt(boxWidth / 2, boxHeight / 2, scale * factor);
	}

	/** Zoom while keeping the graph point under (px, py) pinned to the cursor. */
	function zoomAt(px: number, py: number, target: number) {
		const next = clamp(target, MIN_SCALE, MAX_SCALE);
		const ratio = next / scale;
		tx = px - (px - tx) * ratio;
		ty = py - (py - ty) * ratio;
		scale = next;
	}

	function onWheel(event: WheelEvent) {
		if (!viewport) return;
		event.preventDefault();
		const rect = viewport.getBoundingClientRect();
		zoomAt(
			event.clientX - rect.left,
			event.clientY - rect.top,
			scale * Math.exp(-event.deltaY * 0.0015)
		);
	}

	function onPointerDown(event: PointerEvent) {
		if (event.button !== 0) return;
		// Capturing the pointer here would retarget the following `click` to the
		// viewport, so a press that starts on a node must not begin a pan.
		if ((event.target as HTMLElement).closest('button')) return;
		panning = true;
		(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
	}

	function onPointerMove(event: PointerEvent) {
		if (!panning) return;
		tx += event.movementX;
		ty += event.movementY;
	}

	function onPointerUp(event: PointerEvent) {
		panning = false;
		const el = event.currentTarget as HTMLElement;
		if (el.hasPointerCapture(event.pointerId)) el.releasePointerCapture(event.pointerId);
	}

	/** Keyboard equivalents for drag-pan and wheel-zoom. */
	function onKeydown(event: KeyboardEvent) {
		const step = event.shiftKey ? 120 : 40;
		switch (event.key) {
			case 'Escape':
				selected = null;
				return;
			case 'ArrowLeft':
				tx += step;
				break;
			case 'ArrowRight':
				tx -= step;
				break;
			case 'ArrowUp':
				ty += step;
				break;
			case 'ArrowDown':
				ty -= step;
				break;
			case '+':
			case '=':
				zoomBy(1.2);
				break;
			case '-':
				zoomBy(1 / 1.2);
				break;
			case '0':
				fit();
				break;
			default:
				return;
		}
		event.preventDefault();
	}

	function nodeState(node: FlowNode): 'focus' | 'linked' | 'dim' | 'plain' {
		if (!trace) return 'plain';
		if (node.id === focus) return 'focus';
		return trace.nodes.has(node.id) ? 'linked' : 'dim';
	}

	function warningLabel(warning: FlowWarning): string {
		switch (warning) {
			case 'no-target':
				return t('config.graphWarnNoTarget');
			case 'no-channel':
				return t('config.graphWarnNoChannel');
			case 'unused':
				return t('config.graphWarnUnused');
			case 'not-listening':
				return t('config.graphWarnNotListening');
		}
	}

	const KIND_ACCENT: Record<FlowNode['kind'], string> = {
		source: 'before:bg-chart-1',
		team: 'before:bg-chart-2',
		channel: 'before:bg-chart-3'
	};

	const focusNode = $derived(flow.nodes.find((node) => node.id === focus) ?? null);
</script>

<div class="flex h-full min-h-0 flex-col gap-2">
	<div class="flex flex-wrap items-center gap-2">
		<p id="routing-graph-hint" class={TYPE_HINT}>{t('config.graphHint')}</p>
		<div class="ml-auto flex items-center gap-1.5">
			{#if selected}
				<Badge tone="accent">
					{focusNode?.label}
					{#if trace}
						· {trace.nodes.size - 1}
						{t('config.graphReaches')}
					{/if}
				</Badge>
				<Button variant="ghost" size="sm" onclick={() => (selected = null)}>
					{t('config.graphSelectionCleared')}
				</Button>
			{/if}
			<Button variant="outline" size="sm" onclick={() => zoomBy(1 / 1.2)}>
				<span aria-label={t('config.graphZoomOut')}>−</span>
			</Button>
			<Button variant="outline" size="sm" onclick={() => zoomBy(1.2)}>
				<span aria-label={t('config.graphZoomIn')}>+</span>
			</Button>
			<Button variant="outline" size="sm" onclick={fit}>{t('config.graphFit')}</Button>
		</div>
	</div>

	<!--
		A pan/zoom canvas is a composite widget, so the viewport itself is focusable
		and drives the same navigation from the keyboard (arrows pan, +/- zoom, 0
		fits, Escape clears). Nodes are real buttons layered over the SVG edge
		canvas, so tabbing walks the graph in layout order.

		The a11y rules below classify `role="application"` as non-interactive; for a
		custom widget that owns its keyboard handling it is the correct role, and
		the element carries an accessible name, a description and a focus ring.
	-->
	<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
	<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
	<div
		bind:this={viewport}
		role="application"
		aria-label={t('config.graphTitle')}
		aria-describedby="routing-graph-hint"
		tabindex="0"
		class="relative min-h-0 overflow-hidden rounded-md border border-border bg-muted/10
			focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none
			{fill ? 'flex-1' : ''}
			{panning ? 'cursor-grabbing' : 'cursor-grab'}"
		style={fill ? undefined : `height: ${height};`}
		onwheel={onWheel}
		onpointerdown={onPointerDown}
		onpointermove={onPointerMove}
		onpointerup={onPointerUp}
		onpointercancel={onPointerUp}
		onkeydown={onKeydown}
	>
		{#if flow.nodes.length === 0}
			<p class="absolute inset-0 grid place-items-center {TYPE_HINT}">
				{t('config.graphEmpty')}
			</p>
		{:else}
			<div
				class="absolute top-0 left-0 origin-top-left"
				style="transform: translate({tx}px, {ty}px) scale({scale});"
			>
				<svg
					class="pointer-events-none absolute top-0 left-0 overflow-visible"
					width={flow.width}
					height={flow.height}
					aria-hidden="true"
				>
					{#each flow.edges as edge (edge.id)}
						{@const on = !trace || trace.edges.has(edge.id)}
						<path
							d={edge.path}
							fill="none"
							stroke="currentColor"
							stroke-width={on && trace ? 2 : 1.5}
							stroke-dasharray={edge.wildcard ? '5 4' : undefined}
							class="transition-opacity {on
								? 'text-accent opacity-80'
								: 'text-muted-foreground opacity-15'}"
						/>
					{/each}
				</svg>

				{#each flow.nodes as node (node.id)}
					{@const state = nodeState(node)}
					<button
						type="button"
						class="absolute flex flex-col justify-center gap-0.5 overflow-hidden rounded-lg border
							px-3 text-left transition-[opacity,box-shadow,border-color]
							before:absolute before:top-0 before:bottom-0 before:left-0 before:w-1 before:content-['']
							{KIND_ACCENT[node.kind]}
							{state === 'focus'
							? 'border-accent bg-card shadow-md ring-2 ring-accent/40'
							: state === 'linked'
								? 'border-accent/50 bg-card shadow-sm'
								: state === 'dim'
									? 'border-border bg-card opacity-25'
									: 'border-border bg-card hover:border-accent/60 hover:shadow-sm'}"
						style="left: {node.x}px; top: {node.y}px; width: {NODE_WIDTH}px; height: {NODE_HEIGHT}px;"
						aria-pressed={selected === node.id}
						onclick={() => (selected = selected === node.id ? null : node.id)}
						onmouseenter={() => (hovered = node.id)}
						onmouseleave={() => (hovered = null)}
						onfocus={() => (hovered = node.id)}
						onblur={() => (hovered = null)}
					>
						<div class="flex items-center gap-1.5">
							<span class="truncate text-sm font-medium" title={node.label}>{node.label}</span>
							{#if node.warning}
								<span class="shrink-0 text-warning" title={warningLabel(node.warning)}>▲</span>
							{/if}
						</div>
						<div class="flex items-center gap-1.5 {TYPE_MUTED}">
							<span class="shrink-0 rounded bg-muted px-1 py-px">{node.badge}</span>
							{#if node.sublabel}
								<span class="truncate {TYPE_CODE}" title={node.sublabel}>{node.sublabel}</span>
							{/if}
						</div>
					</button>
				{/each}
			</div>
		{/if}
	</div>

	<div class="flex flex-wrap items-center gap-3 {TYPE_MUTED}">
		<span class="flex items-center gap-1.5">
			<span class="inline-block size-2 rounded-sm bg-chart-1"></span>
			{t('config.graphLegendSource')}
		</span>
		<span class="flex items-center gap-1.5">
			<span class="inline-block size-2 rounded-sm bg-chart-2"></span>
			{t('config.graphLegendTeam')}
		</span>
		<span class="flex items-center gap-1.5">
			<span class="inline-block size-2 rounded-sm bg-chart-3"></span>
			{t('config.graphLegendChannel')}
		</span>
		<span class="flex items-center gap-1.5">
			<svg width="26" height="8" aria-hidden="true" class="text-muted-foreground">
				<line
					x1="0"
					y1="4"
					x2="26"
					y2="4"
					stroke="currentColor"
					stroke-width="1.5"
					stroke-dasharray="5 4"
				/>
			</svg>
			{t('config.routingWildcardBadge')}
		</span>
		<span class="ml-auto tabular-nums">{Math.round(scale * 100)}%</span>
	</div>
</div>
