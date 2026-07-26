import { getAuthMode } from '$lib/auth/config';
import { readOidcTokens } from '$lib/auth/oidc';
import type { LocalSession } from '$lib/auth/types';
import { STORAGE_KEYS } from '$lib/core/constants';

const API_KEY_STORAGE = STORAGE_KEYS.apiKey;
const LOCAL_SESSION_STORAGE = STORAGE_KEYS.localSession;

export function getStoredApiKey(): string | null {
	if (typeof sessionStorage === 'undefined') return null;
	return sessionStorage.getItem(API_KEY_STORAGE);
}

export function setStoredApiKey(key: string): void {
	sessionStorage.setItem(API_KEY_STORAGE, key);
}

export function clearStoredApiKey(): void {
	sessionStorage.removeItem(API_KEY_STORAGE);
}

export function readLocalSession(): LocalSession | null {
	if (typeof sessionStorage === 'undefined') return null;
	const raw = sessionStorage.getItem(LOCAL_SESSION_STORAGE);
	if (!raw) return null;
	try {
		const parsed = JSON.parse(raw) as LocalSession;
		if (!parsed.accessToken || !parsed.expiresAt || !parsed.role) return null;
		if (Date.now() >= parsed.expiresAt) {
			clearLocalSession();
			return null;
		}
		return parsed;
	} catch {
		return null;
	}
}

export function writeLocalSession(session: LocalSession | null): void {
	if (typeof sessionStorage === 'undefined') return;
	if (!session) {
		sessionStorage.removeItem(LOCAL_SESSION_STORAGE);
		return;
	}
	sessionStorage.setItem(LOCAL_SESSION_STORAGE, JSON.stringify(session));
}

export function clearLocalSession(): void {
	writeLocalSession(null);
}

export function getBearerToken(): string | null {
	const mode = getAuthMode();
	const local = readLocalSession();
	const apiKey = getStoredApiKey();
	const oidcTokens = readOidcTokens();

	if (mode === 'oidc') {
		return oidcTokens?.accessToken ?? null;
	}
	if (mode === 'local') {
		return local?.accessToken ?? null;
	}
	if (mode === 'hybrid') {
		return oidcTokens?.accessToken ?? local?.accessToken ?? apiKey;
	}
	return apiKey;
}
