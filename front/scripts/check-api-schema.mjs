/**
 * Fail when `src/lib/api/schema.d.ts` drifts from `api/openapi.json`.
 *
 * Regenerates the schema into a temp file with the same pinned
 * openapi-typescript version as `npm run gen:api` and byte-compares it with
 * the committed file. Wired into `npm run check` so contract drift fails the
 * same gate as type errors.
 */

import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const SPEC = '../api/openapi.json';
const COMMITTED = 'src/lib/api/schema.d.ts';
const GENERATOR = 'openapi-typescript@7.13.0';

const tempDir = mkdtempSync(join(tmpdir(), 'xrelease-api-schema-'));
const generated = join(tempDir, 'schema.d.ts');

try {
	const result = spawnSync('npx', ['-y', GENERATOR, SPEC, '-o', generated], {
		stdio: ['ignore', 'ignore', 'inherit']
	});
	if (result.status !== 0) {
		console.error(`check:api — ${GENERATOR} failed (exit ${result.status})`);
		process.exit(result.status ?? 1);
	}

	const normalize = (text) => text.replaceAll('\r\n', '\n');
	const expected = normalize(readFileSync(generated, 'utf8'));
	const actual = normalize(readFileSync(COMMITTED, 'utf8'));

	if (expected !== actual) {
		console.error(
			`check:api — ${COMMITTED} is stale relative to ${SPEC}.\n` +
				'Run `npm run gen:api` and commit the result.'
		);
		process.exit(1);
	}
	console.log('check:api — API schema is in sync with openapi.json');
} finally {
	rmSync(tempDir, { recursive: true, force: true });
}
