<script lang="ts">
	import type { ConfigApplyResponse, ConfigView } from '$lib/api/types';
	import { api } from '$lib/api/client';
	import Badge from '$lib/components/kit/Badge.svelte';
	import Button from '$lib/components/kit/Button.svelte';
	import KeyValueList from '$lib/components/kit/KeyValueList.svelte';
	import Panel from '$lib/components/kit/Panel.svelte';
	import {
		configApiEnabled,
		configProvenanceItems
	} from '$lib/config/config-presentation';
	import { resolveApiError } from '$lib/core/errors';
	import { TYPE_CODE, TYPE_MUTED } from '$lib/components/kit/layout-styles';
	import { t } from '$lib/i18n';
	import { getAuthState } from '$lib/stores/auth.svelte';
	import { getNetworkState } from '$lib/stores/network.svelte';
	import { getNowStore } from '$lib/stores/now.svelte';

	interface Props {
		view: ConfigView;
		onApplied?: () => void | Promise<void>;
	}

	let { view, onApplied }: Props = $props();

	const auth = getAuthState();
	const network = getNetworkState();
	const now = getNowStore();

	let isBusy = $state(false);
	let statusMessage = $state<string | null>(null);
	let statusTone = $state<'success' | 'danger' | 'muted'>('muted');
	let applyResult = $state<ConfigApplyResponse | null>(null);

	const enabled = $derived(configApiEnabled(view));
	const canWrite = $derived(auth.hasPermission('config:write'));
	const uiEditingEnabled = $derived(Boolean(view.ui_config_enabled));
	const canPush = $derived(view.config_source !== 'local' && enabled);
	const items = $derived(
		configProvenanceItems(
			{
				desired_source: view.desired_source,
				revision: view.revision,
				revision_label: view.revision_label,
				applied_at: view.applied_at,
				content_sha256: view.content_sha256,
				bootstrap_path: view.bootstrap_path,
				bootstrap_sections: view.bootstrap_sections
			},
			now.current
		)
	);

	async function runRollback() {
		isBusy = true;
		statusMessage = null;
		applyResult = null;
		try {
			const result = await api.rollbackConfig();
			applyResult = result;
			statusTone = 'success';
			statusMessage = t('config.rollbackOk');
			await onApplied?.();
		} catch (err) {
			statusTone = 'danger';
			statusMessage = resolveApiError(err, t('errors.rollbackConfig'));
		} finally {
			isBusy = false;
		}
	}
</script>

<Panel title={t('config.provenance')}>
	{#snippet actions()}
		<Badge tone={enabled ? 'success' : 'muted'} dot>
			{enabled ? t('config.configApiOn') : t('config.configApiOff')}
		</Badge>
	{/snippet}
	<KeyValueList {items} />
</Panel>

{#if canPush && canWrite && uiEditingEnabled}
	<!-- Rollback mutates desired state, so it honors `ui_config` exactly
	     like the editor: mode 2 (API/xrctl-only) keeps browsers read-only. -->
	<Panel title={t('org.rollback')}>
		<div class="flex flex-wrap items-center gap-2">
			{#if statusMessage}
				<Badge tone={statusTone} dot>{statusMessage}</Badge>
			{/if}
			{#if applyResult}
				<span class="{TYPE_CODE} {TYPE_MUTED}">
					#{applyResult.revision} · {applyResult.content_sha256.slice(0, 12)}…
				</span>
			{/if}
			<Button
				variant="danger"
				size="sm"
				disabled={isBusy || !network.isOnline}
				onclick={runRollback}
			>
				{t('org.rollback')}
			</Button>
		</div>
	</Panel>
{/if}
