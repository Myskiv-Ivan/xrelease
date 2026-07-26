export type AuthMode = 'local' | 'api_key' | 'oidc' | 'hybrid';

export type AppRole = 'viewer' | 'operator' | 'admin';

export type Permission =
	| 'status:read'
	| 'sources:read'
	| 'outbox:read'
	| 'outbox:requeue'
	| 'diagnostics:read'
	| 'config:read'
	| 'config:write'
	| 'about:read'
	| 'poll:execute'
	| 'settings:write';

export interface OidcConfig {
	issuer: string;
	clientId: string;
	redirectUri: string;
	scopes: string[];
	roleClaim: string;
	roleMapping: Record<AppRole, string[]>;
}

export interface OidcProfile {
	sub: string;
	email?: string;
	name?: string;
	preferredUsername?: string;
}

export interface OidcTokens {
	accessToken: string;
	refreshToken?: string;
	idToken?: string;
	expiresAt: number;
	tokenType?: string;
	scope?: string;
	/** Verified ID Token claims (OIDC Core §3.1.3.7) */
	idTokenPayload?: import('$lib/auth/jwt').JwtPayload;
	/** Verified access token claims when token is a JWT */
	accessTokenPayload?: import('$lib/auth/jwt').JwtPayload;
}

export interface LocalSession {
	accessToken: string;
	expiresAt: number;
	role: AppRole;
	username: string;
	displayName?: string;
}

/** Instance-wide role plus optional per-organization grants. */
export interface ResolvedRoles {
	global: AppRole;
	perOrg: Record<string, AppRole>;
}

export interface AuthProfile {
	method: 'local' | 'api_key' | 'oidc' | 'hybrid';
	oidcProfile?: OidcProfile;
	localUsername?: string;
	/** Instance-wide role (legacy alias for `resolved.global`). */
	appRole: AppRole;
	/** Per-org OIDC grants; empty for local / api-key. */
	organizationRoles: Record<string, AppRole>;
	oidcRoles: string[];
}
