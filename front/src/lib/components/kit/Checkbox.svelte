<script lang="ts">
	import { Checkbox as ShadcnCheckbox } from '$lib/components/ui/checkbox/index.js';
	import { cn } from '$lib/utils';

	interface Props {
		id?: string;
		class?: string;
		checked?: boolean;
		disabled?: boolean;
		/** Fired when the checked state changes (preferred). */
		onCheckedChange?: (checked: boolean) => void;
		/** Legacy HTML-style handler — still supported for existing call sites. */
		onchange?: (event: Event & { currentTarget: HTMLInputElement }) => void;
	}

	let {
		id,
		class: className = '',
		checked = $bindable(false),
		disabled = false,
		onCheckedChange,
		onchange
	}: Props = $props();

	function handleChange(next: boolean | 'indeterminate') {
		const value = next === true;
		checked = value;
		onCheckedChange?.(value);
		if (onchange) {
			onchange({
				currentTarget: { checked: value } as HTMLInputElement
			} as Event & { currentTarget: HTMLInputElement });
		}
	}
</script>

<ShadcnCheckbox
	{id}
	class={cn('cursor-pointer', className)}
	{checked}
	{disabled}
	onCheckedChange={handleChange}
/>
