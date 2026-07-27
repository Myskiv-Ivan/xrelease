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
		// `bg-card` so the table is its own surface: it renders both inside a
		// Panel and bare on the page (Sources), and rows are opaque `bg-card`.
		'w-full overflow-x-auto bg-card',
		bordered && 'rounded-xl border border-border',
		className
	)}
>
	<Table.Root>
		{@render children()}
	</Table.Root>
</div>
