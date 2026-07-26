<script lang="ts">
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import Badge from '$lib/components/kit/Badge.svelte';
	import Button from '$lib/components/kit/Button.svelte';
	import HeaderMenu from '$lib/components/layout/HeaderMenu.svelte';
	import OrgSwitcher from '$lib/components/layout/OrgSwitcher.svelte';
	import AppBrand from '$lib/components/layout/AppBrand.svelte';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu/index.js';
	import { buttonVariants } from '$lib/components/ui/button/button.svelte';
	import { LAYOUT_MAX } from '$lib/components/kit/layout-styles';
	import { NAV_ITEMS } from '$lib/config/navigation';
	import { getStatusStore } from '$lib/data/status.svelte';
	import { t } from '$lib/i18n';
	import { getAuthState } from '$lib/stores/auth.svelte';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import { cn } from '$lib/utils';

	const auth = getAuthState();
	const statusStore = getStatusStore();

	const visibleNavItems = $derived(
		NAV_ITEMS.filter((item) => !item.permission || auth.hasPermission(item.permission))
	);
	const primaryNav = $derived(visibleNavItems.filter((item) => item.group === 'primary'));
	const moreNav = $derived(visibleNavItems.filter((item) => item.group === 'more'));

	function isActive(href: string): boolean {
		if (href === '/') return page.url.pathname === '/';
		return page.url.pathname.startsWith(href);
	}

	const moreActive = $derived(moreNav.some((item) => isActive(item.href)));
	const isAuthRoute = $derived(page.url.pathname.startsWith('/login'));

	const navLinkClass = (active: boolean) =>
		cn(
			'inline-flex min-h-9 cursor-pointer items-center gap-1.5 rounded-md px-3 py-2 text-sm no-underline transition-colors duration-200',
			active
				? 'bg-primary/15 font-medium text-primary'
				: 'text-muted-foreground hover:bg-muted/60 hover:text-foreground'
		);
</script>

<header class="sticky top-0 z-40 border-b border-border bg-background/90 backdrop-blur">
	<div class={cn('mx-auto flex h-14 items-center justify-between gap-4 px-4 sm:px-6', LAYOUT_MAX)}>
		<div class="flex min-w-0 items-center gap-4 lg:gap-6">
			<a
				href="/"
				class="flex shrink-0 cursor-pointer items-center gap-2 font-semibold no-underline hover:no-underline"
			>
				<AppBrand layout="inline" />
			</a>
			{#if auth.isAuthenticated}
				<nav class="hidden items-center gap-1 lg:flex" aria-label={t('nav.main')}>
					{#each primaryNav as item}
						<a
							href={item.href}
							aria-current={isActive(item.href) ? 'page' : undefined}
							class={navLinkClass(isActive(item.href))}
						>
							{t(item.labelKey)}
							{#if item.badge && statusStore.outboxPending > 0}
								<Badge tone="warning">{statusStore.outboxPending}</Badge>
							{/if}
						</a>
					{/each}
					{#if moreNav.length > 0}
						<DropdownMenu.Root>
							<DropdownMenu.Trigger
								class={cn(
									buttonVariants({ variant: 'ghost', size: 'sm' }),
									moreActive && 'bg-primary/15 text-primary'
								)}
								aria-label={t('nav.more')}
							>
								{t('nav.more')}
								<ChevronDownIcon class="size-3.5 opacity-70" />
							</DropdownMenu.Trigger>
							<DropdownMenu.Content align="start" class="min-w-44">
								{#each moreNav as item}
									<DropdownMenu.Item
										class={isActive(item.href) ? 'bg-muted font-medium' : ''}
										onclick={() => void goto(item.href)}
									>
										{t(item.labelKey)}
									</DropdownMenu.Item>
								{/each}
							</DropdownMenu.Content>
						</DropdownMenu.Root>
					{/if}
				</nav>
			{/if}
		</div>
		<div class="flex shrink-0 items-center gap-2">
			{#if auth.isAuthenticated}
				<OrgSwitcher />
				<HeaderMenu />
			{:else if !isAuthRoute}
				<Button href="/login" size="sm">{t('nav.signIn')}</Button>
			{/if}
		</div>
	</div>
	{#if auth.isAuthenticated}
		<nav
			class="flex gap-1.5 overflow-x-auto border-t border-border px-4 py-2 lg:hidden"
			aria-label={t('nav.mobile')}
		>
			{#each [...primaryNav, ...moreNav] as item}
				<a
					href={item.href}
					aria-current={isActive(item.href) ? 'page' : undefined}
					class="{navLinkClass(isActive(item.href))} shrink-0"
				>
					{t(item.labelKey)}
					{#if item.badge && statusStore.outboxPending > 0}
						<Badge tone="warning">{statusStore.outboxPending}</Badge>
					{/if}
				</a>
			{/each}
		</nav>
	{/if}
</header>
