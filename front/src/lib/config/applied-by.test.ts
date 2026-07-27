import { describe, expect, it } from 'vitest';
import { formatAppliedBy, parseAppliedBy } from './applied-by';

describe('parseAppliedBy', () => {
	it('reads a local account as its login', () => {
		expect(parseAppliedBy('local:alice')).toEqual({
			identity: 'alice',
			origin: 'local',
			claimed: undefined
		});
	});

	it('reads an SSO principal as its email', () => {
		expect(parseAppliedBy('oidc:alice@example.com')).toEqual({
			identity: 'alice@example.com',
			origin: 'oidc',
			claimed: undefined
		});
	});

	it('still resolves an SSO principal the IdP identified only by subject', () => {
		expect(parseAppliedBy('oidc:8f3a-b7c1-c21')).toEqual({
			identity: '8f3a-b7c1-c21',
			origin: 'oidc',
			claimed: undefined
		});
	});

	it('names the api-key credential rather than showing its raw token label', () => {
		expect(parseAppliedBy('api_key')).toEqual({
			identity: 'API key',
			origin: 'api',
			claimed: undefined
		});
	});

	it('separates the client-claimed label from the verified identity', () => {
		// The parenthesised part is an unauthenticated X-Config-Applied-By header,
		// so it must never be mistaken for who actually applied the revision.
		expect(parseAppliedBy('local:alice (ci-deploy)')).toEqual({
			identity: 'alice',
			origin: 'local',
			claimed: 'ci-deploy'
		});
	});

	it('flags an unauthenticated apply', () => {
		expect(parseAppliedBy('unauthenticated (ci-deploy)')).toEqual({
			identity: 'Unauthenticated',
			origin: 'anonymous',
			claimed: 'ci-deploy'
		});
	});

	it('keeps an unrecognised label verbatim instead of dropping provenance', () => {
		expect(parseAppliedBy('kerberos:bob')).toEqual({
			identity: 'kerberos:bob',
			origin: 'unknown',
			claimed: undefined
		});
	});

	it('treats missing and blank values as no provenance', () => {
		expect(parseAppliedBy(null)).toBeNull();
		expect(parseAppliedBy(undefined)).toBeNull();
		expect(parseAppliedBy('   ')).toBeNull();
	});

	it('does not mistake an email-only value for a scheme prefix', () => {
		// `mailto:`-less addresses have no colon, but a bare `oidc` with no
		// subject must not be read as an empty identity.
		expect(parseAppliedBy('oidc')).toEqual({
			identity: 'SSO user',
			origin: 'oidc',
			claimed: undefined
		});
	});
});

describe('formatAppliedBy', () => {
	it('renders identity alone when nothing was claimed', () => {
		expect(formatAppliedBy('oidc:alice@example.com')).toBe('alice@example.com');
	});

	it('appends the claimed label so a CI run stays traceable', () => {
		expect(formatAppliedBy('api_key (nightly)')).toBe('API key (nightly)');
	});

	it('falls back to the empty marker', () => {
		expect(formatAppliedBy(null)).toBe('—');
	});
});
