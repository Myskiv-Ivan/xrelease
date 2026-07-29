import { untrack } from 'svelte';
import type { SourceDetail } from '$lib/api/types';
import { createQuery, type QueryState } from '$lib/data/query.svelte';
import { getSourcesStore } from '$lib/data/sources.svelte';
import { t } from '$lib/i18n';
import { getAuthState } from '$lib/stores/auth.svelte';

/**
 * The source-detail endpoint, bound to a reactive id and to auth readiness.
 *
 * Always the *detail* endpoint, never the already-loaded list entry: the list
 * payload omits `advisories` on purpose (`skip_serializing_if` server-side), so
 * standing in for it would render every release as advisory-free.
 *
 * Shared by the source page and its advisories page — the id/auth binding below
 * is subtle enough that a second hand-written copy would drift.
 *
 * Must be called during component initialisation: it owns an `$effect`.
 */
export function createSourceDetailQuery(sourceId: () => string): QueryState<SourceDetail> {
	const auth = getAuthState();
	const sourcesStore = getSourcesStore();

	const query = createQuery<SourceDetail>({
		fetcher: () => sourcesStore.fetchById(untrack(sourceId)),
		fallbackError: t('errors.loadSource'),
		isEnabled: () => Boolean(untrack(sourceId))
	});

	/**
	 * A hard reload mounts the page before `initAuth()` finishes, so the first
	 * `execute()` bails on `!isAuthenticated`. Tracking the id plus the two auth
	 * booleans — not `profile`/`me` — restarts the fetch once the session is
	 * live, without the profile-update loop that previously froze navigation
	 * into Details.
	 */
	$effect(() => {
		const id = sourceId();
		const ready = auth.isReady && auth.isAuthenticated;

		if (!id) {
			untrack(() => query.stop());
			return;
		}
		if (!ready) {
			return;
		}

		untrack(() => {
			query.reset();
			query.start();
		});

		return () => {
			query.stop();
		};
	});

	return query;
}
