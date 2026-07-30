import { STORAGE_KEYS } from '$lib/core/constants';
import { fetchOidcDiscovery } from '$lib/auth/discovery';
import { getOidcConfig } from '$lib/auth/config';
import {
	validateIdTokenClaims,
	expiresAtFromPayload,
	isTokenExpired,
	looksLikeJwt,
	parseJwt,
	type JwtPayload
} from '$lib/auth/jwt';
import { verifyJwtWithJwks } from '$lib/auth/jwks';
import type { OidcProfile, OidcTokens } from '$lib/auth/types';

interface OidcPendingState {
	verifier: string;
	nonce: string;
	state: string;
	returnTo: string;
}

interface OAuthTokenResponse {
	access_token: string;
	refresh_token?: string;
	id_token?: string;
	expires_in?: number;
	token_type?: string;
	scope?: string;
}

function base64UrlEncode(bytes: Uint8Array): string {
	let binary = '';
	for (const byte of bytes) {
		binary += String.fromCharCode(byte);
	}
	return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/g, '');
}

function randomString(byteLength = 32): string {
	const bytes = new Uint8Array(byteLength);
	crypto.getRandomValues(bytes);
	return base64UrlEncode(bytes);
}

async function sha256Base64Url(value: string): Promise<string> {
	const data = new TextEncoder().encode(value);
	const digest = await crypto.subtle.digest('SHA-256', data);
	return base64UrlEncode(new Uint8Array(digest));
}

function readPendingState(): OidcPendingState | null {
	if (typeof sessionStorage === 'undefined') return null;
	const raw = sessionStorage.getItem(STORAGE_KEYS.oidcPending);
	if (!raw) return null;
	try {
		return JSON.parse(raw) as OidcPendingState;
	} catch {
		return null;
	}
}

function writePendingState(state: OidcPendingState | null): void {
	if (typeof sessionStorage === 'undefined') return;
	if (!state) {
		sessionStorage.removeItem(STORAGE_KEYS.oidcPending);
		return;
	}
	sessionStorage.setItem(STORAGE_KEYS.oidcPending, JSON.stringify(state));
}

export function readOidcTokens(): OidcTokens | null {
	if (typeof sessionStorage === 'undefined') return null;
	const raw = sessionStorage.getItem(STORAGE_KEYS.oidcSession);
	if (!raw) return null;
	try {
		const parsed = JSON.parse(raw) as OidcTokens;
		if (!parsed.accessToken || !parsed.expiresAt) return null;
		if (isTokenExpired(parsed.expiresAt)) return null;
		return parsed;
	} catch {
		return null;
	}
}

export function writeOidcTokens(tokens: OidcTokens | null): void {
	if (typeof sessionStorage === 'undefined') return;
	if (!tokens) {
		sessionStorage.removeItem(STORAGE_KEYS.oidcSession);
		sessionStorage.removeItem(STORAGE_KEYS.oidcAppRole);
		sessionStorage.removeItem(STORAGE_KEYS.oidcOrgRoles);
		return;
	}
	sessionStorage.setItem(STORAGE_KEYS.oidcSession, JSON.stringify(tokens));
}

/** Server-assigned global role from `/auth/oidc/sync` or `/auth/me`. */
export function writeOidcAppRole(role: import('$lib/auth/types').AppRole | null): void {
	if (typeof sessionStorage === 'undefined') return;
	if (!role) {
		sessionStorage.removeItem(STORAGE_KEYS.oidcAppRole);
		return;
	}
	sessionStorage.setItem(STORAGE_KEYS.oidcAppRole, role);
}

export function readOidcAppRole(): import('$lib/auth/types').AppRole | null {
	if (typeof sessionStorage === 'undefined') return null;
	const value = sessionStorage.getItem(STORAGE_KEYS.oidcAppRole);
	if (value === 'admin' || value === 'operator' || value === 'viewer') return value;
	return null;
}

/** Server-assigned per-org grants from sync / `/auth/me`. */
export function writeOidcOrganizationRoles(
	roles: Record<string, import('$lib/auth/types').AppRole> | null
): void {
	if (typeof sessionStorage === 'undefined') return;
	if (!roles || Object.keys(roles).length === 0) {
		sessionStorage.removeItem(STORAGE_KEYS.oidcOrgRoles);
		return;
	}
	sessionStorage.setItem(STORAGE_KEYS.oidcOrgRoles, JSON.stringify(roles));
}

export function readOidcOrganizationRoles(): Record<
	string,
	import('$lib/auth/types').AppRole
