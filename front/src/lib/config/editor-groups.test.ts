import { describe, expect, it } from 'vitest';
import { groupIndexedByKind } from './editor-groups';

describe('groupIndexedByKind', () => {
	it('preserves original indexes and follows kindOrder', () => {
		const rows = [
			{ type: 'webhook', name: 'a' },
			{ type: 'apprise', name: 'b' },
			{ type: 'webhook', name: 'c' },
			{ type: 'smtp', name: 'd' }
		];
		const groups = groupIndexedByKind(rows, (row) => row.type, ['apprise', 'webhook', 'smtp']);
		expect(groups.map((g) => g.kind)).toEqual(['apprise', 'webhook', 'smtp']);
		expect(groups[0]!.items.map((e) => e.index)).toEqual([1]);
		expect(groups[1]!.items.map((e) => e.index)).toEqual([0, 2]);
		expect(groups[2]!.items.map((e) => e.index)).toEqual([3]);
	});

	it('appends unknown kinds alphabetically after the ordered list', () => {
		const rows = [{ type: 'zzz' }, { type: 'aaa' }, { type: 'webhook' }];
		const groups = groupIndexedByKind(rows, (row) => row.type, ['webhook']);
		expect(groups.map((g) => g.kind)).toEqual(['webhook', 'aaa', 'zzz']);
	});
});
