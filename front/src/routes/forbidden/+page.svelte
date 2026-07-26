<script lang="ts">
	import PageHeader from '$lib/components/layout/PageHeader.svelte';
	import Button from '$lib/components/kit/Button.svelte';
	import Panel from '$lib/components/kit/Panel.svelte';
	import { FIELD_STACK } from '$lib/components/kit/layout-styles';
	import { t } from '$lib/i18n';
</script>

<div class="mx-auto flex w-full max-w-lg flex-col gap-6">
	<PageHeader title={t('auth.forbiddenTitle')} description={t('auth.forbiddenDescription')} />
	<Panel>
		<div class={FIELD_STACK}>
			<Button href="/" variant="outline" class="w-full sm:w-auto">{t('auth.backHome')}</Button>
		</div>
	</Panel>
</div>
