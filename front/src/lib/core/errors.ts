import { goto } from '$app/navigation';
import { page } from '$app/state';
import { ApiClientError } from '#lib/api/client.js';
import { loginPath } from '#lib/auth/return-to.js';
import { logout } from '#lib/stores/auth.svelte.js';

export function resolveApiError(err: unknown, fallback: string): string {
	if (err instanceof ApiClientError) return err.message;
	if (err instanceof Error && err.message) return err.message;
	return fallback;
}

/** Redirect to login on 401; returns true when handled. */
export function handleUnauthorized(err: unknown): boolean {
	if (err instanceof ApiClientError && err.status === 401) {
		const login = loginPath(page.url);
		// Local clear only — federated IdP logout would navigate away and drop returnTo.
		void logout({ federated: false }).finally(() => goto(login));
		return true;
	}
	return false;
}
