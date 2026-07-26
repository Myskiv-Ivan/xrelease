<script lang="ts">
	import RoutingGraphCanvas from '$lib/components/config/RoutingGraphCanvas.svelte';
	import {
		parseDesiredDocument,
		readNotifiers,
		readSources,
		readTeams,
		type DesiredMap,
		type DesiredNotifierDraft,
		type DesiredSourceDraft,
		type DesiredTeamDraft
	} from '$lib/config/desired-document';
	import { buildRoutingGraph, type RoutingGraph } from '$lib/config/desired-validation';
	import {
		buildRoutingFlow,
		preferredRoutingViewportHeight
	} from '$lib/config/routing-flow';
	import { t } from '$lib/i18n';
	import { TYPE_HINT, TYPE_PANEL_TITLE } from '$lib/components/kit/layout-styles';
	import { cn } from '$lib/utils';

	interface Props {
		desiredContent?: string | null | undefined;
		/** Effective running config — fallback when desired omits a live sink. */
		effectiveContent?: string | null | undefined;
		/**
		 * Live editor drafts — when set, the graph follows unsaved channel/tag edits
		 * instead of the last applied document.
		 */
		drafts?: {
			sources: DesiredSourceDraft[];
			teams: DesiredTeamDraft[];
			notifiers: DesiredNotifierDraft[];
			data: DesiredMap;
		} | null;
		/**
		 * Compact card for Overview; full-bleed fill for dedicated Routing views.
		 * @default 'page'
		 */
		variant?: 'page' | 'card';
		class?: string;
	}

	let {
		desiredContent,
		effectiveContent,
		drafts = null,
		variant = 'page',
		class: className = ''
	}: Props = $props();

	function asObject(value: unknown): DesiredMap {
		if (value && typeof value === 'object' && !Array.isArray(value)) {
			return value as DesiredMap;
		}
		return {};
	}

	function hasAppriseBlock(data: DesiredMap): boolean {
		return Object.keys(asObject(data.apprise)).length > 0;
	}

	function graphFrom(raw: string | null | undefined, overlayApprise?: DesiredMap): RoutingGraph | null {
		const text = raw?.trim();
		if (!text) return null;
		try {
			const parsed = parseDesiredDocument(text);
			const data =
				overlayApprise && !hasAppriseBlock(parsed.data)
					? { ...parsed.data, apprise: overlayApprise }
					: parsed.data;
			// Always pass notifier drafts so global Apprise appears even when urls
			// were wiped (listens:false) — matches Edit preview and Technical view.
			return buildRoutingGraph({
				sources: readSources(parsed.data),
				teams: readTeams(parsed.data),
				data,
				notifiers: readNotifiers(data)
			});
		} catch {
			return null;
		}
	}

	const graph = $derived.by(() => {
		if (drafts) {
			return buildRoutingGraph({
				sources: drafts.sources,
				teams: drafts.teams,
				notifiers: drafts.notifiers,
				data: drafts.data
			});
		}

		let overlay: DesiredMap | undefined;
		const effectiveRaw = effectiveContent?.trim();
		if (effectiveRaw) {
			try {
				const effective = parseDesiredDocument(effectiveRaw);
				if (hasAppriseBlock(effective.data)) {
					overlay = asObject(effective.data.apprise);
				}
			} catch {
				overlay = undefined;
			}
		}
		return graphFrom(desiredContent, overlay) ?? graphFrom(effectiveContent);
	});

	const viewportHeightPx = $derived(
		graph ? preferredRoutingViewportHeight(buildRoutingFlow(graph).height) : 36 * 16
	);
</script>

{#if graph}
	<section class={cn('flex w-full flex-col gap-2', className)}>
		{#if variant === 'page' && !drafts}
			<h2 class={TYPE_PANEL_TITLE}>{t('config.graphTitle')}</h2>
		{/if}
		{#if variant === 'page' && drafts}
			<h2 class={TYPE_PANEL_TITLE}>{t('config.graphTitle')} · {t('config.graphDraftHint')}</h2>
		{/if}
		{#if variant === 'card'}
			<RoutingGraphCanvas {graph} height="22rem" />
		{:else}
			<!-- Explicit height — fill/h-full collapsed to 0 without a sized flex ancestor. -->
			<RoutingGraphCanvas {graph} height={`${viewportHeightPx}px`} />
		{/if}
	</section>
{:else if drafts || desiredContent?.trim() || effectiveContent?.trim()}
	<p class={TYPE_HINT}>{t('config.routingParseError')}</p>
{:else}
	<p class={TYPE_HINT}>{t('config.editNoDesired')}</p>
{/if}
