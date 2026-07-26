/**
 * Split the redacted config payload into bootstrap (infra) vs app layers.
 *
 * `GET /api/v1/config` returns:
 * - `content` — full effective Config (bootstrap + app), always TOML
 * - `desired_content` — desired doc re-serialized via Config (fills infra defaults)
 *
 * So App and Effective look nearly identical in the raw API strings. The UI
 * filters by known section keys instead.
 */

import {
	REDACTED,
	parseDesiredDocument,
	stringifyDesiredDocument,
	type DesiredMap
} from './desired-document';
import { asArray, asObject, str } from './desired-map';

/** Infra keys from bootstrap.toml (API `bootstrap_sections` + organizations). */
export const DEFAULT_BOOTSTRAP_KEYS = [
	'database',
	'api',
	'log',
	'config_api',
	'organizations'
] as const;

/** Desired-state keys edited via Git / UI apply / ledger. */
export const APP_KEYS = [
	'defaults',
	'sources',
	'teams',
	'notifiers',
	'presets'
] as const;

export function pickConfigKeys(data: DesiredMap, keys: readonly string[]): DesiredMap {
	const out: DesiredMap = {};
	for (const key of keys) {
		if (Object.prototype.hasOwnProperty.call(data, key) && data[key] !== undefined) {
			out[key] = data[key];
		}
	}
	return out;
}

/**
 * Drop `null` leaves that Config re-serialization injects (`token: null`, …).
 * Keeps empty arrays (e.g. `urls: []`) so missing Apprise targets stay visible.
 */
export function stripNullLeaves(value: unknown): unknown {
	if (value === null) return undefined;
	if (Array.isArray(value)) {
		return value.map(stripNullLeaves).filter((item) => item !== undefined);
	}
	if (value && typeof value === 'object') {
		const out: DesiredMap = {};
		for (const [key, child] of Object.entries(value as DesiredMap)) {
			const next = stripNullLeaves(child);
			if (next !== undefined) out[key] = next;
		}
		return out;
	}
	return value;
}

/** True when an Apprise notifier exists but has no deliverable targets (matches server). */
export function appriseTargetsMissing(data: DesiredMap): boolean {
	const rows = asArray(data.notifiers).map(asObject).filter((row) => str(row.type) === 'apprise');
	if (rows.length === 0) return false;
	return rows.some((apprise) => {
			const urls = apprise.urls;
			const hasUrls =
				(Array.isArray(urls) && urls.some((item) => str(item) && str(item) !== REDACTED)) ||
				(typeof urls === 'string' && urls.trim().length > 0 && urls !== REDACTED);
			const hasRedacted =
				(Array.isArray(urls) && urls.some((item) => item === REDACTED)) || urls === REDACTED;
			const hasKey = Boolean(str(apprise.config_key));
			const hasUrlsEnv = Boolean(str(apprise.urls_env));
			return !hasUrls && !hasRedacted && !hasKey && !hasUrlsEnv;
		});
	}

function extractLayer(
	raw: string | null | undefined,
	keys: readonly string[],
	opts?: { stripNulls?: boolean }
): string | null {
	const text = raw?.trim();
	if (!text) return null;
	try {
		const parsed = parseDesiredDocument(text);
		let data = pickConfigKeys(parsed.data, keys);
		if (Object.keys(data).length === 0) return null;
		if (opts?.stripNulls) {
			data = stripNullLeaves(data) as DesiredMap;
		}
		return stringifyDesiredDocument({ format: parsed.format, data });
	} catch {
		return null;
	}
}

/** Bootstrap / infra layer from the effective document. */
export function extractBootstrapDocument(
	effectiveRaw: string | null | undefined,
	bootstrapSections: readonly string[] = DEFAULT_BOOTSTRAP_KEYS
): string | null {
	const keys = [...new Set([...bootstrapSections, 'organizations'])];
	return extractLayer(effectiveRaw, keys, { stripNulls: true });
}

/**
 * App layer: prefer `desired_content`, then fall back to filtering effective.
 * Always strips infra defaults that Config re-serialization injects.
 * Null leaves are removed for readability; empty `urls: []` is kept.
 */
export function extractAppDocument(
	desiredRaw: string | null | undefined,
	effectiveRaw?: string | null | undefined
): string | null {
	return (
		extractLayer(desiredRaw, APP_KEYS, { stripNulls: true }) ??
		extractLayer(effectiveRaw, APP_KEYS, { stripNulls: true })
	);
}

/** Parsed app map (null-stripped) for Technical warnings. */
export function parseAppLayerData(
	desiredRaw: string | null | undefined,
	effectiveRaw?: string | null | undefined
): DesiredMap | null {
	const text = extractAppDocument(desiredRaw, effectiveRaw);
	if (!text) return null;
	try {
		return parseDesiredDocument(text).data;
	} catch {
		return null;
	}
}
