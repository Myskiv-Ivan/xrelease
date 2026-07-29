/** Shared placeholder for missing / empty display values. */
export const EMPTY_VALUE = '—';

export function formatUptime(seconds: number): string {
	if (seconds < 60) return `${seconds}s`;
	const minutes = Math.floor(seconds / 60);
	if (minutes < 60) return `${minutes}m`;
	const hours = Math.floor(minutes / 60);
	const remMinutes = minutes % 60;
	if (hours < 24) return remMinutes > 0 ? `${hours}h ${remMinutes}m` : `${hours}h`;
	const days = Math.floor(hours / 24);
	const remHours = hours % 24;
	return remHours > 0 ? `${days}d ${remHours}h` : `${days}d`;
}

export function formatInterval(seconds: number): string {
	if (seconds < 3600) return `${Math.round(seconds / 60)}m`;
	return `${(seconds / 3600).toFixed(1)}h`;
}

/**
 * Compact relative time (“2m”, “3h”).
 *
 * Used for the *secondary* representation — the tooltip on an absolute
 * timestamp, and the refresh toolbar's freshness indicator. Falls back to
 * absolute formatting for dates older than 7 days, where "63d" stops being
 * useful.
 */
export function formatRelative(
	value: string | Date | null | undefined,
	now: Date = new Date()
): string {
	if (value == null || (typeof value === 'string' && value.trim() === '')) return EMPTY_VALUE;
	const date = value instanceof Date ? value : new Date(value);
	if (Number.isNaN(date.getTime())) {
		return typeof value === 'string' ? value : EMPTY_VALUE;
	}

	const diffMs = date.getTime() - now.getTime();
	const absMs = Math.abs(diffMs);
	const past = diffMs <= 0;

	const seconds = Math.round(absMs / 1000);
	if (seconds < 45) return past ? 'now' : 'soon';

	const minutes = Math.round(seconds / 60);
	if (minutes < 60) return past ? `${minutes}m` : `in ${minutes}m`;

	const hours = Math.round(minutes / 60);
	if (hours < 24) return past ? `${hours}h` : `in ${hours}h`;

	const days = Math.round(hours / 24);
	if (days < 7) return past ? `${days}d` : `in ${days}d`;

	return formatDateTimeFull(date);
}

/**
 * The one absolute timestamp format: "Jul 23, 2026, 14:30:07".
 *
 * Every field and table renders this — a single format across the app, so a
 * timestamp read in one table is directly comparable to one in another.
 *
 * The locale is pinned to `en` rather than the browser's: ops screenshots and
 * copy-pasted values must not change shape with the operator's OS settings.
 *
 * Seconds are always included. Two config applies (or two deliveries) a minute
 * apart are indistinguishable without them, which defeats the point of an
 * audit surface.
 */
export function formatDateTimeFull(value: string | Date | null | undefined): string {
	if (value == null || (typeof value === 'string' && value.trim() === '')) return EMPTY_VALUE;
	const date = value instanceof Date ? value : new Date(value);
	if (Number.isNaN(date.getTime())) {
		return typeof value === 'string' ? value : EMPTY_VALUE;
	}
	return new Intl.DateTimeFormat('en', {
		year: 'numeric',
		month: 'short',
		day: 'numeric',
		hour: '2-digit',
		minute: '2-digit',
		second: '2-digit',
		hour12: false
	}).format(date);
}

export function formatNumber(value: number): string {
	return new Intl.NumberFormat().format(value);
}

export function formatBoolean(value: boolean): string {
	return value ? 'Yes' : 'No';
}

export function formatDisplayValue(value: string | number | boolean): string {
	if (typeof value === 'boolean') return formatBoolean(value);
	if (typeof value === 'number') return formatNumber(value);
	return value === '' ? EMPTY_VALUE : value;
}
