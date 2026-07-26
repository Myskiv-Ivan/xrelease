<script lang="ts">
	import * as Table from '$lib/components/ui/table/index.js';
	import { cn } from '$lib/utils';

	interface Props {
		bordered?: boolean;
		children: import('svelte').Snippet;
		class?: string;
	}

	let { bordered = true, children, class: className = '' }: Props = $props();
</script>

<div
	class={cn(
		'w-full overflow-x-auto',
		bordered && 'rounded-xl border border-border',
		className
	)}
>
	<Table.Root>
		{@render children()}
	</Table.Root>
</div>
