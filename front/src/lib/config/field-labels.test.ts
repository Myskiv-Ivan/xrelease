import { describe, expect, it } from 'vitest';
import { KIND_TONE_CLASSES } from './source-kinds';
import { notifierFieldHint, notifierFieldLabel, sourceFieldLabel } from './field-labels';

describe('KIND_TONE_CLASSES', () => {
	it('uses semantic theme tokens (no raw palette colors)', () => {
		const joined = Object.values(KIND_TONE_CLASSES).join(' ');
		expect(joined).not.toMatch(/\b(bg|text)-(blue|purple|green|orange|cyan|pink)-\d+/);
		expect(KIND_TONE_CLASSES.slate).toContain('bg-muted');
		expect(KIND_TONE_CLASSES.green).toContain('text-success');
		expect(KIND_TONE_CLASSES.blue).toContain('text-primary');
	});
});

describe('field-labels', () => {
	it('maps source identity fields', () => {
		expect(sourceFieldLabel('repo')).toContain('Repository');
		expect(sourceFieldLabel('unknown_field')).toBe('unknown field');
	});

	it('maps notifier secret fields with hints', () => {
		expect(notifierFieldLabel('access_token')).toMatch(/access token/i);
		expect(notifierFieldHint('access_token')).toMatch(/Bearer/i);
		expect(notifierFieldLabel('urls')).toMatch(/Target URLs/i);
		expect(notifierFieldHint('urls')).toMatch(/one per line|mailto/i);
		expect(notifierFieldLabel('urls_env')).toMatch(/env/i);
		expect(notifierFieldLabel('headers')).toMatch(/headers/i);
		expect(notifierFieldLabel('headers_env')).toMatch(/env/i);
		expect(notifierFieldHint('headers')).toMatch(/Static|blank|Apply/i);
		expect(notifierFieldHint('url')).toBeNull();
		expect(sourceFieldLabel('token')).toMatch(/token/i);
		expect(sourceFieldLabel('token_env')).toMatch(/env/i);
	});
});
