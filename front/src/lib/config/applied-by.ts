import { EMPTY_VALUE } from '$lib/core/format';

/**
 * `config_revision.applied_by` is written by the server as a machine label —
 * `local:<username>`, `oidc:<email|sub>`, `api_key`, or `unauthenticated`, with
 * an optional ` (X-Config-Applied-By)` annotation appended. Rendering it raw
 * puts `oidc:alice@example.com` in an ops table; this splits it into a readable
 * identity plus the origin it came from.
 */
export interface AppliedBy {
	/** Human identity: login, email, or a fallback describing the credential. */
	identity: string;
	/** Where the identity was proven. */
	origin: 'local' | 'oidc' | 'api' | 'anonymous' | 'unknown';
	/** Free-text label the caller claimed via header — never authenticated. */
	claimed?: string;
}

/** Trailing " (label)" the API appends when a client sent X-Config-Applied-By. */
const CLAIMED_RE = /^(.*?)\s*\(([^()]*)\)\s*$/;

export function parseAppliedBy(value: string | null | undefined): AppliedBy | null {
	const raw = value?.trim();
	if (!raw) return null;

	let subject = raw;
	let claimed: string | undefined;
	const annotated = CLAIMED_RE.exec(raw);
	if (annotated) {
		subject = annotated[1].trim();
		claimed = annotated[2].trim() || undefined;
	}

	if (subject === 'api_key') return { identity: 'API key', origin: 'api', claimed };
	if (subject === 'unauthenticated') {
		return { identity: 'Unauthenticated', origin: 'anonymous', claimed };
	}
	if (subject === 'oidc') return { identity: 'SSO user', origin: 'oidc', claimed };

	const separator = subject.indexOf(':');
	if (separator > 0) {
		const scheme = subject.slice(0, separator);
		const rest = subject.slice(separator + 1).trim();
		if (rest && (scheme === 'local' || scheme === 'oidc')) {
			return { identity: rest, origin: scheme, claimed };
		}
	}

	// Unrecognised shape (older rows, future schemes) — show it verbatim rather
	// than dropping provenance on the floor.
	return { identity: subject, origin: 'unknown', claimed };
}

/** Single-line rendering for narrow cells and tooltips. */
export function formatAppliedBy(value: string | null | undefined): string {
	const parsed = parseAppliedBy(value);
	if (!parsed) return EMPTY_VALUE;
	return parsed.claimed ? `${parsed.identity} (${parsed.claimed})` : parsed.identity;
}
