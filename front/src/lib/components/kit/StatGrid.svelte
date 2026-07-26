<script lang="ts">
	import { cn } from '$lib/utils';

	interface Props {
		columns?: 2 | 3 | 4 | 5;
		class?: string;
		children: import('svelte').Snippet;
	}

	let { columns = 4, class: className = '', children }: Props = $props();

	const columnClasses: Record<2 | 3 | 4 | 5, string> = {
		2: 'sm:grid-cols-2',
		3: 'sm:grid-cols-2 lg:grid-cols-3',
		4: 'sm:grid-cols-2 lg:grid-cols-4',
		5: 'sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-5'
	};
</script>

<div class={cn('grid gap-4', columnClasses[columns], className)}>
	{@render children()}
</div>
