/**
 * Client-side validation + routing projection for the desired-state form editor.
 *
 * Server validate/apply remain authoritative; this catches the common mistakes
 * before a round-trip and powers the Source → Team → Channel overview.
 */

import {
	notifierFieldsForKind,
	primaryFieldsForKind,
	REDACTED,
	type DesiredDefaultsDraft,
	type DesiredMap,
	type DesiredNotifierDraft,
	type DesiredSourceDraft,
	type DesiredTeamDraft
} from './desired-document';
import { asArray, asObject, str } from './desired-map';

export type FieldSeverity = 'error' | 'warning';

export interface FieldIssue {
	severity: FieldSeverity;
	/** Dot path, e.g. `sources.0.fields.repo`, `teams.1.tag`, `defaults.interval_secs`. */
	path: string;
	message: string;
}

export interface ValidationResult {
	ok: boolean;
	errors: FieldIssue[];
	warnings: FieldIssue[];
}

export interface NotifierChannel {
	key: string;
	/** Sink kind: apprise | webhook | smtp | … */
	type: string;
	/** Short human label (url host, name, endpoint). */
	label: string;
	/** Empty tags = wildcard (all events). */
	tags: string[];
	wildcard: boolean;
		/**
		 * Whether this channel satisfies team routing (matches server).
		 * Apprise with empty/non-redacted urls is shown in the graph when it has
		 * routing tags, but `listens: false` so it does not cover orphans.
		 */
		listens: boolean;
}

export interface RoutingEdge {
	sourceKey: string;
	sourceLabel: string;
	sourceType: string;
	teamTag: string | null;
	teamName: string | null;
	channels: NotifierChannel[];
	/** true when a tag is set but no channel matches (and no wildcard). */
	orphaned: boolean;
}

export interface RoutingGraph {
	sources: Array<{ key: string; label: string; type: string; routingTag: string | null }>;
	teams: Array<{ tag: string; name: string; sourceCount: number; channelCount: number }>;
	channels: NotifierChannel[];
	edges: RoutingEdge[];
	untaggedSources: number;
	wildcardChannels: number;
}

const REPO_RE = /^[^/\s]+\/[^/\s]+$/;
const MAVEN_RE = /^[^:\s]+:[^:\s]+$/;
const TAG_RE = /^[A-Za-z0-9][A-Za-z0-9._:-]*$/;
const EDITIONS = new Set(['cloud', 'server']);

function tryCompileRegex(pattern: string): string | null {
	try {
		// eslint-disable-next-line no-new -- compile check only
		new RegExp(pattern);
		return null;
	} catch (err) {
		return err instanceof Error ? err.message : 'invalid regular expression';
	}
}

function looksLikeUrl(value: string): boolean {
	try {
		const url = new URL(value);
		return url.protocol === 'http:' || url.protocol === 'https:';
	} catch {
		return false;
	}
}

function sourceLabel(draft: DesiredSourceDraft): string {
	if (draft.id.trim()) return draft.id.trim();
	const fields = draft.fields;
	return (
		fields.repo ||
		fields.project ||
		fields.image ||
		fields.name ||
		fields.url ||
		draft.type ||
		'source'
	);
}

/** Routing tags may be a YAML list or a comma-separated string. */
function routingTagsOf(value: unknown): string[] {
	if (Array.isArray(value)) return value.map(str).filter(Boolean);
	if (typeof value === 'string' && value.trim()) {
		return value
			.split(',')
			.map((tag) => tag.trim())
			.filter(Boolean);
	}
	return [];
}

/**
 * True when an Apprise notifier row can deliver — matches server
 * `AppriseConfig::is_configured` plus GET redaction placeholders.
 */
