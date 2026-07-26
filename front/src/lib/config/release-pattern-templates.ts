/**
 * Built-in release-filter templates — map `GET /api/v1/config/schema`
 * `builtin_presets` onto source form fields. The server is the SSOT
 * (`builtin_preset_schemas` in Rust); this module only shapes UI drafts.
 */

export interface BuiltinPresetSchemaEntry {
	name: string;
	description: string;
	include_prerelease?: boolean | null;
	prerelease_tags?: string[] | null;
	exclude_updated?: boolean | null;
	pattern?: string | null;
	exclude_pattern?: string | null;
}

export interface ReleasePatternTemplate {
	name: string;
	description: string;
	pattern: string;
	include_prerelease: boolean;
	prerelease_tags: string;
	exclude_updated: boolean;
	/** Optional exclude regex; empty for most built-ins. */
	exclude_pattern?: string;
}

/** Fields a template stamps onto a source draft (excluding identity). */
export type ReleasePatternFields = Pick<
	ReleasePatternTemplate,
	'pattern' | 'include_prerelease' | 'prerelease_tags' | 'exclude_updated'
> & { preset: string; exclude_pattern: string };

export function releasePatternTemplateFromSchema(
	entry: BuiltinPresetSchemaEntry
): ReleasePatternTemplate {
	return {
		name: entry.name,
		description: entry.description || entry.name,
		pattern: entry.pattern ?? '',
		include_prerelease: entry.include_prerelease ?? false,
		prerelease_tags: (entry.prerelease_tags ?? []).join(', '),
		exclude_updated: entry.exclude_updated ?? false,
		exclude_pattern: entry.exclude_pattern ?? undefined
	};
}

export function releasePatternTemplateByName(
	presets: readonly BuiltinPresetSchemaEntry[],
	name: string
): ReleasePatternTemplate | null {
	const entry = presets.find((item) => item.name === name);
	return entry ? releasePatternTemplateFromSchema(entry) : null;
}

export function fieldsFromReleasePatternTemplate(
	template: ReleasePatternTemplate
): ReleasePatternFields {
	return {
		preset: template.name,
		pattern: template.pattern,
		exclude_pattern: template.exclude_pattern ?? '',
		include_prerelease: template.include_prerelease,
		prerelease_tags: template.prerelease_tags,
		exclude_updated: template.exclude_updated
	};
}
