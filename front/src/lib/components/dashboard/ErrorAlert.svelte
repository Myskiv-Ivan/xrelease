<script lang="ts">
	import * as Alert from '$lib/components/ui/alert/index.js';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';

	interface Props {
		message: string;
	}

	let { message }: Props = $props();
</script>

<Alert.Root variant="destructive">
	<CircleAlertIcon />
	<Alert.Description>{message}</Alert.Description>
</Alert.Root>