function isAppriseConfigured(apprise: DesiredMap): boolean {
	const urls = apprise.urls;
	const hasLiveUrls =
		(Array.isArray(urls) && urls.some((item) => str(item) && str(item) !== REDACTED)) ||
		(typeof urls === 'string' && urls.trim().length > 0 && urls !== REDACTED);
	const hasRedactedUrls =
		(Array.isArray(urls) && urls.some((item) => typeof item === 'string' && item === REDACTED)) ||
		urls === REDACTED;
	const hasUrlsEnv = Boolean(str(apprise.urls_env).trim());
	return hasLiveUrls || hasRedactedUrls || hasUrlsEnv || Boolean(str(apprise.config_key));
}

function channelLabelFromRow(row: DesiredMap, type: string, index: number): string {
	return (
		str(row.name) ||
		str(row.url) ||
		str(row.endpoint) ||
		str(row.workflow) ||
		str(row.base_url) ||
		str(row.brokers) ||
		str(row.subject) ||
		str(row.config_key) ||
		`${type}#${index + 1}`
	);
}

function appriseDraftListens(draft: DesiredNotifierDraft): boolean {
	const hasUrls = (draft.fields.urls ?? '').trim().length > 0;
	const hasUrlsEnv = (draft.fields.urls_env ?? '').trim().length > 0;
	const hasKey =
		typeof draft.extra.config_key === 'string' && draft.extra.config_key.trim().length > 0;
	const pendingUrls = draft.redacted.includes('urls');
	return hasUrls || hasUrlsEnv || hasKey || pendingUrls;
}

/** Channels from editor drafts — `listens` matches server. */
function channelsFromNotifierDrafts(drafts: DesiredNotifierDraft[]): NotifierChannel[] {
	const channels: NotifierChannel[] = [];
	drafts.forEach((draft, index) => {
		const tags = draft.tags
			.split(',')
			.map((tag) => tag.trim())
			.filter(Boolean);
		const listens = draft.type === 'apprise' ? appriseDraftListens(draft) : true;
		if (draft.type === 'apprise' && !listens && tags.length === 0) {
			return;
		}
		channels.push({
			key: `notifier-${draft.key || index}`,
			type: draft.type,
			label:
				draft.type === 'apprise' && !draft.name.trim()
					? (typeof draft.extra.config_key === 'string' && draft.extra.config_key.trim()) ||
						'Apprise'
					: draft.name.trim() ||
						(draft.fields.url ?? '').trim() ||
						(draft.fields.workflow ?? '').trim() ||
						(draft.fields.base_url ?? '').trim() ||
						(draft.fields.endpoint ?? '').trim() ||
						draft.type,
			tags,
			wildcard: tags.length === 0,
			listens
		});
	});
	return channels;
}

/**
 * Delivery channels for the routing graph.
 * Prefer editor drafts when provided (redacted Apprise urls still `listens`).
 */
export function listNotifierChannels(
	data: DesiredMap,
	drafts?: DesiredNotifierDraft[]
): NotifierChannel[] {
	if (drafts) return channelsFromNotifierDrafts(drafts);

	const channels: NotifierChannel[] = [];

	asArray(data.notifiers).forEach((entry, index) => {
		const row = asObject(entry);
		const type = str(row.type) || 'unknown';
		const tags = routingTagsOf(row.tags);
		const listens = type === 'apprise' ? isAppriseConfigured(row) : true;
		if (type === 'apprise' && !listens && tags.length === 0) {
			return;
		}
		channels.push({
			key: `notifier-${index}`,
			type,
			label:
				type === 'apprise' && !str(row.name)
					? str(row.config_key) || 'Apprise'
					: channelLabelFromRow(row, type, index),
			tags,
			wildcard: tags.length === 0,
			listens
		});
	});

	return channels;
}

