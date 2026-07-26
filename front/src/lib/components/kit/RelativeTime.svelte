<script lang="ts">
	import { formatDateTime, formatRelative } from '$lib/core/format';
	import { getNowStore } from '$lib/stores/now.svelte';
	import { cn } from '$lib/utils';

	interface Props {
		value: string | Date | null | undefined;
		/** When true, show absolute datetime (Mon D, YYYY HH:MM) instead of relative. */
		absolute?: boolean;
		class?: string;
	}

	let { value, absolute = false, class: className }: Props = $props();

	const now = getNowStore();

	const label = $derived(
		absolute ? formatDateTime(value) : formatRelative(value, now.current)
	);
	const title = $derived.by(() => {
		if (value == null || (typeof value === 'string' && value.trim() === '')) return undefined;
		const absoluteLabel = formatDateTime(value);
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
