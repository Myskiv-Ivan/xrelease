<script lang="ts">
	import { api, ApiClientError } from '$lib/api/client';
	import type { NotifierTestResult, NotifierView } from '$lib/api/types';
	import Badge from '$lib/components/kit/Badge.svelte';
	import Button from '$lib/components/kit/Button.svelte';
	import Input from '$lib/components/kit/Input.svelte';
	import Panel from '$lib/components/kit/Panel.svelte';
	import Select from '$lib/components/kit/Select.svelte';
	import Textarea from '$lib/components/kit/Textarea.svelte';
	import {
		ADDABLE_NOTIFIER_KINDS,
		changeNotifierKind,
		duplicateNotifierDraft,
		emptyNotifierDraft,
		notifierFieldsForKind,
		notifierSupportsName,
		type DesiredNotifierDraft,
		type NotifierFieldSpec
	} from '$lib/config/desired-document';
	import { groupIndexedByKind } from '$lib/config/editor-groups';
	import { notifierFieldHint, notifierFieldLabel } from '$lib/config/field-labels';
	import {
		MESSAGE_BODY_PRESETS,
		MESSAGE_SUBJECT_PRESETS,
		MESSAGE_TEMPLATE_PLACEHOLDERS,
		insertPlaceholder
	} from '$lib/config/message-template';
	import { isSecretEnvCompanion } from '$lib/config/secret-or-env';
	import { getSinkKindLabel, listSinkKindValues } from '$lib/config/sink-kinds';
	import { t } from '$lib/i18n';
	import {
		TYPE_MUTED,
		TYPE_HINT,
		TYPE_FIELD_ERROR,
		TYPE_FIELD_WARNING,
		TYPE_STATUS_WARNING,
		FIELD_GROUP,
		TYPE_OVERLINE,
		TYPE_CODE
	} from '$lib/components/kit/layout-styles';
	import { fieldLabelClass } from '$lib/components/kit/surface-styles';
	import { resolveApiError } from '$lib/core/errors';
	import { getAuthState } from '$lib/stores/auth.svelte';
	import { getNetworkState } from '$lib/stores/network.svelte';

	interface SinkKind {
		value: string;
		label: string;
	}

	interface Props {
		notifiers: DesiredNotifierDraft[];
		/** Team catalogue, offered as routing-tag chips. */
		teamTags: string[];
		/** From `GET /api/v1/config/schema` — labels/order for the kind picker. */
		sinkKinds?: SinkKind[];
		disabled?: boolean;
		fieldError: (path: string) => string | null;
		fieldWarning: (path: string) => string | null;
		/** Reload live sinks after a successful config apply. */
		liveEpoch?: number;
	}

	let {
		notifiers = $bindable(),
		teamTags,
		sinkKinds = [],
		disabled = false,
		fieldError,
		fieldWarning,
		liveEpoch = 0
	}: Props = $props();

	const auth = getAuthState();
	const network = getNetworkState();
	const canTest = $derived(auth.hasPermission('poll:execute'));

	const kindOptions = $derived(
		sinkKinds.length > 0
			? sinkKinds.filter((kind) => ADDABLE_NOTIFIER_KINDS.includes(kind.value))
			: ADDABLE_NOTIFIER_KINDS.map((value) => ({ value, label: value }))
	);

	const kindOrder = $derived(
		kindOptions.length > 0 ? kindOptions.map((kind) => kind.value) : listSinkKindValues()
	);

	let customTag = $state<Record<string, string>>({});
	/** Per-channel expand; missing key = collapsed. */
	let openByKey = $state<Record<string, boolean>>({});
	let liveSinks = $state<NotifierView[]>([]);
	let testingKey = $state<string | null>(null);
	let testResults = $state<Record<string, NotifierTestResult>>({});
	let teamFilter = $state('all');
	let search = $state('');

	const filteredNotifiers = $derived.by(() => {
		const q = search.trim().toLowerCase();
		return notifiers.filter((draft) => {
			if (teamFilter !== 'all') {
				const tags = tagsOf(draft);
				if (tags.length > 0 && !tags.includes(teamFilter)) {
					return false;
				}
			}
			if (!q) return true;
			const hay = [
				draft.name,
				draft.type,
				draft.tags,
				channelLabel(draft),
				...Object.values(draft.fields)
			]
				.join(' ')
				.toLowerCase();
			return hay.includes(q);
		});
	});

	const channelGroups = $derived.by(() => {
		const grouped = groupIndexedByKind(filteredNotifiers, (draft) => draft.type, kindOrder);
		return grouped.map((group) => ({
			...group,
			items: group.items.map(({ item }) => ({
				item,
				index: notifiers.findIndex((entry) => entry.key === item.key)
			}))
		}));
	});

	const shownCount = $derived(filteredNotifiers.length);

	$effect(() => {
		void liveEpoch;
		void loadLiveSinks();
	});

	async function loadLiveSinks() {
		try {
			const response = await api.listNotifiers();
			liveSinks = response.notifiers;
		} catch {
			liveSinks = [];
		}
	}

	function isOpen(key: string): boolean {
		return openByKey[key] === true;
	}

	function toggleOpen(key: string) {
		openByKey = { ...openByKey, [key]: !isOpen(key) };
	}

	function expand(key: string) {
		openByKey = { ...openByKey, [key]: true };
	}

	function tagsOf(draft: DesiredNotifierDraft): string[] {
		return draft.tags
			.split(',')
			.map((tag) => tag.trim())
			.filter(Boolean);
	}

	function tagOptions(draft: DesiredNotifierDraft): string[] {
		return [...new Set([...teamTags, ...tagsOf(draft)])].sort((a, b) => a.localeCompare(b));
	}

	function setTags(draft: DesiredNotifierDraft, tags: string[]) {
		draft.tags = tags.join(', ');
		notifiers = [...notifiers];
	}

	function toggleTag(draft: DesiredNotifierDraft, tag: string) {
		const current = tagsOf(draft);
		setTags(draft, current.includes(tag) ? current.filter((t) => t !== tag) : [...current, tag]);
	}

	function addCustomTag(draft: DesiredNotifierDraft) {
		const tag = (customTag[draft.key] ?? '').trim();
		if (!tag) return;
		const current = tagsOf(draft);
		if (!current.includes(tag)) setTags(draft, [...current, tag]);
		customTag = { ...customTag, [draft.key]: '' };
	}

	function channelLabel(draft: DesiredNotifierDraft): string {
		return (
			draft.name.trim() ||
			(draft.fields.url ?? '').trim() ||
			(draft.fields.endpoint ?? '').trim() ||
			(draft.fields.base_url ?? '').trim() ||
			(draft.fields.host ?? '').trim() ||
			(draft.fields.workflow ?? '').trim() ||
			(draft.fields.channel ?? '').trim() ||
			(draft.fields.chat_id ?? '').trim() ||
			(draft.fields.topic ?? '').trim() ||
			(draft.fields.routing_key ?? '').trim() ||
			(typeof draft.extra.config_key === 'string' ? draft.extra.config_key.trim() : '') ||
			getSinkKindLabel(draft.type)
		);
	}

	/** Apprise without urls/urls_env/config_key is not in the live sink list. */
	function appriseNeedsTargets(draft: DesiredNotifierDraft): boolean {
		if (draft.type !== 'apprise') return false;
		const hasUrls = (draft.fields.urls ?? '').trim().length > 0;
		const hasUrlsEnv = (draft.fields.urls_env ?? '').trim().length > 0;
		const hasKey =
			typeof draft.extra.config_key === 'string' && draft.extra.config_key.trim().length > 0;
		const pending = draft.redacted.includes('urls');
		return !hasUrls && !hasUrlsEnv && !hasKey && !pending;
	}

	/** Map a draft row onto a live sink index (after Apply). */
	function liveIndexFor(draft: DesiredNotifierDraft): number | null {
		if (appriseNeedsTargets(draft)) return null;
		const sameKind = liveSinks.filter((sink) => sink.kind === draft.type);
		if (sameKind.length === 0) return null;
		const label = channelLabel(draft);
		const byName = sameKind.find(
			(sink) => sink.name === draft.name.trim() || sink.name === label
		);
		if (byName) return byName.index;
		const peers = notifiers.filter((entry) => entry.type === draft.type);
		const ord = peers.findIndex((entry) => entry.key === draft.key);
		return sameKind[ord]?.index ?? sameKind[0]?.index ?? null;
	}

	function testDisabledReason(draft: DesiredNotifierDraft): string | undefined {
		if (appriseNeedsTargets(draft)) return t('config.notifiersTestNeedsUrls');
		if (liveIndexFor(draft) == null) return t('config.notifiersTestNeedsApply');
		return undefined;
	}

	function addNotifier() {
		const draft = emptyNotifierDraft('webhook');
		notifiers = [...notifiers, draft];
		expand(draft.key);
	}

	function duplicateNotifier(draft: DesiredNotifierDraft) {
		const copy = duplicateNotifierDraft(draft);
		const at = notifiers.findIndex((entry) => entry.key === draft.key);
		const next = [...notifiers];
		next.splice(at < 0 ? next.length : at + 1, 0, copy);
		notifiers = next;
		expand(copy.key);
	}

	function collapseAll() {
		openByKey = {};
	}

	function removeNotifier(key: string) {
		notifiers = notifiers.filter((draft) => draft.key !== key);
	}

	function setKind(draft: DesiredNotifierDraft, type: string) {
		notifiers = notifiers.map((entry) =>
			entry.key === draft.key ? changeNotifierKind(entry, type) : entry
		);
	}

	function setField(draft: DesiredNotifierDraft, key: string, value: string) {
		draft.fields = { ...draft.fields, [key]: value };
		notifiers = [...notifiers];
	}

		function isWide(spec: NotifierFieldSpec): boolean {
			return (
				spec.type === 'template' ||
				spec.type === 'list' ||
				spec.type === 'headers' ||
				spec.key === 'subject_template'
			);
		}

		function isTemplateField(spec: NotifierFieldSpec): boolean {
			return spec.type === 'template' || spec.key === 'subject_template';
		}

		function appendTemplateToken(draft: DesiredNotifierDraft, key: string, name: string) {
			setField(draft, key, insertPlaceholder(draft.fields[key] ?? '', name));
		}

		function applyTemplatePreset(draft: DesiredNotifierDraft, key: string, body: string) {
			setField(draft, key, body);
		}

	async function testChannel(draft: DesiredNotifierDraft) {
		if (!canTest || !network.isOnline) return;
		const index = liveIndexFor(draft);
		if (index == null) return;
		testingKey = draft.key;
		try {
			const response = await api.testNotifiers({ index });
			const result = response.results[0];
			if (result) {
				testResults = { ...testResults, [draft.key]: result };
			}
		} catch (err) {
			const message =
				err instanceof ApiClientError
					? err.message
					: resolveApiError(err, t('notifiers.testError'));
			testResults = {
				...testResults,
				[draft.key]: {
					index: index ?? -1,
					kind: draft.type,
					name: channelLabel(draft),
					ok: false,
					error: message
				}
			};
		} finally {
			testingKey = null;
			await loadLiveSinks();
		}
	}
