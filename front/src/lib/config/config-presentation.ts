import type { ConfigApplyStatus, ConfigView } from '$lib/api/types';
import type { KeyValueItem } from '$lib/types/ui';
import { EMPTY_VALUE, formatDateTimeFull, formatRelative } from '$lib/core/format';
import { t } from '$lib/i18n';

export function desiredSourceLabel(source: 'ledger' | 'app_file' | 'empty'): string {
	if (source === 'ledger') return t('config.sourceLedger');
	if (source === 'empty') return t('config.sourceEmpty');
	return t('config.sourceAppFile');
}

function shortSha(sha: string | null | undefined): string {
	if (!sha) return EMPTY_VALUE;
	return `${sha.slice(0, 12)}…`;
}

/** Shared provenance rows for Overview (`ConfigApplyStatus`) and Config page (`ConfigView`). */
export function configProvenanceItems(
	input: {
		desired_source: 'ledger' | 'app_file' | 'empty';
		revision?: number | null;
		revision_label?: string | null;
		applied_at?: string | null;
		content_sha256?: string | null;
		bootstrap_path?: string;
		bootstrap_sections?: string[];
		last_rejected_revision?: number | null;
	},
	now: Date = new Date()
): KeyValueItem[] {
	const items: KeyValueItem[] = [
		{
			label: t('config.desiredSource'),
			value: desiredSourceLabel(input.desired_source)
		},
		{
			label: t('config.revision'),
			value: input.revision != null ? String(input.revision) : EMPTY_VALUE
		},
		{
			label: t('config.revisionLabel'),
			value: input.revision_label ?? EMPTY_VALUE
		},
		{
			// Relative alone ("3d") is too coarse to correlate a config change
			// with an incident, so the absolute stamp sits alongside it.
			label: t('config.appliedAt'),
			value: input.applied_at
				? `${formatDateTimeFull(input.applied_at)} (${formatRelative(input.applied_at, now)})`
				: EMPTY_VALUE
		}
	];

	if (input.content_sha256 !== undefined) {
		items.push({ label: t('config.contentSha'), value: shortSha(input.content_sha256) });
	}
	if (input.bootstrap_path !== undefined) {
		items.push({ label: t('config.bootstrapPath'), value: input.bootstrap_path });
	}
	if (input.bootstrap_sections !== undefined) {
		items.push({
			label: t('config.bootstrapSections'),
			value: input.bootstrap_sections.join(', ') || EMPTY_VALUE
		});
	}
	if (input.last_rejected_revision !== undefined) {
		items.push({
			label: t('overview.lastRejected'),
			value:
				input.last_rejected_revision != null
					? `#${input.last_rejected_revision}`
					: EMPTY_VALUE,
			tone: input.last_rejected_revision != null ? 'danger' : 'default'
		});
	}

	return items;
}

export function configApiEnabled(status: ConfigApplyStatus | ConfigView): boolean {
	return Boolean(status.api_config_enabled);
}
