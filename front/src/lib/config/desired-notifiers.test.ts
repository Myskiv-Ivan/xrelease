import { describe, expect, it, beforeEach } from 'vitest';
import {
	changeNotifierKind,
	emptyNotifierDraft,
	notifierFieldsForKind,
	parseDesiredDocument,
	readNotifiers,
	resetDraftKeySeq,
	stringifyDesiredDocument,
	writeNotifiers,
	type DesiredMap
} from './desired-document';
import { validateDesiredDrafts } from './desired-validation';

const SAMPLE = `notifiers:
  - type: apprise
    endpoint: http://apprise:8000
    urls: ["tgram://token/chat"]
    tags: [platform]
  - type: smtp
    name: ops mail
    host: smtp.example.com
    from: xrelease@example.com
    to: [ops@example.com, sre@example.com]
    password: hunter2
    tags: [platform, security]
  - type: express
    base_url: https://cts.example.com
    group_chat_id: g-1
    access_token: permanent-bearer
`;

/** Mirrors what `GET /api/v1/config` returns: secrets replaced by a sentinel. */
const REDACTED_SAMPLE = `notifiers:
  - type: apprise
    endpoint: http://apprise:8000
    urls: ["<redacted>"]
  - type: smtp
    host: smtp.example.com
    from: xrelease@example.com
    to: [ops@example.com]
    password: <redacted>
  - type: webhook
    url: https://hooks.example/x
    secret: <redacted>
    headers:
      Authorization: <redacted>
`;

function drafts(doc: string) {
	return readNotifiers(parseDesiredDocument(doc).data);
}

function baseDrafts() {
	return {
		defaults: {
			interval_secs: 86400 as number | '',
			jitter_secs: 3600 as number | '',
			upstream_requests_per_minute: 0 as number | '',
			poll_on_startup: true,
			notify_schedule: '',
			ops_routing_tag: ''
		},
		teams: [],
		sources: [],
		data: {} as DesiredMap
	};
}

