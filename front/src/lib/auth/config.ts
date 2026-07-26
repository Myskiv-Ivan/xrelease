import type { AppRole, AuthMode, OidcConfig } from '$lib/auth/types';
import { readUiSetting } from '$lib/config/runtime';

function parseList(value: string | undefined, fallback: string[]): string[] {
	if (!value) return fallback;
	return value
		.split(',')
		.map((item) => item.trim())
		.filter(Boolean);
}

export function getAuthMode(): AuthMode {
	const mode = readUiSetting('VITE_AUTH_MODE');
	if (mode === 'oidc' || mode === 'hybrid' || mode === 'api_key' || mode === 'local') {
		return mode;
	}
	return 'local';
}

/** Matches backend: a valid API key is always admin. Override only for UI demos. */
export function getDefaultApiKeyRole(): AppRole {
	const role = readUiSetting('VITE_API_KEY_DEFAULT_ROLE');
	if (role === 'admin' || role === 'operator' || role === 'viewer') return role;
	return 'admin';
}

export function getOidcConfig(): OidcConfig | null {
	const issuer = readUiSetting('VITE_OIDC_ISSUER');
	const clientId = readUiSetting('VITE_OIDC_CLIENT_ID');
	if (!issuer || !clientId) return null;

	const redirectUri =
		readUiSetting('VITE_OIDC_REDIRECT_URI') ||
		(typeof window !== 'undefined' ? `${window.location.origin}/login/callback` : '');

	return {
		issuer: issuer.replace(/\/$/, ''),
		clientId,
		redirectUri,
		scopes: parseList(readUiSetting('VITE_OIDC_SCOPES'), [
			'openid',
			'profile',
			'email',
			'groups'
		]),
		roleClaim: readUiSetting('VITE_OIDC_ROLE_CLAIM') ?? 'groups',
		roleMapping: {
			admin: parseList(readUiSetting('VITE_OIDC_ROLE_ADMIN'), ['xrelease-admin', 'admin']),
			operator: parseList(readUiSetting('VITE_OIDC_ROLE_OPERATOR'), [
				'xrelease-operator',
				'operator'
			]),
			viewer: parseList(readUiSetting('VITE_OIDC_ROLE_VIEWER'), ['xrelease-viewer', 'viewer'])
		}
	};
}

export function isOidcEnabled(): boolean {
	const mode = getAuthMode();
	if (mode !== 'oidc' && mode !== 'hybrid') return false;
	return getOidcConfig() !== null;
}

export function isLocalEnabled(): boolean {
	const mode = getAuthMode();
	return mode === 'local' || mode === 'hybrid';
}

export function isApiKeyEnabled(): boolean {
	const mode = getAuthMode();
	return mode === 'api_key' || mode === 'hybrid';
}

export function requiresApiKeyForApi(): boolean {
	return getAuthMode() === 'api_key';
}
