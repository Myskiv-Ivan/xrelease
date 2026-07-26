/**
 * Secret + `*_env` are alternatives (refs-first): paste a value **or** name an
 * env var — not both. Helpers keep channel / source forms on one vocabulary.
 */

import type { NotifierFieldSpec } from './desired-document';

/** Value-field keys that have a matching `*_env` companion in `specs`. */
export function secretValueKeys(specs: readonly NotifierFieldSpec[]): string[] {
	const keys = new Set(specs.map((spec) => spec.key));
	return specs
		.filter((spec) => keys.has(`${spec.key}_env`) && !spec.key.endsWith('_env'))
		.map((spec) => spec.key);
}

/** True when `key` is the env companion of another field in `specs` (skip solo render). */
export function isSecretEnvCompanion(
	key: string,
	specs: readonly NotifierFieldSpec[]
): boolean {
	if (!key.endsWith('_env')) return false;
	const valueKey = key.slice(0, -'_env'.length);
	return specs.some((spec) => spec.key === valueKey);
}

export type SecretOrEnvMode = 'value' | 'env';

/**
 * Which side of the pair to show. Prefers env when only the env name (or a
 * redacted value + env) is present — the usual post-Apply GitOps shape.
 */
export function detectSecretOrEnvMode(input: {
	value: string;
	env: string;
	valuePending: boolean;
}): SecretOrEnvMode {
	const hasEnv = input.env.trim().length > 0;
	const hasValue = input.value.trim().length > 0;
	if (hasEnv && !hasValue) return 'env';
	if (hasEnv && input.valuePending && !hasValue) return 'env';
	return 'value';
}
