import { getApiBaseUrl } from '$lib/api/client';

/** Build an absolute URL against the configured xrelease API base. */
export function apiUrl(path: string): string {
	const base = getApiBaseUrl().replace(/\/$/, '');
	const normalized = path.startsWith('/') ? path : `/${path}`;
	return `${base}${normalized}`;
}

export function getOpenApiSpecUrl(): string {
	return apiUrl('/openapi.json');
}

export function getHealthUrl(): string {
	return apiUrl('/health');
}

export function getReadyUrl(): string {
	return apiUrl('/ready');
}

export function getMetricsUrl(): string {
	return apiUrl('/metrics');
}
