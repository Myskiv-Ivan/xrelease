import { describe, expect, it, beforeEach, afterEach } from 'vitest';
import { readUiSetting } from '$lib/config/runtime';

describe('readUiSetting', () => {
	const scope = globalThis as typeof globalThis & {
		window: Window & { __XRELEASE_UI__?: Record<string, string> };
	};
	const hadWindow = 'window' in globalThis;
	const previous = hadWindow ? scope.window : undefined;

	beforeEach(() => {
		scope.window = { __XRELEASE_UI__: undefined } as typeof scope.window;
	});

	afterEach(() => {
		if (hadWindow && previous) {
			scope.window = previous;
		} else {
			Reflect.deleteProperty(globalThis, 'window');
		}
	});

	it('prefers runtime window.__XRELEASE_UI__ over bake-time env', () => {
		scope.window.__XRELEASE_UI__ = { VITE_AUTH_MODE: 'oidc' };
		expect(readUiSetting('VITE_AUTH_MODE')).toBe('oidc');
	});

	it('preserves empty string from runtime (same-origin API)', () => {
		scope.window.__XRELEASE_UI__ = { VITE_API_URL: '' };
		expect(readUiSetting('VITE_API_URL')).toBe('');
	});

	it('returns undefined when runtime key is missing', () => {
		scope.window.__XRELEASE_UI__ = {};
		expect(readUiSetting('VITE_OIDC_ISSUER')).toBeUndefined();
	});
});
