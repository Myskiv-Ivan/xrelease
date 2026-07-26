<script lang="ts">
	import { api, ApiClientError } from '$lib/api/client';
	import type { ConfigApplyResponse, ConfigView } from '$lib/api/types';
	import Badge from '$lib/components/kit/Badge.svelte';
	import Button from '$lib/components/kit/Button.svelte';
	import Checkbox from '$lib/components/kit/Checkbox.svelte';
	import Input from '$lib/components/kit/Input.svelte';
	import Panel from '$lib/components/kit/Panel.svelte';
	import Select from '$lib/components/kit/Select.svelte';
		import ConfigNotifiersPanel from '$lib/components/config/ConfigNotifiersPanel.svelte';
	import {
				contentTypeForFormat,
				emptySourceDraft,
				emptyTeamDraft,
				parseDesiredDocument,
				primaryFieldsForKind,
				readDefaults,
				readNotifiers,
				readSources,
				readTeams,
				sourceSupportsToken,
				stringifyDesiredDocument,
				writeDefaults,
				writeNotifiers,
				writeSources,
				writeTeams,
				type DesiredDefaultsDraft,
				type DesiredNotifierDraft,
				type DesiredSourceDraft,
				type DesiredTeamDraft,
				type ParsedDesiredDocument
			} from '$lib/config/desired-document';
		import { firstIssueMessage, validateDesiredDrafts } from '$lib/config/desired-validation';
		import { groupIndexedByKind } from '$lib/config/editor-groups';
		import {
			fieldsFromReleasePatternTemplate,
			releasePatternTemplateByName
		} from '$lib/config/release-pattern-templates';
		import { listSourceKindValues, getSourceKindMeta } from '$lib/config/source-kinds';
		import { sourceFieldHint, sourceFieldLabel } from '$lib/config/field-labels';
	import { TYPE_CODE, TYPE_MUTED, TYPE_HINT, TYPE_OVERLINE } from '$lib/components/kit/layout-styles';
	import { fieldLabelClass } from '$lib/components/kit/surface-styles';
	import { resolveApiError } from '$lib/core/errors';
	import { getConfigSchemaStore } from '$lib/data/config-schema.svelte';
	import { t } from '$lib/i18n';
	import { getAuthState } from '$lib/stores/auth.svelte';
	import { getNetworkState } from '$lib/stores/network.svelte';
	import { onMount } from 'svelte';

	interface Props {
		view: ConfigView;
		etag: string | null;
		/** When set, validate/apply target `/organizations/{id}/config` (multi-org). */
		organization?: string | null;
		onApplied?: () => void | Promise<void>;
	}

	let { view, etag, organization = null, onApplied }: Props = $props();

	const auth = getAuthState();
	const network = getNetworkState();
	const schemaStore = getConfigSchemaStore();

	let parsed = $state<ParsedDesiredDocument | null>(null);
	let parseError = $state<string | null>(null);
	let defaults = $state<DesiredDefaultsDraft | null>(null);
	let teams = $state<DesiredTeamDraft[]>([]);
	let sources = $state<DesiredSourceDraft[]>([]);
	let notifiers = $state<DesiredNotifierDraft[]>([]);
	let baseline = $state<string>('');
	let loadedSha = $state<string | null>(null);
	let revisionLabel = $state('');
	let isBusy = $state(false);
	let statusMessage = $state<string | null>(null);
	let statusTone = $state<'success' | 'danger' | 'muted'>('muted');
	let applyResult = $state<ConfigApplyResponse | null>(null);
	/** Server validate/apply report errors (shown under the toolbar). */
	let serverReportErrors = $state<string[]>([]);
	/** Per-source expand; missing key = collapsed. */
	let sourceOpenByKey = $state<Record<string, boolean>>({});
	/** Bumps after apply so Delivery channels reloads live sinks for Test. */
	let notifiersLiveEpoch = $state(0);

	const canWrite = $derived(
		organization
			? auth.hasPermission('config:write', organization)
			: auth.hasPermission('config:write')
	);
	const apiEnabled = $derived(view.api_config_enabled);
	/** Editor is opt-in separately from API apply (API-only / xrctl without UI). */
	const uiEditingEnabled = $derived(Boolean(view.ui_config_enabled));
	const localAuthoritative = $derived(view.config_source === 'local');
	const kinds = $derived(
		schemaStore.sourceKinds.length > 0
			? schemaStore.sourceKinds.map((k) => k.value)
			: listSourceKindValues()
	);

	const documentData = $derived.by(() => {
		if (!parsed || !defaults) return null;
		const withDefaults = writeDefaults(parsed.data, defaults);
		const withTeams = writeTeams(withDefaults, teams);
		const withSources = writeSources(withTeams, sources);
		return writeNotifiers(withSources, notifiers);
	});

	const documentText = $derived.by(() => {
		if (!parsed || !documentData) return '';
		return stringifyDesiredDocument({ format: parsed.format, data: documentData });
	});

	const isDirty = $derived(Boolean(baseline) && documentText !== baseline);

	const validation = $derived.by(() => {
		if (!defaults || !parsed || !documentData) {
			return { ok: true, errors: [], warnings: [] };
		}
		return validateDesiredDrafts({
			defaults,
			teams,
			sources,
			notifiers,
			data: documentData
		});
	});

	// Deduped: a half-typed catalogue can hold the same tag twice (validation
	// flags it, but the form still renders), and lists below key on the tag.
	const teamOptions = $derived([
		...new Map(
			teams
				.filter((team) => team.tag.trim())
				.map((team) => [team.tag.trim(), { tag: team.tag.trim(), name: team.name.trim() }] as const)
		).values()
	]);

	const teamTags = $derived(teamOptions.map((team) => team.tag));

	const sourceGroups = $derived(groupIndexedByKind(sources, (source) => source.type, kinds));

	/** Schema `builtin_presets` is the SSOT; user `preset_names` append as extras. */
	const patternTemplates = $derived.by(() => {
		const fromSchema = schemaStore.schema?.builtin_presets ?? [];
		const names = schemaStore.schema?.preset_names ?? [];
		const base = fromSchema.map((entry) => ({
			name: entry.name,
			description: entry.description || entry.name
		}));
		const seen = new Set(base.map((entry) => entry.name));
		const extras = names
			.filter((name) => name && !seen.has(name))
			.map((name) => ({
				name,
				description: name
			}));
		return [...base, ...extras];
	});

	function applyPatternTemplate(source: DesiredSourceDraft, name: string) {
		if (!name) {
			source.preset = '';
			sources = [...sources];
			return;
		}
		const schemaPresets = schemaStore.schema?.builtin_presets ?? [];
		const template = releasePatternTemplateByName(schemaPresets, name);
		if (!template) {
			// User-defined preset name — keep fields as-is, only stamp the name.
			source.preset = name;
			sources = [...sources];
			return;
		}
		const fields = fieldsFromReleasePatternTemplate(template);
		source.preset = fields.preset;
		source.pattern = fields.pattern;
		source.exclude_pattern = fields.exclude_pattern;
		source.include_prerelease = fields.include_prerelease;
		source.prerelease_tags = fields.prerelease_tags;
		source.exclude_updated = fields.exclude_updated;
		sources = [...sources];
	}

	function fieldError(path: string): string | null {
		return firstIssueMessage(validation.errors, path);
	}

	function fieldWarning(path: string): string | null {
		return firstIssueMessage(validation.warnings, path);
	}

	function fieldPlaceholder(key: string, type: string): string {
		switch (key) {
			case 'repo':
				return t('config.placeholderRepo');
			case 'project':
				return t('config.placeholderProject');
			case 'image':
				return t('config.placeholderImage');
			case 'name':
				return type === 'maven' ? t('config.placeholderMaven') : t('config.placeholderName');
			case 'url':
				return t('config.placeholderUrl');
			case 'host':
				return t('config.placeholderHost');
			case 'registry':
				return t('config.placeholderRegistry');
			default:
				return '';
		}
	}

	function parseNumberInput(raw: string, fallback: number | ''): number | '' {
		if (raw.trim() === '') return '';
		const n = Number(raw);
		return Number.isFinite(n) ? n : fallback;
	}

	function loadFromView() {
		parseError = null;
		statusMessage = null;
		applyResult = null;
		loadedSha = view.content_sha256 ?? null;
		// UI-first idle: empty/missing document → seed `{}` so the form editor works.
		const raw =
			view.desired_content?.trim() ||
			(view.ui_config_enabled && view.config_source === 'api' ? '{}\n' : '');
		if (!raw) {
			parsed = null;
			defaults = null;
			teams = [];
			sources = [];
			notifiers = [];
			baseline = '';
			return;
		}
		try {
			const next = parseDesiredDocument(raw);
			parsed = next;
			defaults = readDefaults(next.data);
			teams = readTeams(next.data);
			sources = readSources(next.data);
			notifiers = readNotifiers(next.data);
			// Baseline runs the same write pipeline, so a freshly loaded document is
			// never reported dirty just because the round-trip normalises key order.
			baseline = stringifyDesiredDocument({
				format: next.format,
				data: writeNotifiers(
					writeSources(writeTeams(writeDefaults(next.data, defaults), teams), sources),
					notifiers
				)
			});
		} catch (err) {
			parsed = null;
			defaults = null;
			teams = [];
			sources = [];
			notifiers = [];
			baseline = '';
			parseError = err instanceof Error ? err.message : t('config.editParseError');
		}
	}

	onMount(() => {
		// Schema store is owned by +layout for authenticated sessions.
		loadFromView();
	});

	$effect(() => {
		// Reload drafts when the server sha changes — never clobber unsaved edits.
		const sha = view.content_sha256 ?? '';
		if (sha === (loadedSha ?? '')) return;
		if (isDirty) return;
		loadFromView();
	});

	function addTeam() {
		teams = [...teams, emptyTeamDraft()];
	}

	function removeTeam(key: string) {
		teams = teams.filter((team) => team.key !== key);
	}

	function addSource() {
		const type = kinds[0] ?? 'github';
		const draft = emptySourceDraft(type);
		sources = [...sources, draft];
		sourceOpenByKey = { ...sourceOpenByKey, [draft.key]: true };
	}

	function isSourceOpen(key: string): boolean {
		return sourceOpenByKey[key] === true;
	}

	function toggleSourceOpen(key: string) {
		sourceOpenByKey = { ...sourceOpenByKey, [key]: !isSourceOpen(key) };
	}

	function removeSource(key: string) {
		sources = sources.filter((source) => source.key !== key);
	}

		function changeSourceType(source: DesiredSourceDraft, type: string) {
			const fields: Record<string, string> = {};
			for (const field of primaryFieldsForKind(type)) {
				fields[field.key] = source.fields[field.key] ?? '';
			}
			source.type = type;
			source.fields = fields;
			if (!sourceSupportsToken(type)) {
				source.token = '';
				source.token_env = '';
				source.redacted = source.redacted.filter((key) => key !== 'token');
			}
			sources = [...sources];
		}

	async function runValidateOrApply(apply: boolean) {
		if (!parsed || !documentText.trim()) return;
		if (apply && !validation.ok) {
			statusTone = 'danger';
			statusMessage = t('config.clientValidationBlocked');
			return;
		}
		isBusy = true;
		statusMessage = null;
		applyResult = null;
		serverReportErrors = [];
		const contentType = contentTypeForFormat(parsed.format);

		try {
			if (organization) {
				await api.validateOrgConfig(organization, documentText, contentType);
			} else {
				await api.validateConfig(documentText, contentType);
			}
			if (!apply) {
				statusTone = 'success';
				statusMessage = t('config.validateOk');
				return;
			}

			const result = organization
				? await api.applyOrgConfig(organization, documentText, {
						contentType,
						ifMatch: etag ?? view.content_sha256,
						revisionLabel: revisionLabel.trim() || undefined
					})
				: await api.applyConfig(documentText, {
						contentType,
						ifMatch: etag ?? view.content_sha256,
						revisionLabel: revisionLabel.trim() || undefined
					});
			applyResult = result;
			statusTone = 'success';
			statusMessage = result.applied ? t('config.applyOk') : t('config.applyIdempotent');
			baseline = documentText;
			loadedSha = result.content_sha256;
			notifiersLiveEpoch += 1;
			await onApplied?.();
		} catch (err) {
			statusTone = 'danger';
			if (err instanceof ApiClientError && err.status === 412) {
				statusMessage = err.message;
			} else if (err instanceof ApiClientError) {
				const report = (
					err.body as { report?: { errors?: string[]; warnings?: string[] } } | undefined
				)?.report;
				const firstError = report?.errors?.[0];
				statusMessage = firstError
					? firstError
					: resolveApiError(err, apply ? t('errors.applyConfig') : t('errors.validateConfig'));
				if (report?.errors?.length) {
					serverReportErrors = report.errors;
				} else {
					serverReportErrors = [];
				}
			} else {
				statusMessage = resolveApiError(
					err,
					apply ? t('errors.applyConfig') : t('errors.validateConfig')
				);
				serverReportErrors = [];
			}
		} finally {
			isBusy = false;
		}
	}
