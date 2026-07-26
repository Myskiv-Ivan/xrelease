<script lang="ts">
	import logo from '$lib/assets/logo.svg';
	import { cn } from '$lib/utils';

	interface Props {
		size?: 'sm' | 'md' | 'lg';
		class?: string;
	}

	let { size = 'sm', class: className = '' }: Props = $props();

	const sizeClass = $derived(
		size === 'lg' ? 'h-14 w-14' : size === 'md' ? 'h-10 w-10' : 'h-8 w-8'
	);
</script>

<img
	src={logo}
	alt=""
	aria-hidden="true"
	class={cn('shrink-0 rounded-xl shadow-sm', sizeClass, className)}
/>
