<script lang="ts">
	import { EMPTY_VALUE, formatDateTimeFull, formatRelative } from '$lib/core/format';
	import { getNowStore } from '$lib/stores/now.svelte';
	import { cn } from '$lib/utils';

	interface Props {
		value: string | Date | null | undefined;
		/**
		 * - `full` — "Jul 23, 2026, 14:30:07" (default). Every field and table
		 *   shows the complete instant; a compact "2m" is kept in the tooltip so
		 *   recency is still one hover away.
		 * - `relative` — compact "2m" / "3h" with the absolute instant in the
		 *   tooltip. For chrome that reports freshness rather than data (the
		 *   refresh toolbar), where the exact second is noise.
		 *
		 * Seconds are always included in the absolute form: two config applies or
		 * two deliveries a minute apart are indistinguishable without them, which
		 * defeats the point of an audit surface.
		 *
		 * Every timestamp renders through here so it stays a semantic
		 * `<time datetime>` — the formatting helpers are for plain-string contexts
		 * (e.g. `KeyValueItem` values) that cannot host a component.
		 */
		format?: 'full' | 'relative';
		class?: string;
	}

	let { value, format = 'full', class: className }: Props = $props();

	const now = getNowStore();
	const isEmpty = $derived(value == null || (typeof value === 'string' && value.trim() === ''));

	const label = $derived.by(() =>
		format === 'relative' ? formatRelative(value, now.current) : formatDateTimeFull(value)
	);

	/**
	 * The tooltip carries the *other* representation, so both the exact instant
	 * and its recency are reachable from any timestamp.
	 *
	 * Suppressed when it would add nothing: no value, or the two agree —
	 * `formatRelative` itself falls back to the absolute format past 7 days, so
	 * an old timestamp would otherwise get a tooltip repeating its own label.
	 */
	const title = $derived.by(() => {
		if (isEmpty) return undefined;
		const other =
			format === 'relative' ? formatDateTimeFull(value) : formatRelative(value, now.current);
		return other === EMPTY_VALUE || other === label ? undefined : other;
	});
</script>

<time
	datetime={typeof value === 'string' ? value : value?.toISOString()}
	class={cn(className)}
	{title}
>
	{label}
</time>
