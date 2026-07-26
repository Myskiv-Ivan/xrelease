/**
 * Display labels for delivery sink kinds (routing graph badges, editor groups).
 *
 * Schema labels from `GET /api/v1/config/schema` win when loaded; this map is
 * the offline / pre-schema fallback (same pattern as source-kinds).
 */

import { getConfigSchemaStore } from '$lib/data/config-schema.svelte';

		const FALLBACK_LABELS: Record<string, string> = {
			apprise: 'Apprise',
			webhook: 'Webhook',
			express: 'eXpress',
			novu: 'Novu',
			slack: 'Slack',
			telegram: 'Telegram',
			smtp: 'SMTP',
			kafka: 'Kafka',
			nats: 'NATS',
			rabbitmq: 'RabbitMQ'
		};

/** Operator-facing sink kind label (never raw `apprise` lowercase). */
export function getSinkKindLabel(kind: string): string {
	const fromSchema = getConfigSchemaStore().sinkKinds.find((entry) => entry.value === kind);
	if (fromSchema?.label) return fromSchema.label;
	return FALLBACK_LABELS[kind] ?? kind;
}

/** Picker / group order for channel kinds in the editor. */
export function listSinkKindValues(): string[] {
	const fromSchema = getConfigSchemaStore().sinkKinds.map((entry) => entry.value);
	if (fromSchema.length > 0) return fromSchema;
	return Object.keys(FALLBACK_LABELS);
}
