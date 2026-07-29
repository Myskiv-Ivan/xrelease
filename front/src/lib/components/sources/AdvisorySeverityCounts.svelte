<script lang="ts">
	import type { AdvisoryView } from '$lib/api/types';
	import Badge from '$lib/components/kit/Badge.svelte';
	import {
		bucketCode,
		bucketTone,
		severityCounts,
		severityCountsLabel
	} from '$lib/config/advisory-presentation';

	interface Props {
		advisories: AdvisoryView[];
		/** Render as a link to the advisories page instead of a bare strip. */
		href?: string;
		/**
		 * Extra wording prefixed to the spelled-out label, e.g. the panel-level
		 * "Across synced releases". The counts themselves are always appended.
		 */
		title?: string;
	}

	let { advisories, href, title }: Props = $props();

	const counts = $derived(severityCounts(advisories));
	const spelled = $derived(severityCountsLabel(counts));
	const label = $derived(title ? `${title}: ${spelled}` : spelled);

	const STRIP = 'inline-flex flex-wrap items-center gap-1';
</script>

<!--
	Compact severity strip: one chip per populated bucket, worst first.

	Severity is carried three ways on purpose — colour, letter, and the
	spelled-out `aria-label`/`title` — so the cell still reads correctly for a
	screen reader and for anyone who cannot separate the tints. The letter is
	what makes `critical` and `high` distinguishable at all: both are red,
	because both are "drop what you are doing".

	When a `href` is given, the anchor itself carries the label rather than an
	inner `<span>`: the letters are `aria-hidden`, so a label parked on a child
	would leave the link's accessible name to fall out of the accname
	contents-walk — a nameless link for anyone not looking at it.
-->
{#snippet chips()}
	{#each counts as { bucket, count } (bucket)}
		<Badge tone={bucketTone(bucket)} class="px-1.5 font-mono tabular-nums">
			<span aria-hidden="true">{bucketCode(bucket)}{count}</span>
		</Badge>
	{/each}
{/snippet}

{#if counts.length > 0}
	{#if href}
		<a {href} class="{STRIP} cursor-pointer no-underline" title={label} aria-label={label}>
			{@render chips()}
		</a>
	{:else}
		<span class={STRIP} title={label} aria-label={label} role="img">
			{@render chips()}
		</span>
	{/if}
{/if}
