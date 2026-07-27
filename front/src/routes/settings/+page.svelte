<script lang="ts">
	import AuthSecurityPanel from '$lib/components/auth/AuthSecurityPanel.svelte';
	import UserCreatePanel from '$lib/components/auth/UserCreatePanel.svelte';
	import UsersListPanel from '$lib/components/auth/UsersListPanel.svelte';
	import DashboardShell from '$lib/components/layout/DashboardShell.svelte';
	import Panel from '$lib/components/kit/Panel.svelte';
	import Checkbox from '$lib/components/kit/Checkbox.svelte';
	import Select from '$lib/components/kit/Select.svelte';
	import {
		FIELD_GROUP,
		FIELD_STACK,
		PAGE_STACK,
		PANEL_GRID,
		PANEL_STACK
	} from '$lib/components/kit/layout-styles';
	import { fieldLabelClass } from '$lib/components/kit/surface-styles';
	import { t } from '$lib/i18n';
	import {
		getSettingsState,
		setAutoRefresh,
		setRefreshIntervalMs,
		setTheme,
		type ThemeMode
	} from '$lib/stores/settings.svelte';
	import { REFRESH_INTERVALS } from '$lib/core/constants';

	const settings = getSettingsState();

	/** Bumped on create so the users table reloads without a manual refresh. */
	let usersReloadToken = $state(0);
</script>

<!--
	The user directory is a full-width table, so it sits below the two-column
	preference grid rather than inside a half-width column.
-->
<DashboardShell title={t('settings.title')}>
	<div class={PAGE_STACK}>
		<div class={PANEL_GRID}>
			<div class={PANEL_STACK}>
				<AuthSecurityPanel />
			</div>

			<div class={PANEL_STACK}>
				<Panel title={t('settings.appearance')}>
					<div class={FIELD_STACK}>
						<div class={FIELD_GROUP}>
							<label for="theme" class={fieldLabelClass}>{t('settings.theme')}</label>
							<Select
								id="theme"
								value={settings.theme}
								onchange={(e) => setTheme(e.currentTarget.value as ThemeMode)}
							>
								<option value="dark">{t('theme.dark')}</option>
								<option value="light">{t('theme.light')}</option>
								<option value="system">{t('theme.system')}</option>
							</Select>
						</div>
					</div>
				</Panel>

				<Panel title={t('settings.refresh')}>
					<div class={FIELD_STACK}>
						<label class="flex min-h-9 cursor-pointer items-center gap-2.5">
							<Checkbox
								checked={settings.autoRefresh}
								onchange={(e) => setAutoRefresh(e.currentTarget.checked)}
							/>
							<span class={fieldLabelClass}>{t('common.autoRefresh')}</span>
						</label>
						<div class={FIELD_GROUP}>
							<label for="interval" class={fieldLabelClass}>{t('settings.interval')}</label>
							<Select
								id="interval"
								value={settings.refreshIntervalMs}
								disabled={!settings.autoRefresh}
								onchange={(e) => setRefreshIntervalMs(Number(e.currentTarget.value))}
							>
								{#each REFRESH_INTERVALS as option}
									<option value={option.value}>{option.label}</option>
								{/each}
							</Select>
						</div>
					</div>
				</Panel>
			</div>
		</div>

		<UsersListPanel reloadToken={usersReloadToken} />
		<UserCreatePanel onCreated={() => (usersReloadToken += 1)} />
	</div>
</DashboardShell>
