<script lang="ts">
	import { TYPE_PAGE_DESC, TYPE_PAGE_TITLE } from '$lib/components/kit/layout-styles';

	interface Props {
		title: string;
		description?: string;
	}

	let { title, description }: Props = $props();
</script>

<section class="flex flex-col gap-1.5">
	<h1 class={TYPE_PAGE_TITLE}>{title}</h1>
	{#if description}
		<p class="max-w-prose {TYPE_PAGE_DESC}">{description}</p>
	{/if}
</section>
