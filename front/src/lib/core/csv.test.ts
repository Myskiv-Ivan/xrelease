import { describe, expect, it } from 'vitest';
import { csvEscape, toCsv } from './csv';

describe('csvEscape', () => {
	it('leaves plain values alone', () => {
		expect(csvEscape('pending')).toBe('pending');
		expect(csvEscape(42)).toBe('42');
		expect(csvEscape(null)).toBe('');
	});

	it('quotes fields with commas, quotes, or newlines', () => {
		expect(csvEscape('a,b')).toBe('"a,b"');
		expect(csvEscape('say "hi"')).toBe('"say ""hi"""');
		expect(csvEscape('line\nbreak')).toBe('"line\nbreak"');
	});
});

describe('toCsv', () => {
	it('builds CRLF rows with a trailing newline', () => {
		expect(toCsv(['id', 'name'], [[1, 'alpha'], [2, 'be,ta']])).toBe(
			'id,name\r\n1,alpha\r\n2,"be,ta"\r\n'
		);
	});
});
