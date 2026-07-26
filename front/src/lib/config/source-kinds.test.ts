import { describe, expect, it, vi } from 'vitest';

vi.mock('$lib/data/config-schema.svelte', () => ({
	getConfigSchemaStore: () => ({
		labelForKind: (kind: string) => (kind === 'github' ? 'GitHub (schema)' : null),
		sourceKinds: [{ value: 'github', label: 'GitHub (schema)' }]
	})
}));

import { getSourceKindMeta, listSourceKindValues } from './source-kinds';

describe('getSourceKindMeta', () => {
	it('prefers schema labels over local fallback', () => {
		expect(getSourceKindMeta('github').label).toBe('GitHub (schema)');
		expect(getSourceKindMeta('github').glyph).toBe('GH');
	});

	it('falls back for kinds absent from the schema', () => {
		expect(getSourceKindMeta('npm').label).toBe('npm');
		expect(getSourceKindMeta('unknown-kind').label).toBe('unknown-kind');
	});
});

describe('listSourceKindValues', () => {
	it('returns schema values when available', () => {
		expect(listSourceKindValues()).toEqual(['github']);
	});
});
