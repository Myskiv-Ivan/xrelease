<script lang="ts">
	import Badge from '#lib/components/kit/Badge.svelte';
	import Select from '#lib/components/kit/Select.svelte';
	import { TYPE_MUTED } from '#lib/components/kit/layout-styles.js';
	import { getOutboxStore } from '#lib/data/outbox.svelte.js';
	import { getSourcesStore } from '#lib/data/sources.svelte.js';
	import { getTeamsStore } from '#lib/data/teams.svelte.js';
	import { t } from '#lib/i18n/index.js';
	import { getAuthState } from '#lib/stores/auth.svelte.js';
	import { getOrganizationsState } from '#lib/stores/organizations.svelte.js';

	const orgs = getOrganizationsState();
	const auth = getAuthState();
	const sourcesStore = getSourcesStore();
	const outboxStore = getOutboxStore();
	const teamsStore = getTeamsStore();

	const roleHint = $derived(
		orgs.selectedId && auth.organizationRoles[orgs.selectedId]
			? t('org.scopedRoleHint')
			: t('org.switcherHint')
	);

	function onOrgChange(event: Event) {
		const value = (event.currentTarget as HTMLSelectElement).value;
		orgs.select(value);
		// Server-side `?organization=` is baked into store fetchers — reload now.
		void sourcesStore.reload();
		void outboxStore.reload();
		void teamsStore.reload();
	}
</script>

{#if orgs.isMultiOrg}
	<!-- Rail: stacked label + full-width select. Mobile top bar: compact row. -->
	<label class="flex w-full flex-col gap-1 lg:gap-1.5" title={roleHint}>
		<span class="hidden {TYPE_MUTED} lg:inline">{t('org.switcher')}</span>
		<span class="sr-only lg:hidden">{t('org.switcher')}</span>
		<div class="flex min-w-0 items-center gap-2">
			<Select
				class="min-w-0 w-full max-w-[12rem] lg:max-w-none"
				value={orgs.selectedId ?? ''}
				aria-label={t('org.switcher')}
				onchange={onOrgChange}
			>
				{#each orgs.organizations as org (org.id)}
					<option value={org.id}>
						{org.name}
						{#if auth.organizationRoles[org.id]}
							({auth.roleLabel(org.id)})
						{/if}
					</option>
				{/each}
			</Select>
			<span class="hidden shrink-0 sm:inline lg:hidden">
				<Badge tone="muted">{auth.roleLabel(orgs.selectedId)}</Badge>
			</span>
		</div>
		<span class="hidden lg:inline">
			<Badge tone="muted">{auth.roleLabel(orgs.selectedId)}</Badge>
		</span>
	</label>
{/if}
