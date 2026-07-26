<script lang="ts">
	import Badge from '$lib/components/kit/Badge.svelte';
	import { getTeamsStore } from '$lib/data/teams.svelte';

	interface Props {
		tag: string;
	}

	let { tag }: Props = $props();

	const teamsStore = getTeamsStore();

	const label = $derived(teamsStore.nameFor(tag) ?? tag);
	const showsRawTag = $derived(label !== tag);
</script>

<Badge tone="accent">
	<span title={showsRawTag ? tag : undefined}>{label}</span>
</Badge>
