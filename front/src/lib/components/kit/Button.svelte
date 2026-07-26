<script lang="ts">
	import { Button as ShadcnButton } from '$lib/components/ui/button/index.js';
	import type { ButtonSize, ButtonVariant } from '$lib/components/ui/button/button.svelte';

	/** App-facing variants — mapped onto shadcn buttonVariants. */
	type AppVariant = 'primary' | 'accent' | 'outline' | 'ghost' | 'danger';
	type AppSize = 'sm' | 'md';

	interface Props {
		variant?: AppVariant;
		size?: AppSize;
		type?: 'button' | 'submit';
		disabled?: boolean;
		href?: string;
		target?: string;
		rel?: string;
		class?: string;
		onclick?: (event: MouseEvent) => void;
		title?: string;
		children: import('svelte').Snippet;
	}

	let {
		variant = 'primary',
		size = 'md',
		type = 'button',
		disabled = false,
		href,
		target,
		rel,
		class: className = '',
		onclick,
		title,
		children
	}: Props = $props();

	const variantMap: Record<AppVariant, ButtonVariant> = {
		primary: 'default',
		accent: 'accent',
		outline: 'outline',
		ghost: 'ghost',
		danger: 'destructive'
	};

	const sizeMap: Record<AppSize, ButtonSize> = {
		sm: 'sm',
		md: 'default'
	};
</script>

<ShadcnButton
	variant={variantMap[variant]}
	size={sizeMap[size]}
	{type}
	{disabled}
	{href}
	{target}
	{rel}
	{onclick}
	{title}
	class={className}
>
	{@render children()}
</ShadcnButton>
