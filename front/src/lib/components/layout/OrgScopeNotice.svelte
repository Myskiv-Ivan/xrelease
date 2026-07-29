<script lang="ts">
	import { TYPE_HINT } from '$lib/components/kit/layout-styles';
	import { t } from '$lib/i18n';
	import { getOrganizationsState } from '$lib/stores/organizations.svelte';

	const orgs = getOrganizationsState();
</script>

<!--
	Tells the operator that a table is narrowed by the header switcher. Rendered
	by every org-scoped list page, so the wording and placement cannot drift
	between Sources and Outbox.
-->
{#if orgs.isMultiOrg && orgs.selected}
	<p class="mb-3 {TYPE_HINT}">
		{t('org.filterActive').replace('{org}', orgs.selected.name)}
		<span> — {t('org.filterAllHint')}</span>
	</p>
{/if}
