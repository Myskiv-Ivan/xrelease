<script lang="ts">
	import * as Alert from '$lib/components/ui/alert/index.js';
	import { cn } from '$lib/utils';

	type Tone = 'success' | 'warning' | 'danger' | 'info';

	interface Props {
		tone?: Tone;
		children: import('svelte').Snippet;
		class?: string;
	}

	let { tone = 'info', children, class: className = '' }: Props = $props();

	const tones: Record<Tone, string> = {
		success: 'border-success/40 bg-success/10 text-success',
		warning: 'border-warning/40 bg-warning/10 text-warning',
		danger: 'border-destructive/40 bg-destructive/10 text-destructive',
		info: 'border-primary/30 bg-primary/10 text-primary'
	};
</script>

	<Alert.Root
		variant={tone === 'danger' ? 'destructive' : 'default'}
		class={cn(tones[tone], className)}
		role={tone === 'danger' || tone === 'warning' ? 'alert' : 'status'}
	>
	<Alert.Description class="text-current">
		{@render children()}
	</Alert.Description>
</Alert.Root>
