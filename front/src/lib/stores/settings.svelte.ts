import { STORAGE_KEYS, DEFAULT_REFRESH_INTERVAL_MS } from '$lib/core/constants';

export type ThemeMode = 'dark' | 'light' | 'system';

let theme = $state<ThemeMode>('dark');
let autoRefresh = $state(true);
let refreshIntervalMs = $state(DEFAULT_REFRESH_INTERVAL_MS);
let initialized = $state(false);

function readStorage<T>(key: string, parse: (raw: string) => T | null): T | null {
	if (typeof localStorage === 'undefined') return null;
	const raw = localStorage.getItem(key);
	if (!raw) return null;
	return parse(raw);
}

function applyTheme(mode: ThemeMode) {
	if (typeof document === 'undefined') return;
	const root = document.documentElement;
	const resolved =
		mode === 'system'
			? window.matchMedia('(prefers-color-scheme: dark)').matches
				? 'dark'
				: 'light'
			: mode;
	root.dataset.theme = resolved;
	root.classList.toggle('dark', resolved === 'dark');
	root.style.colorScheme = resolved;
	// Mobile browser chrome — mirrors --background (see app.html).
	document
		.querySelector('meta[name="theme-color"]')
		?.setAttribute('content', resolved === 'dark' ? '#070B0E' : '#F2F6F8');
}

export function initSettings() {
	if (initialized) return;

	theme =
		readStorage(STORAGE_KEYS.theme, (raw) =>
			raw === 'light' || raw === 'dark' || raw === 'system' ? raw : null
		) ?? 'dark';

	autoRefresh =
		readStorage(STORAGE_KEYS.autoRefresh, (raw) =>
			raw === 'true' ? true : raw === 'false' ? false : null
		) ?? true;

	refreshIntervalMs =
		readStorage(STORAGE_KEYS.refreshIntervalMs, (raw) => {
			const value = Number(raw);
			return Number.isFinite(value) && value >= 5_000 ? value : null;
		}) ?? DEFAULT_REFRESH_INTERVAL_MS;

	applyTheme(theme);
	if (typeof document !== 'undefined') {
		document.documentElement.lang = 'en';
		window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
			if (theme === 'system') applyTheme('system');
		});
	}
	initialized = true;
}

export function getSettingsState() {
	return {
		get theme() {
			return theme;
		},
		get autoRefresh() {
			return autoRefresh;
		},
		get refreshIntervalMs() {
			return refreshIntervalMs;
		},
		get initialized() {
			return initialized;
		}
	};
}

export function isAutoRefreshEnabled(): boolean {
	return autoRefresh;
}

export function getRefreshIntervalMs(): number {
	return refreshIntervalMs;
}

export function setTheme(mode: ThemeMode) {
	theme = mode;
	localStorage.setItem(STORAGE_KEYS.theme, mode);
	applyTheme(mode);
}

/** Re-apply the app theme to `<html>` (e.g. after third-party widgets mutate the DOM). */
export function reapplyTheme() {
	applyTheme(theme);
}

/**
 * Strip body classes that Scalar (`dark-mode` / `light-mode`) leaves behind, then
 * restore the app theme tokens on `<html>`.
 */
export function clearExternalThemeArtifacts() {
	if (typeof document === 'undefined') return;
	document.body.classList.remove('dark-mode', 'light-mode');
	reapplyTheme();
}

export function setAutoRefresh(enabled: boolean) {
	autoRefresh = enabled;
	localStorage.setItem(STORAGE_KEYS.autoRefresh, String(enabled));
}

export function setRefreshIntervalMs(ms: number) {
	refreshIntervalMs = ms;
	localStorage.setItem(STORAGE_KEYS.refreshIntervalMs, String(ms));
}

export function cycleTheme() {
	const order: ThemeMode[] = ['dark', 'light', 'system'];
	const index = order.indexOf(theme);
	setTheme(order[(index + 1) % order.length]);
}
