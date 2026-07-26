/**
 * Mustache placeholders + short body presets for channel `template` /
 * `subject_template` — mirrors `template_placeholders` from GET /config/schema
 * and `render_template` in the backend.
 */

export const MESSAGE_TEMPLATE_PLACEHOLDERS: readonly string[] = [
	'source_id',
	'source_kind',
	'kind',
	'title',
	'body',
	'url',
	'tag'
] as const;

export interface MessageTemplatePreset {
	id: string;
	label: string;
	/** Inserted into the template field. */
	body: string;
}

/** Body presets for `template` (and similar). */
export const MESSAGE_BODY_PRESETS: readonly MessageTemplatePreset[] = [
	{
		id: 'title-url',
		label: 'Title + link',
		body: '*{{title}}*\n{{url}}'
	},
	{
		id: 'full',
		label: 'Title + body + link',
		body: '{{title}}\n\n{{body}}\n\n{{url}}'
	},
	{
		id: 'compact',
		label: 'Compact',
		body: '[{{kind}}] {{title}} {{url}}'
	},
	{
		id: 'tagged',
		label: 'With routing tag',
		body: '[{{tag}}] {{title}}\n{{url}}'
	},
	{
		id: 'json',
		label: 'JSON fields',
		body: '{"title":"{{title}}","body":"{{body}}","url":"{{url}}","tag":"{{tag}}","source_id":"{{source_id}}"}'
	}
];

/** Subject-line presets (SMTP `subject_template`). */
export const MESSAGE_SUBJECT_PRESETS: readonly MessageTemplatePreset[] = [
	{ id: 'title', label: 'Title', body: '{{title}}' },
	{ id: 'tag-title', label: 'Tag + title', body: '[{{tag}}] {{title}}' },
	{ id: 'kind-title', label: 'Kind + title', body: '{{kind}}: {{title}}' }
];

export function wrapPlaceholder(name: string): string {
	return `{{${name}}}`;
}

/** Append a placeholder at the end of `current` (with a space when needed). */
export function insertPlaceholder(current: string, name: string): string {
	const token = wrapPlaceholder(name);
	if (!current.trim()) return token;
	const needsSpace = !/\s$/.test(current);
	return `${current}${needsSpace ? ' ' : ''}${token}`;
}
