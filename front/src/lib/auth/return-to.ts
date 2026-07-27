/**
 * `returnTo` round-trip for the login bounce.
 *
 * Two directions, one place: guards and the 401 handler build a `/login?returnTo=…`
 * URL when they redirect an unauthenticated visitor, and the login page (plus the
 * OIDC callback) reads it back to resume the original destination.
 *
 * Both sides are here so they cannot drift — in particular, everything written by
 * {@link loginPath} is accepted by {@link safeReturnTo}, and nothing else is.
 */

/** Where an unauthenticated visitor lands when there is nothing to resume. */
export const DEFAULT_RETURN_TO = '/';

/**
 * Whether `value` is a safe in-app destination to navigate to.
 *
 * Only a root-relative single-slash path qualifies. This rejects absolute URLs
 * (`https://evil.example`), scheme-relative ones (`//evil.example`, which a
 * browser resolves against the current protocol), and non-navigational schemes
 * (`javascript:`). `returnTo` arrives from the query string, so it is
 * attacker-controllable on any link a user can be sent: a crafted
 * `/login?returnTo=…` must not become an off-site redirect after a successful
 * login. SvelteKit's `goto` also refuses cross-origin targets, so this is
 * defence in depth rather than the only barrier — but it keeps the guarantee
 * local and testable instead of relying on the router's behaviour.
 */
function isSafeReturnTo(value: string): boolean {
	return value.startsWith('/') && !value.startsWith('//');
}

/**
 * Coerce an untrusted `returnTo` into a safe in-app destination.
 *
 * Falls back to {@link DEFAULT_RETURN_TO} when absent, empty, or unsafe. Use on
 * any value that did not come straight from {@link loginPath} — including one
 * read back out of storage after an OIDC round-trip.
 */
export function sanitizeReturnTo(value: string | null | undefined): string {
	if (!value || !isSafeReturnTo(value)) return DEFAULT_RETURN_TO;
	return value;
}

/**
 * Read a validated `returnTo` out of a query string.
 *
 * Falls back to {@link DEFAULT_RETURN_TO} when absent, empty, or unsafe.
 */
export function safeReturnTo(params: URLSearchParams): string {
	return sanitizeReturnTo(params.get('returnTo'));
}

/**
 * Build the login URL that resumes `url` after authentication.
 *
 * Returns a bare `/login` when there is nothing worth resuming — the root, or
 * `/login` itself, which would otherwise nest a redirect into its own target.
 */
export function loginPath(url: URL): string {
	const path = url.pathname + url.search;
	if (!path || path === DEFAULT_RETURN_TO || url.pathname.startsWith('/login')) {
		return '/login';
	}
	return `/login?returnTo=${encodeURIComponent(path)}`;
}
