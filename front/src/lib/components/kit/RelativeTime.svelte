<script lang="ts">
	import { formatDateTime, formatDateTimeFull, formatRelative } from '$lib/core/format';
	import { getNowStore } from '$lib/stores/now.svelte';
	import { cn } from '$lib/utils';

	interface Props {
		value: string | Date | null | undefined;
		/**
		 * - `relative` — compact "2m" / "3h" for scanning ops tables (default)
		 * - `datetime` — "Jul 23, 2026, 14:30"
		 * - `full` — adds seconds, for audit surfaces where two events a minute
		 *   apart must stay distinguishable (revision ledger, last login)
		 *
		 * Every timestamp renders through here so it stays a semantic
		 * `<time datetime>` — formatting helpers are for plain-string contexts
		 * (e.g. `KeyValueItem` values) that cannot host a component.
		 */
		format?: 'relative' | 'datetime' | 'full';
		class?: string;
	}

	let { value, format = 'relative', class: className }: Props = $props();

	const now = getNowStore();

	const label = $derived.by(() => {
		if (format === 'full') return formatDateTimeFull(value);
		if (format === 'datetime') return formatDateTime(value);
		return formatRelative(value, now.current);
	});
	// A relative label hides the actual instant, so keep it in the tooltip. The
	// absolute formats already show it — repeating it in a title is just noise.
	const title = $derived.by(() => {
		if (format !== 'relative') return undefined;
		if (value == null || (typeof value === 'string' && value.trim() === '')) return undefined;
		const absoluteLabel = formatDateTimeFull(value);
		return absoluteLabel === '—' ? undefined : absoluteLabel;
	});
</script>

<time
	datetime={typeof value === 'string' ? value : value?.toISOString()}
	class={cn(className)}
	{title}
>
	{label}
</time>
