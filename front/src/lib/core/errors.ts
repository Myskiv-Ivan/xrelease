	import { goto } from '$app/navigation';
	import { ApiClientError } from '$lib/api/client';
	import { logout } from '$lib/stores/auth.svelte';

export function resolveApiError(err: unknown, fallback: string): string {
	if (err instanceof ApiClientError) return err.message;
	if (err instanceof Error && err.message) return err.message;
	return fallback;
}

/** Redirect to login on 401; returns true when handled. */
export function handleUnauthorized(err: unknown): boolean {
	if (err instanceof ApiClientError && err.status === 401) {
		void logout().finally(() => goto('/login'));
		return true;
	}
	return false;
}