export function buildRoutingGraph(input: {
	sources: DesiredSourceDraft[];
	teams: DesiredTeamDraft[];
	data: DesiredMap;
	/** Edit form drafts — redacted Apprise urls still listen until re-entered. */
	notifiers?: DesiredNotifierDraft[];
}): RoutingGraph {
	const channels = listNotifierChannels(input.data, input.notifiers);
	const listening = channels.filter((c) => c.listens);
	const wildcardChannels = listening.filter((c) => c.wildcard);
	const teamName = new Map(
		input.teams.filter((t) => t.tag.trim()).map((t) => [t.tag.trim(), t.name.trim()] as const)
	);

	const sources = input.sources.map((source) => ({
		key: source.key,
		label: sourceLabel(source),
		type: source.type,
		routingTag: source.routing_tag.trim() || null
	}));

	const edges: RoutingEdge[] = sources.map((source) => {
		const tag = source.routingTag;
		const matching = tag
			? listening.filter((c) => c.wildcard || c.tags.includes(tag))
			: listening.filter((c) => c.wildcard);
		const orphaned = Boolean(tag) && matching.length === 0 && listening.length > 0;
		return {
			sourceKey: source.key,
			sourceLabel: source.label,
			sourceType: source.type,
			teamTag: tag,
			teamName: tag ? (teamName.get(tag) ?? null) : null,
			channels: matching,
			orphaned
		};
	});

	// Collapse duplicate tags: the editor lets you type the same tag twice (it is
	// flagged as an error but still renders), and consumers key their lists on
	// `tag`, so leaving duplicates in would throw `each_key_duplicate`.
	const uniqueTeams = [
		...new Map(
			input.teams.filter((t) => t.tag.trim()).map((t) => [t.tag.trim(), t] as const)
		).values()
	];

	const teams = uniqueTeams.map((t) => {
		const tag = t.tag.trim();
		return {
			tag,
			name: t.name.trim(),
			sourceCount: sources.filter((s) => s.routingTag === tag).length,
			channelCount: listening.filter((c) => c.wildcard || c.tags.includes(tag)).length
		};
	});

	// Tags used by sources but missing from the catalogue.
	for (const source of sources) {
		if (!source.routingTag) continue;
		if (teams.some((t) => t.tag === source.routingTag)) continue;
		teams.push({
			tag: source.routingTag,
			name: '',
			sourceCount: sources.filter((s) => s.routingTag === source.routingTag).length,
			channelCount: listening.filter(
				(c) => c.wildcard || c.tags.includes(source.routingTag!)
			).length
		});
	}

	return {
		sources,
		teams,
		channels,
		edges,
		untaggedSources: sources.filter((s) => !s.routingTag).length,
		wildcardChannels: wildcardChannels.length
	};
}

/** Accepts a bare `user@host` or an RFC 5322 `Display Name <user@host>` mailbox. */
function looksLikeMailbox(value: string): boolean {
	const angle = value.match(/<([^>]*)>\s*$/);
	const addr = (angle ? angle[1] : value).trim();
	if (!addr || /\s/.test(addr)) return false;
	const at = addr.lastIndexOf('@');
	return at > 0 && at < addr.length - 1;
}

