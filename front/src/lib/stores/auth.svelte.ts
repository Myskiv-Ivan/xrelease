import {
	clearLocalSession,
	clearStoredApiKey,
	getBearerToken,
	getStoredApiKey,
	setStoredApiKey,
	writeLocalSession
} from '$lib/auth/credentials';
import { getAuthMode } from '$lib/auth/config';
import {
	endOidcSession,
	writeOidcAppRole,
	writeOidcOrganizationRoles,
	writeOidcTokens
} from '$lib/auth/oidc';
import { api } from '$lib/api/client';
import {
	hasPermissionFor,
	roleForOrganization,
	roleLabel
} from '$lib/auth/roles';
import {
	buildAuthProfile,
	canCallManagementApi,
	resolvedRolesFromProfile
} from '$lib/auth/session';
import { registerAuthProfileSync } from '$lib/auth/sync';
import { resetOrganizationsStore } from '$lib/stores/organizations.svelte';
import type {
	AppRole,
	AuthProfile,
	LocalSession,
	Permission,
	ResolvedRoles
} from '$lib/auth/types';
import { resetConfigSchemaStore } from '$lib/data/config-schema.svelte';
import { resetConfigStore } from '$lib/data/config.svelte';
import { resetOrgConfigStore } from '$lib/data/org-config.svelte';
import { resetOutboxStore } from '$lib/data/outbox.svelte';
import { resetSourcesStore } from '$lib/data/sources.svelte';
import { resetStatusStore } from '$lib/data/status.svelte';
import { resetTeamsStore } from '$lib/data/teams.svelte';

let profile = $state<AuthProfile | null>(null);
let isReady = $state(false);
/** Overrides from `GET /auth/me` (authoritative when present). */
let serverRoles = $state<ResolvedRoles | null>(null);
let meInFlight: Promise<void> | null = null;

function syncProfile(): void {
	const next = buildAuthProfile();
	if (profilesEquivalent(profile, next)) return;
	profile = next;
}

function orgMapsEqual(
	a: Record<string, AppRole>,
	b: Record<string, AppRole>
): boolean {
	const aKeys = Object.keys(a);
	const bKeys = Object.keys(b);
	if (aKeys.length !== bKeys.length) return false;
	return aKeys.every((key) => a[key] === b[key]);
}

function profilesEquivalent(a: AuthProfile | null, b: AuthProfile | null): boolean {
	if (a === b) return true;
	if (!a || !b) return false;
	return (
		a.method === b.method &&
		a.appRole === b.appRole &&
		a.localUsername === b.localUsername &&
		a.oidcProfile?.sub === b.oidcProfile?.sub &&
		a.oidcProfile?.email === b.oidcProfile?.email &&
		a.oidcProfile?.name === b.oidcProfile?.name &&
		a.oidcRoles.length === b.oidcRoles.length &&
		a.oidcRoles.every((role, i) => role === b.oidcRoles[i]) &&
		orgMapsEqual(a.organizationRoles, b.organizationRoles)
	);
}

function effectiveRoles(): ResolvedRoles | null {
	if (!profile) return null;
	const local = resolvedRolesFromProfile(profile);
	if (!serverRoles) return local;
	return {
		global: serverRoles.global,
		perOrg: { ...local.perOrg, ...serverRoles.perOrg }
	};
}

registerAuthProfileSync(syncProfile);

export function initAuth(): void {
	syncProfile();
	isReady = true;
}

/**
 * Refresh role ladder from the server. Api-key → always admin; OIDC includes
 * `organization_roles`. Soft-fails so a downed `/me` does not lock the UI.
 */
export async function refreshAuthMe(): Promise<void> {
	if (!canCallManagementApi()) {
		serverRoles = null;
		return;
	}
	if (meInFlight) return meInFlight;
	meInFlight = (async () => {
		try {
			const me = await api.getAuthMe();
			const global: AppRole =
				me.role ??
				(me.method === 'api_key' ? 'admin' : (profile?.appRole ?? 'viewer'));
			const perOrg = (me.organization_roles ?? {}) as Record<string, AppRole>;
			serverRoles = { global, perOrg };

			if (me.method === 'oidc' || profile?.method === 'oidc' || profile?.method === 'hybrid') {
				writeOidcAppRole(global);
				writeOidcOrganizationRoles(perOrg);
				syncProfile();
			} else if (me.method === 'api_key' && profile) {
				// Align local profile with server when Vite default drifted.
				if (profile.appRole !== global) {
					profile = { ...profile, appRole: global };
				}
			}
		} catch {
			// Keep client-side mapping; do not clear a previously good server snapshot.
		} finally {
			meInFlight = null;
		}
	})();
	return meInFlight;
}

export function getAuthState() {
	return {
		get profile() {
			return profile;
		},
		get authMode() {
			return getAuthMode();
		},
		get apiKey() {
			return getStoredApiKey();
		},
		get bearerToken() {
			return getBearerToken();
		},
		get isReady() {
			return isReady;
		},
		get isAuthenticated() {
			return Boolean(profile) && canCallManagementApi();
		},
		get appRole(): AppRole {
			return effectiveRoles()?.global ?? profile?.appRole ?? 'viewer';
		},
		get organizationRoles(): Record<string, AppRole> {
			return effectiveRoles()?.perOrg ?? profile?.organizationRoles ?? {};
		},
		/** Effective role for an org (or global when `organizationId` is omitted). */
		roleForOrg(organizationId?: string | null): AppRole {
			const roles = effectiveRoles();
			if (!roles) return 'viewer';
			return roleForOrganization(roles, organizationId);
		},
		roleLabel(organizationId?: string | null): string {
			return roleLabel(this.roleForOrg(organizationId));
		},
		/**
		 * Permission check. Pass `organizationId` for org-scoped config write/validate;
		 * omit for instance-wide actions (poll, requeue, nav).
		 */
		hasPermission(permission: Permission, organizationId?: string | null): boolean {
			const roles = effectiveRoles();
			if (!roles) return false;
			return hasPermissionFor(roles, permission, organizationId);
		}
	};
}

export function loginWithApiKey(key: string): void {
	setStoredApiKey(key);
	serverRoles = null;
	syncProfile();
	void refreshAuthMe();
}

export function loginWithLocalSession(session: LocalSession): void {
	writeLocalSession(session);
	serverRoles = null;
	syncProfile();
	void refreshAuthMe();
}

export function loginWithOidcSession(): void {
	serverRoles = null;
	syncProfile();
	void refreshAuthMe();
}

export async function logout(): Promise<void> {
	const mode = getAuthMode();

	if (mode === 'local' || mode === 'hybrid') {
		try {
			await api.logout();
		} catch {
			// ignore
		}
	}

	clearStoredApiKey();
	clearLocalSession();
	writeOidcTokens(null);
	writeOidcAppRole(null);
	writeOidcOrganizationRoles(null);
	profile = null;
	serverRoles = null;
	resetSourcesStore();
	resetStatusStore();
	resetOutboxStore();
	resetTeamsStore();
	resetConfigStore();
	resetConfigSchemaStore();
	resetOrgConfigStore();
	resetOrganizationsStore();

	if (mode === 'oidc' || mode === 'hybrid') {
		await endOidcSession();
	}
}
