import { en, type Messages } from './locales/en';

function resolvePath(obj: Record<string, unknown>, path: string): string | undefined {
	const parts = path.split('.');
	let current: unknown = obj;
	for (const part of parts) {
		if (current == null || typeof current !== 'object') return undefined;
		current = (current as Record<string, unknown>)[part];
	}
	return typeof current === 'string' ? current : undefined;
}

/** Translate a dot-path key (English only). */
export function t(key: string): string {
	return resolvePath(en as unknown as Record<string, unknown>, key) ?? key;
}

export type { Messages };
