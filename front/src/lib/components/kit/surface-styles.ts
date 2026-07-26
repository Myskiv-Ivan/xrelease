import { TYPE_OVERLINE } from '$lib/components/kit/layout-styles';

/** Stat / KV labels share the overline type scale (CSS uppercase). */
export const statLabelClass = TYPE_OVERLINE;

export const statValueClass =
	'mt-1 font-heading text-2xl font-semibold tabular-nums tracking-tight text-foreground';

/** Shared label for form fields (settings, login) — sentence case, never uppercase. */
export const fieldLabelClass = 'text-sm font-medium leading-none text-foreground';

export type SurfaceTone = 'default' | 'success' | 'warning' | 'danger';

/** Left accent bar color per tone (scannable status on stat cards). */
export const toneBarClass: Record<SurfaceTone, string> = {
	default: '',
	success: 'bg-success',
	warning: 'bg-warning',
	danger: 'bg-destructive'
};

/** Faint background tint per tone — only applied to non-default cards. */
export const toneTintClass: Record<SurfaceTone, string> = {
	default: '',
	success: 'bg-success/5',
	warning: 'bg-warning/5',
	danger: 'bg-destructive/5'
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
