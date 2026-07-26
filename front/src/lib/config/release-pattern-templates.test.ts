import { describe, expect, it } from 'vitest';
import {
	fieldsFromReleasePatternTemplate,
	releasePatternTemplateByName,
	releasePatternTemplateFromSchema,
	type BuiltinPresetSchemaEntry
} from './release-pattern-templates';

const SAMPLE_PRESETS: BuiltinPresetSchemaEntry[] = [
	{
		name: 'wildcard',
		description: 'All tags including pre-releases (no pattern filter)',
		include_prerelease: true
	},
	{
		name: 'prerelease',
		description: 'Pre-release channels only',
		include_prerelease: true,
		prerelease_tags: ['alpha', 'beta', 'rc'],
		pattern: String.raw`^v?\d+\.\d+\.\d+-(alpha|beta|rc)`
	},
	{
		name: 'docker-semver',
		description: 'Numeric semver; exclude latest',
		include_prerelease: false,
		pattern: String.raw`^\d+\.\d+\.\d+$`,
		exclude_pattern: String.raw`^(latest|nightly|edge)$`
	}
];

describe('release-pattern-templates', () => {
	it('maps schema presets onto form fields including preset name', () => {
		const template = releasePatternTemplateByName(SAMPLE_PRESETS, 'prerelease');
		expect(template).not.toBeNull();
		expect(fieldsFromReleasePatternTemplate(template!)).toEqual({
			preset: 'prerelease',
			pattern: String.raw`^v?\d+\.\d+\.\d+-(alpha|beta|rc)`,
			exclude_pattern: '',
			include_prerelease: true,
			prerelease_tags: 'alpha, beta, rc',
			exclude_updated: false
		});
	});

	it('maps wildcard with empty pattern and prereleases on', () => {
		const template = releasePatternTemplateFromSchema(SAMPLE_PRESETS[0]!);
		expect(fieldsFromReleasePatternTemplate(template)).toMatchObject({
			preset: 'wildcard',
			pattern: '',
			include_prerelease: true
		});
	});

	it('keeps exclude_pattern from schema', () => {
		const template = releasePatternTemplateByName(SAMPLE_PRESETS, 'docker-semver');
		expect(fieldsFromReleasePatternTemplate(template!)).toMatchObject({
			exclude_pattern: String.raw`^(latest|nightly|edge)$`,
			pattern: String.raw`^\d+\.\d+\.\d+$`
		});
	});
});
