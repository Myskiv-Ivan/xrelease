<script lang="ts">
	import { getDisplayName } from '$lib/auth/session';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu/index.js';
	import { buttonVariants } from '$lib/components/ui/button/button.svelte';
	import { TYPE_MUTED } from '$lib/components/kit/layout-styles';
	import { t } from '$lib/i18n';
	import { getAuthState, logout } from '$lib/stores/auth.svelte';
	import { getOrganizationsState } from '$lib/stores/organizations.svelte';
	import { getSettingsState, setTheme, type ThemeMode } from '$lib/stores/settings.svelte';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import MonitorIcon from '@lucide/svelte/icons/monitor';
	import MoonIcon from '@lucide/svelte/icons/moon';
	import SunIcon from '@lucide/svelte/icons/sun';

	const auth = getAuthState();
	const orgs = getOrganizationsState();
	const settings = getSettingsState();

	const themes: Array<{ value: ThemeMode; labelKey: string; icon: typeof MoonIcon }> = [
		{ value: 'dark', labelKey: 'theme.dark', icon: MoonIcon },
		{ value: 'light', labelKey: 'theme.light', icon: SunIcon },
		{ value: 'system', labelKey: 'theme.system', icon: MonitorIcon }
	];

	const displayName = $derived(getDisplayName(auth.profile));
	const roleLabel = $derived(
		orgs.isMultiOrg && orgs.selectedId
			? auth.roleLabel(orgs.selectedId)
			: auth.roleLabel()
	);
	const triggerLabel = $derived(displayName || roleLabel || t('nav.account'));
</script>

<DropdownMenu.Root>
	<DropdownMenu.Trigger
		class={buttonVariants({ variant: 'outline', size: 'sm' })}
		aria-label={t('nav.account')}
		title={t('nav.account')}
	>
		<span class="max-w-[8rem] truncate">{triggerLabel}</span>
		<ChevronDownIcon class="size-3.5 opacity-70" />
	</DropdownMenu.Trigger>
	<DropdownMenu.Content align="end" class="min-w-52">
		{#if auth.profile}
			<DropdownMenu.Label class="font-normal">
				<div class="flex flex-col gap-0.5">
					{#if displayName}
						<span class="font-medium text-foreground">{displayName}</span>
					{/if}
					<span class={TYPE_MUTED}>{roleLabel}</span>
				</div>
			</DropdownMenu.Label>
			<DropdownMenu.Separator />
		{/if}

		<DropdownMenu.Label>{t('settings.appearance')}</DropdownMenu.Label>
		<DropdownMenu.RadioGroup
			value={settings.theme}
			onValueChange={(value) => {
				if (value === 'dark' || value === 'light' || value === 'system') {
					setTheme(value);
				}
			}}
		>
			{#each themes as theme (theme.value)}
				{@const Icon = theme.icon}
				<DropdownMenu.RadioItem value={theme.value} class="gap-2">
					<Icon class="size-3.5" />
					{t(theme.labelKey)}
				</DropdownMenu.RadioItem>
			{/each}
		</DropdownMenu.RadioGroup>

		{#if auth.isAuthenticated}
			<DropdownMenu.Separator />
			<DropdownMenu.Item
				variant="destructive"
				onclick={() => void logout()}
			>
				{t('nav.signOut')}
			</DropdownMenu.Item>
		{/if}
	</DropdownMenu.Content>
</DropdownMenu.Root>
