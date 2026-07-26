import {
	getAuthMode,
	getDefaultApiKeyRole,
	getOidcConfig
} from '$lib/auth/config';
import { getStoredApiKey, readLocalSession } from '$lib/auth/credentials';
import {
	readOidcAppRole,
	readOidcOrganizationRoles,
	readOidcProfile,
	readOidcRoles,
	readOidcTokens
} from '$lib/auth/oidc';
import {
	hasPermissionFor,
	resolveResolvedRoles
} from '$lib/auth/roles';
import type {
	AppRole,
	AuthProfile,
	Permission,
	ResolvedRoles
} from '$lib/auth/types';

const EMPTY_ROLE_MAPPING = {
	admin: [] as string[],
	operator: [] as string[],
	viewer: [] as string[]
};

const EMPTY_ORG_ROLES: Record<string, AppRole> = {};

function oidcResolvedRoles(
	oidcRoles: string[],
	roleMapping: Record<AppRole, string[]>,
	fallback: AppRole
): ResolvedRoles {
	const fromClaims = resolveResolvedRoles(oidcRoles, roleMapping, fallback);
	const serverGlobal = readOidcAppRole();
	const serverOrgs = readOidcOrganizationRoles();
	return {
		global: serverGlobal ?? fromClaims.global,
		// Prefer server map when present (including empty after explicit sync);
		// otherwise fall back to client-side `alias:org` parsing.
		perOrg: serverOrgs ?? fromClaims.perOrg
	};
}

function profileFromResolved(
	base: Omit<AuthProfile, 'appRole' | 'organizationRoles'>,
	resolved: ResolvedRoles
): AuthProfile {
	return {
		...base,
		appRole: resolved.global,
		organizationRoles: resolved.perOrg
	};
}

export function buildAuthProfile(): AuthProfile | null {
	const mode = getAuthMode();
	const apiKey = getStoredApiKey();
	const local = readLocalSession();
	const oidcTokens = readOidcTokens();
	const config = getOidcConfig();
	const roleMapping = config?.roleMapping ?? EMPTY_ROLE_MAPPING;

	if (mode === 'local') {
		if (!local) return null;
		return {
			method: 'local',
			localUsername: local.username,
			appRole: local.role,
			organizationRoles: EMPTY_ORG_ROLES,
			oidcRoles: []
		};
	}

	if (mode === 'api_key') {
		if (!apiKey) return null;
		return {
			method: 'api_key',
			appRole: getDefaultApiKeyRole(),
			organizationRoles: EMPTY_ORG_ROLES,
			oidcRoles: []
		};
	}

	if (mode === 'oidc') {
		if (!oidcTokens) return null;
		const oidcRoles = readOidcRoles(oidcTokens);
		return profileFromResolved(
			{
				method: 'oidc',
				oidcProfile: readOidcProfile(oidcTokens) ?? undefined,
				oidcRoles
			},
			oidcResolvedRoles(oidcRoles, roleMapping, 'viewer')
		);
	}

	// hybrid
	if (oidcTokens) {
		const oidcRoles = readOidcRoles(oidcTokens);
		return profileFromResolved(
			{
				method: 'hybrid',
				oidcProfile: readOidcProfile(oidcTokens) ?? undefined,
				oidcRoles
			},
			oidcResolvedRoles(oidcRoles, roleMapping, getDefaultApiKeyRole())
		);
	}

	if (local) {
		return {
			method: 'local',
			localUsername: local.username,
			appRole: local.role,
			organizationRoles: EMPTY_ORG_ROLES,
			oidcRoles: []
		};
	}

	if (!apiKey) return null;

	return {
		method: 'hybrid',
		oidcRoles: [],
		appRole: getDefaultApiKeyRole(),
		organizationRoles: EMPTY_ORG_ROLES
	};
}

export function resolvedRolesFromProfile(profile: AuthProfile): ResolvedRoles {
	return {
		global: profile.appRole,
		perOrg: profile.organizationRoles
	};
}

export function isSessionAuthenticated(): boolean {
	const mode = getAuthMode();
	const apiKey = getStoredApiKey();
	const local = readLocalSession();
	const oidcTokens = readOidcTokens();

	if (mode === 'local') return Boolean(local);
	if (mode === 'api_key') return Boolean(apiKey);
	if (mode === 'oidc') return Boolean(oidcTokens);
	return Boolean(oidcTokens) || Boolean(local) || Boolean(apiKey);
}

export function canCallManagementApi(): boolean {
	if (!isSessionAuthenticated()) return false;
	return Boolean(getBearerTokenSafe());
}

function getBearerTokenSafe(): string | null {
	const mode = getAuthMode();
	const local = readLocalSession();
	const apiKey = getStoredApiKey();
	const oidcTokens = readOidcTokens();
	if (mode === 'oidc') return oidcTokens?.accessToken ?? null;
	if (mode === 'local') return local?.accessToken ?? null;
	if (mode === 'hybrid') {
		return oidcTokens?.accessToken ?? local?.accessToken ?? apiKey;
	}
	return apiKey;
}

export function userHasPermission(
	permission: Permission,
	organizationId?: string | null
): boolean {
	const profile = buildAuthProfile();
	if (!profile) return false;
	return hasPermissionFor(resolvedRolesFromProfile(profile), permission, organizationId);
}

export function getDisplayName(profile: AuthProfile | null): string | null {
	if (!profile) return null;
	if (profile.localUsername) return profile.localUsername;
	if (!profile.oidcProfile) return null;
	return (
		profile.oidcProfile.name ??
		profile.oidcProfile.preferredUsername ??
		profile.oidcProfile.email ??
		null
	);
}

export function getCurrentAppRole(): AppRole {
	return buildAuthProfile()?.appRole ?? getDefaultApiKeyRole();
}
