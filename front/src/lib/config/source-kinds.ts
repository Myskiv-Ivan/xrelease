/**
 * Client-side presentation for source kinds (glyph + tone).
 *
 * Canonical kind **labels** come from `GET /api/v1/config/schema` via
 * `getConfigSchemaStore()` — this module only supplies UI chrome that the
 * server does not own. When the schema has not loaded yet (or a kind is
 * unknown to the running binary), we fall back to the local map / raw kind.
 */

import { getConfigSchemaStore } from '$lib/data/config-schema.svelte';

export interface SourceKindMeta {
	label: string;
	/** Short glyph for compact tables (no external icon deps). */
	glyph: string;
	tone: 'blue' | 'purple' | 'green' | 'orange' | 'cyan' | 'pink' | 'slate';
}

interface SourceKindPresentation {
	glyph: string;
	tone: SourceKindMeta['tone'];
	/** Offline / pre-schema fallback label only. */
	fallbackLabel: string;
}

const DEFAULT_PRESENTATION: SourceKindPresentation = {
	glyph: '◆',
	tone: 'slate',
	fallbackLabel: 'Source'
};

const KIND_PRESENTATION: Record<string, SourceKindPresentation> = {
	github: { glyph: 'GH', tone: 'slate', fallbackLabel: 'GitHub' },
	codeberg: { glyph: 'CB', tone: 'green', fallbackLabel: 'Codeberg' },
	gitea: { glyph: 'GT', tone: 'green', fallbackLabel: 'Gitea' },
	gitlab: { glyph: 'GL', tone: 'orange', fallbackLabel: 'GitLab' },
	bitbucket: { glyph: 'BB', tone: 'blue', fallbackLabel: 'Bitbucket' },
	docker: { glyph: 'DK', tone: 'blue', fallbackLabel: 'Docker' },
	ghcr: { glyph: 'CR', tone: 'slate', fallbackLabel: 'GHCR' },
	quay: { glyph: 'QY', tone: 'orange', fallbackLabel: 'Quay' },
	feed: { glyph: 'RSS', tone: 'orange', fallbackLabel: 'Feed' },
	pypi: { glyph: 'Py', tone: 'cyan', fallbackLabel: 'PyPI' },
	npm: { glyph: 'npm', tone: 'pink', fallbackLabel: 'npm' },
	cargo: { glyph: 'Rs', tone: 'orange', fallbackLabel: 'Cargo' },
	maven: { glyph: 'MV', tone: 'orange', fallbackLabel: 'Maven' },
	nuget: { glyph: 'Nu', tone: 'blue', fallbackLabel: 'NuGet' },
	hex: { glyph: 'Hx', tone: 'purple', fallbackLabel: 'Hex' },
	rubygems: { glyph: 'Rb', tone: 'pink', fallbackLabel: 'RubyGems' },
	packagist: { glyph: 'PHP', tone: 'purple', fallbackLabel: 'Packagist' },
	artifacthub: { glyph: 'AH', tone: 'cyan', fallbackLabel: 'Artifact Hub' },
	yarn: { glyph: 'Yn', tone: 'pink', fallbackLabel: 'Yarn' },
	cpan: { glyph: 'CP', tone: 'purple', fallbackLabel: 'CPAN' },
	ecr: { glyph: 'EC', tone: 'orange', fallbackLabel: 'ECR Public' }
};

export function getSourceKindMeta(kind: string): SourceKindMeta {
	const presentation = KIND_PRESENTATION[kind] ?? {
		...DEFAULT_PRESENTATION,
		fallbackLabel: kind
	};
	const schemaLabel = getConfigSchemaStore().labelForKind(kind);
	return {
		label: schemaLabel ?? presentation.fallbackLabel,
		glyph: presentation.glyph,
		tone: presentation.tone
	};
}

/** All known kind values from the schema, falling back to the presentation map. */
export function listSourceKindValues(): string[] {
	const fromSchema = getConfigSchemaStore().sourceKinds.map((entry) => entry.value);
	if (fromSchema.length > 0) return fromSchema;
	return Object.keys(KIND_PRESENTATION).sort();
}

/** Badge chrome — semantic tokens only (no raw Tailwind palette colors). */
export const KIND_TONE_CLASSES: Record<SourceKindMeta['tone'], string> = {
	blue: 'bg-primary/15 text-primary',
	purple: 'bg-secondary text-secondary-foreground',
	green: 'bg-success/15 text-success',
	orange: 'bg-warning/15 text-warning',
	cyan: 'bg-chart-1/15 text-primary',
	pink: 'bg-destructive/15 text-destructive',
	slate: 'bg-muted text-muted-foreground'
};
