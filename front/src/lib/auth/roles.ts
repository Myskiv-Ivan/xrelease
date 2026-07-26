import type { AppRole, Permission, ResolvedRoles } from '$lib/auth/types';

export const ROLE_PERMISSIONS: Record<AppRole, readonly Permission[]> = {
	viewer: [
		'status:read',
		'sources:read',
		'outbox:read',
		'diagnostics:read',
		'config:read',
		'about:read'
	],
	operator: [
		'status:read',
		'sources:read',
		'outbox:read',
		'outbox:requeue',
		'diagnostics:read',
		'config:read',
		'about:read',
		'poll:execute'
	],
	admin: [
		'status:read',
		'sources:read',
		'outbox:read',
		'outbox:requeue',
		'diagnostics:read',
		'config:read',
		'config:write',
		'about:read',
		'poll:execute',
		'settings:write'
	]
};

const ROLE_RANK: Record<AppRole, number> = {
	viewer: 1,
	operator: 2,
	admin: 3
};

export function roleRank(role: AppRole): number {
	return ROLE_RANK[role];
}

/** Higher-privilege of two roles (matches backend `AppRole::max`). */
export function maxRole(a: AppRole, b: AppRole): AppRole {
	return ROLE_RANK[a] >= ROLE_RANK[b] ? a : b;
}

export function hasPermission(role: AppRole, permission: Permission): boolean {
	return ROLE_PERMISSIONS[role].includes(permission);
}

/**
 * Effective role for a target organization: higher of global and any org grant.
 * `organizationId` null/undefined → instance-wide (global only), matching
 * backend `ResolvedRoles::for_org(None)`.
 */
export function roleForOrganization(
	roles: ResolvedRoles,
	organizationId?: string | null
): AppRole {
	if (!organizationId) return roles.global;
	const scoped = roles.perOrg[organizationId];
	return scoped ? maxRole(roles.global, scoped) : roles.global;
}

export function hasPermissionFor(
	roles: ResolvedRoles,
	permission: Permission,
	organizationId?: string | null
): boolean {
	return hasPermission(roleForOrganization(roles, organizationId), permission);
}

/** Bare IdP aliases → global role (no `alias:org` parsing). */
export function resolveAppRole(
	oidcRoles: string[],
	mapping: Record<AppRole, string[]>,
	fallback: AppRole
): AppRole {
	const normalized = new Set(oidcRoles.map((role) => role.toLowerCase()));
	let resolved: AppRole = fallback;

	for (const appRole of ['admin', 'operator', 'viewer'] as const) {
		const aliases = mapping[appRole].map((role) => role.toLowerCase());
		if (aliases.some((alias) => normalized.has(alias))) {
			if (ROLE_RANK[appRole] >= ROLE_RANK[resolved]) {
				resolved = appRole;
			}
		}
	}

	return resolved;
}

function roleForAlias(
	alias: string,
	mapping: Record<AppRole, string[]>
): AppRole | null {
	const needle = alias.trim().toLowerCase();
	for (const appRole of ['admin', 'operator', 'viewer'] as const) {
		if (mapping[appRole].some((a) => a.toLowerCase() === needle)) {
			return appRole;
		}
	}
	return null;
}

/**
 * Global role + per-org grants from IdP claims.
 * Bare alias → global; `alias:org` → org grant only.
 */
export function resolveResolvedRoles(
	oidcRoles: string[],
	mapping: Record<AppRole, string[]>,
	fallback: AppRole
): ResolvedRoles {
	const global = resolveAppRole(oidcRoles, mapping, fallback);
	const perOrg: Record<string, AppRole> = {};

	for (const group of oidcRoles) {
		const sep = group.indexOf(':');
		if (sep <= 0) continue;
		const alias = group.slice(0, sep);
		const org = group.slice(sep + 1).trim();
		if (!org) continue;
		const role = roleForAlias(alias, mapping);
		if (!role) continue;
		const current = perOrg[org];
		perOrg[org] = current ? maxRole(current, role) : role;
	}

	return { global, perOrg };
}

export function roleLabel(role: AppRole): string {
	switch (role) {
		case 'admin':
			return 'Admin';
		case 'operator':
			return 'Operator';
		case 'viewer':
			return 'Viewer';
	}
}
