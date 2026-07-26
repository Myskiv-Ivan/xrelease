/**
 * JWT helpers — RFC 7519 (JSON Web Token) + RFC 8725 (JWT BCP).
 * Signature verification is performed separately via JWKS (RFC 7517).
 */

export interface JwtHeader {
	alg: string;
	typ?: string;
	kid?: string;
	[key: string]: unknown;
}

export interface JwtPayload {
	iss?: string;
	sub?: string;
	aud?: string | string[];
	azp?: string;
	exp?: number;
	nbf?: number;
	iat?: number;
	nonce?: string;
	email?: string;
	name?: string;
	preferred_username?: string;
	[key: string]: unknown;
}

export interface ParsedJwt {
	header: JwtHeader;
	payload: JwtPayload;
	signature: Uint8Array;
	signingInput: string;
}

/** Algorithms rejected per RFC 8725 §3.1 */
export const FORBIDDEN_JWT_ALGORITHMS = new Set(['none']);

/** Default clock skew — OAuth 2.0 implementations commonly use 1–5 minutes. */
export const DEFAULT_CLOCK_SKEW_SEC = 60;

function base64UrlToBytes(input: string): Uint8Array {
	const normalized = input.replace(/-/g, '+').replace(/_/g, '/');
	const padded = normalized.padEnd(Math.ceil(normalized.length / 4) * 4, '=');
	const binary = atob(padded);
	const bytes = new Uint8Array(binary.length);
	for (let i = 0; i < binary.length; i += 1) {
		bytes[i] = binary.charCodeAt(i);
	}
	return bytes;
}

function base64UrlDecodeJson<T>(input: string): T | null {
	try {
		const normalized = input.replace(/-/g, '+').replace(/_/g, '/');
		const padded = normalized.padEnd(Math.ceil(normalized.length / 4) * 4, '=');
		return JSON.parse(atob(padded)) as T;
	} catch {
		return null;
	}
}

export function parseJwt(token: string): ParsedJwt | null {
	const parts = token.split('.');
	if (parts.length !== 3) return null;

	const header = base64UrlDecodeJson<JwtHeader>(parts[0]);
	const payload = base64UrlDecodeJson<JwtPayload>(parts[1]);
	if (!header?.alg || !payload) return null;

	return {
		header,
		payload,
		signature: base64UrlToBytes(parts[2]),
		signingInput: `${parts[0]}.${parts[1]}`
	};
}

export function getClaimByPath(payload: JwtPayload, path: string): unknown {
	return path.split('.').reduce<unknown>((value, key) => {
		if (value && typeof value === 'object' && key in value) {
			return (value as Record<string, unknown>)[key];
		}
		return undefined;
	}, payload);
}

export function extractStringArrayClaim(payload: JwtPayload, path: string): string[] {
	const value = getClaimByPath(payload, path);
	if (Array.isArray(value)) {
		return value.filter((item): item is string => typeof item === 'string');
	}
	if (typeof value === 'string') {
		return [value];
	}
	return [];
}

export function isJwtExpired(
	payload: JwtPayload,
	clockSkewSec = DEFAULT_CLOCK_SKEW_SEC,
	nowSec = Math.floor(Date.now() / 1000)
): boolean {
	if (typeof payload.exp !== 'number') return true;
	return nowSec >= payload.exp + clockSkewSec;
}

export function isJwtNotYetValid(
	payload: JwtPayload,
	clockSkewSec = DEFAULT_CLOCK_SKEW_SEC,
	nowSec = Math.floor(Date.now() / 1000)
): boolean {
	if (typeof payload.nbf === 'number' && nowSec + clockSkewSec < payload.nbf) {
		return true;
	}
	return false;
}

export function isJwtIssuedTooFarInFuture(
	payload: JwtPayload,
	clockSkewSec = DEFAULT_CLOCK_SKEW_SEC,
	nowSec = Math.floor(Date.now() / 1000)
): boolean {
	if (typeof payload.iat === 'number' && payload.iat > nowSec + clockSkewSec) {
		return true;
	}
	return false;
}

export function isJwtTooOld(
	payload: JwtPayload,
	maxAgeSec: number,
	clockSkewSec = DEFAULT_CLOCK_SKEW_SEC,
	nowSec = Math.floor(Date.now() / 1000)
): boolean {
	if (typeof payload.iat !== 'number') return false;
	return nowSec - clockSkewSec > payload.iat + maxAgeSec;
}

function audienceMatches(payload: JwtPayload, clientId: string): boolean {
	const aud = payload.aud;
	if (typeof aud === 'string') return aud === clientId;
	if (Array.isArray(aud)) return aud.includes(clientId);
	return false;
}

export interface IdTokenValidationOptions {
	issuer: string;
	clientId: string;
	nonce?: string;
	clockSkewSec?: number;
	/** OIDC Core §3.1.3.7 — max auth time; default 24h */
	maxTokenAgeSec?: number;
}

/**
 * OIDC Core §3.1.3.7 — validate ID Token claims (after signature verification).
 * Returns list of validation errors (empty = valid).
 */
export function validateIdTokenClaims(
	payload: JwtPayload,
	options: IdTokenValidationOptions
): string[] {
	const errors: string[] = [];
	const skew = options.clockSkewSec ?? DEFAULT_CLOCK_SKEW_SEC;
	const nowSec = Math.floor(Date.now() / 1000);
	const maxAge = options.maxTokenAgeSec ?? 86_400;

	if (payload.iss !== options.issuer) {
		errors.push('iss mismatch');
	}

	if (!audienceMatches(payload, options.clientId)) {
		if (payload.azp !== options.clientId) {
			errors.push('aud/azp mismatch');
		}
	}

	if (typeof payload.sub !== 'string' || payload.sub.length === 0) {
		errors.push('missing sub');
	}

	if (isJwtExpired(payload, skew, nowSec)) {
		errors.push('token expired');
	}

	if (isJwtNotYetValid(payload, skew, nowSec)) {
		errors.push('token not yet valid');
	}

	if (isJwtIssuedTooFarInFuture(payload, skew, nowSec)) {
		errors.push('iat in the future');
	}

	if (isJwtTooOld(payload, maxAge, skew, nowSec)) {
		errors.push('token too old');
	}

	if (options.nonce && payload.nonce !== options.nonce) {
		errors.push('nonce mismatch');
	}

	return errors;
}

export function isTokenExpired(expiresAt: number, skewMs = 30_000): boolean {
	return Date.now() >= expiresAt - skewMs;
}

export function expiresAtFromPayload(payload: JwtPayload, fallbackSec = 3600): number {
	if (typeof payload.exp === 'number') {
		return payload.exp * 1000;
	}
	return Date.now() + fallbackSec * 1000;
}

/** True when the token has three base64url segments (may still be opaque to us). */
export function looksLikeJwt(token: string): boolean {
	return token.split('.').length === 3;
}
