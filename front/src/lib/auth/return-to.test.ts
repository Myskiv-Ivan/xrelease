import { describe, expect, it } from 'vitest';
import { DEFAULT_RETURN_TO, loginPath, safeReturnTo, sanitizeReturnTo } from '$lib/auth/return-to';

describe('loginPath', () => {
	it('preserves the deep link, path and query, so the bounce is resumable', () => {
		const path = loginPath(new URL('https://xrelease.test/sources?kind=github&page=2'));
		expect(path).toBe(`/login?returnTo=${encodeURIComponent('/sources?kind=github&page=2')}`);
	});

	it('omits returnTo for the root', () => {
		expect(loginPath(new URL('https://xrelease.test/'))).toBe('/login');
	});

	it('does not nest a login redirect inside itself', () => {
		expect(loginPath(new URL('https://xrelease.test/login?returnTo=%2Foutbox'))).toBe('/login');
		expect(loginPath(new URL('https://xrelease.test/login/callback?code=abc'))).toBe('/login');
	});
});

describe('safeReturnTo', () => {
	it('round-trips whatever loginPath produced', () => {
		const original = '/config?org=platform';
		const built = new URL(`https://xrelease.test${loginPath(new URL(`https://xrelease.test${original}`))}`);
		expect(safeReturnTo(built.searchParams)).toBe(original);
	});

	it('falls back to the root when absent', () => {
		expect(safeReturnTo(new URLSearchParams())).toBe(DEFAULT_RETURN_TO);
		expect(safeReturnTo(new URLSearchParams('returnTo='))).toBe(DEFAULT_RETURN_TO);
	});
});

describe('sanitizeReturnTo', () => {
	it('accepts root-relative in-app paths', () => {
		expect(sanitizeReturnTo('/outbox')).toBe('/outbox');
		expect(sanitizeReturnTo('/sources?kind=npm')).toBe('/sources?kind=npm');
	});

	// `returnTo` is attacker-controllable via any crafted /login?returnTo=… link,
	// so a successful login must never be able to land off-site.
	it.each([
		'https://evil.example/phish',
		'//evil.example/phish',
		'javascript:alert(1)',
		'outbox',
		'',
		null,
		undefined
	])('rejects %p in favour of the root', (value) => {
		expect(sanitizeReturnTo(value)).toBe(DEFAULT_RETURN_TO);
	});
});
