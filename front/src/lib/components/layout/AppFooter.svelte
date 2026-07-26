<script lang="ts">
	import { APP_ISSUES_URL, APP_NAME, APP_REPOSITORY, APP_VERSION } from '$lib/core/constants';
	import { cn } from '$lib/utils';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import { t } from '$lib/i18n';
	import { LAYOUT_MAX, TYPE_MUTED } from '$lib/components/kit/layout-styles';

	interface Props {
		showVersion?: boolean;
		class?: string;
	}

	let { showVersion = true, class: className = '' }: Props = $props();

	const linkClass =
		'inline-flex cursor-pointer items-center gap-1.5 rounded-md px-2 py-1 text-muted-foreground no-underline transition-colors duration-200 hover:bg-muted/60 hover:text-foreground';
</script>

<footer class={cn('border-t border-border py-3', className)}>
	<div
		class={cn(
			'mx-auto flex flex-col items-center gap-2 px-4 text-center sm:px-6',
			LAYOUT_MAX,
			TYPE_MUTED
		)}
	>
		{#if showVersion}
			<p>{APP_NAME} v{APP_VERSION}</p>
		{/if}
		<nav class="flex flex-wrap items-center justify-center gap-1" aria-label={t('footer.links')}>
			<a
				href={APP_REPOSITORY}
				target="_blank"
				rel="noopener noreferrer"
				class={linkClass}
			>
				<svg class="size-3.5" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
					<path
						d="M12 0c-6.626 0-12 5.373-12 12 0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23.957-.266 1.983-.399 3.003-.404 1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576 4.765-1.589 8.199-6.086 8.199-11.386 0-6.627-5.373-12-12-12z"
					/>
				</svg>
				{t('footer.github')}
			</a>
			<span class="text-border" aria-hidden="true">·</span>
			<a href={APP_ISSUES_URL} target="_blank" rel="noopener noreferrer" class={linkClass}>
				<CircleAlertIcon class="size-3.5" aria-hidden="true" />
				{t('footer.issues')}
			</a>
		</nav>
	</div>
</footer>
