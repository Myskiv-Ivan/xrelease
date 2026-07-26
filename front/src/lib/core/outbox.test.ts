import { describe, expect, it } from 'vitest';
import { isDeferredDelivery } from './outbox';

describe('isDeferredDelivery', () => {
	const now = new Date('2026-07-22T12:00:00.000Z');

	it('is false when deliver_after is null', () => {
		expect(isDeferredDelivery({ deliver_after: null }, now)).toBe(false);
	});

	it('is false when deliver_after is in the past', () => {
		expect(isDeferredDelivery({ deliver_after: '2026-07-22T11:00:00.000Z' }, now)).toBe(
			false
		);
	});

	it('is true when deliver_after is in the future', () => {
		expect(isDeferredDelivery({ deliver_after: '2026-07-22T18:00:00.000Z' }, now)).toBe(true);
	});
});
