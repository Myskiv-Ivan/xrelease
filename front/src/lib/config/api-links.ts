import {
	getHealthUrl,
	getMetricsUrl,
	getOpenApiSpecUrl,
	getReadyUrl
} from '$lib/api/urls';

export interface ApiResourceLink {
	/** i18n dot-path (e.g. `about.health`). */
	labelKey: string;
	href: string;
	external?: boolean;
}

/** External system endpoints served by the xrelease API process. */
export function getSystemApiLinks(): ApiResourceLink[] {
	return [
		{ labelKey: 'about.health', href: getHealthUrl(), external: true },
		{ labelKey: 'about.ready', href: getReadyUrl(), external: true },
		{ labelKey: 'about.metrics', href: getMetricsUrl(), external: true },
		{ labelKey: 'about.openapi', href: getOpenApiSpecUrl(), external: true }
	];
}

/** In-app API documentation routes. */
export const INTERNAL_API_LINKS: ApiResourceLink[] = [
	{ labelKey: 'nav.apiDocs', href: '/docs', external: false }
];