function validateNotifiers(
	notifiers: DesiredNotifierDraft[],
	push: (severity: FieldSeverity, path: string, message: string) => void
): void {
	notifiers.forEach((notifier, index) => {
		const prefix = `notifiers.${index}`;

		const specs = notifierFieldsForKind(notifier.type);
		if (specs.length === 0) {
			push('warning', `${prefix}.type`, `Unknown channel kind “${notifier.type}”`);
		}

		for (const tag of notifier.tags.split(',').map((t) => t.trim()).filter(Boolean)) {
			if (!TAG_RE.test(tag)) {
				push('error', `${prefix}.tags`, `Invalid routing tag “${tag}”`);
			}
		}

		for (const spec of specs) {
			const path = `${prefix}.fields.${spec.key}`;
			const value = (notifier.fields[spec.key] ?? '').trim();

			if (!value) {
				if (notifier.redacted.includes(spec.key)) {
					push(
						'warning',
						path,
						`“${spec.key}” is stored but hidden — leave blank to keep it, or paste a new value to rotate`
					);
				} else if (spec.required) {
					// Refs-first: `url` / `access_token` may be empty when `*_env` is set.
					const envCompanion = (notifier.fields[`${spec.key}_env`] ?? '').trim();
					if (!envCompanion) {
						push(
							'error',
							path,
							`${spec.key} is required for “${notifier.type}” channels`
						);
					}
				}
				continue;
			}

			if (spec.type === 'headers') {
				for (const line of value.split(/\r?\n/).map((v) => v.trim()).filter(Boolean)) {
					if (line.startsWith('#')) continue;
					if (!line.includes(':') || line.indexOf(':') === 0) {
						push(
							'error',
							path,
							`Header line must be “Name: value” (got “${line}”)`
						);
					}
				}
				continue;
			}

			if (spec.type === 'select' && spec.options && !spec.options.includes(value)) {
				push('error', path, `${spec.key} must be one of ${spec.options.join(', ')}`);
				continue;
			}

			switch (spec.key) {
				case 'url':
				case 'base_url':
				case 'endpoint':
					if (notifier.type === 'nats' || notifier.type === 'rabbitmq') {
						if (!/^[a-z][a-z0-9+.-]*:\/\/.+/i.test(value)) {
							push('error', path, `${spec.key} must be a URL with a scheme`);
						}
					} else if (!looksLikeUrl(value)) {
						push('error', path, `${spec.key} must be an http(s) URL`);
					}
					break;
				case 'port': {
					const port = Number(value);
					if (!Number.isInteger(port) || port < 1 || port > 65535) {
						push('error', path, 'Port must be between 1 and 65535');
					}
					break;
				}
				case 'from':
					if (!looksLikeMailbox(value)) {
						push('error', path, 'From must be an email address');
					}
					break;
				case 'to':
					for (const addr of value.split(/\r?\n/).map((v) => v.trim()).filter(Boolean)) {
						if (!looksLikeMailbox(addr)) {
							push('error', path, `“${addr}” is not a valid email address`);
						}
					}
					break;
				case 'brokers':
					for (const broker of value.split(/\r?\n/).map((v) => v.trim()).filter(Boolean)) {
						if (!/^[\w.-]+:\d+$/.test(broker)) {
							push('warning', path, `“${broker}” is not host:port`);
						}
					}
					break;
				default:
					break;
			}
		}

		if (notifier.type === 'apprise') {
			const hasUrls = (notifier.fields.urls ?? '').trim().length > 0;
			const hasUrlsEnv = (notifier.fields.urls_env ?? '').trim().length > 0;
			const keyFromExtra =
				typeof notifier.extra.config_key === 'string'
					? notifier.extra.config_key.trim()
					: '';
			const hasKey = keyFromExtra.length > 0;
			const pending = notifier.redacted.includes('urls');
			if (hasUrls && hasKey) {
				push(
					'error',
					`${prefix}.fields.urls`,
					'Apprise takes either urls or config_key, not both — clear urls or remove config_key from YAML'
				);
			} else if (!hasUrls && !hasUrlsEnv && !hasKey && !pending) {
				push(
					'error',
					`${prefix}.fields.urls`,
					'Apprise needs urls, urls_env, or a YAML config_key'
				);
			} else if (!hasUrls && !hasKey && pending) {
				push(
					'warning',
					`${prefix}.fields.urls`,
					'No urls shown — paste new targets, or leave blank to keep saved urls'
				);
			}
		}

		if (notifier.type === 'webhook') {
			const url = (notifier.fields.url ?? '').trim();
			const urlEnv = (notifier.fields.url_env ?? '').trim();
			const pending = notifier.redacted.includes('url');
			if (!url && !urlEnv && !pending) {
				push(
					'error',
					`${prefix}.fields.url`,
					'Webhook needs url or url_env'
				);
			}
		}

		if (notifier.type === 'express') {
			const token = (notifier.fields.access_token ?? '').trim();
			const tokenEnv = (notifier.fields.access_token_env ?? '').trim();
			const pending = notifier.redacted.includes('access_token');
			if (!token && !tokenEnv && !pending) {
				push(
					'error',
					`${prefix}.fields.access_token`,
					'Express needs access_token or access_token_env (or XRELEASE_EXPRESS_ACCESS_TOKEN)'
				);
			}
		}

		if (notifier.type === 'novu') {
			const topic = (notifier.fields.topic_key ?? '').trim();
			const subscriber = (notifier.fields.subscriber_id ?? '').trim();
			if (!topic && !subscriber) {
				push(
					'error',
					`${prefix}.fields.topic_key`,
					'Novu needs topic_key or subscriber_id'
				);
			}
			const apiKey = (notifier.fields.api_key ?? '').trim();
			const apiKeyEnv = (notifier.fields.api_key_env ?? '').trim();
			const pendingKey = notifier.redacted.includes('api_key');
			if (!apiKey && !apiKeyEnv && !pendingKey) {
				push(
					'error',
					`${prefix}.fields.api_key`,
					'Novu needs api_key or api_key_env (or XRELEASE_NOVU_API_KEY on the server)'
				);
			}
		}

		if (notifier.type === 'slack') {
			const webhook =
				(notifier.fields.webhook_url ?? '').trim() ||
				(notifier.fields.webhook_url_env ?? '').trim() ||
				(notifier.redacted.includes('webhook_url') ? 'pending' : '');
			const bot =
				(notifier.fields.bot_token ?? '').trim() ||
				(notifier.fields.bot_token_env ?? '').trim() ||
				(notifier.redacted.includes('bot_token') ? 'pending' : '');
			const channel = (notifier.fields.channel ?? '').trim();
			if (webhook && bot) {
				push(
					'error',
					`${prefix}.fields.webhook_url`,
					'Slack: use webhook_url or bot_token+channel, not both'
				);
			} else if (!webhook && !bot) {
				push(
					'error',
					`${prefix}.fields.webhook_url`,
					'Slack needs webhook_url (or env) or bot_token+channel'
				);
			} else if (bot && !channel) {
				push(
					'error',
					`${prefix}.fields.channel`,
					'Slack bot mode needs channel'
				);
			}
		}

		if (notifier.type === 'telegram') {
			const token =
				(notifier.fields.bot_token ?? '').trim() ||
				(notifier.fields.bot_token_env ?? '').trim();
			const pending = notifier.redacted.includes('bot_token');
			if (!token && !pending) {
				push(
					'error',
					`${prefix}.fields.bot_token`,
					'Telegram needs bot_token or bot_token_env (or XRELEASE_TELEGRAM_BOT_TOKEN)'
				);
			}
			const mode = (notifier.fields.parse_mode ?? '').trim();
			if (mode && !['HTML', 'Markdown', 'MarkdownV2'].includes(mode)) {
				push(
					'error',
					`${prefix}.fields.parse_mode`,
					'parse_mode must be HTML, Markdown, or MarkdownV2'
				);
			}
		}

		if (notifier.type === 'rabbitmq' || notifier.type === 'nats') {
			const url = (notifier.fields.url ?? '').trim();
			const urlEnv = (notifier.fields.url_env ?? '').trim();
			const pending = notifier.redacted.includes('url');
			const globalHint =
				notifier.type === 'nats' ? 'XRELEASE_NATS_URL' : 'XRELEASE_RABBITMQ_URL';
			if (!url && !urlEnv && !pending) {
				push(
					'error',
					`${prefix}.fields.url`,
					`${notifier.type === 'nats' ? 'NATS' : 'RabbitMQ'} needs url or url_env (or ${globalHint})`
				);
			}
		}

		if (notifier.type === 'smtp') {
			const user = (notifier.fields.username ?? '').trim();
			const pass = (notifier.fields.password ?? '').trim();
			const passEnv = (notifier.fields.password_env ?? '').trim();
			if (user && !pass && !passEnv && !notifier.redacted.includes('password')) {
				push(
					'warning',
					`${prefix}.fields.password`,
					'Username without a password — set password, password_env, or XRELEASE_SMTP_PASSWORD'
				);
			}
		}
	});
}

