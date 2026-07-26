/**
 * OpenID Provider Configuration — RFC 8414 (OAuth 2.0 Authorization Server Metadata)
 * + OpenID Connect Discovery 1.0.
 */

export interface OidcDiscoveryDocument {
	issuer: string;
	authorization_endpoint: string;
	token_endpoint: string;
	jwks_uri: string;
	end_session_endpoint?: string;
	userinfo_endpoint?: string;
	response_types_supported?: string[];
	grant_types_supported?: string[];
	code_challenge_methods_supported?: string[];
}

const DISCOVERY_CACHE = new Map<string, { fetchedAt: number; doc: OidcDiscoveryDocument }>();
const DISCOVERY_CACHE_TTL_MS = 5 * 60_000;
const DISCOVERY_RETRY_ATTEMPTS = 5;
const DISCOVERY_RETRY_BASE_DELAY_MS = 250;

/** RFC 8414 §3 — issuer identifier comparison (exact string match). */
export function issuerMatches(expected: string, actual: string): boolean {
	return expected.replace(/\/$/, '') === actual.replace(/\/$/, '');
}

async function withRetry<T>(
	fn: () => Promise<T>,
	{
		attempts,
		baseDelayMs
	}: { attempts: number; baseDelayMs: number }
): Promise<T> {
	let lastErr: unknown;
	for (let attempt = 0; attempt < attempts; attempt++) {
		try {
			return await fn();
		} catch (err) {
			lastErr = err;
			// last attempt -> rethrow below
			if (attempt === attempts - 1) break;

			// Lightweight jitter to avoid thundering herd on restarts.
			const jitter = Math.floor(Math.random() * 100);
			const delay = baseDelayMs * (2 ** attempt) + jitter;
			await new Promise((r) => setTimeout(r, delay));
		}
	}
	throw lastErr instanceof Error ? lastErr : new Error(String(lastErr));
}

export async function fetchOidcDiscovery(issuer: string): Promise<OidcDiscoveryDocument> {
	const normalizedIssuer = issuer.replace(/\/$/, '');
	const cached = DISCOVERY_CACHE.get(normalizedIssuer);
	if (cached && Date.now() - cached.fetchedAt < DISCOVERY_CACHE_TTL_MS) {
		return cached.doc;
	}

	return withRetry(
		async () => {
			const response = await fetch(
				`${normalizedIssuer}/.well-known/openid-configuration`,
				{ cache: 'no-store' }
			);
			if (!response.ok) {
				throw new Error(`OIDC discovery failed (${response.status})`);
			}

			const doc = (await response.json()) as OidcDiscoveryDocument;
			if (
				typeof doc.issuer !== 'string' ||
				typeof doc.authorization_endpoint !== 'string' ||
				typeof doc.token_endpoint !== 'string' ||
				typeof doc.jwks_uri !== 'string'
			) {
				throw new Error('OIDC discovery document is incomplete');
			}

			if (!issuerMatches(normalizedIssuer, doc.issuer)) {
				throw new Error('OIDC discovery issuer mismatch');
			}

			const methods = doc.code_challenge_methods_supported ?? [];
			if (!methods.includes('S256')) {
				throw new Error('OIDC provider does not advertise PKCE S256');
			}

			DISCOVERY_CACHE.set(normalizedIssuer, { fetchedAt: Date.now(), doc });
			return doc;
		},
		{
			attempts: DISCOVERY_RETRY_ATTEMPTS,
			baseDelayMs: DISCOVERY_RETRY_BASE_DELAY_MS
		}
	);
}

export function clearDiscoveryCache(): void {
	DISCOVERY_CACHE.clear();
}
