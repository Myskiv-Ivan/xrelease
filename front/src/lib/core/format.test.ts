import { describe, expect, it } from 'vitest';
import {
	formatBoolean,
	formatInterval,
	formatDateTime,
	formatRelative,
	formatUptime,
	EMPTY_VALUE
} from './format';

describe('formatUptime', () => {
	it('formats seconds, minutes, hours, days', () => {
		expect(formatUptime(45)).toBe('45s');
		expect(formatUptime(120)).toBe('2m');
		expect(formatUptime(3660)).toBe('1h 1m');
		expect(formatUptime(7200)).toBe('2h');
		expect(formatUptime(90000)).toBe('1d 1h');
	});
});

describe('formatInterval', () => {
	it('uses minutes below an hour', () => {
		expect(formatInterval(1800)).toBe('30m');
	});

	it('uses hours at or above an hour', () => {
		expect(formatInterval(86400)).toBe('24.0h');
	});
});

describe('formatDateTime', () => {
	it('returns a dash for empty input', () => {
		expect(formatDateTime(null)).toBe(EMPTY_VALUE);
		expect(formatDateTime('   ')).toBe(EMPTY_VALUE);
	});

	it('returns the raw value for unparseable input', () => {
		expect(formatDateTime('not-a-date')).toBe('not-a-date');
	});

	it('formats absolute month day year time', () => {
		const label = formatDateTime('2026-07-23T13:05:00.000Z');
		expect(label).toMatch(/Jul/);
		expect(label).toMatch(/23/);
		expect(label).toMatch(/2026/);
		expect(label).toMatch(/\d{2}:\d{2}/);
		expect(label).not.toMatch(/^\d+[mhd]$/);
	});
});

describe('formatBoolean', () => {
	it('maps booleans to Yes/No', () => {
		expect(formatBoolean(true)).toBe('Yes');
		expect(formatBoolean(false)).toBe('No');
	});
});

describe('formatRelative', () => {
	const now = new Date('2026-07-19T12:00:00.000Z');

	it('returns a dash for empty input', () => {
		expect(formatRelative(null, now)).toBe(EMPTY_VALUE);
		expect(formatRelative('   ', now)).toBe(EMPTY_VALUE);
	});

	it('formats recent past times', () => {
		expect(formatRelative(new Date('2026-07-19T11:59:50.000Z'), now)).toBe('now');
		expect(formatRelative(new Date('2026-07-19T11:55:00.000Z'), now)).toBe('5m');
		expect(formatRelative(new Date('2026-07-19T10:00:00.000Z'), now)).toBe('2h');
		expect(formatRelative(new Date('2026-07-17T12:00:00.000Z'), now)).toBe('2d');
	});

	it('falls back to absolute formatting after a week', () => {
		const older = new Date('2026-07-01T12:00:00.000Z');
		expect(formatRelative(older, now)).toBe(formatDateTime(older.toISOString()));
	});
});
