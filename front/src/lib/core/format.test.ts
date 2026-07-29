import { describe, expect, it } from 'vitest';
import {
	formatBoolean,
	formatInterval,
	formatDateTimeFull,
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

describe('formatDateTimeFull', () => {
	it('returns a dash for empty input', () => {
		expect(formatDateTimeFull(null)).toBe(EMPTY_VALUE);
		expect(formatDateTimeFull('   ')).toBe(EMPTY_VALUE);
	});

	it('returns the raw value for unparseable input', () => {
		expect(formatDateTimeFull('not-a-date')).toBe('not-a-date');
	});

	it('formats absolute month day year time', () => {
		const label = formatDateTimeFull('2026-07-23T13:05:00.000Z');
		expect(label).toMatch(/Jul/);
		expect(label).toMatch(/23/);
		expect(label).toMatch(/2026/);
		expect(label).not.toMatch(/^\d+[mhd]$/);
	});

	it('always includes seconds — two events a minute apart must differ', () => {
		const a = formatDateTimeFull('2026-07-23T13:05:07.000Z');
		const b = formatDateTimeFull('2026-07-23T13:05:42.000Z');
		expect(a).toMatch(/\d{2}:\d{2}:\d{2}/);
		expect(a).not.toBe(b);
	});

	it('pins the locale so the shape does not follow the operator OS', () => {
		// `toLocaleString()` would render differently per machine; ops tables and
		// screenshots have to be comparable.
		expect(formatDateTimeFull('2026-07-23T13:05:07.000Z')).toBe(
			new Intl.DateTimeFormat('en', {
				year: 'numeric',
				month: 'short',
				day: 'numeric',
				hour: '2-digit',
				minute: '2-digit',
				second: '2-digit',
				hour12: false
			}).format(new Date('2026-07-23T13:05:07.000Z'))
		);
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

	it('falls back to the one absolute format after a week', () => {
		// Same format as every other timestamp — "63d" stops being useful, and a
		// second absolute shape would defeat the point of unifying them.
		const older = new Date('2026-07-01T12:00:00.000Z');
		expect(formatRelative(older, now)).toBe(formatDateTimeFull(older));
	});
});
