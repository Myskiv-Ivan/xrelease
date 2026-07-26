// See https://svelte.dev/docs/kit/types#app.d.ts
// for information about these interfaces
declare const __APP_VERSION__: string;

declare global {
	namespace App {
		// interface Error {}
		// interface Locals {}
		// interface PageData {}
		// interface PageState {}
		// interface Platform {}
	}

	interface ImportMetaEnv {
		readonly VITE_API_URL?: string;
		readonly VITE_AUTH_MODE?: string;
		readonly VITE_API_KEY_DEFAULT_ROLE?: string;
		readonly VITE_OIDC_ISSUER?: string;
		readonly VITE_OIDC_CLIENT_ID?: string;
		readonly VITE_OIDC_REDIRECT_URI?: string;
		readonly VITE_OIDC_SCOPES?: string;
		readonly VITE_OIDC_ROLE_CLAIM?: string;
		readonly VITE_OIDC_ROLE_ADMIN?: string;
		readonly VITE_OIDC_ROLE_OPERATOR?: string;
		readonly VITE_OIDC_ROLE_VIEWER?: string;
		readonly VITE_GRAFANA_EMBED_URL?: string;
	}

	interface ImportMeta {
		readonly env: ImportMetaEnv;
	}
}

export {};
