import { describe, expect, it } from 'vitest';
import {
	MESSAGE_TEMPLATE_PLACEHOLDERS,
	insertPlaceholder,
	wrapPlaceholder
} from './message-template';

describe('message-template', () => {
	it('lists schema placeholders', () => {
		expect(MESSAGE_TEMPLATE_PLACEHOLDERS).toContain('title');
		expect(MESSAGE_TEMPLATE_PLACEHOLDERS).toContain('tag');
		expect(MESSAGE_TEMPLATE_PLACEHOLDERS).toContain('kind');
	});

	it('wraps and inserts placeholders', () => {
		expect(wrapPlaceholder('title')).toBe('{{title}}');
		expect(insertPlaceholder('', 'url')).toBe('{{url}}');
		expect(insertPlaceholder('hi', 'url')).toBe('hi {{url}}');
		expect(insertPlaceholder('hi ', 'url')).toBe('hi {{url}}');
	});
});
