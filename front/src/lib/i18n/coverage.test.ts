import { describe, expect, it } from 'vitest';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { en } from './locales/en';

const SRC = fileURLToPath(new URL('../..', import.meta.url));
const SCANNED = ['.svelte', '.ts'];
/** Direct `t('a.b')` calls — these must resolve or the UI renders a raw key. */
const KEY_RE = /\bt\(\s*'([^']+)'\s*\)/g;
/** Any dotted string literal, to catch indirection like `labelKey: 'nav.about'`. */
const LITERAL_RE = /'([a-zA-Z][\w]*(?:\.[\w]+)+)'/g;
/** `t(`outboxStatus.${x}`)` — the whole `outboxStatus.` subtree is reachable. */
const DYNAMIC_RE = /\bt\(\s*`([^`$]*)\$\{/g;

function walk(dir: string, out: string[] = []): string[] {
	for (const entry of readdirSync(dir)) {
		const path = join(dir, entry);
		if (statSync(path).isDirectory()) {
			walk(path, out);
		} else if (SCANNED.some((ext) => entry.endsWith(ext)) && !entry.includes('.test.')) {
			out.push(path);
		}
	}
	return out;
}

function resolve(path: string): unknown {
	return path
		.split('.')
		.reduce<unknown>(
			(node, part) =>
				node && typeof node === 'object'
					? (node as Record<string, unknown>)[part]
					: undefined,
			en
		);
}

function flatten(node: unknown, prefix = '', out: string[] = []): string[] {
	if (typeof node === 'string') {
		out.push(prefix);
		return out;
	}
	if (node && typeof node === 'object') {
		for (const [key, value] of Object.entries(node)) {
			flatten(value, prefix ? `${prefix}.${key}` : key, out);
		}
	}
	return out;
}

const files = walk(SRC);
/** Literal `t()` calls, mapped to the files that make them. */
const used = new Map<string, string[]>();
/** Every dotted literal in the tree — covers keys passed around as data. */
const literals = new Set<string>();
const dynamicPrefixes = new Set<string>();

for (const file of files) {
	const source = readFileSync(file, 'utf8');
	const where = file.slice(SRC.length);
	for (const match of source.matchAll(KEY_RE)) {
		used.set(match[1], [...(used.get(match[1]) ?? []), where]);
	}
	for (const match of source.matchAll(LITERAL_RE)) {
		literals.add(match[1]);
	}
	for (const match of source.matchAll(DYNAMIC_RE)) {
		if (match[1]) dynamicPrefixes.add(match[1]);
	}
}

function isReachable(key: string): boolean {
	if (literals.has(key)) return true;
	return [...dynamicPrefixes].some((prefix) => key.startsWith(prefix));
}

describe('i18n coverage', () => {
	it('scans the source tree', () => {
		expect(files.length).toBeGreaterThan(50);
		expect(used.size).toBeGreaterThan(50);
	});

	it('resolves every t() key to a string', () => {
		const missing = [...used.entries()]
			.filter(([key]) => typeof resolve(key) !== 'string')
			.map(([key, where]) => `${key} — used in ${where.join(', ')}`);
		expect(missing).toEqual([]);
	});

	it('has no unused locale entries', () => {
		const unused = flatten(en).filter((key) => !isReachable(key));
		expect(unused).toEqual([]);
	});
});
