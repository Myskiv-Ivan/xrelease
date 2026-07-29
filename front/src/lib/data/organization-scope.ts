import { belongsToOrganization } from '$lib/core/organization';
import { getOrganizationsState } from '$lib/stores/organizations.svelte';

/**
 * Organization scoping, in one place.
 *
 * Two mechanisms, both keyed off the switcher selection:
 * - {@link organizationParam} narrows the *request* (list endpoints accept
 *   `?organization=`), used by every org-aware resource store;
 * - {@link scopeToOrganization} narrows an *already-fetched* list client-side,
 *   used by table pages that also render counts for the unscoped set.
 *
 * Both are no-ops on a single-document instance, so callers never branch on
 * `isMultiOrg` themselves — that check drifted across three stores and two
 * pages before it lived here.
 */

/** `?organization=` value for list endpoints; null in single-document mode. */
export function organizationParam(): string | null {
	const orgs = getOrganizationsState();
	return orgs.isMultiOrg ? orgs.selectedId : null;
}

/**
 * Filter a fetched list down to the selected organization.
 *
 * Reads the org state reactively, so call it inside `$derived` — the list
 * re-scopes when the operator switches orgs without a refetch.
 */
export function scopeToOrganization<T>(items: T[], idOf: (item: T) => string): T[] {
	const orgs = getOrganizationsState();
	if (!orgs.isMultiOrg || !orgs.selectedId) return items;
	const selected = orgs.selectedId;
	return items.filter((item) => belongsToOrganization(idOf(item), selected));
}
