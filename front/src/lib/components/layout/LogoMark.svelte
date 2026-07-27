<script lang="ts">
	import logo from '$lib/assets/logo.svg';
	import { cn } from '$lib/utils';

	interface Props {
		size?: 'sm' | 'md' | 'lg';
		class?: string;
	}

	let { size = 'sm', class: className = '' }: Props = $props();

	/** Wide terminal wordmark — viewBox 120×44 (~2.73:1). */
	const sizeClass = $derived(
		size === 'lg' ? 'h-12 w-[8.25rem]' : size === 'md' ? 'h-9 w-24' : 'h-7 w-[4.75rem]'
	);
</script>

<img
	src={logo}
	alt=""
	aria-hidden="true"
	class={cn(
		'shrink-0 rounded-md ring-1 ring-[#39FF14]/30 shadow-[0_0_14px_-3px_rgba(57,255,20,0.4)]',
		sizeClass,
		className
	)}
/>
