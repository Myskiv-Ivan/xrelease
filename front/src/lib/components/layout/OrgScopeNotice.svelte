<script lang="ts">
	import { TYPE_HINT } from '#lib/components/kit/layout-styles.js';
	import { t } from '#lib/i18n/index.js';
	import { getOrganizationsState } from '#lib/stores/organizations.svelte.js';

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
