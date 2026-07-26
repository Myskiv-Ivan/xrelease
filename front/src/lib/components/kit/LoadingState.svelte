<script lang="ts">
	import { Skeleton } from '$lib/components/ui/skeleton/index.js';
	import { t } from '$lib/i18n';
	import { TYPE_HINT } from '$lib/components/kit/layout-styles';

	interface Props {
		/** Optional status label; defaults to common.loading. */
		label?: string;
	}

	let { label }: Props = $props();
	const text = $derived(label ?? t('common.loading'));
</script>

<div class="flex flex-col gap-3 py-8" role="status" aria-live="polite" aria-label={text}>
	<div class="flex items-center gap-3">
		<Skeleton class="size-4 rounded-full" />
		<span class={TYPE_HINT}>{text}</span>
	</div>
	<div class="grid gap-2">
		<Skeleton class="h-4 w-full" />
		<Skeleton class="h-4 w-5/6" />
		<Skeleton class="h-4 w-2/3" />
	</div>
</div>