> | null {
	if (typeof sessionStorage === 'undefined') return null;
	const raw = sessionStorage.getItem(STORAGE_KEYS.oidcOrgRoles);
	if (!raw) return null;
	try {
		const parsed = JSON.parse(raw) as Record<string, unknown>;
		const out: Record<string, import('$lib/auth/types').AppRole> = {};
		for (const [org, role] of Object.entries(parsed)) {
			if (role === 'admin' || role === 'operator' || role === 'viewer') {
				out[org] = role;
			}
		}
		return Object.keys(out).length > 0 ? out : null;
	} catch {
		return null;
	}
}

function profileFromPayload(payload: JwtPayload): OidcProfile | null {
	if (!payload.sub) return null;
	return {
		sub: payload.sub,
		email: typeof payload.email === 'string' ? payload.email : undefined,
		name: typeof payload.name === 'string' ? payload.name : undefined,
		preferredUsername:
			typeof payload.preferred_username === 'string'
				? payload.preferred_username
				: undefined
	};
}

export function readOidcProfile(tokens: OidcTokens): OidcProfile | null {
	const payload = tokens.idTokenPayload;
	if (payload) return profileFromPayload(payload);
	return null;
}

export function readOidcRoles(tokens: OidcTokens): string[] {
	const config = getOidcConfig();
	if (!config) return [];

	const roles = new Set<string>();
	for (const payload of [tokens.idTokenPayload, tokens.accessTokenPayload]) {
		if (!payload) continue;
		const value = config.roleClaim.split('.').reduce<unknown>((current, key) => {
			if (current && typeof current === 'object' && key in current) {
				return (current as Record<string, unknown>)[key];
			}
			return undefined;
		}, payload);

		if (Array.isArray(value)) {
			for (const item of value) {
				if (typeof item === 'string') roles.add(item);
			}
		} else if (typeof value === 'string') {
			roles.add(value);
		}
	}

	return Array.from(roles);
}

async function validateIdToken(
	idToken: string,
	jwksUri: string,
	issuer: string,
	clientId: string,
	nonce?: string
): Promise<JwtPayload> {
	const parsed = await verifyJwtWithJwks(idToken, jwksUri);
	const errors = validateIdTokenClaims(parsed.payload, { issuer, clientId, nonce });
	if (errors.length > 0) {
		throw new Error(`ID Token validation failed: ${errors.join(', ')}`);
	}
	return parsed.payload;
}

/**
 * Decode (NOT verify) the access token payload when it happens to be a JWT.
 *
 * The access token is opaque to this client per OAuth 2.0 — the Rust API is
 * the resource server and performs the real signature/issuer/scope
 * validation on every request. The payload is decoded only for UX: role
 * claims shown in the UI and `exp` for scheduling the refresh. Verifying the
 * signature here was security theater (an attacker controls this runtime)
 * and broke logins whenever an IdP issued access tokens outside our JWKS
 * assumptions.
 */
function decodeAccessTokenIfJwt(accessToken: string): JwtPayload | undefined {
	if (!looksLikeJwt(accessToken)) return undefined;
	return parseJwt(accessToken)?.payload;
}

async function persistTokenResponse(
	payload: OAuthTokenResponse,
	discoveryJwksUri: string,
	issuer: string,
	clientId: string,
	nonce?: string
): Promise<OidcTokens> {
	if (payload.token_type && payload.token_type.toLowerCase() !== 'bearer') {
		throw new Error(`Unsupported token_type: ${payload.token_type}`);
	}

	if (!payload.id_token) {
		throw new Error('OIDC response missing id_token (openid scope required)');
	}

	const idTokenPayload = await validateIdToken(
		payload.id_token,
		discoveryJwksUri,
		issuer,
		clientId,
		nonce
	);

	const accessTokenPayload = decodeAccessTokenIfJwt(payload.access_token);

	const expiresAt = accessTokenPayload
		? expiresAtFromPayload(accessTokenPayload, payload.expires_in ?? 3600)
		: Date.now() + (payload.expires_in ?? 3600) * 1000;

	return {
		accessToken: payload.access_token,
		refreshToken: payload.refresh_token,
		idToken: payload.id_token,
		expiresAt,
		tokenType: 'Bearer',
		scope: payload.scope,
		idTokenPayload,
		accessTokenPayload
	};
}

