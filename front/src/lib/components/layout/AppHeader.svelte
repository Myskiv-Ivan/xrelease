<script lang="ts">
	import { page } from '$app/state';
	import Badge from '#lib/components/kit/Badge.svelte';
	import Button from '#lib/components/kit/Button.svelte';
	import HeaderMenu from '#lib/components/layout/HeaderMenu.svelte';
	import OrgSwitcher from '#lib/components/layout/OrgSwitcher.svelte';
	import AppBrand from '#lib/components/layout/AppBrand.svelte';
	import { NAV_ITEMS } from '#lib/config/navigation.js';
	import { getStatusStore } from '#lib/data/status.svelte.js';
	import { t } from '#lib/i18n/index.js';
	import { getAuthState } from '#lib/stores/auth.svelte.js';
	import { cn } from '#lib/utils.js';

	const auth = getAuthState();
	const statusStore = getStatusStore();

	const visibleNavItems = $derived(
		NAV_ITEMS.filter((item) => !item.permission || auth.hasPermission(item.permission))
	);
	/** Sidebar has room — primary + more in one left rail (no overflow menu). */
	const railNav = $derived(visibleNavItems);
	const mobileNav = $derived(visibleNavItems);

	function isActive(href: string): boolean {
		if (href === '/') return page.url.pathname === '/';
		return page.url.pathname.startsWith(href);
	}

	const isAuthRoute = $derived(page.url.pathname.startsWith('/login'));

	const railLinkClass = (active: boolean) =>
		cn(
			'relative flex min-h-11 cursor-pointer items-center gap-2 rounded-md px-2.5 text-sm no-underline transition-colors duration-150',
			active
				? 'bg-sidebar-accent font-medium text-sidebar-accent-foreground'
				: 'text-sidebar-foreground/75 hover:bg-sidebar-accent/60 hover:text-sidebar-foreground'
		);

	const mobileLinkClass = (active: boolean) =>
		cn(
			'relative inline-flex min-h-11 cursor-pointer items-center gap-1.5 px-3 py-2 text-sm no-underline transition-colors duration-150',
			active
				? 'font-medium text-foreground after:absolute after:inset-x-2.5 after:bottom-1.5 after:h-0.5 after:rounded-full after:bg-primary'
				: 'text-muted-foreground hover:text-foreground'
		);
</script>

{#if isAuthRoute}
	<!-- Login shell: no chrome chrome — brand lives on the page. -->
{:else if auth.isAuthenticated}
	<!--
		Left titlebar / rail (lg+). Workbench chrome: brand + destinations left,
		utilities pinned to the rail foot. Mobile keeps a compact top strip.
	-->
	<aside
		data-app-rail
		class="fixed inset-y-0 left-0 z-40 hidden w-56 flex-col border-r border-sidebar-border bg-sidebar text-sidebar-foreground lg:flex"
		aria-label={t('nav.main')}
	>
		<div class="flex h-14 shrink-0 items-center border-b border-sidebar-border px-3">
			<a
				href="/"
				class="flex min-h-11 min-w-0 cursor-pointer items-center gap-2 font-semibold no-underline hover:no-underline"
			>
				<AppBrand layout="inline" />
			</a>
		</div>

		<nav class="flex flex-1 flex-col gap-0.5 overflow-y-auto px-2 py-3" aria-label={t('nav.main')}>
			{#each railNav as item (item.href)}
				<a
					href={item.href}
					aria-current={isActive(item.href) ? 'page' : undefined}
					class={railLinkClass(isActive(item.href))}
				>
					<span class="min-w-0 flex-1 truncate">{t(item.labelKey)}</span>
					{#if item.badge && statusStore.outboxPending > 0}
						<Badge tone="warning">{statusStore.outboxPending}</Badge>
					{/if}
				</a>
			{/each}
		</nav>

		<div
			class="mt-auto flex flex-col gap-2 border-t border-sidebar-border px-2 py-3"
		>
			<OrgSwitcher />
			<HeaderMenu />
		</div>
	</aside>

	<!-- Mobile / tablet top bar -->
	<header
		class="sticky top-0 z-40 border-b border-border/80 bg-background/95 backdrop-blur-md lg:hidden"
	>
		<div class="flex h-14 items-center justify-between gap-3 px-4">
			<a
				href="/"
				class="flex min-h-11 min-w-0 cursor-pointer items-center gap-2 font-semibold no-underline hover:no-underline"
			>
				<AppBrand layout="inline" />
			</a>
			<div class="flex shrink-0 items-center gap-2">
				<OrgSwitcher />
				<HeaderMenu />
			</div>
		</div>
		<nav
			data-mobile-nav
			class="flex gap-1.5 overflow-x-auto border-t border-border px-4 py-2"
			aria-label={t('nav.mobile')}
		>
			{#each mobileNav as item (item.href)}
				<a
					href={item.href}
					aria-current={isActive(item.href) ? 'page' : undefined}
					class="{mobileLinkClass(isActive(item.href))} shrink-0"
				>
					{t(item.labelKey)}
					{#if item.badge && statusStore.outboxPending > 0}
						<Badge tone="warning">{statusStore.outboxPending}</Badge>
					{/if}
				</a>
			{/each}
		</nav>
	</header>
{:else}
	<header class="sticky top-0 z-40 border-b border-border/80 bg-background/95 backdrop-blur-md">
		<div class="flex h-14 items-center justify-between gap-4 px-4 sm:px-6">
			<a
				href="/"
				class="flex min-h-11 cursor-pointer items-center gap-2 font-semibold no-underline hover:no-underline"
			>
				<AppBrand layout="inline" />
			</a>
			<Button href="/login">{t('nav.signIn')}</Button>
		</div>
	</header>
{/if}
