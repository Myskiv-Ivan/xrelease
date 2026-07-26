<script lang="ts">
	import { TYPE_CODE } from '$lib/components/kit/layout-styles';
	import { getSourceKindMeta, KIND_TONE_CLASSES } from '$lib/config/source-kinds';
	import { cn } from '$lib/utils';

	interface Props {
		kind: string;
		label?: string;
	}

	let { kind, label }: Props = $props();
	const meta = $derived(getSourceKindMeta(kind));
</script>

<span
	class={cn(
		'inline-flex max-w-none shrink-0 items-center gap-1 whitespace-nowrap rounded px-2 py-0.5 text-xs font-medium',
		KIND_TONE_CLASSES[meta.tone]
	)}
	title={label ?? meta.label}
>
	<span class={cn(TYPE_CODE, 'shrink-0 opacity-80')}>{meta.glyph}</span>
	<span class="whitespace-nowrap">{label ?? meta.label}</span>
</span>