describe('notifier drafts', () => {
	beforeEach(() => resetDraftKeySeq());

	it('reads [[notifiers]] entries including type=apprise', () => {
		const list = drafts(SAMPLE);
		expect(list).toHaveLength(3);
		expect(list[0]).toMatchObject({ type: 'apprise', tags: 'platform' });
		expect(list[0].fields.urls).toBe('tgram://token/chat');
		expect(list[1]).toMatchObject({ type: 'smtp', name: 'ops mail' });
		expect(list[1].fields.to).toBe('ops@example.com\nsre@example.com');
		expect(list[1].tags).toBe('platform, security');
		expect(list[2]).toMatchObject({ type: 'express' });
	});

	it('round-trips channels back into notifiers only', () => {
		const parsed = parseDesiredDocument(SAMPLE);
		const out = writeNotifiers(parsed.data, readNotifiers(parsed.data));
		expect('apprise' in out).toBe(false);

		const list = out.notifiers as DesiredMap[];
		expect(list).toHaveLength(3);
		expect(list[0]).toMatchObject({
			type: 'apprise',
			urls: ['tgram://token/chat'],
			tags: ['platform']
		});
		expect(list[1]).toMatchObject({
			type: 'smtp',
			host: 'smtp.example.com',
			password: 'hunter2',
			to: ['ops@example.com', 'sre@example.com'],
			tags: ['platform', 'security']
		});
		expect(list[2]).toMatchObject({ type: 'express', access_token: 'permanent-bearer' });
	});

	it('drops redacted placeholders from extra and HMAC fields when access_token is set', () => {
		resetDraftKeySeq();
		const draft = emptyNotifierDraft('express');
		draft.fields.base_url = 'https://cts.example.com';
		draft.fields.group_chat_id = 'g-1';
		draft.fields.access_token = 'SFMyNTY.example-token';
		draft.extra = {
			bot_id: '1586cad1-d017-5546-ad90-2b57a7ac668a',
			secret_key: '<redacted>'
		};
		const out = writeNotifiers({}, [draft]);
		const row = (out.notifiers as DesiredMap[])[0];
		expect(row.access_token).toBe('SFMyNTY.example-token');
		expect(row.secret_key).toBeUndefined();
		expect(row.bot_id).toBeUndefined();
	});

	it('never writes a redacted placeholder back as the credential', () => {
		const parsed = parseDesiredDocument(REDACTED_SAMPLE);
		const list = readNotifiers(parsed.data);

		expect(list[0].redacted).toContain('urls');
		expect(list[0].fields.urls).toBe('');
		expect(list[1].redacted).toContain('password');
		expect(list[2].redacted).toContain('secret');

		const out = writeNotifiers(parsed.data, list);
		const written = out.notifiers as DesiredMap[];

		expect(written[0].urls).toBeUndefined();
		expect(written[1].password).toBeUndefined();
		expect(written[2].secret).toBeUndefined();
		expect(JSON.stringify(out)).not.toContain('<redacted>@');
	});

	it('keeps env refs when GET only shows redacted placeholders', () => {
		const raw = `notifiers:
  - type: telegram
    chat_id: "-100"
    bot_token: <redacted>
    bot_token_env: XRELEASE_UI_N_0_TELEGRAM_BOT
  - type: express
    base_url: https://cts.example.com
    group_chat_id: g-1
    access_token: <redacted>
    access_token_env: XRELEASE_UI_N_1_EXPRESS_TOKEN
`;
		const list = readNotifiers(parseDesiredDocument(raw).data);
		expect(list[0].redacted).toContain('bot_token');
		expect(list[0].fields.bot_token_env).toBe('XRELEASE_UI_N_0_TELEGRAM_BOT');
		expect(list[1].redacted).toContain('access_token');
		expect(list[1].fields.access_token_env).toBe('XRELEASE_UI_N_1_EXPRESS_TOKEN');

		const written = writeNotifiers({}, list).notifiers as DesiredMap[];
		expect(written[0].bot_token).toBeUndefined();
		expect(written[0].bot_token_env).toBe('XRELEASE_UI_N_0_TELEGRAM_BOT');
		expect(written[1].access_token).toBeUndefined();
		expect(written[1].access_token_env).toBe('XRELEASE_UI_N_1_EXPRESS_TOKEN');
		expect(JSON.stringify(written)).not.toContain('<redacted>');
	});

	it('keeps a re-entered secret', () => {
		const parsed = parseDesiredDocument(REDACTED_SAMPLE);
		const list = readNotifiers(parsed.data);
		list[1].fields.password = 'new-password';
		const written = writeNotifiers(parsed.data, list).notifiers as DesiredMap[];
		expect(written[1].password).toBe('new-password');
	});

	it('omits redacted webhook headers so Apply can restore them', () => {
		const parsed = parseDesiredDocument(REDACTED_SAMPLE);
		const list = readNotifiers(parsed.data);
		expect(list[2].redacted).toContain('headers');
		expect(list[2].fields.headers).toBe('');
		const written = writeNotifiers(parsed.data, list).notifiers as DesiredMap[];
		expect(written[2].headers).toBeUndefined();
	});

	it('writes webhook headers from Name: value lines', () => {
		resetDraftKeySeq();
		const draft = emptyNotifierDraft('webhook');
		draft.fields.url = 'https://hooks.example/x';
		draft.fields.headers = 'Authorization: Bearer tok\nX-Team: platform';
		const out = writeNotifiers({}, [draft]);
		const row = (out.notifiers as DesiredMap[])[0];
		expect(row.headers).toEqual({
			Authorization: 'Bearer tok',
			'X-Team': 'platform'
		});
	});

	it('drops the apprise key when writing notifiers', () => {
		const parsed = parseDesiredDocument(SAMPLE);
		const out = writeNotifiers({ ...parsed.data, apprise: { urls: ['mailto://x'] } }, readNotifiers(parsed.data));
		expect('apprise' in out).toBe(false);
		expect(out.notifiers).toHaveLength(3);
	});

	it('drops the notifiers key when the last entry is removed', () => {
		const parsed = parseDesiredDocument(SAMPLE);
		const out = writeNotifiers(parsed.data, []);
		expect('notifiers' in out).toBe(false);
	});

	it('round-trips through TOML table arrays', () => {
		const toml = `[[notifiers]]
type = "apprise"
endpoint = "http://apprise:8000"
urls = ["mailto://ops@example.com"]

[[notifiers]]
type = "smtp"
host = "smtp.example.com"
from = "bot@example.com"
to = ["ops@example.com"]
port = 2525
`;
		const parsed = parseDesiredDocument(toml, 'toml');
		expect(parsed.format).toBe('toml');

		const list = readNotifiers(parsed.data);
		expect(list).toHaveLength(2);
		list[1].fields.host = 'smtp.internal';

		const out = stringifyDesiredDocument({
			format: 'toml',
			data: writeNotifiers(parsed.data, list)
		});
		expect(out).not.toContain('[apprise]');
		expect(out).toContain('[[notifiers]]');
		expect(out).toContain('smtp.internal');

		const reparsed = readNotifiers(parseDesiredDocument(out, 'toml').data);
		expect(reparsed[1].fields.host).toBe('smtp.internal');
		expect(reparsed[1].fields.port).toBe('2525');
		expect(reparsed[0].fields.urls).toBe('mailto://ops@example.com');
	});

	it('adds a first channel to a document that had none', () => {
		const parsed = parseDesiredDocument('sources: []\n');
		const draft = emptyNotifierDraft('webhook');
		draft.fields.url = 'https://hooks.example/new';
		draft.tags = 'platform';

		const out = writeNotifiers(parsed.data, [draft]);
		expect(out.notifiers).toEqual([
			{
				type: 'webhook',
				url: 'https://hooks.example/new',
				method: 'POST',
				content_type: 'application/json',
				signature_header: 'X-Signature-256',
				tags: ['platform']
			}
		]);
	});

	it('re-keys fields when the channel kind changes, carrying shared keys', () => {
		const draft = emptyNotifierDraft('nats');
		draft.fields.url = 'nats://bus:4222';
		draft.fields.subject = 'releases';
		draft.tags = 'platform';

		const next = changeNotifierKind(draft, 'rabbitmq');
		expect(next.type).toBe('rabbitmq');
		expect(next.key).toBe(draft.key);
		expect(next.tags).toBe('platform');
		expect(next.fields.url).toBe('nats://bus:4222');
		expect(next.fields.subject).toBeUndefined();
		expect(next.fields.routing_key).toBe('');
	});

	it('exposes required fields per kind', () => {
		const required = (kind: string) =>
			notifierFieldsForKind(kind)
				.filter((spec) => spec.required)
				.map((spec) => spec.key);
		expect(required('smtp')).toEqual(['host', 'from', 'to']);
		expect(required('express')).toEqual(['base_url', 'group_chat_id']);
		expect(required('webhook')).toEqual([]);
		expect(required('apprise')).toEqual([]);
		expect(notifierFieldsForKind('apprise').map((s) => s.key)).toEqual([
			'endpoint',
			'urls',
			'urls_env',
			'format'
		]);
	});

	it('preserves YAML-only Apprise config_key/tag in extra and strips them when urls are set', () => {
		const doc = `notifiers:
  - type: apprise
    endpoint: http://apprise:8000
    config_key: release-channels
    tag: platform
    format: markdown
`;
		const parsed = parseDesiredDocument(doc);
		const list = readNotifiers(parsed.data);
		expect(list).toHaveLength(1);
		expect(list[0].extra.config_key).toBe('release-channels');
		expect(list[0].extra.tag).toBe('platform');

		list[0].fields.urls = 'mailto://ops@example.com';
		const out = writeNotifiers(parsed.data, list);
		const row = (out.notifiers as DesiredMap[])[0];
		expect(row.urls).toEqual(['mailto://ops@example.com']);
		expect(row.config_key).toBeUndefined();
		expect(row.tag).toBeUndefined();
	});

	it('flags missing Apprise urls as an error', () => {
		const draft = emptyNotifierDraft('apprise');
		const result = validateDesiredDrafts({
			...baseDrafts(),
			notifiers: [draft]
		});
		expect(result.ok).toBe(false);
		expect(result.errors.some((e) => e.path.includes('fields.urls'))).toBe(true);
	});

	it('accepts Apprise urls_env without inline urls', () => {
		const draft = emptyNotifierDraft('apprise');
		draft.fields.urls_env = 'XRELEASE_UI_N_0_APPRISE_URLS';
		const result = validateDesiredDrafts({
			...baseDrafts(),
			notifiers: [draft]
		});
		expect(result.ok).toBe(true);
	});

	it('accepts webhook url_env without inline url', () => {
		const draft = emptyNotifierDraft('webhook');
		draft.fields.url = '';
		draft.fields.url_env = 'XRELEASE_UI_N_0_WEBHOOK_URL';
		const result = validateDesiredDrafts({
			...baseDrafts(),
			notifiers: [draft]
		});
		expect(result.ok).toBe(true);
	});

	it('warns when Apprise urls are redacted placeholders', () => {
		const draft = emptyNotifierDraft('apprise');
		draft.redacted = ['urls'];
		const result = validateDesiredDrafts({
			...baseDrafts(),
			notifiers: [draft]
		});
		expect(result.ok).toBe(true);
		expect(result.warnings.some((e) => e.path.includes('fields.urls'))).toBe(true);
	});
});
