import { describe, expect, it } from 'vitest';
import {
	appriseTargetsMissing,
	extractAppDocument,
	extractBootstrapDocument,
	pickConfigKeys
} from './config-layers';
import { parseDesiredDocument } from './desired-document';

const EFFECTIVE = `database:
  url: postgres://localhost/xrelease
api:
  listen: 0.0.0.0:8080
  api_key: <redacted>
log:
  level: info
config_api:
  api_config: true
  ui_config: true
  source: api
defaults:
  interval_secs: 3600
sources:
  - id: nginx
    type: docker
    image: library/nginx
teams:
  - tag: platform
    name: Platform
notifiers:
  - type: webhook
    name: hooks
    url: https://example.com/hook
`;

const DESIRED_WITH_DEFAULTS = `database:
  url: postgres://unused
api:
  listen: 127.0.0.1:0
defaults:
  interval_secs: 3600
sources:
  - id: nginx
    type: docker
    image: library/nginx
`;

describe('config-layers', () => {
	it('pickConfigKeys keeps only listed sections', () => {
		const data = parseDesiredDocument(EFFECTIVE).data;
		expect(Object.keys(pickConfigKeys(data, ['database', 'api'])).sort()).toEqual([
			'api',
			'database'
		]);
	});

	it('extractBootstrapDocument drops app sections', () => {
		const out = extractBootstrapDocument(EFFECTIVE, ['database', 'api', 'log', 'config_api']);
		expect(out).toBeTruthy();
		const parsed = parseDesiredDocument(out!);
		expect(parsed.data.database).toBeTruthy();
		expect(parsed.data.api).toBeTruthy();
		expect(parsed.data.sources).toBeUndefined();
		expect(parsed.data.teams).toBeUndefined();
	});

	it('extractAppDocument strips infra defaults from desired_content', () => {
		const out = extractAppDocument(DESIRED_WITH_DEFAULTS, EFFECTIVE);
		expect(out).toBeTruthy();
		const parsed = parseDesiredDocument(out!);
		expect(parsed.data.sources).toBeTruthy();
		expect(parsed.data.defaults).toBeTruthy();
		expect(parsed.data.database).toBeUndefined();
		expect(parsed.data.api).toBeUndefined();
	});

	it('extractAppDocument falls back to effective when desired is missing', () => {
		const out = extractAppDocument(null, EFFECTIVE);
		expect(out).toBeTruthy();
		const parsed = parseDesiredDocument(out!);
		expect(parsed.data.sources).toBeTruthy();
		expect(parsed.data.database).toBeUndefined();
	});

	it('stripNullLeaves drops null source fields but keeps empty urls arrays', () => {
		const raw = `notifiers:
  - type: apprise
    endpoint: http://localhost:8000
    urls: []
    config_key: null
sources:
  - type: github
    id: tokio
    repo: tokio-rs/tokio
    token: null
    pattern: null
`;
		const out = extractAppDocument(raw);
		expect(out).toBeTruthy();
		expect(out!).not.toMatch(/token:/);
		expect(out!).not.toMatch(/config_key:/);
		expect(out!).toMatch(/urls:\s*\[\s*\]/);
		expect(appriseTargetsMissing(parseDesiredDocument(out!).data)).toBe(true);
	});

	it('appriseTargetsMissing is false when urls are redacted placeholders', () => {
		const data = parseDesiredDocument(`notifiers:
  - type: apprise
    urls: ["<redacted>"]
`).data;
		expect(appriseTargetsMissing(data)).toBe(false);
	});

	it('appriseTargetsMissing is false when urls_env is set', () => {
		const data = parseDesiredDocument(`notifiers:
  - type: apprise
    urls_env: XRELEASE_UI_N_0_APPRISE_URLS
`).data;
		expect(appriseTargetsMissing(data)).toBe(false);
	});
});