</script>

<Panel title={t('config.notifiers')}>
	{#snippet actions()}
		<div class="flex flex-wrap items-center gap-2">
			{#if notifiers.length > 0}
				<Button variant="ghost" size="sm" {disabled} onclick={collapseAll}>
					{t('config.notifiersCollapseAll')}
				</Button>
			{/if}
			<Button variant="outline" size="sm" {disabled} onclick={addNotifier}>
				{t('config.notifiersAdd')}
			</Button>
		</div>
	{/snippet}

	<p class="mb-2 {TYPE_HINT}">{t('config.notifiersHint')}</p>
	{#if notifiers.length >= 5}
		<p class="mb-3 {TYPE_MUTED}">{t('config.notifiersScaleHint')}</p>
	{/if}

	{#if notifiers.length === 0}
		<p class={TYPE_STATUS_WARNING}>{t('config.notifiersEmpty')}</p>
	{:else}
		<div class="mb-3 flex flex-wrap items-end gap-2">
			<label class="{FIELD_GROUP} min-w-[10rem] flex-1 text-sm">
				<span class={fieldLabelClass}>{t('config.notifiersSearch')}</span>
				<Input bind:value={search} {disabled} placeholder={t('config.notifiersSearch')} />
			</label>
			<label class="{FIELD_GROUP} w-auto text-sm">
				<span class={fieldLabelClass}>{t('config.notifiersFilterTeam')}</span>
				<Select class="w-auto min-w-[9rem]" bind:value={teamFilter} {disabled}>
					<option value="all">{t('config.notifiersFilterAll')}</option>
					{#each teamTags as tag (tag)}
						<option value={tag}>{tag}</option>
					{/each}
				</Select>
			</label>
			<span class="{TYPE_MUTED} pb-2 tabular-nums">
				{t('config.notifiersShown')
					.replace('{shown}', String(shownCount))
					.replace('{total}', String(notifiers.length))}
			</span>
		</div>

		{#if shownCount === 0}
			<p class="mb-3 {TYPE_HINT}">{t('config.notifiersFilterEmpty')}</p>
		{/if}

		<div class="flex flex-col gap-4">
			{#each channelGroups as group (group.kind)}
				<section class="flex flex-col gap-2">
					<h3 class={TYPE_OVERLINE}>
						{getSinkKindLabel(group.kind)}
						<span class="ml-1 tabular-nums opacity-70">({group.items.length})</span>
					</h3>
					{#each group.items as { item: draft, index } (draft.key)}
						{@const selected = tagsOf(draft)}
						{@const open = isOpen(draft.key)}
						{@const liveIndex = liveIndexFor(draft)}
						{@const testReason = testDisabledReason(draft)}
						{@const testResult = testResults[draft.key]}
						<div class="rounded-md border border-border">
							<div class="flex flex-wrap items-center gap-2 px-3 py-2">
								<Button
									variant="ghost"
									size="sm"
									onclick={() => toggleOpen(draft.key)}
								>
									{open ? t('config.collapseItem') : t('config.expandItem')}
								</Button>
								<Badge tone="muted">{getSinkKindLabel(draft.type)}</Badge>
								{#if selected.length === 0}
									<Badge tone="accent">{t('config.notifiersWildcard')}</Badge>
								{:else}
									{#each selected.slice(0, 3) as tag (tag)}
										<Badge tone="muted">{tag}</Badge>
									{/each}
									{#if selected.length > 3}
										<Badge tone="muted">+{selected.length - 3}</Badge>
									{/if}
								{/if}
								{#if appriseNeedsTargets(draft)}
									<Badge tone="warning" dot>{t('config.notifiersAppriseNoUrls')}</Badge>
								{/if}
								<span class="min-w-0 truncate text-sm font-medium" title={channelLabel(draft)}>
									{channelLabel(draft)}
								</span>
								{#if testResult}
									<Badge tone={testResult.ok ? 'success' : 'danger'} dot>
										{testResult.ok ? t('notifiers.resultOk') : t('notifiers.resultFail')}
									</Badge>
								{/if}
								<div class="ml-auto flex flex-wrap items-center gap-2">
									{#if canTest}
										<Button
											variant="outline"
											size="sm"
											disabled={
												disabled ||
												!network.isOnline ||
												liveIndex == null ||
												testingKey !== null
											}
											title={testReason}
											onclick={() => void testChannel(draft)}
										>
											{testingKey === draft.key
												? t('notifiers.testing')
												: t('notifiers.testOne')}
										</Button>
									{/if}
									<Button
										variant="ghost"
										size="sm"
										{disabled}
										onclick={() => duplicateNotifier(draft)}
									>
										{t('config.notifiersDuplicate')}
									</Button>
									<Button
										variant="ghost"
										size="sm"
										{disabled}
										onclick={() => removeNotifier(draft.key)}
									>
										{t('config.notifiersRemove')}
									</Button>
								</div>
							</div>

							{#if testResult?.error}
								<p class="border-t border-border px-3 py-2 {TYPE_FIELD_ERROR}">
									{testResult.error}
								</p>
							{/if}

							{#if open}
								<div class="border-t border-border p-3">
									{#if liveIndex == null && testReason}
										<p class="mb-3 {TYPE_MUTED}">{testReason}</p>
									{/if}

									<div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
										<label class="{FIELD_GROUP} text-sm">
											<span class={fieldLabelClass}>{t('config.notifiersKind')}</span>
											<Select
												value={draft.type}
												{disabled}
												onchange={(e) => setKind(draft, e.currentTarget.value)}
											>
												{#each kindOptions as kind (kind.value)}
													<option value={kind.value}>{kind.label}</option>
												{/each}
												{#if !kindOptions.some((kind) => kind.value === draft.type)}
													<option value={draft.type}>{getSinkKindLabel(draft.type)}</option>
												{/if}
											</Select>
										</label>

										{#if notifierSupportsName(draft.type)}
											<label class="{FIELD_GROUP} text-sm">
												<span class={fieldLabelClass}>{t('config.notifiersName')}</span>
												<Input bind:value={draft.name} {disabled} />
											</label>
										{/if}

										{#each notifierFieldsForKind(draft.type) as spec (spec.key)}
											{@const path = `notifiers.${index}.fields.${spec.key}`}
											{@const pending = draft.redacted.includes(spec.key)}
											{@const specs = notifierFieldsForKind(draft.type)}
											{#if !isSecretEnvCompanion(spec.key, specs)}
												{#if specs.some((s) => s.key === `${spec.key}_env`)}
												{@const envKey = `${spec.key}_env`}
												{@const envPath = `notifiers.${index}.fields.${envKey}`}
												<div
													class="{FIELD_GROUP} text-sm sm:col-span-2 lg:col-span-2 rounded-md border border-border/60 p-2"
												>
													<span class={fieldLabelClass}>
														{notifierFieldLabel(spec.key)}{spec.required ? ' *' : ''}
													</span>
													<span class="mb-1.5 {TYPE_MUTED}">{t('config.notifiersSecretPairHint')}</span>
													{#if spec.type === 'list' || spec.type === 'headers'}
														<Textarea
															mono
															rows={3}
															{disabled}
															placeholder={pending
																? t('config.notifiersSecretRedacted')
																: spec.type === 'headers'
																	? t('config.notifiersHeadersPlaceholder')
																	: ''}
															value={draft.fields[spec.key] ?? ''}
															oninput={(e) =>
																setField(draft, spec.key, e.currentTarget.value)}
														/>
													{:else}
														<Input
															type={spec.type === 'secret' ? 'password' : 'text'}
															{disabled}
															placeholder={pending
																? t('config.notifiersSecretRedacted')
																: ''}
															value={draft.fields[spec.key] ?? ''}
															oninput={(e) =>
																setField(draft, spec.key, e.currentTarget.value)}
														/>
													{/if}
													{#if pending && !(draft.fields[spec.key] ?? '').trim()}
														<span class={TYPE_MUTED}>{t('config.notifiersSecretHint')}</span>
													{/if}
													<label class="{FIELD_GROUP} mt-2 text-sm">
														<span class={fieldLabelClass}>{notifierFieldLabel(envKey)}</span>
														<Input
															{disabled}
															placeholder="XRELEASE_…"
															value={draft.fields[envKey] ?? ''}
															oninput={(e) => setField(draft, envKey, e.currentTarget.value)}
														/>
													</label>
													{#if fieldError(path)}
														<span class={TYPE_FIELD_ERROR}>{fieldError(path)}</span>
													{/if}
													{#if fieldError(envPath)}
														<span class={TYPE_FIELD_ERROR}>{fieldError(envPath)}</span>
													{/if}
													{#if fieldWarning(path)}
														<span class={TYPE_FIELD_WARNING}>{fieldWarning(path)}</span>
													{/if}
													{#if fieldWarning(envPath)}
														<span class={TYPE_FIELD_WARNING}>{fieldWarning(envPath)}</span>
													{/if}
												</div>
												{:else}
											<label
												class="{FIELD_GROUP} text-sm {isWide(spec)
													? 'sm:col-span-2 lg:col-span-3'
													: ''}"
											>
												<span class={fieldLabelClass}>
													{notifierFieldLabel(spec.key)}{spec.required ? ' *' : ''}
												</span>

												{#if spec.type === 'select'}
													<Select
														value={draft.fields[spec.key] ?? ''}
														{disabled}
														onchange={(e) => setField(draft, spec.key, e.currentTarget.value)}
													>
														{#each spec.options ?? [] as option (option)}
															<option value={option}>{option}</option>
														{/each}
													</Select>
												{:else if isTemplateField(spec)}
													{@const presets =
														spec.key === 'subject_template'
															? MESSAGE_SUBJECT_PRESETS
															: MESSAGE_BODY_PRESETS}
													<div class="mb-1.5 flex flex-wrap items-center gap-2">
														<label class="inline-flex items-center gap-1.5 text-xs">
															<span class={TYPE_MUTED}>{t('config.notifiersTemplatePresets')}</span>
															<Select
																value=""
																{disabled}
																onchange={(e) => {
																	const id = e.currentTarget.value;
																	const preset = presets.find((p) => p.id === id);
																	if (preset) {
																		applyTemplatePreset(draft, spec.key, preset.body);
																	}
																	e.currentTarget.value = '';
																}}
															>
																<option value="">—</option>
																{#each presets as preset (preset.id)}
																	<option value={preset.id}>{preset.label}</option>
																{/each}
															</Select>
														</label>
													</div>
													<div class="mb-1.5 flex flex-wrap gap-1">
														<span class="{TYPE_MUTED} w-full sm:w-auto"
															>{t('config.notifiersTemplatePlaceholders')}</span
														>
														{#each MESSAGE_TEMPLATE_PLACEHOLDERS as name (name)}
															<button
																type="button"
																class="rounded-md border border-border px-1.5 py-0.5 {TYPE_CODE} hover:bg-muted"
																{disabled}
																onclick={() => appendTemplateToken(draft, spec.key, name)}
															>
																{'{{'}{name}{'}}'}
															</button>
														{/each}
													</div>
													{#if spec.type === 'template'}
														<Textarea
															mono
															rows={4}
															{disabled}
															value={draft.fields[spec.key] ?? ''}
															oninput={(e) => setField(draft, spec.key, e.currentTarget.value)}
														/>
													{:else}
														<Input
															{disabled}
															value={draft.fields[spec.key] ?? ''}
															oninput={(e) => setField(draft, spec.key, e.currentTarget.value)}
														/>
													{/if}
													<span class={TYPE_MUTED}>{t('config.notifiersTemplateHint')}</span>
												{:else if spec.type === 'list' || spec.type === 'headers'}
													<Textarea
														mono
														rows={3}
														{disabled}
														placeholder={pending
															? t('config.notifiersSecretRedacted')
															: spec.type === 'headers'
																? t('config.notifiersHeadersPlaceholder')
																: ''}
														value={draft.fields[spec.key] ?? ''}
														oninput={(e) => setField(draft, spec.key, e.currentTarget.value)}
													/>
													<span class={TYPE_MUTED}>
														{spec.type === 'headers'
															? t('config.notifiersHeadersHint')
															: t('config.notifiersListHint')}
													</span>
												{:else}
													<Input
														type={spec.type === 'secret'
															? 'password'
															: spec.type === 'number'
																? 'number'
																: 'text'}
														{disabled}
														placeholder={pending ? t('config.notifiersSecretRedacted') : ''}
														value={draft.fields[spec.key] ?? ''}
														oninput={(e) => setField(draft, spec.key, e.currentTarget.value)}
													/>
												{/if}

												{#if notifierFieldHint(spec.key)}
													<span class={TYPE_MUTED}>{notifierFieldHint(spec.key)}</span>
												{/if}
												{#if pending && !(draft.fields[spec.key] ?? '').trim()}
													<span class={TYPE_MUTED}>{t('config.notifiersSecretHint')}</span>
												{/if}
												{#if fieldError(path)}
													<span class={TYPE_FIELD_ERROR}>{fieldError(path)}</span>
												{/if}
												{#if fieldWarning(path)}
													<span class={TYPE_FIELD_WARNING}>{fieldWarning(path)}</span>
												{/if}
											</label>
												{/if}
											{/if}
										{/each}
									</div>

									<div class="mt-3">
										<p class="mb-2 {TYPE_HINT}">{t('config.notifiersTags')}</p>
										<div class="flex flex-wrap gap-2">
											{#each tagOptions(draft) as tag (tag)}
												<label class="inline-flex items-center gap-1.5 text-sm">
													<input
														type="checkbox"
														class="size-3.5 accent-accent"
														checked={selected.includes(tag)}
														{disabled}
														onchange={() => toggleTag(draft, tag)}
													/>
													<span class="font-mono text-xs">{tag}</span>
												</label>
											{/each}
										</div>
										<div class="mt-2">
											<Input
												class="max-w-xs"
												placeholder={t('config.notifiersCustomTag')}
												{disabled}
												value={customTag[draft.key] ?? ''}
												oninput={(e) => {
													customTag = { ...customTag, [draft.key]: e.currentTarget.value };
												}}
												onkeydown={(e) => {
													if (e.key === 'Enter') {
														e.preventDefault();
														addCustomTag(draft);
													}
												}}
											/>
										</div>
										{#if selected.length === 0}
											<p class="mt-2 {TYPE_MUTED}">{t('config.notifiersTagsHint')}</p>
										{/if}
										{#if fieldError(`notifiers.${index}.tags`)}
											<p class="mt-1 {TYPE_FIELD_ERROR}">
												{fieldError(`notifiers.${index}.tags`)}
											</p>
										{/if}
									</div>
								</div>
							{/if}
						</div>
					{/each}
				</section>
			{/each}
		</div>
	{/if}

	{#if fieldWarning('notifiers')}
		<p class="mt-3 {TYPE_FIELD_WARNING}">{fieldWarning('notifiers')}</p>
	{/if}
</Panel>
