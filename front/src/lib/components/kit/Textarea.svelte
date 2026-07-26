<script lang="ts">
	import { TYPE_CODE } from '$lib/components/kit/layout-styles';
	import { cn } from '$lib/utils';

	interface Props {
		id?: string;
		class?: string;
		value?: string;
		rows?: number;
		placeholder?: string;
		disabled?: boolean;
		name?: string;
		required?: boolean;
		/** Monospace content (config fragments, templates, URL lists). */
		mono?: boolean;
		'aria-label'?: string;
		onchange?: (event: Event & { currentTarget: HTMLTextAreaElement }) => void;
		oninput?: (event: Event & { currentTarget: HTMLTextAreaElement }) => void;
	}

	let {
		id,
		class: className = '',
		value = $bindable(''),
		rows = 3,
		placeholder,
		disabled = false,
		name,
		required = false,
		mono = false,
		'aria-label': ariaLabel,
		onchange,
		oninput
	}: Props = $props();

	/** Mirrors the shadcn input chrome so text fields read as one control family. */
	const controlClass =
		'border-input bg-background text-foreground placeholder:text-muted-foreground ' +
		'focus-visible:border-ring focus-visible:ring-ring/50 w-full rounded-md border px-3 py-2 ' +
		'text-sm shadow-xs outline-none transition-[color,box-shadow] focus-visible:ring-3 ' +
		'disabled:cursor-not-allowed disabled:opacity-50';
</script>

<textarea
	{id}
	class={cn(controlClass, mono && TYPE_CODE, className)}
	bind:value
	{rows}
	{placeholder}
	{disabled}
	{name}
	{required}
	aria-label={ariaLabel}
	{onchange}
	{oninput}
></textarea>
