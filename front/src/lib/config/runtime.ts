/** Runtime UI settings injected by the container as `/ui-config.js`. */
export type UiRuntimeConfig = Partial<{
	VITE_API_URL: string;
	VITE_AUTH_MODE: string;
	VITE_API_KEY_DEFAULT_ROLE: string;
	VITE_OIDC_ISSUER: string;
	VITE_OIDC_CLIENT_ID: string;
	VITE_OIDC_REDIRECT_URI: string;
	VITE_OIDC_SCOPES: string;
	VITE_OIDC_ROLE_CLAIM: string;
	VITE_OIDC_ROLE_ADMIN: string;
	VITE_OIDC_ROLE_OPERATOR: string;
	VITE_OIDC_ROLE_VIEWER: string;
	VITE_GRAFANA_EMBED_URL: string;
}>;

declare global {
	interface Window {
		__XRELEASE_UI__?: UiRuntimeConfig;
	}
}

/**
 * Read a UI setting: runtime `/ui-config.js` wins over bake-time `import.meta.env`.
 * Returns `undefined` when unset; empty string is preserved (e.g. same-origin API).
 */
export function readUiSetting(name: keyof UiRuntimeConfig): string | undefined {
	if (typeof window !== 'undefined') {
		const runtime = window.__XRELEASE_UI__;
		if (runtime && Object.prototype.hasOwnProperty.call(runtime, name)) {
			const value = runtime[name];
			if (typeof value === 'string') return value;
		}
	}

	const baked = import.meta.env[name as string];
	return typeof baked === 'string' ? baked : undefined;
}
