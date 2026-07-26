<script lang="ts">
	import { Input as ShadcnInput } from '$lib/components/ui/input/index.js';
	import { cn } from '$lib/utils';

	interface Props {
		id?: string;
		type?: 'text' | 'password' | 'search' | 'email' | 'url' | 'number';
		class?: string;
		value?: string;
		placeholder?: string;
		/** Accessible name when no visible label (e.g. table search). */
		'aria-label'?: string;
		disabled?: boolean;
		autocomplete?: HTMLInputElement['autocomplete'];
		name?: string;
		required?: boolean;
		onchange?: (event: Event & { currentTarget: HTMLInputElement }) => void;
		oninput?: (event: Event & { currentTarget: HTMLInputElement }) => void;
		onkeydown?: (event: KeyboardEvent & { currentTarget: HTMLInputElement }) => void;
	}

	let {
		id,
		type = 'text',
		class: className = '',
		value = $bindable(''),
		placeholder,
		'aria-label': ariaLabel,
		disabled = false,
		autocomplete,
		name,
		required = false,
		onchange,
		oninput,
		onkeydown
	}: Props = $props();
</script>

<ShadcnInput
	{id}
	{type}
	class={cn(className)}
	bind:value
	{placeholder}
	aria-label={ariaLabel}
	{disabled}
	{autocomplete}
	{name}
	{required}
	{onchange}
	{oninput}
	{onkeydown}
/>