export function validateDesiredDrafts(input: {
	defaults: DesiredDefaultsDraft;
	teams: DesiredTeamDraft[];
	sources: DesiredSourceDraft[];
	notifiers?: DesiredNotifierDraft[];
	data: DesiredMap;
}): ValidationResult {
	const errors: FieldIssue[] = [];
	const warnings: FieldIssue[] = [];

	const push = (severity: FieldSeverity, path: string, message: string) => {
		(severity === 'error' ? errors : warnings).push({ severity, path, message });
	};

	// Defaults
	if (input.defaults.interval_secs !== '' && Number(input.defaults.interval_secs) <= 0) {
		push('error', 'defaults.interval_secs', 'Interval must be greater than 0');
	}
	if (input.defaults.jitter_secs !== '' && Number(input.defaults.jitter_secs) < 0) {
		push('error', 'defaults.jitter_secs', 'Jitter cannot be negative');
	}
	if (
		input.defaults.upstream_requests_per_minute !== '' &&
		Number(input.defaults.upstream_requests_per_minute) < 0
	) {
		push('error', 'defaults.upstream_requests_per_minute', 'Rate limit cannot be negative');
	}

	const opsTag = input.defaults.ops_routing_tag.trim();
	if (opsTag) {
		const catalog = new Set(
			input.teams.map((team) => team.tag.trim()).filter(Boolean)
		);
		if (catalog.size > 0 && !catalog.has(opsTag)) {
			push(
				'warning',
				'defaults.ops_routing_tag',
				`Ops routing tag “${opsTag}” is not listed in Teams`
			);
		}
	}

	// Teams
	const seenTags = new Set<string>();
	input.teams.forEach((team, index) => {
		const tag = team.tag.trim();
		const name = team.name.trim();
		if (!tag && name) {
			push('error', `teams.${index}.tag`, 'Team tag is required when a name is set');
			return;
		}
		if (!tag) return;
		if (!TAG_RE.test(tag)) {
			push(
				'error',
				`teams.${index}.tag`,
				'Tag must start with a letter/digit and use only A–Z, 0–9, `.` `_` `:` `-`'
			);
		}
		const lower = tag.toLowerCase();
		if (seenTags.has(lower)) {
			push('error', `teams.${index}.tag`, `Duplicate team tag “${tag}”`);
		}
		seenTags.add(lower);
	});

	const catalog = new Set(
		input.teams.map((t) => t.tag.trim()).filter(Boolean)
	);

	// Sources
	const seenIds = new Set<string>();
	input.sources.forEach((source, index) => {
		const prefix = `sources.${index}`;
		if (!source.type.trim()) {
			push('error', `${prefix}.type`, 'Source kind is required');
		}

		const id = source.id.trim();
		if (id) {
			const lower = id.toLowerCase();
			if (seenIds.has(lower)) {
				push('error', `${prefix}.id`, `Duplicate source id “${id}”`);
			}
			seenIds.add(lower);
		}

		for (const field of primaryFieldsForKind(source.type)) {
			const value = (source.fields[field.key] ?? '').trim();
			const path = `${prefix}.fields.${field.key}`;
			if (field.required && !value) {
				push('error', path, `${field.key} is required for type “${source.type}”`);
				continue;
			}
			if (!value) continue;

			switch (field.key) {
				case 'repo':
					if (!REPO_RE.test(value)) {
						push('error', path, 'Use owner/repo (exactly one slash, no spaces)');
					}
					break;
				case 'project':
					if (!value.includes('/') || /\s/.test(value)) {
						push('error', path, 'Use a GitLab path like group/project');
					}
					break;
				case 'image':
					if (/\s/.test(value)) {
						push('error', path, 'Image path must not contain spaces');
					}
					break;
				case 'name':
					if (source.type === 'maven' && !MAVEN_RE.test(value)) {
						push('error', path, 'Maven name must be group:artifact');
					} else if (/\s/.test(value)) {
						push('error', path, 'Package name must not contain spaces');
					}
					break;
				case 'url':
					if (!looksLikeUrl(value)) {
						push('error', path, 'Feed URL must be http(s)');
					}
					break;
				case 'host':
					if (!looksLikeUrl(value) && !/^[\w.-]+(:\d+)?$/.test(value)) {
						push('error', path, 'Host should be an http(s) URL or hostname');
					}
					break;
				case 'edition': {
					const edition = value.toLowerCase();
					if (!EDITIONS.has(edition)) {
						push('error', path, 'Edition must be cloud or server');
					} else if (edition === 'server' && !(source.fields.host ?? '').trim()) {
						push('error', `${prefix}.fields.host`, 'Bitbucket server edition requires host');
					}
					break;
				}
				case 'registry':
					if (value && !looksLikeUrl(value) && !/^[\w.-]+(:\d+)?$/.test(value)) {
						push('warning', path, 'Registry looks unusual — prefer a full registry URL');
					}
					break;
				default:
					break;
			}
		}

		if (source.pattern.trim()) {
			const err = tryCompileRegex(source.pattern.trim());
			if (err) push('error', `${prefix}.pattern`, `Invalid include pattern: ${err}`);
		}
		if (source.exclude_pattern.trim()) {
			const err = tryCompileRegex(source.exclude_pattern.trim());
			if (err) push('error', `${prefix}.exclude_pattern`, `Invalid exclude pattern: ${err}`);
		}

		if (source.prerelease_tags.trim() && !source.include_prerelease) {
			push(
				'warning',
				`${prefix}.prerelease_tags`,
				'Pre-release tags are ignored unless “Include pre-releases” is enabled'
			);
		}

		if (source.interval_secs.trim()) {
			const n = Number(source.interval_secs);
			if (!Number.isFinite(n) || n <= 0) {
				push('error', `${prefix}.interval_secs`, 'interval_secs must be > 0');
			}
		}

		const tag = source.routing_tag.trim();
		if (tag && catalog.size > 0 && !catalog.has(tag)) {
			push(
				'warning',
				`${prefix}.routing_tag`,
				`Routing tag “${tag}” is not listed in Teams`
			);
		}
	});

	if (input.notifiers) {
		validateNotifiers(input.notifiers, push);
	}

	const graph = buildRoutingGraph(input);
	for (const edge of graph.edges) {
		if (edge.orphaned) {
			// Match server validate_team_routing: tagged sources need a matching
			// channel once at least one listening channel exists. No channels yet
			// is a supported incremental variant (warning below).
			push(
				'error',
				`sources.${input.sources.findIndex((s) => s.key === edge.sourceKey)}.routing_tag`,
				`No channel listens for “${edge.teamTag}” — select that tag on a delivery channel, or leave a channel’s tags empty (wildcard)`
			);
		}
	}

	if (opsTag) {
		const listening = graph.channels.filter((c) => c.listens);
		if (listening.length > 0) {
			const matched = listening.some((c) => c.wildcard || c.tags.includes(opsTag));
			if (!matched) {
				push(
					'error',
					'defaults.ops_routing_tag',
					`No channel listens for ops tag “${opsTag}” — meta-alerts on dead outbox / tripped breakers will fail to deliver`
				);
			}
		} else {
			push(
				'warning',
				'defaults.ops_routing_tag',
				`Ops tag “${opsTag}” has no delivery channel yet — set a matching channel before relying on ops alerts`
			);
		}
	}

	const listening = graph.channels.filter((c) => c.listens);
	if (listening.length === 0 && input.sources.length > 0) {
		push(
			'warning',
			'notifiers',
			'No delivery channels configured — teams and sources can be applied now; add a channel when you want notifications'
		);
	} else if (graph.channels.length === 0) {
		push(
			'warning',
			'notifiers',
			'No delivery channels configured — add one in Channels, or releases go nowhere'
		);
	}

	return {
		ok: errors.length === 0,
		errors,
		warnings
	};
}

export function issuesForPath(issues: FieldIssue[], path: string): FieldIssue[] {
	return issues.filter((issue) => issue.path === path);
}

export function firstIssueMessage(issues: FieldIssue[], path: string): string | null {
	return issuesForPath(issues, path)[0]?.message ?? null;
}
