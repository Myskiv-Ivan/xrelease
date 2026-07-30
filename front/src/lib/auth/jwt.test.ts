import { describe, expect, it } from 'vitest';
import { validateIdTokenClaims, type JwtPayload } from '$lib/auth/jwt';

const ISSUER = 'https://idp.example.test';
const CLIENT_ID = 'test-client';

function payload(overrides: Partial<JwtPayload> = {}): JwtPayload {
	const now = Math.floor(Date.now() / 1000);
	return {
		iss: ISSUER,
		sub: 'user-1',
		aud: CLIENT_ID,
		exp: now + 3600,
		iat: now,
		...overrides
	};
}

describe('validateIdTokenClaims', () => {
	it('accepts issuer when only trailing slash differs', () => {
		const errors = validateIdTokenClaims(payload({ iss: `${ISSUER}/` }), {
			issuer: ISSUER,
			clientId: CLIENT_ID
		});
		expect(errors).toEqual([]);
	});

	it('accepts configured issuer that already has a trailing slash', () => {
		const errors = validateIdTokenClaims(payload({ iss: `${ISSUER}/` }), {
			issuer: `${ISSUER}/`,
			clientId: CLIENT_ID
		});
		expect(errors).toEqual([]);
	});

	it('rejects a different issuer host', () => {
		const errors = validateIdTokenClaims(payload({ iss: 'https://other.example.test/' }), {
			issuer: ISSUER,
			clientId: CLIENT_ID
		});
		expect(errors).toContain('iss mismatch');
	});

	it('rejects missing iss', () => {
		const errors = validateIdTokenClaims(payload({ iss: undefined }), {
			issuer: ISSUER,
			clientId: CLIENT_ID
		});
		expect(errors).toContain('iss mismatch');
	});
});
