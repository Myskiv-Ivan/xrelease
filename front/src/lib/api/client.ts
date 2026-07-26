import type {
	AuthCreateUserRequest,
	AuthLoginRequest,
	AuthLoginResponse,
	AuthMethodsResponse,
	AuthMeResponse,
	AuthUserListResponse,
	AuthUserView,
	CheckResponse,
	ConfigApplyResponse,
	OrganizationConfigView,
	OrganizationListResponse,
	OrganizationRevisionsResponse,
	ConfigRevisionsResponse,
	ConfigSchema,
	ConfigValidateResponse,
	ConfigView,
	HealthResponse,
	NotifierListResponse,
	NotifierTestRequest,
	NotifierTestResponse,
	OutboxListResponse,
	OutboxRequeueResponse,
	ReadyResponse,
	SourceDetail,
	StatusResponse,
	TeamListResponse
} from './types';

import { isOidcEnabled } from '$lib/auth/config';
import { getBearerToken, getStoredApiKey } from '$lib/auth/credentials';
import { refreshOidcTokensIfNeeded } from '$lib/auth/oidc';
import { notifyAuthProfileSync } from '$lib/auth/sync';
import { readUiSetting } from '$lib/config/runtime';
import { DEFAULT_API_URL } from '$lib/core/constants';

export { getStoredApiKey, setStoredApiKey, clearStoredApiKey, getBearerToken } from '$lib/auth/credentials';

export function getApiBaseUrl(): string {
	const fromEnv = readUiSetting('VITE_API_URL');
	if (fromEnv === '') return '';
	if (typeof fromEnv === 'string' && fromEnv.length > 0) {
		return fromEnv.replace(/\/$/, '');
	}
	return DEFAULT_API_URL;
}

export class ApiClientError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly body?: unknown
	) {
		super(message);
		this.name = 'ApiClientError';
	}
}

export interface ConfigFetchResult {
	view: ConfigView;
	/** Strong ETag from GET /api/v1/config (quoted sha), or null when unset. */
	etag: string | null;
}

interface RequestOptions {
	method?: string;
	apiKey?: string | null;
	/** JSON-serializable body (sets Content-Type: application/json). */
	body?: unknown;
	/** Raw body string with an explicit Content-Type (e.g. application/toml). */
	rawBody?: string;
	contentType?: string;
	/** Extra request headers (If-Match, X-Config-Revision, …). */
	headers?: Record<string, string>;
}

async function requestRaw(path: string, options: RequestOptions = {}): Promise<Response> {
	if (isOidcEnabled()) {
		const before = getBearerToken();
		await refreshOidcTokensIfNeeded();
		const after = getBearerToken();
		// Only rebuild the auth profile when tokens actually change — syncing on
		// every request reassigns `$state` profile and retriggers layout / page
		// `$effect`s that read `auth.isAuthenticated` (infinite fetch loops).
		if (before !== after) {
			notifyAuthProfileSync();
		}
	}

	const bearer = options.apiKey ?? getBearerToken();
	const headers: Record<string, string> = {
		Accept: 'application/json',
		...options.headers
	};

	if (bearer) {
		headers.Authorization = `Bearer ${bearer}`;
	}

	let body: string | undefined;
	if (options.rawBody !== undefined) {
		headers['Content-Type'] = options.contentType ?? 'text/plain';
		body = options.rawBody;
	} else if (options.body !== undefined) {
		headers['Content-Type'] = 'application/json';
		body = JSON.stringify(options.body);
	}

	const response = await fetch(`${getApiBaseUrl()}${path}`, {
		method: options.method ?? 'GET',
		headers,
		body
	});

	if (!response.ok) {
		let message = response.statusText;
		let errorBody: unknown;
		try {
			errorBody = await response.json();
			const payload = errorBody as { error?: string };
			if (payload.error) message = payload.error;
		} catch {
			// ignore parse errors
		}
		throw new ApiClientError(message, response.status, errorBody);
	}

	return response;
}

async function request<T>(path: string, options: RequestOptions = {}): Promise<T> {
	const response = await requestRaw(path, options);

	if (response.status === 204) {
		return undefined as T;
	}

	return (await response.json()) as T;
}

/** Normalize sha / ETag into a quoted strong validator for If-Match. */
export function formatIfMatch(etagOrSha: string): string {
	const trimmed = etagOrSha.trim();
	if (trimmed === '*') return '*';
	if (trimmed.startsWith('W/')) return trimmed;
	if (trimmed.startsWith('"') && trimmed.endsWith('"')) return trimmed;
	return `"${trimmed}"`;
}

