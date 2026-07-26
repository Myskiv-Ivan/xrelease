/**
 * JSON Web Key Set — RFC 7517 + signature verification via Web Crypto (RFC 7518).
 */

import {
	FORBIDDEN_JWT_ALGORITHMS,
	type JwtHeader,
	type ParsedJwt,
	parseJwt
} from '$lib/auth/jwt';

interface JwkKey {
	kty: string;
	kid?: string;
	alg?: string;
	use?: string;
	n?: string;
	e?: string;
}

export interface JsonWebKeySet {
	keys: JwkKey[];
}

interface RsaJwk {
	kty: 'RSA';
	kid?: string;
	alg?: string;
	use?: string;
	n: string;
	e: string;
}

const JWKS_CACHE = new Map<string, { fetchedAt: number; jwks: JsonWebKeySet }>();
const JWKS_CACHE_TTL_MS = 5 * 60_000;
const JWKS_RETRY_ATTEMPTS = 5;
const JWKS_RETRY_BASE_DELAY_MS = 250;

const SUPPORTED_ALGORITHMS = new Set(['RS256', 'RS384', 'RS512']);

function isRsaJwk(jwk: JwkKey): jwk is RsaJwk {
	return jwk.kty === 'RSA' && typeof jwk.n === 'string' && typeof jwk.e === 'string';
}

export async function fetchJsonWebKeySet(jwksUri: string): Promise<JsonWebKeySet> {
	const cached = JWKS_CACHE.get(jwksUri);
	if (cached && Date.now() - cached.fetchedAt < JWKS_CACHE_TTL_MS) {
		return cached.jwks;
	}

	let lastErr: unknown;
	for (let attempt = 0; attempt < JWKS_RETRY_ATTEMPTS; attempt++) {
		try {
			const response = await fetch(jwksUri, { cache: 'no-store' });
			if (!response.ok) {
				throw new Error(`JWKS fetch failed (${response.status})`);
			}

			const jwks = (await response.json()) as JsonWebKeySet;
			if (!Array.isArray(jwks.keys)) {
				throw new Error('Invalid JWKS document');
			}

			JWKS_CACHE.set(jwksUri, { fetchedAt: Date.now(), jwks });
			return jwks;
		} catch (err) {
			lastErr = err;
			if (attempt === JWKS_RETRY_ATTEMPTS - 1) break;
			const jitter = Math.floor(Math.random() * 100);
			const delay = JWKS_RETRY_BASE_DELAY_MS * (2 ** attempt) + jitter;
			await new Promise((r) => setTimeout(r, delay));
		}
	}

	throw lastErr instanceof Error ? lastErr : new Error(String(lastErr));
}

function findVerificationKey(jwks: JsonWebKeySet, header: JwtHeader): RsaJwk | null {
	const candidates = jwks.keys.filter(isRsaJwk);
	if (header.kid) {
		const match = candidates.find((key) => key.kid === header.kid);
		if (match) return match;
	}

	const byAlg = candidates.find((key) => key.alg === header.alg);
	if (byAlg) return byAlg;

	return candidates[0] ?? null;
}

function hashForAlgorithm(alg: string): 'SHA-256' | 'SHA-384' | 'SHA-512' {
	if (alg === 'RS384') return 'SHA-384';
	if (alg === 'RS512') return 'SHA-512';
	return 'SHA-256';
}

async function importRsaKey(jwk: RsaJwk, alg: string): Promise<CryptoKey> {
	return crypto.subtle.importKey(
		'jwk',
		{ kty: 'RSA', n: jwk.n, e: jwk.e, alg, ext: true },
		{ name: 'RSASSA-PKCS1-v1_5', hash: hashForAlgorithm(alg) },
		false,
		['verify']
	);
}

/**
 * Verify JWT signature using JWKS (RFC 7517). Does not validate claims.
 */
export async function verifyJwtWithJwks(token: string, jwksUri: string): Promise<ParsedJwt> {
	const parsed = parseJwt(token);
	if (!parsed) {
		throw new Error('Malformed JWT');
	}

	const alg = parsed.header.alg;
	if (FORBIDDEN_JWT_ALGORITHMS.has(alg.toLowerCase())) {
		throw new Error(`Forbidden JWT algorithm: ${alg}`);
	}

	if (!SUPPORTED_ALGORITHMS.has(alg)) {
		throw new Error(`Unsupported JWT algorithm: ${alg}`);
	}

	const jwks = await fetchJsonWebKeySet(jwksUri);
	const jwk = findVerificationKey(jwks, parsed.header);
	if (!jwk) {
		throw new Error('No matching JWK for token kid/alg');
	}

	const key = await importRsaKey(jwk, alg);
	const data = new TextEncoder().encode(parsed.signingInput);
	const signature = new Uint8Array(parsed.signature);
	const valid = await crypto.subtle.verify(
		{ name: 'RSASSA-PKCS1-v1_5', hash: hashForAlgorithm(alg) },
		key,
		signature,
		data
	);

	if (!valid) {
		throw new Error('JWT signature verification failed');
	}

	return parsed;
}

export function clearJwksCache(): void {
	JWKS_CACHE.clear();
}
