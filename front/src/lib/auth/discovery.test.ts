import { describe, expect, it } from 'vitest';
import { issuerMatches } from '$lib/auth/discovery';

const ISSUER = 'https://idp.example.test';

describe('issuerMatches', () => {
	it('treats trailing slash as insignificant', () => {
		expect(issuerMatches(ISSUER, `${ISSUER}/`)).toBe(true);
		expect(issuerMatches(`${ISSUER}/`, ISSUER)).toBe(true);
	});

	it('requires an exact host/path match otherwise', () => {
		expect(issuerMatches(ISSUER, `${ISSUER}/tenants/a`)).toBe(false);
	});
});
