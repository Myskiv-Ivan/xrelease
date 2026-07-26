/**
 * Shared coercions for desired-state document maps (editor + validation).
 */

export type DesiredMap = Record<string, unknown>;

export function asObject(value: unknown): DesiredMap {
	if (value && typeof value === 'object' && !Array.isArray(value)) {
		return value as DesiredMap;
	}
	return {};
}

export function asArray(value: unknown): unknown[] {
	return Array.isArray(value) ? value : [];
}

export function str(value: unknown): string {
	if (value == null) return '';
	return String(value);
}

export function numOrEmpty(value: unknown): number | '' {
	if (typeof value === 'number' && Number.isFinite(value)) return value;
	if (typeof value === 'string' && value.trim() !== '' && Number.isFinite(Number(value))) {
		return Number(value);
	}
	return '';
}
