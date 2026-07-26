/**
 * Parse / stringify desired-state documents for the form config editor.
 *
 * Unknown top-level keys (apprise, notifiers, database, …) are preserved so a
 * UI that only edits defaults/teams/sources cannot strip delivery sinks.
 */

import { parse as parseToml, stringify as stringifyToml } from 'smol-toml';
import { parse as parseYaml, stringify as stringifyYaml } from 'yaml';

import { asArray, asObject, numOrEmpty, str, type DesiredMap } from './desired-map';

export type DesiredFormat = 'yaml' | 'toml';

export type { DesiredMap };

export interface ParsedDesiredDocument {
	format: DesiredFormat;
	data: DesiredMap;
}

export interface DesiredDefaultsDraft {
	interval_secs: number | '';
	jitter_secs: number | '';
	upstream_requests_per_minute: number | '';
	poll_on_startup: boolean;
	notify_schedule: string;
	/** Ops meta-alert routing tag (dead outbox / tripped breaker). */
	ops_routing_tag: string;
}

export interface DesiredTeamDraft {
	/** Stable editor row id. */
	key: string;
	tag: string;
	name: string;
}

export interface DesiredSourceDraft {
	/** Stable editor row id (not necessarily the config `id`). */
	key: string;
	type: string;
	id: string;
	routing_tag: string;
	/** Named filter preset (`semver`, …) — merged before per-source fields. */
	preset: string;
	pattern: string;
	exclude_pattern: string;
	include_prerelease: boolean;
	prerelease_tags: string;
	exclude_updated: boolean;
	interval_secs: string;
	jitter_secs: string;
	notify_schedule: string;
	/** Kind-specific identity fields (repo, project, image, name, url, host, …). */
	fields: Record<string, string>;
	/** Forge / registry PAT (empty when redacted or unset). */
	token: string;
	/** Env-var name for the PAT (refs-first / GitOps). */
	token_env: string;
	/** Field keys that arrived redacted and have not been re-entered. */
	redacted: string[];
	/** Keys present on the original object that the form does not own. */
	extra: DesiredMap;
}

const OWNED_SOURCE_KEYS = new Set([
	'type',
	'id',
	'routing_tag',
	'preset',
	'pattern',
	'exclude_pattern',
	'include_prerelease',
	'prerelease_tags',
	'exclude_updated',
	'interval_secs',
	'jitter_secs',
	'poll_on_startup',
	'notify_schedule',
	'repo',
	'project',
	'image',
	'name',
	'url',
	'host',
	'registry',
	'edition',
	'package_kind',
	'token',
	'token_env'
]);

/**
 * Placeholder `GET /api/v1/config` substitutes for secrets. The real value never
 * reaches the browser, so writing this back would set the literal string as the
 * credential — see `writeNotifiers` / `writeSources`, which drop such fields.
 */
export const REDACTED = '<redacted>';

function isRedacted(value: unknown): boolean {
	return typeof value === 'string' && value === REDACTED;
}

/** Broker URLs keep host visible but mask userinfo as `…://<redacted>@host`. */
function containsRedactionMarker(value: unknown): boolean {
	return typeof value === 'string' && value.includes(REDACTED);
}

