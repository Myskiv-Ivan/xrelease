import { describe, expect, it } from 'vitest';
import {
	detectSecretOrEnvMode,
	isSecretEnvCompanion,
	secretValueKeys
} from './secret-or-env';
import type { NotifierFieldSpec } from './desired-document';

const telegramSpecs: NotifierFieldSpec[] = [
	{ key: 'bot_token', type: 'secret' },
	{ key: 'bot_token_env', type: 'text' },
	{ key: 'chat_id', type: 'text', required: true },
	{ key: 'template', type: 'template' }
];

describe('secret-or-env', () => {
	it('lists value keys that have an _env companion', () => {
		expect(secretValueKeys(telegramSpecs)).toEqual(['bot_token']);
	});

	it('skips env companions when rendering pairs', () => {
		expect(isSecretEnvCompanion('bot_token_env', telegramSpecs)).toBe(true);
		expect(isSecretEnvCompanion('bot_token', telegramSpecs)).toBe(false);
		expect(isSecretEnvCompanion('chat_id', telegramSpecs)).toBe(false);
	});

	it('prefers env mode when only env (or redacted + env) is set', () => {
		expect(
			detectSecretOrEnvMode({ value: '', env: 'XRELEASE_UI_BOT', valuePending: false })
		).toBe('env');
		expect(
			detectSecretOrEnvMode({ value: '', env: 'XRELEASE_UI_BOT', valuePending: true })
		).toBe('env');
		expect(detectSecretOrEnvMode({ value: '', env: '', valuePending: false })).toBe('value');
		expect(
			detectSecretOrEnvMode({ value: 'secret', env: 'XRELEASE_UI_BOT', valuePending: false })
		).toBe('value');
	});
});
