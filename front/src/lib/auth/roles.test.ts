import { describe, expect, it } from 'vitest';
import {
	hasPermissionFor,
	maxRole,
	resolveAppRole,
	resolveResolvedRoles,
	roleForOrganization
} from '$lib/auth/roles';
import {
	belongsToOrganization,
	displayIdWithoutOrg,
	organizationIdFromNamespaced
} from '$lib/core/organization';

const mapping = {
	admin: ['xrelease-admin', 'admin'],
	operator: ['xrelease-operator', 'operator'],
	viewer: ['xrelease-viewer', 'viewer']
};

describe('resolveResolvedRoles', () => {
	it('parses scoped aliases into per-org grants', () => {
		const roles = resolveResolvedRoles(
			['xrelease-viewer', 'xrelease-admin:platform', 'xrelease-operator:security'],
			mapping,
			'viewer'
		);
		expect(roles.global).toBe('viewer');
		expect(roles.perOrg.platform).toBe('admin');
		expect(roles.perOrg.security).toBe('operator');
		expect(roleForOrganization(roles, 'platform')).toBe('admin');
		expect(roleForOrganization(roles, 'other')).toBe('viewer');
		expect(roleForOrganization(roles, null)).toBe('viewer');
	});

	it('lets global admin win everywhere', () => {
		const roles = resolveResolvedRoles(
			['xrelease-admin', 'xrelease-viewer:platform'],
			mapping,
			'viewer'
		);
		expect(roles.global).toBe('admin');
		expect(roleForOrganization(roles, 'platform')).toBe('admin');
	});

	it('keeps bare resolveAppRole for global-only mapping', () => {
		expect(
			resolveAppRole(['xrelease-operator', 'xrelease-admin'], mapping, 'viewer')
		).toBe('admin');
	});

	it('does not promote unmatched claims to admin', () => {
		expect(resolveAppRole(['engineering'], mapping, 'viewer')).toBe('viewer');
		expect(resolveAppRole(['xrelease-operator'], mapping, 'viewer')).toBe('operator');
	});

	it('trims scoped org ids and ignores unknown aliases', () => {
		const roles = resolveResolvedRoles(
			['xrelease-admin: platform ', 'unknown:security'],
			mapping,
			'viewer'
		);
		expect(roles.global).toBe('viewer');
		expect(roles.perOrg.platform).toBe('admin');
		expect(roles.perOrg.security).toBeUndefined();
	});
});

describe('hasPermissionFor', () => {
	it('gates config:write by org effective role', () => {
		const roles = resolveResolvedRoles(
			['xrelease-viewer', 'xrelease-admin:platform'],
			mapping,
			'viewer'
		);
		expect(hasPermissionFor(roles, 'config:write')).toBe(false);
		expect(hasPermissionFor(roles, 'config:write', 'platform')).toBe(true);
		expect(hasPermissionFor(roles, 'config:write', 'security')).toBe(false);
		expect(hasPermissionFor(roles, 'config:read', 'security')).toBe(true);
	});
});

describe('maxRole', () => {
	it('picks the higher privilege', () => {
		expect(maxRole('viewer', 'admin')).toBe('admin');
		expect(maxRole('operator', 'viewer')).toBe('operator');
	});
});

describe('organization namespacing', () => {
	it('extracts org from namespaced ids', () => {
		expect(organizationIdFromNamespaced('platform::github:o/r')).toBe('platform');
		expect(organizationIdFromNamespaced('github:o/r')).toBeNull();
	});

	it('filters by selected organization', () => {
		expect(belongsToOrganization('platform::pypi:requests', 'platform')).toBe(true);
		expect(belongsToOrganization('security::pypi:requests', 'platform')).toBe(false);
		expect(belongsToOrganization('pypi:requests', null)).toBe(true);
	});

	it('strips org prefix for display', () => {
		expect(displayIdWithoutOrg('platform::github:o/r', 'platform')).toBe('github:o/r');
		expect(displayIdWithoutOrg('platform::github:o/r', 'security')).toBe(
			'platform::github:o/r'
		);
	});
});
