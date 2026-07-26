<script lang="ts">
	import DashboardShell from '$lib/components/layout/DashboardShell.svelte';
	import Panel from '$lib/components/kit/Panel.svelte';
	import ResourceLinkList from '$lib/components/kit/ResourceLinkList.svelte';
	import { PAGE_STACK, TYPE_BODY } from '$lib/components/kit/layout-styles';
	import { getSystemApiLinks, INTERNAL_API_LINKS } from '$lib/config/api-links';
	import { t } from '$lib/i18n';

	const systemLinks = getSystemApiLinks();
</script>

<DashboardShell title={t('about.title')} permission="about:read">
	<div class={PAGE_STACK}>
		<Panel title={t('about.overview')}>
			<p class={TYPE_BODY}>{t('about.gitopsNote')}</p>
		</Panel>

		<Panel title={t('about.api')}>
			<ResourceLinkList links={INTERNAL_API_LINKS} />
		</Panel>

		<Panel title={t('about.links')}>
			<ResourceLinkList links={systemLinks} />
		</Panel>
	</div>
</DashboardShell>
