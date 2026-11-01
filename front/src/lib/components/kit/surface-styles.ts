import { TYPE_OVERLINE } from '#lib/components/kit/layout-styles.js';

/** Stat / KV labels share the overline type scale (CSS uppercase). */
export const statLabelClass = TYPE_OVERLINE;

export const statValueClass =
	'mt-0.5 font-heading text-xl font-semibold tabular-nums tracking-tight text-foreground sm:text-2xl';

/** Shared label for form fields (settings, login) — sentence case, never uppercase. */
export const fieldLabelClass = 'text-sm font-medium leading-none text-foreground';

export type SurfaceTone = 'default' | 'success' | 'warning' | 'danger';

/** @deprecated Prefer value color + tint; left bars read as AI-slop accent cards. */
export const toneBarClass: Record<SurfaceTone, string> = {
	default: '',
	success: '',
	warning: '',
	danger: ''
};

/** Faint background tint per tone — only applied to non-default metric cells. */
export const toneTintClass: Record<SurfaceTone, string> = {
	default: '',
	success: 'bg-success/[0.06]',
	warning: 'bg-warning/[0.07]',
	danger: 'bg-destructive/[0.07]'
};

/** Value text color per tone (KeyValueList, StatCard). */
export const toneTextClass: Record<SurfaceTone, string> = {
	default: 'text-foreground',
	success: 'text-success',
	warning: 'text-warning',
	danger: 'text-destructive'
};

/** Muted-default text tone for secondary table metrics (zero errors, etc.). */
export const toneMutedTextClass: Record<SurfaceTone, string> = {
	default: 'text-muted-foreground',
	success: 'text-success',
	warning: 'text-warning',
	danger: 'text-destructive'
};