export const api = {
	getHealth: () => request<HealthResponse>('/health'),
	getReady: () => request<ReadyResponse>('/ready'),
	getStatus: (apiKey?: string) => request<StatusResponse>('/api/v1/status', { apiKey }),
	listSources: (organization?: string | null) =>
		request<SourceDetail[]>(
			organization
				? `/api/v1/sources?organization=${encodeURIComponent(organization)}`
				: '/api/v1/sources'
		),
	getSource: (id: string) =>
		request<SourceDetail>(`/api/v1/sources/${encodeURIComponent(id)}`),
	listOutbox: (limit = 50, organization?: string | null) => {
		const params = new URLSearchParams({ limit: String(limit) });
		if (organization) params.set('organization', organization);
		return request<OutboxListResponse>(`/api/v1/outbox?${params}`);
	},
	requeueOutbox: () =>
		request<OutboxRequeueResponse>('/api/v1/outbox/requeue', { method: 'POST' }),
	listTeams: (organization?: string | null) =>
		request<TeamListResponse>(
			organization
				? `/api/v1/teams?organization=${encodeURIComponent(organization)}`
				: '/api/v1/teams'
		),
	listNotifiers: () => request<NotifierListResponse>('/api/v1/notifiers'),
	testNotifiers: (body: NotifierTestRequest = {}) =>
		request<NotifierTestResponse>('/api/v1/notifiers/test', {
			method: 'POST',
			body
		}),
	getConfig: async (): Promise<ConfigFetchResult> => {
		const response = await requestRaw('/api/v1/config');
		const view = (await response.json()) as ConfigView;
		const etag = response.headers.get('ETag');
		return { view, etag };
	},
	getConfigSchema: () => request<ConfigSchema>('/api/v1/config/schema'),
	listConfigRevisions: (limit = 50, offset = 0) =>
		request<ConfigRevisionsResponse>(
			`/api/v1/config/revisions?limit=${limit}&offset=${offset}`
		),
	validateConfig: (document: string, contentType = 'application/toml') =>
		request<ConfigValidateResponse>('/api/v1/config/validate', {
			method: 'POST',
			rawBody: document,
			contentType
		}),
	applyConfig: (
		document: string,
		options: {
			contentType?: string;
			ifMatch?: string | null;
			revisionLabel?: string;
		} = {}
	) => {
		const headers: Record<string, string> = {};
		if (options.ifMatch) {
			headers['If-Match'] = formatIfMatch(options.ifMatch);
		}
		if (options.revisionLabel) {
			headers['X-Config-Revision'] = options.revisionLabel;
		}
		return request<ConfigApplyResponse>('/api/v1/config/apply', {
			method: 'POST',
			rawBody: document,
			contentType: options.contentType ?? 'application/yaml',
			headers
		});
	},
	triggerCheck: (dryRun = false) =>
		request<CheckResponse>('/api/v1/check', {
			method: 'POST',
			body: { dry_run: dryRun }
		}),
	triggerCheckSource: (id: string, dryRun = false) =>
		request<CheckResponse>(`/api/v1/check/${encodeURIComponent(id)}`, {
			method: 'POST',
			body: { dry_run: dryRun }
		}),
	verifyApiKey: async (apiKey: string) => {
		await request<StatusResponse>('/api/v1/status', { apiKey });
	},
	getAuthMethods: () => request<AuthMethodsResponse>('/api/v1/auth/methods'),
	login: (body: AuthLoginRequest) =>
		request<AuthLoginResponse>('/api/v1/auth/login', {
			method: 'POST',
			body
		}),
	syncOidcUser: () =>
		request<AuthUserView>('/api/v1/auth/oidc/sync', { method: 'POST' }),
	getAuthMe: () => request<AuthMeResponse>('/api/v1/auth/me'),
	/** Revoke the caller's live sessions server-side (bumps session_version). */
	logout: () => request<{ revoked: boolean }>('/api/v1/auth/logout', { method: 'POST' }),
	/** Admin: list local + OIDC users. */
	listUsers: () => request<AuthUserListResponse>('/api/v1/auth/users'),
	/** Admin: create a local username/password user. */
	createUser: (body: AuthCreateUserRequest) =>
		request<AuthUserView>('/api/v1/auth/users', { method: 'POST', body }),
	/** Admin: link / unlink OIDC subject on a local user. */
	linkUserOidc: (id: number, oidcSub: string | null) =>
		request<AuthUserView>(`/api/v1/auth/users/${id}/oidc`, {
			method: 'POST',
			body: { oidc_sub: oidcSub }
		}),

	/** Organizations (multi-tenant). */
	listOrganizations: () =>
		request<OrganizationListResponse>('/api/v1/organizations'),
	getOrgConfig: async (
		org: string
	): Promise<{ view: OrganizationConfigView; etag: string | null }> => {
		const response = await requestRaw(
			`/api/v1/organizations/${encodeURIComponent(org)}/config`
		);
		const view = (await response.json()) as OrganizationConfigView;
		return { view, etag: response.headers.get('ETag') };
	},
	listOrgConfigRevisions: (org: string, limit = 50, offset = 0) =>
		request<OrganizationRevisionsResponse>(
			`/api/v1/organizations/${encodeURIComponent(org)}/config/revisions?limit=${limit}&offset=${offset}`
		),
	validateOrgConfig: (org: string, document: string, contentType = 'application/yaml') =>
		request<ConfigValidateResponse>(
			`/api/v1/organizations/${encodeURIComponent(org)}/config/validate`,
			{ method: 'POST', rawBody: document, contentType }
		),
	applyOrgConfig: (
		org: string,
		document: string,
		options: { contentType?: string; ifMatch?: string | null; revisionLabel?: string } = {}
	) => {
		const headers: Record<string, string> = {};
		if (options.ifMatch) headers['If-Match'] = formatIfMatch(options.ifMatch);
		if (options.revisionLabel) headers['X-Config-Revision'] = options.revisionLabel;
		return request<ConfigApplyResponse>(
			`/api/v1/organizations/${encodeURIComponent(org)}/config/apply`,
			{
				method: 'POST',
				rawBody: document,
				contentType: options.contentType ?? 'application/yaml',
				headers
			}
		);
	},
	rollbackOrgConfig: (org: string) =>
		request<ConfigApplyResponse>(
			`/api/v1/organizations/${encodeURIComponent(org)}/config/rollback`,
			{ method: 'POST' }
		),
	/** `POST /api/v1/config/rollback` — single-document ledger only. */
	rollbackConfig: () =>
		request<ConfigApplyResponse>('/api/v1/config/rollback', { method: 'POST' })
};
