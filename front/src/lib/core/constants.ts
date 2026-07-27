declare const __APP_VERSION__: string;

export const APP_NAME = 'xrelease';
export const APP_VERSION =
	typeof __APP_VERSION__ !== 'undefined' ? __APP_VERSION__ : '0.1.0';
export const APP_REPOSITORY = 'https://github.com/Myskiv-Ivan/xrelease';
export const APP_ISSUES_URL = `${APP_REPOSITORY}/issues`;
/** SPDX id — matches the repo LICENSE and front/package.json. */
export const APP_LICENSE = 'Apache-2.0';
export const APP_LICENSE_URL = `${APP_REPOSITORY}/blob/main/LICENSE`;

export const STORAGE_KEYS = {
	apiKey: 'xrelease_api_key',
	localSession: 'xrelease_local_session',
	oidcSession: 'xrelease_oidc_session',
	oidcPending: 'xrelease_oidc_pending',
	oidcAppRole: 'xrelease_oidc_app_role',
	oidcOrgRoles: 'xrelease_oidc_org_roles',
	theme: 'xrelease_theme',
	autoRefresh: 'xrelease_auto_refresh',
	refreshIntervalMs: 'xrelease_refresh_interval_ms',
	selectedOrg: 'xrelease_selected_org'
} as const;

/** Separator between organization id and inner source id / routing tag. */
export const ORGANIZATION_SEP = '::';

export const DEFAULT_API_URL = 'http://127.0.0.1:8080';

/** Auto-refresh interval options (ms). */
export const REFRESH_INTERVALS = [
	{ label: '15s', value: 15_000 },
	{ label: '30s', value: 30_000 },
	{ label: '1m', value: 60_000 },
	{ label: '5m', value: 300_000 }
] as const;

export const DEFAULT_REFRESH_INTERVAL_MS = 30_000;

/** Inbound webhook endpoints (operator reference — register in each forge). */
export const WEBHOOK_ENDPOINTS = [
	{ forge: 'GitHub', path: '/api/v1/webhooks/github', auth: 'X-Hub-Signature-256 (HMAC)' },
	{ forge: 'GitLab', path: '/api/v1/webhooks/gitlab', auth: 'X-Gitlab-Token' },
	{ forge: 'Gitea / Codeberg', path: '/api/v1/webhooks/gitea', auth: 'X-Webhook-Secret' },
	{ forge: 'Bitbucket', path: '/api/v1/webhooks/bitbucket', auth: 'X-Hub-Signature(-256)' },
	{ forge: 'Docker Hub', path: '/api/v1/webhooks/docker', auth: 'X-Webhook-Secret' },
	{ forge: 'Generic', path: '/api/v1/webhooks/generic', auth: 'X-Webhook-Secret / Bearer' }
] as const;