export async function startOidcLogin(returnTo = '/'): Promise<void> {
	const config = getOidcConfig();
	if (!config) {
		throw new Error('OIDC is not configured');
	}

	const discovery = await fetchOidcDiscovery(config.issuer);
	const verifier = randomString();
	const challenge = await sha256Base64Url(verifier);
	const state = randomString(16);
	const nonce = randomString(16);

	writePendingState({ verifier, nonce, state, returnTo });

	const params = new URLSearchParams({
		client_id: config.clientId,
		response_type: 'code',
		scope: config.scopes.join(' '),
		redirect_uri: config.redirectUri,
		code_challenge: challenge,
		code_challenge_method: 'S256',
		state,
		nonce
	});

	window.location.assign(`${discovery.authorization_endpoint}?${params.toString()}`);
}

export async function completeOidcCallback(
	searchParams: URLSearchParams
): Promise<{ returnTo: string }> {
	const config = getOidcConfig();
	if (!config) {
		throw new Error('OIDC is not configured');
	}

	const error = searchParams.get('error');
	if (error) {
		throw new Error(searchParams.get('error_description') ?? error);
	}

	const code = searchParams.get('code');
	if (!code) {
		throw new Error('Missing authorization code');
	}

	const state = searchParams.get('state');
	const pending = readPendingState();
	if (!pending) {
		throw new Error('OIDC session expired — start sign-in again');
	}
	if (!state || state !== pending.state) {
		throw new Error('Invalid OIDC state');
	}

	const discovery = await fetchOidcDiscovery(config.issuer);
	const body = new URLSearchParams({
		grant_type: 'authorization_code',
		client_id: config.clientId,
		code,
		redirect_uri: config.redirectUri,
		code_verifier: pending.verifier
	});

	const response = await fetch(discovery.token_endpoint, {
		method: 'POST',
		headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
		body
	});

	if (!response.ok) {
		let message = `Token exchange failed (${response.status})`;
		try {
			const errPayload = (await response.json()) as { error?: string; error_description?: string };
			if (errPayload.error_description) message = errPayload.error_description;
			else if (errPayload.error) message = errPayload.error;
		} catch {
			// ignore
		}
		throw new Error(message);
	}

	const tokenPayload = (await response.json()) as OAuthTokenResponse;
	const tokens = await persistTokenResponse(
		tokenPayload,
		discovery.jwks_uri,
		discovery.issuer,
		config.clientId,
		pending.nonce
	);

	writeOidcTokens(tokens);
	writePendingState(null);

	return { returnTo: pending.returnTo };
}

export async function refreshOidcTokensIfNeeded(): Promise<OidcTokens | null> {
	const current = readOidcTokens();
	if (current && !isTokenExpired(current.expiresAt)) {
		return current;
	}

	if (typeof sessionStorage === 'undefined') return null;
	const raw = sessionStorage.getItem(STORAGE_KEYS.oidcSession);
	if (!raw) return null;

	let stored: OidcTokens;
	try {
		stored = JSON.parse(raw) as OidcTokens;
	} catch {
		return null;
	}

	if (!stored.refreshToken) {
		writeOidcTokens(null);
		return null;
	}

	const config = getOidcConfig();
	if (!config) return null;

	const discovery = await fetchOidcDiscovery(config.issuer);
	const body = new URLSearchParams({
		grant_type: 'refresh_token',
		client_id: config.clientId,
		refresh_token: stored.refreshToken
	});

	const response = await fetch(discovery.token_endpoint, {
		method: 'POST',
		headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
		body
	});

	if (!response.ok) {
		writeOidcTokens(null);
		return null;
	}

	const tokenPayload = (await response.json()) as OAuthTokenResponse;
	const tokens = await persistTokenResponse(
		tokenPayload,
		discovery.jwks_uri,
		discovery.issuer,
		config.clientId
	);

	writeOidcTokens(tokens);
	return tokens;
}

export async function endOidcSession(): Promise<void> {
	const config = getOidcConfig();
	const tokens = readOidcTokens();
	writeOidcTokens(null);
	writePendingState(null);

	if (!config || !tokens?.idToken) return;

	try {
		const discovery = await fetchOidcDiscovery(config.issuer);
		if (!discovery.end_session_endpoint) return;

		const params = new URLSearchParams({
			id_token_hint: tokens.idToken,
			post_logout_redirect_uri: config.redirectUri.replace('/login/callback', '/login')
		});
		window.location.assign(`${discovery.end_session_endpoint}?${params.toString()}`);
	} catch {
		// local logout is enough when end_session is unavailable
	}
}