export function detectDesiredFormat(raw: string): DesiredFormat {
	const lines = raw.split(/\r?\n/);
	let sample = '';
	for (const line of lines) {
		const trimmed = line.trim();
		if (!trimmed || trimmed.startsWith('#')) continue;
		sample += `${trimmed}\n`;
		if (sample.length > 400) break;
	}
	if (!sample) {
		const head = raw.trimStart();
		if (head.startsWith('---')) return 'yaml';
		return 'yaml';
	}
	if (sample.startsWith('---')) return 'yaml';
	// TOML: table header or key = value on a significant line.
	if (/^\[/.test(sample) || /^\w[\w.-]*\s*=/.test(sample)) return 'toml';
	// YAML: mapping key:
	if (/^\w[\w.-]*:\s/.test(sample) || /^\w[\w.-]*:$/m.test(sample)) return 'yaml';
	return 'yaml';
}

export function contentTypeForFormat(format: DesiredFormat): string {
	return format === 'toml' ? 'application/toml' : 'application/yaml';
}

export function parseDesiredDocument(raw: string, formatHint?: DesiredFormat): ParsedDesiredDocument {
	const preferred = formatHint ?? detectDesiredFormat(raw);
	const order: DesiredFormat[] =
		preferred === 'toml' ? ['toml', 'yaml'] : ['yaml', 'toml'];

	let lastError: unknown;
	for (const format of order) {
		try {
			const parsed = format === 'toml' ? parseToml(raw) : parseYaml(raw);
			if (parsed == null || typeof parsed !== 'object' || Array.isArray(parsed)) {
				throw new Error('Desired document must be a mapping at the root');
			}
			return { format, data: parsed as DesiredMap };
		} catch (err) {
			lastError = err;
		}
	}
	throw lastError instanceof Error
		? lastError
		: new Error('Desired document must be a mapping at the root');
}

export function stringifyDesiredDocument(doc: ParsedDesiredDocument): string {
	if (doc.format === 'toml') {
		return stringifyToml(doc.data);
	}
	return stringifyYaml(doc.data, {
		lineWidth: 100,
		defaultKeyType: 'PLAIN',
		defaultStringType: 'PLAIN'
	}).trimEnd() + '\n';
}

let draftKeySeq = 0;
function nextDraftKey(): string {
	draftKeySeq += 1;
	return `src-${draftKeySeq}`;
}

export function resetDraftKeySeq(): void {
	draftKeySeq = 0;
}

export function readDefaults(data: DesiredMap): DesiredDefaultsDraft {
	const defaults = asObject(data.defaults);
	return {
		interval_secs: numOrEmpty(defaults.interval_secs) === '' ? 86400 : numOrEmpty(defaults.interval_secs),
		jitter_secs: numOrEmpty(defaults.jitter_secs) === '' ? 3600 : numOrEmpty(defaults.jitter_secs),
		upstream_requests_per_minute: numOrEmpty(defaults.upstream_requests_per_minute) === ''
			? 0
			: numOrEmpty(defaults.upstream_requests_per_minute),
		poll_on_startup: defaults.poll_on_startup !== false,
		notify_schedule: str(defaults.notify_schedule),
		ops_routing_tag: str(defaults.ops_routing_tag)
	};
}

export function writeDefaults(data: DesiredMap, draft: DesiredDefaultsDraft): DesiredMap {
	const prev = asObject(data.defaults);
	const next: DesiredMap = { ...prev };
	const writeNum = (key: string, value: number | '') => {
		if (value === '') return;
		const n = Number(value);
		if (Number.isFinite(n)) next[key] = n;
	};
	writeNum('interval_secs', draft.interval_secs);
	writeNum('jitter_secs', draft.jitter_secs);
	writeNum('upstream_requests_per_minute', draft.upstream_requests_per_minute);
	next.poll_on_startup = draft.poll_on_startup;
	const schedule = draft.notify_schedule.trim();
	if (schedule) next.notify_schedule = schedule;
	else delete next.notify_schedule;
	const opsTag = draft.ops_routing_tag.trim();
	if (opsTag) next.ops_routing_tag = opsTag;
	else delete next.ops_routing_tag;
	return { ...data, defaults: next };
}

export function readTeams(data: DesiredMap): DesiredTeamDraft[] {
	return asArray(data.teams).map((entry) => {
		const row = asObject(entry);
		return { key: nextDraftKey(), tag: str(row.tag), name: str(row.name) };
	});
}

export function emptyTeamDraft(): DesiredTeamDraft {
	return { key: nextDraftKey(), tag: '', name: '' };
}

export function writeTeams(data: DesiredMap, teams: DesiredTeamDraft[]): DesiredMap {
	const next = teams
		.filter((team) => team.tag.trim())
		.map((team) => {
			const row: DesiredMap = { tag: team.tag.trim() };
			if (team.name.trim()) row.name = team.name.trim();
			return row;
		});
	return { ...data, teams: next };
}

export function primaryFieldsForKind(kind: string): Array<{ key: string; required?: boolean }> {
	switch (kind) {
		case 'github':
		case 'codeberg':
			return [{ key: 'repo', required: true }];
		case 'gitea':
			return [
				{ key: 'host', required: true },
				{ key: 'repo', required: true }
			];
		case 'gitlab':
			return [
				{ key: 'project', required: true },
				{ key: 'host' }
			];
		case 'bitbucket':
			return [
				{ key: 'repo', required: true },
				{ key: 'host' },
				{ key: 'edition' }
			];
		case 'docker':
			return [
				{ key: 'image', required: true },
				{ key: 'registry' }
			];
		case 'ghcr':
		case 'quay':
		case 'ecr':
			return [{ key: 'image', required: true }];
		case 'feed':
			return [{ key: 'url', required: true }];
		case 'artifacthub':
			return [
				{ key: 'name', required: true },
				{ key: 'host' },
				{ key: 'package_kind' }
			];
		default:
			return [{ key: 'name', required: true }];
	}
}

/** Forge / registry kinds that accept a PAT (`token` / `token_env`). */
export function sourceSupportsToken(kind: string): boolean {
	switch (kind) {
		case 'github':
		case 'codeberg':
		case 'gitea':
		case 'gitlab':
		case 'bitbucket':
		case 'docker':
		case 'ghcr':
		case 'quay':
		case 'ecr':
			return true;
		default:
			return false;
	}
}

export function readSources(data: DesiredMap): DesiredSourceDraft[] {
	return asArray(data.sources).map((entry) => {
		const row = asObject(entry);
		const type = str(row.type) || 'github';
		const fields: Record<string, string> = {};
		for (const field of primaryFieldsForKind(type)) {
			fields[field.key] = str(row[field.key]);
		}
		const extra: DesiredMap = {};
		for (const [key, value] of Object.entries(row)) {
			if (!OWNED_SOURCE_KEYS.has(key)) {
				extra[key] = value;
			}
		}

		const redacted: string[] = [];
		let token = '';
		let token_env = '';
		if (sourceSupportsToken(type)) {
			token_env = str(row.token_env).trim();
			if (isRedacted(row.token) || containsRedactionMarker(row.token)) {
				redacted.push('token');
			} else if (row.token !== undefined && str(row.token).trim()) {
				token = str(row.token);
			}
		}

		const tags = row.prerelease_tags;
		const prerelease_tags = Array.isArray(tags)
			? tags.map(String).join(', ')
			: str(tags);

		return {
			key: nextDraftKey(),
			type,
			id: str(row.id),
			routing_tag: str(row.routing_tag),
			preset: str(row.preset),
			pattern: str(row.pattern),
			exclude_pattern: str(row.exclude_pattern),
			include_prerelease: row.include_prerelease === true,
			prerelease_tags,
			exclude_updated: row.exclude_updated === true,
			interval_secs: str(row.interval_secs),
			jitter_secs: str(row.jitter_secs),
			notify_schedule: str(row.notify_schedule),
			fields,
			token,
			token_env,
			redacted,
			extra
		};
	});
}

export function emptySourceDraft(type = 'github'): DesiredSourceDraft {
	const fields: Record<string, string> = {};
	for (const field of primaryFieldsForKind(type)) {
		fields[field.key] = field.key === 'edition' ? 'cloud' : '';
	}
	return {
		key: nextDraftKey(),
		type,
		id: '',
		routing_tag: '',
		preset: '',
		pattern: '',
		exclude_pattern: '',
		include_prerelease: false,
		prerelease_tags: '',
		exclude_updated: false,
		interval_secs: '',
		jitter_secs: '',
		notify_schedule: '',
		fields,
		token: '',
		token_env: '',
		redacted: [],
		extra: {}
	};
}

export function writeSources(data: DesiredMap, sources: DesiredSourceDraft[]): DesiredMap {
	const next = sources.map((draft) => {
		const row: DesiredMap = { ...draft.extra, type: draft.type };
		if (draft.id.trim()) row.id = draft.id.trim();
		else delete row.id;

		// Same posture as writeNotifiers: never persist API redaction markers.
		for (const [key, value] of Object.entries(row)) {
			if (value === REDACTED || (typeof value === 'string' && value.includes(REDACTED))) {
				delete row[key];
			}
		}
		// Owned by draft.token / draft.token_env — never leave stale copies in extra.
		delete row.token;
		delete row.token_env;

		for (const field of primaryFieldsForKind(draft.type)) {
			const value = (draft.fields[field.key] ?? '').trim();
			if (value) row[field.key] = value;
			else delete row[field.key];
		}

		if (sourceSupportsToken(draft.type)) {
			const token = draft.token.trim();
			if (token && token !== REDACTED && !token.includes(REDACTED)) {
				row.token = token;
			}
			const tokenEnv = draft.token_env.trim();
			if (tokenEnv) row.token_env = tokenEnv;
		}

		if (draft.routing_tag.trim()) {
			row.routing_tag = draft.routing_tag.trim();
			delete row.apprise_tag;
		} else {
			delete row.routing_tag;
			delete row.apprise_tag;
		}

		if (draft.preset.trim()) row.preset = draft.preset.trim();
		else delete row.preset;

		if (draft.pattern.trim()) row.pattern = draft.pattern.trim();
		else delete row.pattern;

		if (draft.exclude_pattern.trim()) row.exclude_pattern = draft.exclude_pattern.trim();
		else delete row.exclude_pattern;

		row.include_prerelease = draft.include_prerelease;
		if (!draft.include_prerelease) delete row.include_prerelease;

		const tags = draft.prerelease_tags
			.split(',')
			.map((tag) => tag.trim())
			.filter(Boolean);
		if (tags.length) row.prerelease_tags = tags;
		else delete row.prerelease_tags;

		if (draft.exclude_updated) row.exclude_updated = true;
		else delete row.exclude_updated;

		if (draft.interval_secs.trim()) row.interval_secs = Number(draft.interval_secs);
		else delete row.interval_secs;

		if (draft.jitter_secs.trim()) row.jitter_secs = Number(draft.jitter_secs);
		else delete row.jitter_secs;

		if (draft.notify_schedule.trim()) row.notify_schedule = draft.notify_schedule.trim();
		else delete row.notify_schedule;

		return row;
	});
	return { ...data, sources: next };
}

export type NotifierFieldType =
	| 'text'
	| 'secret'
	| 'number'
	| 'list'
	| 'select'
	| 'template'
	/** Multiline `Header-Name: value` (webhook static headers). */
	| 'headers';

export interface NotifierFieldSpec {
	key: string;
	type: NotifierFieldType;
	required?: boolean;
	/** Allowed values for `select` fields. */
	options?: string[];
	/** Server-side default, surfaced as an input placeholder. */
	fallback?: string;
}

/** Channel draft for one `[[notifiers]]` / `notifiers:` entry. */
export interface DesiredNotifierDraft {
	/** Stable editor row id. */
	key: string;
	type: string;
	name: string;
	/** Routing tags, comma separated; empty = wildcard (every event). */
	tags: string;
	/** Scalar fields per kind; `list` fields are newline separated. */
	fields: Record<string, string>;
	/** Field keys that arrived redacted and have not been re-entered. */
	redacted: string[];
	/** Keys the form does not own (headers, …), preserved verbatim. */
	extra: DesiredMap;
}

const NOTIFIER_FIELDS: Record<string, NotifierFieldSpec[]> = {
	// UI owns the stateless path (endpoint + urls + format). Persistent Apprise
	// `config_key` / channel `tag` stay YAML/env-only and round-trip via `extra`.
	apprise: [
		{ key: 'endpoint', type: 'text', fallback: 'http://apprise:8000' },
		{ key: 'urls', type: 'list' },
		{ key: 'urls_env', type: 'text' },
		{ key: 'format', type: 'select', options: ['markdown', 'text', 'html'], fallback: 'markdown' }
	],
	webhook: [
		// url / access_token are not HTML-required: `*_env` is a valid refs-first alternative.
		{ key: 'url', type: 'secret' },
		{ key: 'url_env', type: 'text' },
		{ key: 'method', type: 'select', options: ['POST', 'PUT'], fallback: 'POST' },
		{ key: 'content_type', type: 'text', fallback: 'application/json' },
		{ key: 'headers', type: 'headers' },
		{ key: 'headers_env', type: 'text' },
		{ key: 'secret', type: 'secret' },
		{ key: 'secret_env', type: 'text' },
		{ key: 'signature_header', type: 'text', fallback: 'X-Signature-256' },
		{ key: 'template', type: 'template' }
	],
	express: [
		{ key: 'base_url', type: 'text', required: true },
		{ key: 'group_chat_id', type: 'text', required: true },
		{ key: 'access_token', type: 'secret' },
		{ key: 'access_token_env', type: 'text' },
		{ key: 'recipients', type: 'list' },
		{ key: 'template', type: 'template' }
	],
	novu: [
		{ key: 'base_url', type: 'text', fallback: 'https://api.novu.co' },
		{ key: 'workflow', type: 'text', required: true },
		{ key: 'topic_key', type: 'text', fallback: '{{tag}}' },
		{ key: 'subscriber_id', type: 'text' },
		{ key: 'api_key', type: 'secret' },
		{ key: 'api_key_env', type: 'text' }
	],
	slack: [
		{ key: 'webhook_url', type: 'secret' },
		{ key: 'webhook_url_env', type: 'text' },
		{ key: 'bot_token', type: 'secret' },
		{ key: 'bot_token_env', type: 'text' },
		{ key: 'channel', type: 'text' },
		{ key: 'api_base', type: 'text', fallback: 'https://slack.com/api' },
		{ key: 'template', type: 'template' }
	],
	telegram: [
		{ key: 'api_base', type: 'text', fallback: 'https://api.telegram.org' },
		{ key: 'bot_token', type: 'secret' },
		{ key: 'bot_token_env', type: 'text' },
		{ key: 'chat_id', type: 'text', required: true },
		{
			key: 'parse_mode',
			type: 'select',
			options: ['', 'HTML', 'Markdown', 'MarkdownV2'],
			fallback: ''
		},
		{ key: 'template', type: 'template' }
	],
	smtp: [
		{ key: 'host', type: 'text', required: true },
		{ key: 'port', type: 'number', fallback: '587' },
		{ key: 'from', type: 'text', required: true },
		{ key: 'to', type: 'list', required: true },
		{ key: 'username', type: 'text' },
		{ key: 'password', type: 'secret' },
		{ key: 'password_env', type: 'text' },
		{ key: 'tls', type: 'select', options: ['starttls', 'tls', 'plain'], fallback: 'starttls' },
		{ key: 'subject_template', type: 'text' },
		{ key: 'template', type: 'template' },
		{ key: 'body_format', type: 'select', options: ['text', 'markdown'], fallback: 'text' }
	],
	kafka: [
		{ key: 'brokers', type: 'list', required: true },
		{ key: 'topic', type: 'text', required: true },
		{ key: 'key', type: 'text' },
		{ key: 'template', type: 'template' }
	],
	nats: [
		{ key: 'url', type: 'text' },
		{ key: 'url_env', type: 'text' },
		{ key: 'subject', type: 'text', required: true },
		{ key: 'template', type: 'template' }
	],
	rabbitmq: [
		{ key: 'url', type: 'text' },
		{ key: 'url_env', type: 'text' },
		{ key: 'exchange', type: 'text' },
		{ key: 'routing_key', type: 'text', required: true },
		{ key: 'template', type: 'template' }
	]
};

/** Kinds the form can edit, in picker order. */
export const NOTIFIER_KINDS = Object.keys(NOTIFIER_FIELDS);

/** Channel kinds operators can add from the UI (includes Apprise as `[[notifiers]]`). */
export const ADDABLE_NOTIFIER_KINDS = [...NOTIFIER_KINDS];

export function notifierFieldsForKind(kind: string): NotifierFieldSpec[] {
	return NOTIFIER_FIELDS[kind] ?? [];
}

/** Apprise sink config has no `name` field; other kinds do. */
export function notifierSupportsName(kind: string): boolean {
	return kind !== 'apprise';
}

/** Parse UI `Header-Name: value` lines into a headers map. */
export function parseHeadersField(text: string): Record<string, string> {
	const out: Record<string, string> = {};
	for (const line of text.split(/\r?\n/)) {
		const trimmed = line.trim();
		if (!trimmed || trimmed.startsWith('#')) continue;
		const idx = trimmed.indexOf(':');
		if (idx <= 0) continue;
		const key = trimmed.slice(0, idx).trim();
		const value = trimmed.slice(idx + 1).trim();
		if (key) out[key] = value;
	}
	return out;
}

/** Format a headers map for the form textarea. */
export function formatHeadersField(headers: Record<string, string>): string {
	return Object.entries(headers)
		.map(([key, value]) => `${key}: ${value}`)
		.join('\n');
}

/** Routing tags may be a YAML/TOML list or a comma-separated / scalar string. */
function routingTagsFromValue(value: unknown): string[] {
	if (Array.isArray(value)) return value.map(str).filter(Boolean);
	if (typeof value === 'string' && value.trim()) {
		return value
			.split(',')
			.map((tag) => tag.trim())
			.filter(Boolean);
	}
	return [];
}

function readNotifierRow(row: DesiredMap, type: string): DesiredNotifierDraft {
	const specs = notifierFieldsForKind(type);
	const fields: Record<string, string> = {};
	const redacted: string[] = [];

	for (const spec of specs) {
		const raw = row[spec.key];
		if (spec.type === 'list') {
			const items = asArray(raw).map(str);
			// `apprise.urls` collapses to a single `<redacted>` entry, losing the count.
			if (items.some(isRedacted)) {
				redacted.push(spec.key);
				fields[spec.key] = '';
			} else {
				fields[spec.key] = items.join('\n');
			}
			continue;
		}
		if (spec.type === 'headers') {
			const map = asObject(raw);
			const entries = Object.entries(map);
			if (entries.some(([, value]) => isRedacted(value))) {
				redacted.push(spec.key);
				// Leave blank — Apply restores previous header values (same as secrets).
				fields[spec.key] = '';
			} else {
				fields[spec.key] = formatHeadersField(
					Object.fromEntries(entries.map(([key, value]) => [key, str(value)]))
				);
			}
			continue;
		}
		if (spec.type === 'secret' && (isRedacted(raw) || containsRedactionMarker(raw))) {
			redacted.push(spec.key);
			fields[spec.key] = '';
			continue;
		}
		// NATS / RabbitMQ keep host visible on GET (`amqp://<redacted>@host`) —
		// treat that as a pending secret so Apply does not persist the marker.
		if (
			(spec.key === 'url' || spec.key === 'webhook_url') &&
			containsRedactionMarker(raw)
		) {
			redacted.push(spec.key);
			fields[spec.key] = '';
			continue;
		}
		fields[spec.key] = str(raw);
	}

	const owned = new Set(['type', 'name', 'tags', ...specs.map((spec) => spec.key)]);
	const extra: DesiredMap = {};
	for (const [key, value] of Object.entries(row)) {
		if (!owned.has(key)) extra[key] = value;
	}

	return {
		key: nextDraftKey(),
		type,
		name: str(row.name),
		tags: routingTagsFromValue(row.tags).join(', '),
		fields,
		redacted,
		extra
	};
}

/** Read every `[[notifiers]]` / `notifiers:` delivery channel in document order. */
export function readNotifiers(data: DesiredMap): DesiredNotifierDraft[] {
	return asArray(data.notifiers).map((entry) => {
		const row = asObject(entry);
		return readNotifierRow(row, str(row.type) || 'webhook');
	});
}

export function emptyNotifierDraft(type = 'webhook'): DesiredNotifierDraft {
	const fields: Record<string, string> = {};
	for (const spec of notifierFieldsForKind(type)) {
		fields[spec.key] =
			spec.type === 'select'
				? (spec.fallback ?? spec.options?.[0] ?? '')
				: (spec.fallback ?? '');
	}
	return {
		key: nextDraftKey(),
		type,
		name: '',
		tags: '',
		fields,
		redacted: [],
		extra: {}
	};
}

/** Re-key a draft's fields when the operator switches channel kind. */
export function changeNotifierKind(
	draft: DesiredNotifierDraft,
	type: string
): DesiredNotifierDraft {
	const next = emptyNotifierDraft(type);
	const carried: Record<string, string> = { ...next.fields };
	for (const spec of notifierFieldsForKind(type)) {
		const previous = draft.fields[spec.key];
		if (previous) carried[spec.key] = previous;
	}
	return {
		...next,
		key: draft.key,
		name: draft.name,
		tags: draft.tags,
		fields: carried,
		redacted: draft.redacted.filter((key) => key in carried),
		extra: draft.extra
	};
}

/**
 * Clone a channel for another team/destination. Secrets are cleared so Apply
 * creates a fresh `XRELEASE_UI_*` vault entry instead of sharing refs.
 */
export function duplicateNotifierDraft(draft: DesiredNotifierDraft): DesiredNotifierDraft {
	const specs = notifierFieldsForKind(draft.type);
	const fieldKeys = new Set(specs.map((spec) => spec.key));
	const fields: Record<string, string> = { ...draft.fields };
	for (const spec of specs) {
		if (spec.key.endsWith('_env')) continue;
		if (!fieldKeys.has(`${spec.key}_env`)) continue;
		fields[spec.key] = '';
		fields[`${spec.key}_env`] = '';
	}
	const name = draft.name.trim();
	return {
		key: nextDraftKey(),
		type: draft.type,
		name: name ? `${name}-copy` : '',
		tags: draft.tags,
		fields,
		redacted: [],
		extra: { ...draft.extra }
	};
}

function writeNotifierRow(draft: DesiredNotifierDraft): DesiredMap {
	const row: DesiredMap = { ...draft.extra, type: draft.type };

	// Never persist API redaction placeholders from `extra` (e.g. secret_key after
	// it left the form field list) — they become the real credential on Apply.
	for (const [key, value] of Object.entries(row)) {
		if (value === REDACTED || (Array.isArray(value) && value.some(isRedacted))) {
			delete row[key];
		}
	}

	if (notifierSupportsName(draft.type) && draft.name.trim()) row.name = draft.name.trim();
	else delete row.name;

	const tags = draft.tags
		.split(',')
		.map((tag) => tag.trim())
		.filter(Boolean);
	if (tags.length) row.tags = tags;
	else delete row.tags;

	for (const spec of notifierFieldsForKind(draft.type)) {
		const value = (draft.fields[spec.key] ?? '').trim();

		// A redacted secret left untouched must not be written back: the literal
		// `<redacted>` would silently become the credential. Dropping the key lets
		// Apply restore the previous value (or fall back to XRELEASE_* env).
		if (!value && draft.redacted.includes(spec.key)) {
			delete row[spec.key];
			continue;
		}

		if (!value || value.includes(REDACTED)) {
			delete row[spec.key];
			continue;
		}

			if (spec.type === 'list') {
			const items = value
				.split(/\r?\n/)
				.map((item) => item.trim())
				.filter(Boolean);
			if (items.length) row[spec.key] = items;
			else delete row[spec.key];
			continue;
		}

		if (spec.type === 'headers') {
			const map = parseHeadersField(value);
			for (const [key, headerValue] of Object.entries(map)) {
				if (headerValue === REDACTED || !headerValue) delete map[key];
			}
			if (Object.keys(map).length) row.headers = map;
			else delete row.headers;
			continue;
		}

		if (spec.type === 'number') {
			const n = Number(value);
			if (Number.isFinite(n)) row[spec.key] = n;
			else delete row[spec.key];
			continue;
		}

		row[spec.key] = value;
	}

	// UI Apprise is urls-mode: if the operator set urls, drop YAML-only persistent
	// key fields so Apply cannot leave both modes set (server rejects / double path).
	if (draft.type === 'apprise') {
		const urls = row.urls;
		const hasUrls = Array.isArray(urls) ? urls.length > 0 : typeof urls === 'string' && urls.length > 0;
		if (hasUrls) {
			delete row.config_key;
			delete row.tag;
		}
	}

	// UI express prefers static Bearer: drop HMAC leftovers so Apply cannot keep
	// a poisoned secret_key="<redacted>" and silently take the token endpoint path.
	if (draft.type === 'express') {
		const token = String(row.access_token ?? '').trim();
		if (token && token !== REDACTED) {
			delete row.bot_id;
			delete row.secret_key;
			delete row.secret_key_env;
			delete row.access_token_env;
		}
	}

	return row;
}

/**
 * Write channels back as `notifiers:`. Drop any leftover top-level `apprise`
 * key so Apply cannot reintroduce the removed singleton shape.
 */
export function writeNotifiers(data: DesiredMap, drafts: DesiredNotifierDraft[]): DesiredMap {
	const next: DesiredMap = { ...data };
	delete next.apprise;

	const list = drafts.map(writeNotifierRow);
	if (list.length) next.notifiers = list;
	else delete next.notifiers;

	return next;
}

export function notifierSummary(data: DesiredMap): { apprise: boolean; count: number } {
	const notifiers = asArray(data.notifiers).map(asObject);
	const hasApprise = notifiers.some((row) => str(row.type) === 'apprise');
	return { apprise: hasApprise, count: notifiers.length };
}
