<script lang="ts">
	import { api } from '$lib/api/client';
	import type {
		ConfigApplyResponse,
		ConfigRevisionSummary,
		ConfigView,
		OrganizationConfigView
	} from '$lib/api/types';
	import Badge from '$lib/components/kit/Badge.svelte';
	import Button from '$lib/components/kit/Button.svelte';
	import KeyValueList from '$lib/components/kit/KeyValueList.svelte';
	import Panel from '$lib/components/kit/Panel.svelte';
	import ConfigEditPanel from '$lib/components/config/ConfigEditPanel.svelte';
	import ConfigRevisionsPanel from '$lib/components/config/ConfigRevisionsPanel.svelte';
	import * as Tabs from '$lib/components/kit/tabs';
	import { PAGE_STACK, TYPE_CODE, TYPE_HINT, TYPE_MUTED } from '$lib/components/kit/layout-styles';
	import { desiredSourceLabel } from '$lib/config/config-presentation';
	import { resolveApiError } from '$lib/core/errors';
	import { EMPTY_VALUE } from '$lib/core/format';
	import { cn } from '$lib/utils';
	import { t } from '$lib/i18n';
	import { getAuthState } from '$lib/stores/auth.svelte';
	import { getNetworkState } from '$lib/stores/network.svelte';
	import type { KeyValueItem } from '$lib/types/ui';

	interface Props {
		view: OrganizationConfigView;
		etag: string | null;
		revisions: ConfigRevisionSummary[];
		revisionsTotal: number;
		onApplied?: () => void | Promise<void>;
	}

	let { view, etag, revisions, revisionsTotal, onApplied }: Props = $props();

	const auth = getAuthState();
	const network = getNetworkState();

	let tab = $state('overview');
	let isBusy = $state(false);
	let statusMessage = $state<string | null>(null);
	let statusTone = $state<'success' | 'danger' | 'muted'>('muted');
	let applyResult = $state<ConfigApplyResponse | null>(null);

	const canWrite = $derived(auth.hasPermission('config:write', view.organization));
	const localAuthoritative = $derived(view.config_source === 'local');
	const canPush = $derived(view.accepts_pushed_config === true);
	/** Form editor only when ui_config + api_config + source=api (server-computed). */
	const uiEditingEnabled = $derived(Boolean(view.ui_config_enabled));

	const editorView = $derived.by((): ConfigView => {
		const desired =
			view.desired_content?.trim() ||
			(view.desired_source === 'empty' || canPush ? '{}\n' : '');
		return {
			content: desired,
			desired_content: desired,
			desired_source: view.desired_source,
			content_sha256: view.content_sha256,
			bootstrap_path: view.app_path ?? '',
			api_config_enabled: view.api_config_enabled ?? canPush,
			ui_config_enabled: view.ui_config_enabled,
			config_source: view.config_source,
			bootstrap_sections: []
		};
	});

	const provenanceItems = $derived<KeyValueItem[]>([
		{ label: t('org.orgId'), value: view.organization },
		{ label: t('org.orgName'), value: view.name },
		{ label: t('config.desiredSource'), value: desiredSourceLabel(view.desired_source) },
		{
			label: t('config.contentSha'),
			value: view.content_sha256 ? `${view.content_sha256.slice(0, 12)}…` : EMPTY_VALUE
		},
		{ label: t('org.appPath'), value: view.app_path ?? EMPTY_VALUE },
		{ label: t('org.acceptsPushed'), value: canPush, tone: canPush ? 'success' : 'default' }
	]);

	async function runRollback() {
		isBusy = true;
		statusMessage = null;
		applyResult = null;
		try {
			const result = await api.rollbackOrgConfig(view.organization);
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

<div class={PAGE_STACK}>
	<Tabs.Root bind:value={tab} class="gap-4">
		<Tabs.List variant="line" class="w-full justify-start">
			<Tabs.Trigger value="overview">{t('config.tabOverview')}</Tabs.Trigger>
			<Tabs.Trigger value="edit">{t('config.tabEdit')}</Tabs.Trigger>
		</Tabs.List>

		<Tabs.Content value="overview" class="flex flex-col gap-4">
			<Panel title={t('config.provenance')}>
				{#snippet actions()}
					<Badge tone={canPush ? 'success' : 'muted'} dot>
						{canPush ? t('config.configApiOn') : t('config.configApiOff')}
					</Badge>
				{/snippet}
				<KeyValueList items={provenanceItems} />
			</Panel>

			<Panel title={t('org.desiredDocument')}>
				{#if view.desired_content}
					<pre
						class={cn(
							'max-h-[min(70dvh,36rem)] overflow-auto rounded-md border border-border bg-muted/40 p-3 leading-relaxed whitespace-pre-wrap',
							TYPE_CODE
						)}>{view.desired_content}</pre>
				{:else}
					<p class={TYPE_HINT}>{t('config.desiredEmpty')}</p>
				{/if}
			</Panel>

			<ConfigRevisionsPanel {revisions} total={revisionsTotal} />

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
		</Tabs.Content>

		<Tabs.Content value="edit" class="flex flex-col gap-4">
			{#if localAuthoritative || !canPush}
				<Panel title={t('config.tabEdit')}>
					<p class={TYPE_HINT}>{t('org.editDisabledLocal')}</p>
				</Panel>
			{:else if !canWrite}
				<Panel title={t('config.tabEdit')}>
					<p class={TYPE_HINT}>{t('config.editDisabledRole')}</p>
				</Panel>
			{:else if uiEditingEnabled}
				<!-- Keyed by org: the panel keeps dirty drafts across refreshes, so
				     switching organizations must remount it — otherwise unsaved edits
				     from the previous org would be applied to the newly selected one. -->
				{#key view.organization}
					<ConfigEditPanel
						view={editorView}
						organization={view.organization}
						{etag}
						onApplied={onApplied}
					/>
				{/key}
			{:else}
				<Panel title={t('config.tabEdit')}>
					<p class={TYPE_HINT}>{t('config.editDisabledUi')}</p>
				</Panel>
			{/if}
		</Tabs.Content>
	</Tabs.Root>
</div>
