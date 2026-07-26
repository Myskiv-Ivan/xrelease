import { ORGANIZATION_SEP } from '$lib/core/constants';

/**
 * Extract organization id from a namespaced source id or routing tag
 * (`platform::github:owner/repo` → `platform`). Matches backend
 * `organization_id_from_source_id`.
 */
export function organizationIdFromNamespaced(id: string | null | undefined): string | null {
	if (!id) return null;
	const sep = id.indexOf(ORGANIZATION_SEP);
	if (sep <= 0) return null;
	const org = id.slice(0, sep);
	return org.length > 0 ? org : null;
}

/** Whether a namespaced id belongs to the selected organization. */
export function belongsToOrganization(
	namespacedId: string | null | undefined,
	organizationId: string | null | undefined
): boolean {
	if (!organizationId) return true;
	return organizationIdFromNamespaced(namespacedId) === organizationId;
}

/**
 * Strip the `{org}::` prefix for display when already scoped by the switcher.
 * Leaves the string unchanged when it is not namespaced for that org.
 */
export function displayIdWithoutOrg(
	namespacedId: string,
	organizationId: string | null | undefined
): string {
	if (!organizationId) return namespacedId;
	const prefix = `${organizationId}${ORGANIZATION_SEP}`;
	return namespacedId.startsWith(prefix)
		? namespacedId.slice(prefix.length)
		: namespacedId;
}
