<script lang="ts">
	import type { ApiResourceLink } from '$lib/config/api-links';
	import { cn } from '$lib/utils';
	import { t } from '$lib/i18n';

	interface Props {
		links: ApiResourceLink[];
		class?: string;
	}

	let { links, class: className = '' }: Props = $props();

	const linkClass =
		'inline-flex cursor-pointer items-center gap-1 text-sm text-foreground no-underline transition-colors duration-200 hover:text-primary hover:underline';
</script>

<ul class={cn('space-y-2 text-sm', className)}>
	{#each links as link (link.href)}
		<li>
			<a
				href={link.href}
				class={linkClass}
				{...link.external ? { target: '_blank', rel: 'noopener noreferrer' } : {}}
			>
				{t(link.labelKey)}
			</a>
		</li>
	{/each}
</ul>
