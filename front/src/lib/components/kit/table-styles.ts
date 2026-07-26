import { TYPE_TABLE_HEAD } from '$lib/components/kit/layout-styles';

/**
 * Table header row chrome. Type tokens live on the cell / sort button — not only
 * the `<thead>` — because UA/`button` styles reset inherited text styles and
 * left sortable columns looking different from static ones.
 */
export const TABLE_HEAD_ROW = 'border-b border-border bg-muted/40';
/**
 * Shared head cell — left-aligned to match body cells (Applied by, Content SHA-256, …).
 * Sentence case via TYPE_TABLE_HEAD; nowrap so multi-word labels stay one line.
 */
export const TABLE_HEAD_CELL = `px-4 py-3 text-left align-middle ${TYPE_TABLE_HEAD} whitespace-nowrap`;
export const TABLE_BODY_ROW = 'border-b border-border/60 last:border-0 hover:bg-muted/20';
export const TABLE_BODY_CELL = 'px-4 py-3 text-left align-middle';
export const TABLE_DATE_CELL = `${TABLE_BODY_CELL} whitespace-nowrap tabular-nums text-muted-foreground`;
export const TABLE_SORT_BUTTON = `inline-flex w-full cursor-pointer items-center justify-start gap-1 rounded ${TYPE_TABLE_HEAD} whitespace-nowrap transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-background`;
/** Sticky end column (Details) so wide tables keep the action visible. */
export const TABLE_STICKY_END =
	'sticky right-0 z-10 bg-card shadow-[-8px_0_8px_-8px_rgba(0,0,0,0.12)] dark:shadow-[-8px_0_8px_-8px_rgba(0,0,0,0.4)]';
export const TABLE_STICKY_END_HEAD = `${TABLE_STICKY_END} bg-muted/95 backdrop-blur-sm`;
