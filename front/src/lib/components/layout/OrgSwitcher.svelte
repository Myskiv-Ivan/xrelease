<script lang="ts">
	import Select from '$lib/components/kit/Select.svelte';
	import { TYPE_MUTED } from '$lib/components/kit/layout-styles';
	import { getOutboxStore } from '$lib/data/outbox.svelte';
	import { getSourcesStore } from '$lib/data/sources.svelte';
	import { getTeamsStore } from '$lib/data/teams.svelte';
	import { t } from '$lib/i18n';
	import { getAuthState } from '$lib/stores/auth.svelte';
	import { getOrganizationsState } from '$lib/stores/organizations.svelte';

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
	<label class="flex items-center gap-2" title={roleHint}>
		<span class="hidden {TYPE_MUTED} xl:inline">{t('org.switcher')}</span>
		<span class="sr-only xl:hidden">{t('org.switcher')}</span>
		<Select
			class="min-w-[9rem] max-w-[12rem]"
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
		<span class="hidden rounded-md border border-border px-1.5 py-0.5 text-[0.65rem] uppercase tracking-wide text-muted-foreground sm:inline">
			{auth.roleLabel(orgs.selectedId)}
		</span>
	</label>
{/if}