</script>

{#if !apiEnabled}
	<Panel title={t('config.tabEdit')}>
		<p class={TYPE_HINT}>{t('config.editDisabledApi')}</p>
	</Panel>
{:else if localAuthoritative}
	<Panel title={t('config.tabEdit')}>
		<p class={TYPE_HINT}>{t('config.editDisabledLocal')}</p>
	</Panel>
{:else if !uiEditingEnabled}
	<Panel title={t('config.tabEdit')}>
		<p class={TYPE_HINT}>{t('config.editDisabledUi')}</p>
	</Panel>
{:else if !canWrite}
	<Panel title={t('config.tabEdit')}>
		<p class={TYPE_HINT}>{t('config.editDisabledRole')}</p>
	</Panel>
	{:else if !view.desired_content && !(view.ui_config_enabled && view.config_source === 'api')}
		<Panel title={t('config.tabEdit')}>
			<p class={TYPE_HINT}>{t('config.editNoDesired')}</p>
		</Panel>
	{:else if parseError || !defaults || !parsed}
	<Panel title={t('config.tabEdit')}>
		<p class="text-sm text-destructive">{parseError ?? t('config.editParseError')}</p>
	</Panel>
{:else}
	<div class="flex flex-col gap-4">
		<div class="flex flex-wrap items-center gap-2">
			{#if isDirty}
				<Badge tone="warning" dot>{t('config.dirty')}</Badge>
			{/if}
			{#if validation.errors.length > 0}
				<Badge tone="danger" dot
					>{validation.errors.length} {t('config.clientValidation')}</Badge
				>
			{:else if validation.warnings.length > 0}
				<Badge tone="warning" dot
					>{validation.warnings.length} warn · {t('config.clientValidationOk')}</Badge
				>
			{:else}
				<Badge tone="success" dot>{t('config.clientValidationOk')}</Badge>
			{/if}
			{#if statusMessage}
				<Badge tone={statusTone} dot>{statusMessage}</Badge>
			{/if}
			{#if applyResult}
				<span class="{TYPE_CODE} {TYPE_MUTED}">
					#{applyResult.revision} · {applyResult.content_sha256.slice(0, 12)}…
				</span>
				{#if applyResult.sources_added?.length}
					<span class={TYPE_MUTED}>
						{t('config.diffAdded')}: {applyResult.sources_added.join(', ')}
					</span>
				{/if}
				{#if applyResult.sources_removed?.length}
					<span class={TYPE_MUTED}>
						{t('config.diffRemoved')}: {applyResult.sources_removed.join(', ')}
					</span>
				{/if}
				{#if applyResult.sources_changed?.length}
					<span class={TYPE_MUTED}>
						{t('config.diffChanged')}: {applyResult.sources_changed.join(', ')}
					</span>
				{/if}
			{/if}
			<div class="ml-auto flex flex-wrap items-center gap-2">
				<Button variant="ghost" size="sm" disabled={!isDirty || isBusy} onclick={loadFromView}>
					{t('config.discard')}
				</Button>
				<Button
					variant="outline"
					size="sm"
					disabled={isBusy || !network.isOnline}
					onclick={() => runValidateOrApply(false)}
				>
					{isBusy ? t('config.validating') : t('config.validateOnly')}
				</Button>
				<Button
					size="sm"
					disabled={isBusy || !network.isOnline || !isDirty || !validation.ok}
					onclick={() => runValidateOrApply(true)}
				>
					{isBusy ? t('config.applying') : t('config.apply')}
				</Button>
			</div>
		</div>

		{#if validation.errors.length > 0 || validation.warnings.length > 0}
			<ul class="rounded-md border border-border bg-muted/20 px-3 py-2 text-sm">
				{#each [...validation.errors, ...validation.warnings].slice(0, 12) as issue (issue.path + issue.message)}
					<li class={issue.severity === 'error' ? 'text-destructive' : 'text-warning'}>
						<span class="{TYPE_CODE} opacity-70">{issue.path}</span>
						— {issue.message}
					</li>
				{/each}
			</ul>
		{/if}
		{#if serverReportErrors.length > 0}
			<ul class="rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-sm text-destructive">
				{#each serverReportErrors.slice(0, 12) as message (message)}
					<li>{message}</li>
				{/each}
			</ul>
		{/if}

		<label class="flex max-w-md flex-col gap-1 text-sm">
			<span class={fieldLabelClass}>{t('config.revisionLabelField')}</span>
			<Input
				bind:value={revisionLabel}
				placeholder={t('config.revisionLabelPlaceholder')}
				disabled={isBusy}
			/>
		</label>

		<Panel title={t('config.defaults')}>
			<p class="mb-3 {TYPE_HINT}">{t('config.defaultsHint')}</p>
			<div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
				<label class="flex flex-col gap-1 text-sm">
					<span class={fieldLabelClass}>{t('config.intervalSecs')}</span>
					<Input
						type="number"
						value={String(defaults.interval_secs)}
						disabled={isBusy}
						oninput={(e) => {
							defaults = {
								...defaults!,
								interval_secs: parseNumberInput(e.currentTarget.value, defaults!.interval_secs)
							};
						}}
					/>
				</label>
				<label class="flex flex-col gap-1 text-sm">
					<span class={fieldLabelClass}>{t('config.jitterSecs')}</span>
					<Input
						type="number"
						value={String(defaults.jitter_secs)}
						disabled={isBusy}
						oninput={(e) => {
							defaults = {
								...defaults!,
								jitter_secs: parseNumberInput(e.currentTarget.value, defaults!.jitter_secs)
							};
						}}
					/>
				</label>
				<label class="flex flex-col gap-1 text-sm">
					<span class={fieldLabelClass}>{t('config.upstreamRpm')}</span>
					<Input
						type="number"
						value={String(defaults.upstream_requests_per_minute)}
						disabled={isBusy}
						oninput={(e) => {
							defaults = {
								...defaults!,
								upstream_requests_per_minute: parseNumberInput(
									e.currentTarget.value,
									defaults!.upstream_requests_per_minute
								)
							};
						}}
					/>
				</label>
				<label class="flex flex-col gap-1 text-sm sm:col-span-2">
					<span class={fieldLabelClass}>{t('config.notifySchedule')}</span>
					<Input bind:value={defaults.notify_schedule} disabled={isBusy} />
				</label>
				<label class="flex flex-col gap-1 text-sm sm:col-span-2">
					<span class={fieldLabelClass}>{t('config.opsRoutingTag')}</span>
					<Input
						bind:value={defaults.ops_routing_tag}
						placeholder={t('config.opsRoutingTagPlaceholder')}
						disabled={isBusy}
					/>
					{#if fieldError('defaults.ops_routing_tag')}
						<span class="text-xs text-destructive">{fieldError('defaults.ops_routing_tag')}</span>
					{:else if fieldWarning('defaults.ops_routing_tag')}
						<span class="text-xs text-warning">{fieldWarning('defaults.ops_routing_tag')}</span>
					{/if}
				</label>
				<label class="flex items-center gap-2 text-sm">
					<Checkbox
						checked={defaults.poll_on_startup}
						disabled={isBusy}
						onCheckedChange={(checked) => {
							defaults = { ...defaults!, poll_on_startup: checked };
						}}
					/>
					<span>{t('config.pollOnStartup')}</span>
				</label>
			</div>
		</Panel>

		<Panel title={t('config.teams')}>
			{#snippet actions()}
				<Button variant="outline" size="sm" disabled={isBusy} onclick={addTeam}>
					{t('config.addTeam')}
				</Button>
			{/snippet}
			<p class="mb-3 {TYPE_HINT}">{t('config.teamsHint')}</p>
			{#if teams.length === 0}
				<p class={TYPE_HINT}>{t('diagnostics.teamsEmpty')}</p>
			{:else}
				<div class="flex flex-col gap-2">
					{#each teams as team, index (team.key)}
						<div class="grid gap-2 rounded-md border border-border p-3 sm:grid-cols-[1fr_1fr_auto]">
							<label class="flex flex-col gap-1 text-sm">
								<span class={fieldLabelClass}>{t('config.teamTag')}</span>
								<Input bind:value={team.tag} disabled={isBusy} placeholder="platform-team" />
								{#if fieldError(`teams.${index}.tag`)}
									<span class="text-xs text-destructive">{fieldError(`teams.${index}.tag`)}</span>
								{/if}
							</label>
							<label class="flex flex-col gap-1 text-sm">
								<span class={fieldLabelClass}>{t('config.teamName')}</span>
								<Input bind:value={team.name} disabled={isBusy} />
							</label>
							<div class="flex items-end">
								<Button
									variant="ghost"
									size="sm"
									disabled={isBusy}
									onclick={() => removeTeam(team.key)}
								>
									{t('config.removeTeam')}
								</Button>
							</div>
						</div>
					{/each}
				</div>
			{/if}
		</Panel>

		<Panel title={t('config.sources')}>
			{#snippet actions()}
				<Button variant="outline" size="sm" disabled={isBusy} onclick={addSource}>
					{t('config.addSource')}
				</Button>
			{/snippet}
			<p class="mb-3 {TYPE_HINT}">{t('config.sourcesHint')}</p>
			{#if sources.length === 0}
				<p class="text-sm text-warning">{t('config.sourcesEmpty')}</p>
			{:else}
				<div class="flex flex-col gap-4">
					{#each sourceGroups as group (group.kind)}
						<section class="flex flex-col gap-2">
							<h3 class={TYPE_OVERLINE}>
								{getSourceKindMeta(group.kind).label}
								<span class="ml-1 tabular-nums opacity-70">({group.items.length})</span>
							</h3>
							{#each group.items as { item: source, index } (source.key)}
								{@const open = isSourceOpen(source.key)}
								<div class="rounded-md border border-border">
									<div class="flex flex-wrap items-center gap-2 px-3 py-2">
										<Button
											variant="ghost"
											size="sm"
											onclick={() => toggleSourceOpen(source.key)}
										>
											{open ? t('config.collapseItem') : t('config.expandItem')}
										</Button>
										<span class="min-w-0 truncate text-sm font-medium">
											{source.id || getSourceKindMeta(source.type).label}
										</span>
										<Badge tone="muted">{getSourceKindMeta(source.type).label}</Badge>
										{#if source.routing_tag}
											<span class="{TYPE_CODE} {TYPE_MUTED}">{source.routing_tag}</span>
										{/if}
										<div class="ml-auto">
											<Button
												variant="ghost"
												size="sm"
												disabled={isBusy}
												onclick={() => removeSource(source.key)}
											>
												{t('config.removeSource')}
											</Button>
										</div>
									</div>
									{#if open}
										<div class="border-t border-border p-3">
											<div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
												<label class="flex flex-col gap-1 text-sm">
													<span class={fieldLabelClass}>{t('config.sourceType')}</span>
													<Select
														value={source.type}
														disabled={isBusy}
														onchange={(e) => changeSourceType(source, e.currentTarget.value)}
													>
														{#each kinds as kind (kind)}
															<option value={kind}>{getSourceKindMeta(kind).label}</option>
														{/each}
													</Select>
												</label>
												<label class="flex flex-col gap-1 text-sm">
													<span class={fieldLabelClass}>{t('config.sourceId')}</span>
													<Input bind:value={source.id} disabled={isBusy} />
													{#if fieldError(`sources.${index}.id`)}
														<span class="text-xs text-destructive"
															>{fieldError(`sources.${index}.id`)}</span
														>
													{/if}
												</label>
												<label class="flex flex-col gap-1 text-sm">
													<span class={fieldLabelClass}>{t('config.routingTag')}</span>
													<Select
														value={source.routing_tag}
														disabled={isBusy}
														onchange={(e) => {
															source.routing_tag = e.currentTarget.value;
															sources = [...sources];
														}}
													>
														<option value="">{t('common.untagged')}</option>
														{#each teamOptions as team (team.tag)}
															<option value={team.tag}>{team.name || team.tag}</option>
														{/each}
														{#if source.routing_tag && !teams.some((team) => team.tag === source.routing_tag)}
															<option value={source.routing_tag}>{source.routing_tag}</option>
														{/if}
													</Select>
													{#if fieldWarning(`sources.${index}.routing_tag`)}
														<span class="text-xs text-warning"
															>{fieldWarning(`sources.${index}.routing_tag`)}</span
														>
													{/if}
												</label>
												{#each primaryFieldsForKind(source.type) as field (field.key)}
													<label class="flex flex-col gap-1 text-sm">
														<span class={fieldLabelClass}>
															{sourceFieldLabel(field.key)}{field.required ? ' *' : ''}
														</span>
														{#if field.key === 'edition'}
															<Select
																value={source.fields.edition ?? 'cloud'}
																disabled={isBusy}
																onchange={(e) => {
																	source.fields = {
																		...source.fields,
																		edition: e.currentTarget.value
																	};
																	sources = [...sources];
																}}
															>
																<option value="cloud">cloud</option>
																<option value="server">server</option>
															</Select>
														{:else}
															<Input
																value={source.fields[field.key] ?? ''}
																disabled={isBusy}
																required={field.required}
																placeholder={fieldPlaceholder(field.key, source.type)}
																oninput={(e) => {
																	source.fields = {
																		...source.fields,
																		[field.key]: e.currentTarget.value
																	};
																	sources = [...sources];
																}}
															/>
														{/if}
														{#if fieldError(`sources.${index}.fields.${field.key}`)}
															<span class="text-xs text-destructive"
																>{fieldError(`sources.${index}.fields.${field.key}`)}</span
															>
														{/if}
													</label>
												{/each}
												{#if sourceSupportsToken(source.type)}
													{@const tokenPending = source.redacted.includes('token')}
													<div
														class="flex flex-col gap-1 text-sm sm:col-span-2 rounded-md border border-border/60 p-2"
													>
														<span class={fieldLabelClass}>{sourceFieldLabel('token')}</span>
														<span class={TYPE_MUTED}>{t('config.notifiersSecretPairHint')}</span>
														<Input
															type="password"
															value={source.token}
															disabled={isBusy}
															placeholder={tokenPending
																? t('config.notifiersSecretRedacted')
																: ''}
															oninput={(e) => {
																source.token = e.currentTarget.value;
																sources = [...sources];
															}}
														/>
														{#if sourceFieldHint('token')}
															<span class={TYPE_MUTED}>{sourceFieldHint('token')}</span>
														{/if}
														{#if tokenPending && !source.token.trim()}
															<span class={TYPE_MUTED}>{t('config.notifiersSecretHint')}</span>
														{/if}
														<label class="mt-2 flex flex-col gap-1 text-sm">
															<span class={fieldLabelClass}>{sourceFieldLabel('token_env')}</span>
															<Input
																value={source.token_env}
																disabled={isBusy}
																placeholder="XRELEASE_…"
																oninput={(e) => {
																	source.token_env = e.currentTarget.value;
																	sources = [...sources];
																}}
															/>
															{#if sourceFieldHint('token_env')}
																<span class={TYPE_MUTED}>{sourceFieldHint('token_env')}</span>
															{/if}
														</label>
													</div>
												{/if}
												<label class="flex flex-col gap-1 text-sm sm:col-span-2">
													<span class={fieldLabelClass}>{t('config.patternTemplate')}</span>
													<Select
														value={source.preset}
														disabled={isBusy}
														aria-label={t('config.patternTemplate')}
														onchange={(e) => applyPatternTemplate(source, e.currentTarget.value)}
													>
														<option value="">{t('config.patternTemplateCustom')}</option>
														{#each patternTemplates as entry (entry.name)}
															<option value={entry.name}
																>{entry.name} — {entry.description}</option
															>
														{/each}
													</Select>
													<span class={TYPE_HINT}>{t('config.patternTemplateHint')}</span>
												</label>
												<label class="flex flex-col gap-1 text-sm">
													<span class={fieldLabelClass}>{t('config.pattern')}</span>
													<Input
														bind:value={source.pattern}
														disabled={isBusy}
														placeholder={t('config.placeholderPattern')}
														oninput={() => {
															if (source.preset) {
																source.preset = '';
																sources = [...sources];
															}
														}}
													/>
													{#if fieldError(`sources.${index}.pattern`)}
														<span class="text-xs text-destructive"
															>{fieldError(`sources.${index}.pattern`)}</span
														>
													{/if}
												</label>
												<label class="flex flex-col gap-1 text-sm">
													<span class={fieldLabelClass}>{t('config.excludePattern')}</span>
													<Input bind:value={source.exclude_pattern} disabled={isBusy} />
													{#if fieldError(`sources.${index}.exclude_pattern`)}
														<span class="text-xs text-destructive"
															>{fieldError(`sources.${index}.exclude_pattern`)}</span
														>
													{/if}
												</label>
												<label class="flex flex-col gap-1 text-sm">
													<span class={fieldLabelClass}>{t('config.prereleaseTags')}</span>
													<Input
														bind:value={source.prerelease_tags}
														disabled={isBusy}
														placeholder={t('config.placeholderPrerelease')}
													/>
													{#if fieldWarning(`sources.${index}.prerelease_tags`)}
														<span class="text-xs text-warning"
															>{fieldWarning(`sources.${index}.prerelease_tags`)}</span
														>
													{/if}
												</label>
												<label class="flex items-center gap-2 text-sm">
													<Checkbox
														checked={source.include_prerelease}
														disabled={isBusy}
														onCheckedChange={(checked) => {
															source.include_prerelease = checked;
															sources = [...sources];
														}}
													/>
													<span>{t('config.includePrerelease')}</span>
												</label>
												<label class="flex items-center gap-2 text-sm">
													<Checkbox
														checked={source.exclude_updated}
														disabled={isBusy}
														onCheckedChange={(checked) => {
															source.exclude_updated = checked;
															sources = [...sources];
														}}
													/>
													<span>{t('config.excludeUpdated')}</span>
												</label>
											</div>
										</div>
									{/if}
								</div>
							{/each}
						</section>
					{/each}
				</div>
			{/if}
		</Panel>

		<ConfigNotifiersPanel
			bind:notifiers
			{teamTags}
			sinkKinds={schemaStore.sinkKinds}
			disabled={isBusy}
			{fieldError}
			{fieldWarning}
			liveEpoch={notifiersLiveEpoch}
		/>
	</div>
{/if}
